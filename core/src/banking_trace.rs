use {
    crate::sigverify::SigverifyTracerPacketStats,
    bincode::serialize_into,
    chrono::{DateTime, Local},
    crossbeam_channel::{unbounded, Receiver, SendError, Sender, TryRecvError},
    rolling_file::{RollingCondition, RollingConditionBasic, RollingFileAppender},
    solana_perf::{
        packet::{to_packet_batches, PacketBatch},
        test_tx::test_tx,
    },
    solana_sdk::{hash::Hash, slot_history::Slot},
    std::{
        fs::{create_dir_all, remove_dir_all},
        io::{self, Write},
        path::PathBuf,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Mutex,
        },
        thread::{self, sleep, JoinHandle},
        time::{Duration, SystemTime},
    },
    tempfile::TempDir,
    thiserror::Error,
};

pub type BankingPacketBatch = Arc<(Vec<PacketBatch>, Option<SigverifyTracerPacketStats>)>;
pub type BankingPacketSender = TracedSender;
pub type BankingPacketReceiver = Receiver<BankingPacketBatch>;
pub type TracerThreadResult = Result<(), TraceError>;
pub type TracerThread = Option<JoinHandle<TracerThreadResult>>;
pub type DirByteLimit = u64;

#[derive(Error, Debug)]
pub enum TraceError {
    #[error("IO Error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Serialization Error: {0}")]
    SerializeError(#[from] bincode::Error),

    #[error("Integer Cast Error: {0}")]
    IntegerCastError(#[from] std::num::TryFromIntError),

    #[error("dir byte limit is too small (must be larger than {1}): {0}")]
    TooSmallDirByteLimit(DirByteLimit, DirByteLimit),
}

const BASENAME: &str = "events";
const TRACE_FILE_ROTATE_COUNT: u64 = 14; // target 2 weeks retention under normal load
const TRACE_FILE_WRITE_INTERVAL_MS: u64 = 100;
const BUF_WRITER_CAPACITY: usize = 10 * 1024 * 1024;
pub const TRACE_FILE_DEFAULT_ROTATE_BYTE_THRESHOLD: u64 = 1024 * 1024 * 1024;
pub const DISABLED_BAKING_TRACE_DIR: DirByteLimit = 0;
pub const BANKING_TRACE_DIR_DEFAULT_BYTE_LIMIT: DirByteLimit =
    TRACE_FILE_DEFAULT_ROTATE_BYTE_THRESHOLD * TRACE_FILE_ROTATE_COUNT;

#[allow(clippy::type_complexity)]
#[derive(Debug)]
pub struct BankingTracer {
    enabled_tracer: Option<(
        Sender<TimedTracedEvent>,
        Mutex<Option<JoinHandle<TracerThreadResult>>>,
        Arc<AtomicBool>,
    )>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TimedTracedEvent(std::time::SystemTime, TracedEvent);

#[derive(Serialize, Deserialize, Debug)]
enum TracedEvent {
    PacketBatch(ChannelLabel, BankingPacketBatch),
    Bank(Slot, u32, BankStatus, usize),
    BlockAndBankHash(Slot, Hash, Hash),
}

#[derive(Serialize, Deserialize, Debug)]
enum BankStatus {
    Started,
    Ended,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum ChannelLabel {
    NonVote,
    TpuVote,
    GossipVote,
    Dummy,
}

struct RollingConditionGrouped {
    basic: RollingConditionBasic,
    tried_rollover_after_opened: bool,
    is_checked: bool,
}

impl RollingConditionGrouped {
    fn new(basic: RollingConditionBasic) -> Self {
        Self {
            basic,
            tried_rollover_after_opened: bool::default(),
            is_checked: bool::default(),
        }
    }

    fn reset(&mut self) {
        self.is_checked = false;
    }
}

struct GroupedWriter<'a> {
    now: DateTime<Local>,
    underlying: &'a mut RollingFileAppender<RollingConditionGrouped>,
}

impl<'a> GroupedWriter<'a> {
    fn new(underlying: &'a mut RollingFileAppender<RollingConditionGrouped>) -> Self {
        Self {
            now: Local::now(),
            underlying,
        }
    }
}

impl RollingCondition for RollingConditionGrouped {
    fn should_rollover(&mut self, now: &DateTime<Local>, current_filesize: u64) -> bool {
        if !self.tried_rollover_after_opened {
            self.tried_rollover_after_opened = true;

            // rollover normally if empty to reuse it if possible
            if current_filesize > 0 {
                // forcibly rollover anew, so that we always avoid to append
                // to a possibly-damaged tracing file even after unclean
                // restarts
                return true;
            }
        }

        if !self.is_checked {
            self.is_checked = true;
            self.basic.should_rollover(now, current_filesize)
        } else {
            false
        }
    }
}

impl<'a> Write for GroupedWriter<'a> {
    fn write(&mut self, buf: &[u8]) -> std::result::Result<usize, io::Error> {
        self.underlying.write_with_datetime(buf, &self.now)
    }
    fn flush(&mut self) -> std::result::Result<(), io::Error> {
        self.underlying.flush()
    }
}

pub fn receiving_loop_with_minimized_sender_overhead<T, E, const SLEEP_MS: u64>(
    exit: Arc<AtomicBool>,
    receiver: Receiver<T>,
    mut on_recv: impl FnMut(T) -> Result<(), E>,
) -> Result<(), E> {
    'outer: while !exit.load(Ordering::Relaxed) {
        'inner: loop {
            // avoid futex-based blocking here, otherwise a sender would have to
            // wake me up at a syscall cost...
            match receiver.try_recv() {
                Ok(message) => on_recv(message)?,
                Err(TryRecvError::Empty) => break 'inner,
                Err(TryRecvError::Disconnected) => {
                    break 'outer;
                }
            };
            if exit.load(Ordering::Relaxed) {
                break 'outer;
            }
        }
        sleep(Duration::from_millis(SLEEP_MS));
    }

    Ok(())
}

impl BankingTracer {
    pub fn new(
        maybe_config: Option<(PathBuf, Arc<AtomicBool>, DirByteLimit)>,
    ) -> Result<Arc<Self>, TraceError> {
        let enabled_tracer = maybe_config
            .map(|(path, exit, dir_byte_limit)| {
                let rotate_threshold_size = dir_byte_limit / TRACE_FILE_ROTATE_COUNT;
                if rotate_threshold_size == 0 {
                    return Err(TraceError::TooSmallDirByteLimit(
                        dir_byte_limit,
                        TRACE_FILE_ROTATE_COUNT,
                    ));
                }

                let (trace_sender, trace_receiver) = unbounded();

                Self::ensure_prepare_path(&path)?;
                let file_appender = Self::create_file_appender(path, rotate_threshold_size)?;

                let tracing_thread =
                    Self::spawn_background_thread(trace_receiver, file_appender, exit.clone())?;

                Ok((trace_sender, Mutex::new(Some(tracing_thread)), exit))
            })
            .transpose()?;

        Ok(Arc::new(Self { enabled_tracer }))
    }

    pub fn new_disabled() -> Arc<Self> {
        Arc::new(Self {
            enabled_tracer: None,
        })
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled_tracer.is_some()
    }

    fn create_channel(&self, label: ChannelLabel) -> (BankingPacketSender, BankingPacketReceiver) {
        Self::channel(
            label,
            self.enabled_tracer
                .as_ref()
                .map(|(sender, _, exit)| (sender.clone(), exit.clone())),
        )
    }

    pub fn create_channel_non_vote(&self) -> (BankingPacketSender, BankingPacketReceiver) {
        self.create_channel(ChannelLabel::NonVote)
    }

    pub fn create_channel_tpu_vote(&self) -> (BankingPacketSender, BankingPacketReceiver) {
        self.create_channel(ChannelLabel::TpuVote)
    }

    pub fn create_channel_gossip_vote(&self) -> (BankingPacketSender, BankingPacketReceiver) {
        self.create_channel(ChannelLabel::GossipVote)
    }

    pub fn take_tracer_thread_join_handle(&self) -> TracerThread {
        self.enabled_tracer.as_ref().map(|(_, tracer_thread, _)| {
            tracer_thread
                .lock()
                .unwrap()
                .take()
                .expect("no double take; BankingStage should only do once!")
        })
    }

    fn bank_event(
        &self,
        slot: Slot,
        id: u32,
        status: BankStatus,
        unprocessed_transaction_count: usize,
    ) {
        self.trace_event(TimedTracedEvent(
            SystemTime::now(),
            TracedEvent::Bank(slot, id, status, unprocessed_transaction_count),
        ))
    }

    pub fn hash_event(&self, slot: Slot, blockhash: Hash, bank_hash: Hash) {
        self.trace_event(TimedTracedEvent(
            SystemTime::now(),
            TracedEvent::BlockAndBankHash(slot, blockhash, bank_hash),
        ))
    }

    fn trace_event(&self, event: TimedTracedEvent) {
        if let Some((sender, _, exit)) = &self.enabled_tracer {
            if !exit.load(Ordering::Relaxed) {
                sender
                    .send(event)
                    .expect("active tracer thread unless exited");
            }
        }
    }

    pub fn bank_start(&self, slot: Slot, id: u32, unprocessed_transaction_count: usize) {
        self.bank_event(slot, id, BankStatus::Started, unprocessed_transaction_count);
    }

    pub fn bank_end(&self, slot: Slot, id: u32, unprocessed_transaction_count: usize) {
        self.bank_event(slot, id, BankStatus::Ended, unprocessed_transaction_count);
    }

    pub fn channel_for_test() -> (TracedSender, Receiver<BankingPacketBatch>) {
        Self::channel(ChannelLabel::Dummy, None)
    }

    pub fn channel(
        label: ChannelLabel,
        trace_sender: Option<(Sender<TimedTracedEvent>, Arc<AtomicBool>)>,
    ) -> (TracedSender, Receiver<BankingPacketBatch>) {
        let (sender, receiver) = unbounded();
        (TracedSender::new(label, sender, trace_sender), receiver)
    }

    fn ensure_prepare_path(path: &PathBuf) -> Result<(), io::Error> {
        create_dir_all(path)
    }

    pub fn ensure_cleanup_path(path: &PathBuf) -> Result<(), io::Error> {
        remove_dir_all(path).or_else(|err| {
            if err.kind() == io::ErrorKind::NotFound {
                Ok(())
            } else {
                Err(err)
            }
        })
    }

    fn create_file_appender(
        path: PathBuf,
        rotate_threshold_size: u64,
    ) -> Result<RollingFileAppender<RollingConditionGrouped>, TraceError> {
        let grouped = RollingConditionGrouped::new(
            RollingConditionBasic::new()
                .daily()
                .max_size(rotate_threshold_size),
        );
        let appender = RollingFileAppender::new_with_buffer_capacity(
            path.join(BASENAME),
            grouped,
            (TRACE_FILE_ROTATE_COUNT - 1).try_into()?,
            BUF_WRITER_CAPACITY,
        )?;
        Ok(appender)
    }

    fn spawn_background_thread(
        trace_receiver: Receiver<TimedTracedEvent>,
        mut file_appender: RollingFileAppender<RollingConditionGrouped>,
        exit: Arc<AtomicBool>,
    ) -> Result<JoinHandle<TracerThreadResult>, TraceError> {
        let thread = thread::Builder::new().name("solBanknTracer".into()).spawn(
            move || -> TracerThreadResult {
                receiving_loop_with_minimized_sender_overhead::<_, _, TRACE_FILE_WRITE_INTERVAL_MS>(
                    exit,
                    trace_receiver,
                    |event| -> Result<(), TraceError> {
                        file_appender.condition_mut().reset();
                        serialize_into(&mut GroupedWriter::new(&mut file_appender), &event)?;
                        Ok(())
                    },
                )?;
                file_appender.flush()?;
                Ok(())
            },
        )?;

        Ok(thread)
    }
}

pub struct TracedSender {
    label: ChannelLabel,
    sender: Sender<BankingPacketBatch>,
    trace_sender: Option<(Sender<TimedTracedEvent>, Arc<AtomicBool>)>,
}

impl TracedSender {
    fn new(
        label: ChannelLabel,
        sender: Sender<BankingPacketBatch>,
        trace_sender: Option<(Sender<TimedTracedEvent>, Arc<AtomicBool>)>,
    ) -> Self {
        Self {
            label,
            sender,
            trace_sender,
        }
    }

    pub fn send(&self, batch: BankingPacketBatch) -> Result<(), SendError<BankingPacketBatch>> {
        if let Some((trace_sender, exit)) = &self.trace_sender {
            if !exit.load(Ordering::Relaxed) {
                trace_sender
                    .send(TimedTracedEvent(
                        SystemTime::now(),
                        TracedEvent::PacketBatch(self.label, BankingPacketBatch::clone(&batch)),
                    ))
                    .map_err(|err| {
                        error!(
                            "unexpected error when tracing a banking event...: {:?}",
                            err
                        );
                        SendError(BankingPacketBatch::clone(&batch))
                    })?;
            }
        }
        self.sender.send(batch)
    }
}

pub mod for_test {
    use super::*;

    pub fn sample_packet_batch() -> BankingPacketBatch {
        BankingPacketBatch::new((to_packet_batches(&vec![test_tx(); 4], 10), None))
    }

    pub fn drop_and_clean_temp_dir_unless_suppressed(temp_dir: TempDir) {
        std::env::var("BANKING_TRACE_LEAVE_FILES_FROM_LAST_ITERATION")
            .is_ok()
            .then(|| {
                warn!("prevented to remove {:?}", temp_dir.path());
                drop(temp_dir.into_path());
            });
    }

    pub fn terminate_tracer(
        tracer: Arc<BankingTracer>,
        main_thread: JoinHandle<TracerThreadResult>,
        sender: TracedSender,
        exit: Option<Arc<AtomicBool>>,
    ) {
        if let Some(exit) = exit {
            exit.store(true, Ordering::Relaxed);
        }
        let tracer_thread = tracer.take_tracer_thread_join_handle();
        drop((sender, tracer));
        main_thread.join().unwrap().unwrap();
        if let Some(tracer_thread) = tracer_thread {
            tracer_thread.join().unwrap().unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        bincode::ErrorKind::Io as BincodeIoError,
        std::{
            fs::File,
            io::{BufReader, ErrorKind::UnexpectedEof},
            str::FromStr,
        },
    };

    #[test]
    fn test_new_disabled() {
        let exit = Arc::<AtomicBool>::default();

        let tracer = BankingTracer::new_disabled();
        let (non_vote_sender, non_vote_receiver) = tracer.create_channel_non_vote();

        let dummy_main_thread = thread::spawn(move || {
            receiving_loop_with_minimized_sender_overhead::<_, TraceError, 0>(
                exit,
                non_vote_receiver,
                |_packet_batch| Ok(()),
            )
        });

        non_vote_sender
            .send(BankingPacketBatch::new((vec![], None)))
            .unwrap();
        for_test::terminate_tracer(tracer, dummy_main_thread, non_vote_sender, None);
    }

    #[test]
    fn test_send_after_exited() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("banking-trace");
        let exit = Arc::<AtomicBool>::default();
        let tracer =
            BankingTracer::new(Some((path, exit.clone(), DirByteLimit::max_value()))).unwrap();
        let tracer_thread = tracer.take_tracer_thread_join_handle();
        let (non_vote_sender, non_vote_receiver) = tracer.create_channel_non_vote();

        let exit_for_dummy_thread = Arc::<AtomicBool>::default();
        let exit_for_dummy_thread2 = exit_for_dummy_thread.clone();
        let dummy_main_thread = thread::spawn(move || {
            receiving_loop_with_minimized_sender_overhead::<_, TraceError, 0>(
                exit_for_dummy_thread,
                non_vote_receiver,
                |_packet_batch| Ok(()),
            )
        });

        // kill and join the tracer thread
        exit.store(true, Ordering::Relaxed);
        tracer_thread.unwrap().join().unwrap().unwrap();

        // shouldn't panic
        tracer.bank_end(1, 2, 3);

        drop(tracer);

        // shouldn't panic
        non_vote_sender
            .send(for_test::sample_packet_batch())
            .unwrap();

        // finally terminate and join the main thread
        exit_for_dummy_thread2.store(true, Ordering::Relaxed);
        dummy_main_thread.join().unwrap().unwrap();
    }

    #[test]
    fn test_record_and_restore() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("banking-trace");
        let exit = Arc::<AtomicBool>::default();
        let tracer = BankingTracer::new(Some((
            path.clone(),
            exit.clone(),
            DirByteLimit::max_value(),
        )))
        .unwrap();
        let (non_vote_sender, non_vote_receiver) = tracer.create_channel_non_vote();

        let dummy_main_thread = thread::spawn(move || {
            receiving_loop_with_minimized_sender_overhead::<_, TraceError, 0>(
                exit,
                non_vote_receiver,
                |_packet_batch| Ok(()),
            )
        });

        non_vote_sender
            .send(for_test::sample_packet_batch())
            .unwrap();
        tracer.bank_start(1, 2, 3);
        let blockhash = Hash::from_str("B1ockhash1111111111111111111111111111111111").unwrap();
        let bank_hash = Hash::from_str("BankHash11111111111111111111111111111111111").unwrap();
        tracer.hash_event(4, blockhash, bank_hash);

        for_test::terminate_tracer(tracer, dummy_main_thread, non_vote_sender, None);

        let mut stream = BufReader::new(File::open(path.join(BASENAME)).unwrap());
        let results = (0..=3)
            .map(|_| bincode::deserialize_from::<_, TimedTracedEvent>(&mut stream))
            .collect::<Vec<_>>();

        let mut i = 0;
        assert_matches!(
            results[i],
            Ok(TimedTracedEvent(
                _,
                TracedEvent::PacketBatch(ChannelLabel::NonVote, _)
            ))
        );
        i += 1;
        assert_matches!(
            results[i],
            Ok(TimedTracedEvent(
                _,
                TracedEvent::Bank(1, 2, BankStatus::Started, 3)
            ))
        );
        i += 1;
        assert_matches!(
            results[i],
            Ok(TimedTracedEvent(
                _,
                TracedEvent::BlockAndBankHash(4, actual_blockhash, actual_bank_hash)
            )) if actual_blockhash == blockhash && actual_bank_hash == bank_hash
        );
        i += 1;
        assert_matches!(
            results[i],
            Err(ref err) if matches!(
                **err,
                BincodeIoError(ref error) if error.kind() == UnexpectedEof
            )
        );

        for_test::drop_and_clean_temp_dir_unless_suppressed(temp_dir);
    }
}

pub struct BankingSimulator {
    path: PathBuf,
    non_vote_channel: (Sender<BankingPacketBatch>, Receiver<BankingPacketBatch>),
    tpu_vote_channel: (Sender<BankingPacketBatch>, Receiver<BankingPacketBatch>),
    gossip_vote_channel: (Sender<BankingPacketBatch>, Receiver<BankingPacketBatch>),
}

impl BankingSimulator {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            non_vote_channel: unbounded(),
            tpu_vote_channel: unbounded(),
            gossip_vote_channel: unbounded(),
        }
    }

    pub fn prepare_receivers(
        &self,
    ) -> (
        Receiver<BankingPacketBatch>,
        Receiver<BankingPacketBatch>,
        Receiver<BankingPacketBatch>,
    ) {
        (
            self.non_vote_channel.1.clone(),
            self.tpu_vote_channel.1.clone(),
            self.gossip_vote_channel.1.clone(),
        )
    }

    pub fn dump(&self) {
        let mut stream = BufReader::new(File::open(&self.path).unwrap());
        let mut bank_starts_by_slot = std::collections::BTreeMap::new();
        let mut packet_batches_by_time = std::collections::BTreeMap::new();

        let mut packet_count = 0;
        loop {
            let d = bincode::deserialize_from::<_, TimedTracedEvent>(&mut stream);
            let Ok(event) = d else {
                info!("deserialize error: {:?}", &d);
                break;
            };
            let event_time = event.0;
            let event = event.1;
            let datetime: chrono::DateTime<chrono::Utc> = event_time.into();

            match &event {
                TracedEvent::PacketBatch(_label, batch) => {
                    let sum = batch.0.iter().map(|v| v.len()).sum::<usize>();
                    packet_count += sum;
                    trace!("event parsed: {}: <{}: {} = {:?}> {:?}", datetime.format("%Y-%m-%d %H:%M:%S.%f"), packet_count, sum, &batch.0.iter().map(|v| v.len()).collect::<Vec<_>>(), &event);
                }
                _ => {
                    trace!("event parsed: {}: {:?}", datetime.format("%Y-%m-%d %H:%M:%S.%f"), &event);
                }
            }
            match event {
                TracedEvent::Bank(slot, _, BankStatus::Started, _) => {
                    bank_starts_by_slot.insert(slot, event_time);
                }
                TracedEvent::PacketBatch(label, batch) => {
                    packet_batches_by_time.insert(event_time, (label, batch));
                }
                _ => {}
            }
        }
    }

    pub fn simulate(
        &self,
        bank_forks: Arc<std::sync::RwLock<solana_runtime::bank_forks::BankForks>>,
        blockstore: Arc<solana_ledger::blockstore::Blockstore>,
    ) {
        use {
            crate::banking_stage::BankingStage, log::*,
            solana_client::connection_cache::ConnectionCache, solana_gossip::cluster_info::Node,
            solana_ledger::leader_schedule_cache::LeaderScheduleCache,
            solana_poh::poh_recorder::create_test_recorder, solana_runtime::bank::Bank,
            solana_sdk::signature::Keypair, solana_streamer::socket::SocketAddrSpace,
            solana_tpu_client::tpu_connection_cache::DEFAULT_TPU_CONNECTION_POOL_SIZE,
        };
        use std::io::BufReader;
        use std::fs::File;

        let mut bank = bank_forks.read().unwrap().working_bank();

        let (non_vote_sender, tpu_vote_sender, gossip_vote_sender) = (
            self.non_vote_channel.0.clone(),
            self.tpu_vote_channel.0.clone(),
            self.gossip_vote_channel.0.clone(),
        );
        let bank_slot = bank.slot();

        std::thread::spawn(move || {
            let (most_recent_past_leader_slot, range_iter) = if let Some((most_recent_past_leader_slot, start)) = bank_starts_by_slot.range(..bank_slot).next() {
                (Some(most_recent_past_leader_slot), packet_batches_by_time.range(*start..))
            } else {
                (None, packet_batches_by_time.range(..))
            };
            info!(
                "simulating banking trace events: {} out of {}, starting at slot {} (adjusted to {:?})",
                range_iter.clone().count(),
                packet_batches_by_time.len(),
                bank_slot,
                most_recent_past_leader_slot,
            );

            loop {
                for (&_key, ref value) in range_iter.clone() {
                    let (label, batch) = &value;
                    debug!("sent {:?} {} batches", label, batch.0.len());

                    match label {
                        ChannelLabel::NonVote => non_vote_sender.send(batch.clone()).unwrap(),
                        ChannelLabel::TpuVote => tpu_vote_sender.send(batch.clone()).unwrap(),
                        ChannelLabel::GossipVote => gossip_vote_sender.send(batch.clone()).unwrap(),
                        ChannelLabel::Dummy => unreachable!(),
                    }
                }
            }
        });

        let (non_vote_receiver, tpu_vote_receiver, gossip_vote_receiver) = self.prepare_receivers();

        let collector = solana_sdk::pubkey::new_rand();
        let leader_schedule_cache = Arc::new(LeaderScheduleCache::new_from_bank(&bank));
        let (exit, poh_recorder, poh_service, _signal_receiver) =
            create_test_recorder(&bank, &blockstore, None, Some(leader_schedule_cache));

        let banking_tracer = BankingTracer::new_disabled();
        let cluster_info = solana_gossip::cluster_info::ClusterInfo::new(
            Node::new_localhost().info,
            Arc::new(Keypair::new()),
            SocketAddrSpace::Unspecified,
        );
        let cluster_info = Arc::new(cluster_info);
        let connection_cache = ConnectionCache::new(DEFAULT_TPU_CONNECTION_POOL_SIZE);
        let (replay_vote_sender, _replay_vote_receiver) = unbounded();
        let banking_stage = BankingStage::new_num_threads(
            &cluster_info,
            &poh_recorder,
            non_vote_receiver,
            tpu_vote_receiver,
            gossip_vote_receiver,
            4,
            None,
            replay_vote_sender,
            None,
            Arc::new(connection_cache),
            bank_forks.clone(),
            banking_tracer,
        );

        let clear_sigs = std::env::var("CLEAR_SIGS").is_ok();
        if clear_sigs {
            warn!("will clear sigs as requested....");
        }
        if std::env::var("SKIP_CHECK_AGE").is_ok() {
            warn!("skipping check age as requested....");
            bank.skip_check_age();
        }

        if clear_sigs {
            bank.clear_signatures();
        }
        poh_recorder.write().unwrap().set_bank(&bank, false);

        for _ in 0..200 {
            if poh_recorder.read().unwrap().bank().is_none() {
                poh_recorder
                    .write()
                    .unwrap()
                    .reset(bank.clone(), Some((bank.slot(), bank.slot() + 1)));
                let new_bank = Bank::new_from_parent(&bank, &collector, bank.slot() + 1);
                bank_forks.write().unwrap().insert(new_bank);
                bank = bank_forks.read().unwrap().working_bank();
            }
            // set cost tracker limits to MAX so it will not filter out TXs
            bank.write_cost_tracker().unwrap().set_limits(
                std::u64::MAX,
                std::u64::MAX,
                std::u64::MAX,
            );

            if clear_sigs {
                bank.clear_signatures();
            }
            poh_recorder.write().unwrap().set_bank(&bank, false);

            sleep(std::time::Duration::from_millis(100));
        }
        exit.store(true, Ordering::Relaxed);
        banking_stage.join().unwrap();
        poh_service.join().unwrap();
    }
}

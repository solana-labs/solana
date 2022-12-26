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

// Not all of TracedEvents need to be timed for proper simulation functioning; however, do so for
// consistency, implementation simplicity, and direct human inspection of trace files for
// debugging.
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
                    info!("disconnected!");
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
                info!("send bank event!: {:?}", &event);
                sender
                    .send(event)
                    .expect("active tracer thread unless exited");
            } else {
                info!("NOT send bank event...!");
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
                info!("flushed!");
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

// This creates a simulated environment around banking stage to reproduce leader's blocks based on
// recorded banking trace events (`TimedTracedEvent`).
//
// The task of banking stage at the highest level is to pack transactions into their blocks as much
// as possible for scheduled fixed duration.  So, there's 3 abstract inputs to simulate: blocks,
// time, and transactions.
//
// In the context of simulation, the first two are simple; both are well defined.
//
// For ancestor blocks, we firstly replay certain number of blocks immediately up to target
// simulation leader's slot with halt_at_slot mechanism, possibly priming various caches,
// ultimately freezing the ancestor block with expected and deterministic hashes.
//
// After replay, a minor tweak is applied: we forcibly override leader's block hashes as simulated
// banking stage creates them, using recorded `BlockAndBankHash` events. This is to provide
// undistinguishable sysvars to TX execution and identical TX age resolution as simulation goes on.
// Otherwise, these overridden hashes would definitely differ as slight block composition
// difference is inevitable.
//
// For poh time, we just use PohRecorder as same as real environment, which is just 400ms timer,
// external to banking stage and thus mostly irrelevant to banking stage performance.  For wall
// time, we use the first BankStatus::Started and `SystemTime::now()` to define T=0 for simulation
// after calculating the offset. Then, simulation progress is timed accordingly. (TODO: maybe we
// can use first `BlockAndBankHash`?)  That's needed because all trace events are recorded in UTC,
// not relative to poh nor to leader schedule for simplicity at recording.
//
// Lastly, here's the last and most complicated input to simulate: transactions.
//
// A bit closer look of transaction load profile is like below, regardless of internal banking
// implementation and simulation:
//
// There's ever accumulated transactions to be processed as leader slot nears. This is due to
// solana's general tx broadcast strategy of node's forwarding and client's submission, which are
// unlikely to chabge soon. So, we take this as granted. Then, any initial leader block creation
// starts with rather large number of schedule-able transactions. Also, note that additional
// transactions arrive for the 4 leader slot window (roughly ~1.6 seconds).
//
// Simulation have to mimic this load pattern while being agnostic to internal bnaking impl as much
// as possible. For that agnostic objective, `TracedSender`s are sneaked into the SigVerify stage
// and gossip subsystem by `BankingTracer` to trace **all** of `BankingPacketBatch`s' exact payload
// and _sender's timing with `SystemTime::now()` for all `ChannelLabel`s. This deliberate tracing
// placement is not to be affected by any banking-tage's capping (if any) and its channel
// consumption pattern.
//
// Then, BankingSimulator consists of 2 modes chronologically: pre-loading and on-the-fly. The 2
// stage is segregated by the aforementioned T=0.
//
// Pre-load firstly buffers transactions into crossbeam_channels. it traverses trace file in
// reverse chronological order until it hit the unprocessed_count in TracedEvent::Bank. this is
// done for each `ChannelLabel`s. (todo: Come to think of it, maybe this can be replaced with
// T-120secs ideling-but-only-sending mode (say, _priming_ or warmup mode?) at the cost of some
// wait time...)
//
// Then, at T=0, we invoke `BankingStage::new_num_threads()`. At the same time, we set leader's
// first bank to PoHRecorder's working bank. Then, BankingStage starts to pack transactions into
// banks.
//
// At the simulation side, it's now on-the-fly mode. In this, BankingSimulator just sends
// BankingPacketBatch pretending to be sigveirfy stage while burning 1 thread to busy loop for
// precise T=N at ~1us granularity.
pub struct BankingSimulator {
    path: PathBuf,
}

impl BankingSimulator {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
        }
    }

    pub fn dump(&self, bank: Option<Arc<solana_runtime::bank::Bank>>) -> (std::collections::BTreeMap<Slot, std::collections::HashMap<u32, (std::time::SystemTime, usize)>>, std::collections::BTreeMap<std::time::SystemTime, (ChannelLabel, BankingPacketBatch)>, std::collections::HashMap<u64, (solana_sdk::hash::Hash, solana_sdk::hash::Hash)>, (usize, usize, usize)) {
        use std::io::BufReader;
        use std::fs::File;
        let mut stream = BufReader::new(File::open(&self.path).unwrap());
        let mut bank_starts_by_slot = std::collections::BTreeMap::new();
        let mut packet_batches_by_time = std::collections::BTreeMap::new();
        let mut hashes_by_slot = std::collections::HashMap::new();

        let mut packet_count = 0;
        let mut events = vec![];

        loop {
            let d = bincode::deserialize_from::<_, TimedTracedEvent>(&mut stream);
            let Ok(event) = d else {
                info!("deserialize error: {:?}", &d);
                break;
            };
            events.push(event);
        }
        for event in &events {
            let event_time = event.0;
            let event = &event.1;
            let datetime: chrono::DateTime<chrono::Utc> = event_time.into();

            match event {
                TracedEvent::Bank(slot, id, BankStatus::Started, unprocessed_count) => {
                    bank_starts_by_slot.entry(*slot)
                        .and_modify(|e: &mut std::collections::HashMap<u32, (SystemTime, usize)>| {e.insert(*id, (event_time, *unprocessed_count));})
                        .or_insert(std::collections::HashMap::from([(*id, (event_time, *unprocessed_count));1]));
                }
                TracedEvent::PacketBatch(label, batch) => {
                    packet_batches_by_time.insert(event_time, (label.clone(), batch.clone()));
                }
                TracedEvent::BlockAndBankHash(slot, blockhash, bank_hash) => {
                    hashes_by_slot.insert(*slot, (*blockhash, *bank_hash));
                },
                _ => {},
            }
        }

        let (reference_time, unprocessed_counts) = if let Some(bank) = &bank {
            let b = bank_starts_by_slot.range(bank.slot()..).next();
            info!("found: {:?}", b);
            let (mut non_vote, mut tpu_vote, mut gossip_vote) = (0, 0, 0);
            for (id, (_time, count)) in b.map(|b| b.1.clone()).unwrap_or_default().iter() {
                match id {
                    0 => gossip_vote += count,
                    1 => tpu_vote += count,
                    _ => non_vote += count,
                }
            }
            info!("summed: {:?}", (non_vote, tpu_vote, gossip_vote));
            (b.map(|(_, s)| s.values().map(|a| a.0).min().unwrap()).unwrap().clone(), (non_vote, tpu_vote, gossip_vote))
        } else {
            (packet_batches_by_time.keys().next().unwrap().clone(), Default::default())
        };

        for event in &events {
            let event_time = event.0;
            let event = &event.1;
            let datetime: chrono::DateTime<chrono::Utc> = event_time.into();

            let delta_label = if event_time < reference_time {
                format!("-{}us", reference_time.duration_since(event_time).unwrap().as_micros())
            } else {
                format!("+{}us", event_time.duration_since(reference_time).unwrap().as_micros())
            };

            match &event {
                TracedEvent::PacketBatch(_label, batch) => {
                    let sum = batch.0.iter().map(|v| v.len()).sum::<usize>();
                    packet_count += sum;
                    let st_label = if let Some(bank) = &bank {
                        let st = crate::packet_deserializer::PacketDeserializer::deserialize_and_collect_packets(0, &[batch.clone()]).deserialized_packets.iter().filter_map(|ip| ip.build_sanitized_transaction(&bank.feature_set, false, bank.as_ref())).filter_map(|s| bank.check_age_tx(&s).0.ok()).count();
                        format!("({})", st)
                    } else {
                        "".into()
                    };
                    trace!("event parsed: {delta_label} {}: <{}: {}{} = {:?}> {:?}", datetime.format("%Y-%m-%d %H:%M:%S.%f"), packet_count, sum, st_label, &batch.0.iter().map(|v| v.len()).collect::<Vec<_>>(), &event);
                }
                _ => {
                    trace!("event parsed: {delta_label} {}: {:?}", datetime.format("%Y-%m-%d %H:%M:%S.%f"), &event);
                }
            }
        }

        (bank_starts_by_slot, packet_batches_by_time, hashes_by_slot, unprocessed_counts)
    }

    pub fn simulate(
        &self,
        genesis_config: &solana_sdk::genesis_config::GenesisConfig,
        bank_forks: Arc<std::sync::RwLock<solana_runtime::bank_forks::BankForks>>,
        blockstore: Arc<solana_ledger::blockstore::Blockstore>,
    ) {
        use {
            crate::banking_stage::{BankingStage, NUM_THREADS}, log::*,
            solana_client::connection_cache::ConnectionCache, solana_gossip::cluster_info::Node,
            solana_ledger::leader_schedule_cache::LeaderScheduleCache,
            solana_poh::poh_recorder::create_test_recorder, solana_runtime::bank::Bank,
            solana_sdk::signature::Keypair, solana_streamer::socket::SocketAddrSpace,
            solana_tpu_client::tpu_connection_cache::DEFAULT_TPU_CONNECTION_POOL_SIZE,
        };

        let mut bank = bank_forks.read().unwrap().working_bank();

        let (bank_starts_by_slot, packet_batches_by_time, hashes_by_slot, unprocessed_counts) = self.dump(Some(bank.clone()));
        if std::env::var("DUMP_WITH_BANK").is_ok() {
            return
        }

        let bank_slot = bank.slot();



        let leader_schedule_cache = Arc::new(LeaderScheduleCache::new_from_bank(&bank));
        let simulated_slot = bank.slot() + 1;
        let simulated_leader = leader_schedule_cache.slot_leader_at(simulated_slot, None).unwrap();
        info!("simulated leader and slot: {}, {}", simulated_leader, simulated_slot);
        let start_bank = bank_forks.read().unwrap().root_bank();

        let (exit, poh_recorder, poh_service, _signal_receiver) = {
            use std::sync::RwLock;
            use solana_poh::poh_service::PohService;
            use solana_poh::poh_recorder::PohRecorder;

            let exit = Arc::new(AtomicBool::default());
            //create_test_recorder(&bank, &blockstore, None, Some(leader_schedule_cache));
            info!("poh is starting!");
            let (r, entry_receiver, record_receiver) = PohRecorder::new_with_clear_signal(
                start_bank.tick_height(),
                start_bank.last_blockhash(),
                start_bank.clone(),
                Some((simulated_slot, simulated_slot + 4)),
                start_bank.ticks_per_slot(),
                &simulated_leader,
                &blockstore,
                blockstore.get_new_shred_signal(0),
                &leader_schedule_cache,
                &genesis_config.poh_config,
                None,
                exit.clone(),
            );
            let r = Arc::new(RwLock::new(r));
            let s = PohService::new(
                r.clone(),
                &genesis_config.poh_config,
                &exit,
                start_bank.ticks_per_slot(),
                solana_poh::poh_service::DEFAULT_PINNED_CPU_CORE + 4,
                solana_poh::poh_service::DEFAULT_HASHES_PER_BATCH,
                record_receiver,
            );
            (exit, r, s, entry_receiver)
        };
        let target_ns_per_slot = solana_poh::poh_service::PohService::target_ns_per_tick(
            start_bank.ticks_per_slot(),
            genesis_config.poh_config.target_tick_duration.as_nanos() as u64,
        ) * start_bank.ticks_per_slot();
        let warmup_duration = std::time::Duration::from_nanos((simulated_slot - (start_bank.slot() + 1)) * target_ns_per_slot);
        // if slot is too short => bail
        info!("warmup_duration: {:?}", warmup_duration);

        let banking_retracer = BankingTracer::new(Some((
            blockstore.banking_retracer_path(),
            exit.clone(),
            BANKING_TRACE_DIR_DEFAULT_BYTE_LIMIT,
        ))).unwrap();
        if banking_retracer.is_enabled() {
            info!(
                "Enabled banking retracer (dir_byte_limit: {})",
                BANKING_TRACE_DIR_DEFAULT_BYTE_LIMIT,
            );
        } else {
            info!("Disabled banking retracer");
        }

        let (non_vote_sender, non_vote_receiver) = banking_retracer.create_channel_non_vote();
        let (tpu_vote_sender, tpu_vote_receiver) = banking_retracer.create_channel_tpu_vote();
        let (gossip_vote_sender, gossip_vote_receiver) =
            banking_retracer.create_channel_gossip_vote();

        let (mut non_vote_tx_count, mut tpu_vote_tx_count, mut gossip_vote_tx_count) = (0, 0, 0);
        if let Some((most_recent_past_leader_slot, starts)) = bank_starts_by_slot.range(bank_slot..).next() {
            let start = starts.values().map(|a| a.0).min().unwrap();
            let mut batches_with_label = vec![];
            for (&time, ref value)  in packet_batches_by_time.range(..start).rev() {
                let (label, batch) = &value;

                match label {
                    ChannelLabel::NonVote => {
                        batches_with_label.push((ChannelLabel::NonVote, batch.clone()));
                        non_vote_tx_count += batch.0.iter().map(|b| b.len()).sum::<usize>();
                    }
                    ChannelLabel::TpuVote => {
                        batches_with_label.push((ChannelLabel::TpuVote, batch.clone()));
                        tpu_vote_tx_count += batch.0.iter().map(|b| b.len()).sum::<usize>();
                    }
                    ChannelLabel::GossipVote => {
                        batches_with_label.push((ChannelLabel::GossipVote, batch.clone()));
                        gossip_vote_tx_count += batch.0.iter().map(|b| b.len()).sum::<usize>();
                    }
                    ChannelLabel::Dummy => unreachable!(),
                }

                if non_vote_tx_count >= unprocessed_counts.0 &&
                    tpu_vote_tx_count >= unprocessed_counts.1 &&
                    gossip_vote_tx_count >= unprocessed_counts.2  {
                        break;
                }
            }
            for (label, batch) in batches_with_label.iter().rev() {
                match label {
                    ChannelLabel::NonVote => {
                        non_vote_sender.send(batch.clone()).unwrap();
                    }
                    ChannelLabel::TpuVote => {
                        tpu_vote_sender.send(batch.clone()).unwrap();
                    }
                    ChannelLabel::GossipVote => {
                        gossip_vote_sender.send(batch.clone()).unwrap();
                    }
                    ChannelLabel::Dummy => unreachable!(),
                }
            }
            info!("finished buffered sending...(non_vote: {}, tpu_vote: {}, gossip_vote: {})", non_vote_tx_count, tpu_vote_tx_count, gossip_vote_tx_count);
        };



        let sender_thread = std::thread::spawn( { let exit = exit.clone(); move || {
            let (adjusted_reference, range_iter) = if let Some((most_recent_past_leader_slot, starts)) = bank_starts_by_slot.range(bank_slot..).next() {
                let start = starts.values().map(|a| a.0).min().unwrap();
                (Some(({
                    let datetime: chrono::DateTime<chrono::Utc> = (start).into();
                    format!("{}", datetime.format("%Y-%m-%d %H:%M:%S.%f"))
                }, most_recent_past_leader_slot, start)), packet_batches_by_time.range(start..))
            } else {
                (None, packet_batches_by_time.range(..))
            };
            info!(
                "simulating banking trace events: {} out of {}, starting at slot {} (adjusted to {:?})",
                range_iter.clone().count(),
                packet_batches_by_time.len(),
                bank_slot,
                adjusted_reference,
            );
            let (mut non_vote_count, mut tpu_vote_count, mut gossip_vote_count) = (0, 0, 0);
            let (mut non_vote_tx_count, mut tpu_vote_tx_count, mut gossip_vote_tx_count) = (0, 0, 0);

            let reference_time= adjusted_reference.map(|b| b.2.clone()).unwrap_or_else(|| std::time::SystemTime::now());
            let burst = std::env::var("BURST").is_ok();

            //loop {
                info!("start sending!...");
                let simulation_time = std::time::SystemTime::now();
                for (&time, ref value) in range_iter.clone() {
                    let (label, batch) = &value;
                    debug!("sent {:?} {} batches", label, batch.0.len());

                    if !burst && time > reference_time {
                        let target_duration = time.duration_since(reference_time).unwrap();
                        while simulation_time.elapsed().unwrap() < target_duration {
                        }
                    }

                    match label {
                        ChannelLabel::NonVote => {
                            non_vote_sender.send(batch.clone()).unwrap();
                            non_vote_count += batch.0.len();
                            non_vote_tx_count += batch.0.iter().map(|b| b.len()).sum::<usize>();
                        }
                        ChannelLabel::TpuVote => {
                            tpu_vote_sender.send(batch.clone()).unwrap();
                            tpu_vote_count += batch.0.len();
                            tpu_vote_tx_count += batch.0.iter().map(|b| b.len()).sum::<usize>();
                        }
                        ChannelLabel::GossipVote => {
                            gossip_vote_sender.send(batch.clone()).unwrap();
                            gossip_vote_count += batch.0.len();
                            gossip_vote_tx_count += batch.0.iter().map(|b| b.len()).sum::<usize>();
                        }
                        ChannelLabel::Dummy => unreachable!(),
                    }

                    if exit.load(Ordering::Relaxed) {
                        break
                    }
                }
            //}
            info!("finished sending...(non_vote: {}({}), tpu_vote: {}({}), gossip_vote: {}({}))", non_vote_count, non_vote_tx_count, tpu_vote_count, tpu_vote_tx_count, gossip_vote_count, gossip_vote_tx_count);
            // hold these senders in join_handle to control banking stage termination!
            (non_vote_sender, tpu_vote_sender, gossip_vote_sender)
        }});


        let cluster_info = solana_gossip::cluster_info::ClusterInfo::new(
            Node::new_localhost_with_pubkey(&simulated_leader).info,
            Arc::new(Keypair::new()),
            SocketAddrSpace::Unspecified,
        );
        let cluster_info = Arc::new(cluster_info);
        let connection_cache = ConnectionCache::new(DEFAULT_TPU_CONNECTION_POOL_SIZE);
        let (replay_vote_sender, _replay_vote_receiver) = unbounded();
        info!("start banking stage!...");
        let banking_stage = BankingStage::new_num_threads(
            &cluster_info,
            &poh_recorder,
            non_vote_receiver,
            tpu_vote_receiver,
            gossip_vote_receiver,
            NUM_THREADS,
            None,
            replay_vote_sender,
            None,
            Arc::new(connection_cache),
            bank_forks.clone(),
            banking_retracer.clone(),
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

        use solana_sdk::hash::Hash;
        for i in 0..5000 {
            let slot = poh_recorder.read().unwrap().slot();
            info!("poh: {}, {}", i, slot);
            if slot >= simulated_slot {
                break;
            }
            sleep(std::time::Duration::from_millis(10));
        }

        for _ in 0..500 {
            if poh_recorder.read().unwrap().bank().is_none() {
                poh_recorder
                    .write()
                    .unwrap()
                    .reset(bank.clone(), Some((bank.slot(), bank.slot() + 1)));
                info!("Bank::new_from_parent()!");
                use solana_runtime::bank::NewBankOptions;

                let old_slot = bank.slot();
                bank.freeze_with_bank_hash_override(hashes_by_slot.get(&old_slot).map(|hh| hh.1));
                let new_slot = bank.slot() + 1;
                let new_leader = leader_schedule_cache.slot_leader_at(new_slot, None).unwrap();
                if simulated_leader != new_leader {
                    info!("{} isn't leader anymore at slot {}; new leader: {}", simulated_leader, new_slot, new_leader);
                    break;
                }
                let options = NewBankOptions {
                    blockhash_override: hashes_by_slot.get(&new_slot).map(|hh| hh.0),
                    ..Default::default()
                };
                let new_bank = Bank::new_from_parent_with_options(&bank, &simulated_leader, new_slot, options);
                // make sure parent is frozen for finalized hashes via the above
                // new()-ing of its child bank
                banking_retracer.hash_event(bank.slot(), bank.last_blockhash(), bank.hash());
                bank_forks.write().unwrap().insert(new_bank);
                bank = bank_forks.read().unwrap().working_bank();
                poh_recorder.write().unwrap().set_bank(&bank, false);
            }

            if clear_sigs {
                bank.clear_signatures();
            }

            sleep(std::time::Duration::from_millis(10));
        }
        info!("sleeping just before exit...");
        sleep(std::time::Duration::from_millis(3000));
        exit.store(true, Ordering::Relaxed);
        // the order is important. dropping sender_thread will terminate banking_stage, in turn
        // banking_retracer thread
        sender_thread.join().unwrap();
        banking_stage.join().unwrap();
        poh_service.join().unwrap();

        // TODO: add flat to store shreds into ledger so that we can even benchmark replay stgage with
        // actua blocks created by these simulation
    }
}

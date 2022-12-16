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

#[derive(Error, Debug)]
pub enum TraceError {
    #[error("IO Error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Serialization Error: {0}")]
    SerializeError(#[from] bincode::Error),

    #[error("Integer Cast Error: {0}")]
    IntegerCastError(#[from] std::num::TryFromIntError),

    #[error("Trace size is too small (must be larger than {1}): {0}")]
    TooSmallTraceSize(u64, u64),
}

const BASENAME: &str = "events";
const TRACE_FILE_ROTATE_COUNT: u64 = 14; // target 2 weeks retention under normal load
const TRACE_FILE_WRITE_INTERVAL_MS: u64 = 100;
const BUF_WRITER_CAPACITY: usize = 10 * 1024 * 1024;
pub const TRACE_FILE_DEFAULT_ROTATE_BYTE_THRESHOLD: u64 = 1024 * 1024 * 1024;
pub const EMPTY_BANKING_TRACE_SIZE: u64 = 0;
pub const DEFAULT_BANKING_TRACE_SIZE: u64 =
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
    is_checked: bool,
}

impl RollingConditionGrouped {
    fn new(basic: RollingConditionBasic) -> Self {
        Self {
            basic,
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

pub fn sender_overhead_minimized_receiver_loop<T, E, const SLEEP_MS: u64>(
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
        maybe_config: Option<(PathBuf, Arc<AtomicBool>, u64)>,
    ) -> Result<Arc<Self>, TraceError> {
        let enabled_tracer = maybe_config
            .map(|(path, exit, total_size)| -> Result<_, TraceError> {
                let rotate_threshold_size = total_size / TRACE_FILE_ROTATE_COUNT;
                if rotate_threshold_size == 0 {
                    return Err(TraceError::TooSmallTraceSize(
                        total_size,
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
                sender_overhead_minimized_receiver_loop::<
                    _,
                    TraceError,
                    TRACE_FILE_WRITE_INTERVAL_MS,
                >(exit, trace_receiver, |event| -> Result<(), TraceError> {
                    file_appender.condition_mut().reset();
                    serialize_into(&mut GroupedWriter::new(&mut file_appender), &event)?;
                    Ok(())
                })?;
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
                        TracedEvent::PacketBatch(self.label, Arc::clone(&batch)),
                    ))
                    .expect("active tracer thread unless exited");
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
            sender_overhead_minimized_receiver_loop::<_, TraceError, 0>(
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
        let tracer = BankingTracer::new(Some((path, exit.clone(), u64::max_value()))).unwrap();
        let tracer_thread = tracer.take_tracer_thread_join_handle();
        let (non_vote_sender, non_vote_receiver) = tracer.create_channel_non_vote();

        let exit_for_dummy_thread = Arc::<AtomicBool>::default();
        let exit_for_dummy_thread2 = exit_for_dummy_thread.clone();
        let dummy_main_thread = thread::spawn(move || {
            sender_overhead_minimized_receiver_loop::<_, TraceError, 0>(
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
        let tracer =
            BankingTracer::new(Some((path.clone(), exit.clone(), u64::max_value()))).unwrap();
        let (non_vote_sender, non_vote_receiver) = tracer.create_channel_non_vote();

        let dummy_main_thread = thread::spawn(move || {
            sender_overhead_minimized_receiver_loop::<_, TraceError, 0>(
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

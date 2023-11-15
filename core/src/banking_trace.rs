use {
    crate::sigverify::SigverifyTracerPacketStats,
    bincode::serialize_into,
    chrono::{DateTime, Local},
    crossbeam_channel::{unbounded, Receiver, SendError, Sender, TryRecvError},
    rolling_file::{RollingCondition, RollingConditionBasic, RollingFileAppender},
    solana_perf::packet::PacketBatch,
    solana_sdk::{hash::Hash, slot_history::Slot},
    std::{
        fs::{create_dir_all, remove_dir_all},
        io::{self, Write},
        path::PathBuf,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        thread::{self, sleep, JoinHandle},
        time::{Duration, SystemTime},
    },
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

    #[error("Trace directory's byte limit is too small (must be larger than {1}): {0}")]
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

#[derive(Clone, Debug)]
struct ActiveTracer {
    trace_sender: Sender<TimedTracedEvent>,
    exit: Arc<AtomicBool>,
}

#[derive(Debug)]
pub struct BankingTracer {
    active_tracer: Option<ActiveTracer>,
}

#[frozen_abi(digest = "Eq6YrAFtTbtPrCEvh6Et1mZZDCARUg1gcK2qiZdqyjUz")]
#[derive(Serialize, Deserialize, Debug, AbiExample)]
pub struct TimedTracedEvent(pub std::time::SystemTime, pub TracedEvent);

#[derive(Serialize, Deserialize, Debug, AbiExample, AbiEnumVisitor)]
pub enum TracedEvent {
    PacketBatch(ChannelLabel, BankingPacketBatch),
    BlockAndBankHash(Slot, Hash, Hash),
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, AbiExample, AbiEnumVisitor)]
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
        maybe_config: Option<(&PathBuf, Arc<AtomicBool>, DirByteLimit)>,
    ) -> Result<(Arc<Self>, TracerThread), TraceError> {
        match maybe_config {
            None => Ok((Self::new_disabled(), None)),
            Some((path, exit, dir_byte_limit)) => {
                let rotate_threshold_size = dir_byte_limit / TRACE_FILE_ROTATE_COUNT;
                if rotate_threshold_size == 0 {
                    return Err(TraceError::TooSmallDirByteLimit(
                        dir_byte_limit,
                        TRACE_FILE_ROTATE_COUNT,
                    ));
                }

                let (trace_sender, trace_receiver) = unbounded();

                let file_appender = Self::create_file_appender(path, rotate_threshold_size)?;

                let tracer_thread =
                    Self::spawn_background_thread(trace_receiver, file_appender, exit.clone())?;

                Ok((
                    Arc::new(Self {
                        active_tracer: Some(ActiveTracer { trace_sender, exit }),
                    }),
                    Some(tracer_thread),
                ))
            }
        }
    }

    pub fn new_disabled() -> Arc<Self> {
        Arc::new(Self {
            active_tracer: None,
        })
    }

    pub fn is_enabled(&self) -> bool {
        self.active_tracer.is_some()
    }

    fn create_channel(&self, label: ChannelLabel) -> (BankingPacketSender, BankingPacketReceiver) {
        Self::channel(label, self.active_tracer.as_ref().cloned())
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

    pub fn hash_event(&self, slot: Slot, blockhash: &Hash, bank_hash: &Hash) {
        self.trace_event(|| {
            TimedTracedEvent(
                SystemTime::now(),
                TracedEvent::BlockAndBankHash(slot, *blockhash, *bank_hash),
            )
        })
    }

    fn trace_event(&self, on_trace: impl Fn() -> TimedTracedEvent) {
        if let Some(ActiveTracer { trace_sender, exit }) = &self.active_tracer {
            if !exit.load(Ordering::Relaxed) {
                trace_sender
                    .send(on_trace())
                    .expect("active tracer thread unless exited");
            }
        }
    }

    pub fn channel_for_test() -> (TracedSender, Receiver<BankingPacketBatch>) {
        Self::channel(ChannelLabel::Dummy, None)
    }

    fn channel(
        label: ChannelLabel,
        active_tracer: Option<ActiveTracer>,
    ) -> (TracedSender, Receiver<BankingPacketBatch>) {
        let (sender, receiver) = unbounded();
        (TracedSender::new(label, sender, active_tracer), receiver)
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
        path: &PathBuf,
        rotate_threshold_size: u64,
    ) -> Result<RollingFileAppender<RollingConditionGrouped>, TraceError> {
        create_dir_all(path)?;
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
    active_tracer: Option<ActiveTracer>,
}

impl TracedSender {
    fn new(
        label: ChannelLabel,
        sender: Sender<BankingPacketBatch>,
        active_tracer: Option<ActiveTracer>,
    ) -> Self {
        Self {
            label,
            sender,
            active_tracer,
        }
    }

    pub fn send(&self, batch: BankingPacketBatch) -> Result<(), SendError<BankingPacketBatch>> {
        if let Some(ActiveTracer { trace_sender, exit }) = &self.active_tracer {
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

#[cfg(any(test, feature = "dev-context-only-utils"))]
pub mod for_test {
    use {
        super::*,
        solana_perf::{packet::to_packet_batches, test_tx::test_tx},
        tempfile::TempDir,
    };

    pub fn sample_packet_batch() -> BankingPacketBatch {
        BankingPacketBatch::new((to_packet_batches(&vec![test_tx(); 4], 10), None))
    }

    pub fn drop_and_clean_temp_dir_unless_suppressed(temp_dir: TempDir) {
        std::env::var("BANKING_TRACE_LEAVE_FILES").is_ok().then(|| {
            warn!("prevented to remove {:?}", temp_dir.path());
            drop(temp_dir.into_path());
        });
    }

    pub fn terminate_tracer(
        tracer: Arc<BankingTracer>,
        tracer_thread: TracerThread,
        main_thread: JoinHandle<TracerThreadResult>,
        sender: TracedSender,
        exit: Option<Arc<AtomicBool>>,
    ) {
        if let Some(exit) = exit {
            exit.store(true, Ordering::Relaxed);
        }
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
        tempfile::TempDir,
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
        for_test::terminate_tracer(tracer, None, dummy_main_thread, non_vote_sender, None);
    }

    #[test]
    fn test_send_after_exited() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("banking-trace");
        let exit = Arc::<AtomicBool>::default();
        let (tracer, tracer_thread) =
            BankingTracer::new(Some((&path, exit.clone(), DirByteLimit::max_value()))).unwrap();
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

        // .hash_event() must succeed even after exit is already set to true
        let blockhash = Hash::from_str("B1ockhash1111111111111111111111111111111111").unwrap();
        let bank_hash = Hash::from_str("BankHash11111111111111111111111111111111111").unwrap();
        tracer.hash_event(4, &blockhash, &bank_hash);

        drop(tracer);

        // .send() must succeed even after exit is already set to true and further tracer is
        // already dropped
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
        let (tracer, tracer_thread) =
            BankingTracer::new(Some((&path, exit.clone(), DirByteLimit::max_value()))).unwrap();
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
        let blockhash = Hash::from_str("B1ockhash1111111111111111111111111111111111").unwrap();
        let bank_hash = Hash::from_str("BankHash11111111111111111111111111111111111").unwrap();
        tracer.hash_event(4, &blockhash, &bank_hash);

        for_test::terminate_tracer(
            tracer,
            tracer_thread,
            dummy_main_thread,
            non_vote_sender,
            None,
        );

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

    #[test]
    fn test_spill_over_at_rotation() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("banking-trace");
        const REALLY_SMALL_ROTATION_THRESHOLD: u64 = 1;

        let mut file_appender =
            BankingTracer::create_file_appender(&path, REALLY_SMALL_ROTATION_THRESHOLD).unwrap();
        file_appender.write_all(b"foo").unwrap();
        file_appender.condition_mut().reset();
        file_appender.write_all(b"bar").unwrap();
        file_appender.condition_mut().reset();
        file_appender.flush().unwrap();

        assert_eq!(
            [
                std::fs::read_to_string(path.join("events")).ok(),
                std::fs::read_to_string(path.join("events.1")).ok(),
                std::fs::read_to_string(path.join("events.2")).ok(),
            ],
            [Some("bar".into()), Some("foo".into()), None]
        );

        for_test::drop_and_clean_temp_dir_unless_suppressed(temp_dir);
    }

    #[test]
    fn test_reopen_with_blank_file() {
        let temp_dir = TempDir::new().unwrap();

        let path = temp_dir.path().join("banking-trace");

        let mut file_appender =
            BankingTracer::create_file_appender(&path, TRACE_FILE_DEFAULT_ROTATE_BYTE_THRESHOLD)
                .unwrap();
        // assume this is unclean write
        file_appender.write_all(b"f").unwrap();
        file_appender.flush().unwrap();

        // reopen while shadow-dropping the old tracer
        let mut file_appender =
            BankingTracer::create_file_appender(&path, TRACE_FILE_DEFAULT_ROTATE_BYTE_THRESHOLD)
                .unwrap();
        // new file won't be created as appender is lazy
        assert_eq!(
            [
                std::fs::read_to_string(path.join("events")).ok(),
                std::fs::read_to_string(path.join("events.1")).ok(),
                std::fs::read_to_string(path.join("events.2")).ok(),
            ],
            [Some("f".into()), None, None]
        );

        // initial write actually creates the new blank file
        file_appender.write_all(b"bar").unwrap();
        assert_eq!(
            [
                std::fs::read_to_string(path.join("events")).ok(),
                std::fs::read_to_string(path.join("events.1")).ok(),
                std::fs::read_to_string(path.join("events.2")).ok(),
            ],
            [Some("".into()), Some("f".into()), None]
        );

        // flush actually write the actual data
        file_appender.flush().unwrap();
        assert_eq!(
            [
                std::fs::read_to_string(path.join("events")).ok(),
                std::fs::read_to_string(path.join("events.1")).ok(),
                std::fs::read_to_string(path.join("events.2")).ok(),
            ],
            [Some("bar".into()), Some("f".into()), None]
        );

        for_test::drop_and_clean_temp_dir_unless_suppressed(temp_dir);
    }
}

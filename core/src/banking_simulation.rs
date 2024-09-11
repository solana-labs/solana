#![cfg(feature = "dev-context-only-utils")]
use {
    crate::{
        banking_stage::{BankingStage, LikeClusterInfo},
        banking_trace::{
            BankingPacketBatch, BankingTracer, ChannelLabel, TimedTracedEvent, TracedEvent,
            TracedSender, TracerThread, BANKING_TRACE_DIR_DEFAULT_BYTE_LIMIT, BASENAME,
        },
        validator::BlockProductionMethod,
    },
    bincode::deserialize_from,
    crossbeam_channel::{unbounded, Sender},
    itertools::Itertools,
    log::*,
    solana_client::connection_cache::ConnectionCache,
    solana_gossip::{
        cluster_info::{ClusterInfo, Node},
        contact_info::ContactInfo,
    },
    solana_ledger::{
        blockstore::{Blockstore, PurgeType},
        leader_schedule_cache::LeaderScheduleCache,
    },
    solana_poh::{
        poh_recorder::{PohRecorder, GRACE_TICKS_FACTOR, MAX_GRACE_SLOTS},
        poh_service::{PohService, DEFAULT_HASHES_PER_BATCH, DEFAULT_PINNED_CPU_CORE},
    },
    solana_runtime::{
        bank::{Bank, HashOverrides},
        bank_forks::BankForks,
        installed_scheduler_pool::BankWithScheduler,
        prioritization_fee_cache::PrioritizationFeeCache,
    },
    solana_sdk::{
        clock::{Slot, DEFAULT_MS_PER_SLOT, HOLD_TRANSACTIONS_SLOT_OFFSET},
        genesis_config::GenesisConfig,
        pubkey::Pubkey,
        shred_version::compute_shred_version,
        signature::Signer,
        signer::keypair::Keypair,
    },
    solana_streamer::socket::SocketAddrSpace,
    solana_turbine::broadcast_stage::{BroadcastStage, BroadcastStageType},
    std::{
        collections::BTreeMap,
        fmt::Display,
        fs::File,
        io::{self, BufRead, BufReader},
        net::{Ipv4Addr, UdpSocket},
        path::PathBuf,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
        thread::{self, sleep, JoinHandle},
        time::{Duration, SystemTime},
    },
    thiserror::Error,
};

/// This creates a simulated environment around `BankingStage` to produce leader's blocks based on
/// recorded banking trace events (`TimedTracedEvent`).
///
/// At a high level, the task of `BankingStage` is to pack transactions into assigned,
/// fixed-duration, leader blocks. So, there are 3 abstract inputs to simulate: blocks, time, and
/// transactions.
///
/// In the context of simulation, the first two are simple; both are well defined.
///
/// For ancestor blocks, we first replay a certain number of blocks immediately up to target
/// simulation leader's slot with `halt_at_slot` mechanism. Ultimately freezing the ancestor block
/// with expected and deterministic hashes. This has the added possible benefit of warming caches
/// that may be used during simulation.
///
/// After replay, a minor tweak is applied during simulation: we forcibly override leader's hashes
/// as the simulated `BankingStage` creates them, using recorded `BlockAndBankHash` events. This is
/// to provide indistinguishable sysvars to TX execution and identical TX age resolution as the
/// simulation goes on. Otherwise, the vast majority of TX processing would differ because the
/// simulated block's hashes would differ than the recorded ones as block composition difference is
/// inevitable.
///
/// As in the real environment, for PoH time we use the `PohRecorder`. This is simply a 400ms
/// timer, external to `BankingStage` and thus mostly irrelevant to `BankingStage` performance. For
/// wall time, we use the first `BankStatus::BlockAndBankHash` and `SystemTime::now()` to define
/// T=0 for simulation. Then, simulation progress is timed accordingly. For context, this syncing
/// is necessary because all trace events are recorded in UTC, not relative to poh nor to leader
/// schedule for simplicity at recording.
///
/// Lastly, the last and most complicated input to simulate: transactions.
///
/// A closer look of the transaction load profile is below, regardless of internal banking
/// implementation and simulation:
///
/// Due to solana's general tx broadcast strategy of client's submission and optional node
/// forwarding, many transactions often arrive before the first leader slot begins. Thus, the
/// initial leader block creation typically starts with rather large number of schedule-able
/// transactions. Also, note that additional transactions arrive during the 4 leader slot window
/// (roughly ~1.6 seconds).
///
/// Simulation must mimic this load pattern while being agnostic to internal banking impl as much
/// as possible. For that agnostic objective, `TracedSender`s were introduced into the `SigVerify`
/// stage and gossip subsystem by `BankingTracer` to trace **all** `BankingPacketBatch`s' exact
/// payload and _sender_'s timing with `SystemTime::now()` for all `ChannelLabel`s. This deliberate
/// tracing placement is not to be affected by any `BankingStage`'s internal capacity (if any) nor
/// by its channel consumption pattern.
///
/// BankingSimulator consists of 2 phases chronologically: warm-up and on-the-fly. The 2 phases are
/// segregated by the aforementioned T=0.
///
/// Both phases just send `BankingPacketBatch` in the same fashion, pretending to be
/// `SigVerifyStage`/gossip from a single thread to busy loop for precise T=N at ~1us granularity.
///
/// Warm-up starts at T=-WARMUP_DURATION (~ 13 secs). As soon as warm up is initiated, we invoke
/// `BankingStage::new_num_threads()` as well to simulate the pre-leader slot's tx-buffering time.
pub struct BankingSimulator {
    banking_trace_events: BankingTraceEvents,
    first_simulated_slot: Slot,
}

#[derive(Error, Debug)]
pub enum SimulateError {
    #[error("IO Error: {0}")]
    IoError(#[from] io::Error),

    #[error("Deserialization Error: {0}")]
    DeserializeError(#[from] bincode::Error),
}

// Defined to be enough to cover the holding phase prior to leader slots with some idling (+5 secs)
const WARMUP_DURATION: Duration =
    Duration::from_millis(HOLD_TRANSACTIONS_SLOT_OFFSET * DEFAULT_MS_PER_SLOT + 5000);

/// BTreeMap is intentional because events could be unordered slightly due to tracing jitter.
type PacketBatchesByTime = BTreeMap<SystemTime, (ChannelLabel, BankingPacketBatch)>;

type FreezeTimeBySlot = BTreeMap<Slot, SystemTime>;

type TimedBatchesToSend = Vec<(
    (Duration, (ChannelLabel, BankingPacketBatch)),
    (usize, usize),
)>;

type EventSenderThread = JoinHandle<(TracedSender, TracedSender, TracedSender)>;

#[derive(Default)]
pub struct BankingTraceEvents {
    packet_batches_by_time: PacketBatchesByTime,
    freeze_time_by_slot: FreezeTimeBySlot,
    hash_overrides: HashOverrides,
}

impl BankingTraceEvents {
    fn read_event_file(
        event_file_path: &PathBuf,
        mut callback: impl FnMut(TimedTracedEvent),
    ) -> Result<(), SimulateError> {
        let mut reader = BufReader::new(File::open(event_file_path)?);

        // EOF is reached at a correct deserialization boundary or just the file is just empty.
        // We want to look-ahead the buf, so NOT calling reader.consume(..) is correct.
        while !reader.fill_buf()?.is_empty() {
            callback(deserialize_from(&mut reader)?);
        }

        Ok(())
    }

    pub fn load(event_file_paths: &[PathBuf]) -> Result<Self, SimulateError> {
        let mut event_count = 0;
        let mut events = Self::default();
        for event_file_path in event_file_paths {
            let old_event_count = event_count;
            let read_result = Self::read_event_file(event_file_path, |event| {
                event_count += 1;
                events.load_event(event);
            });
            info!(
                "Read {} events from {:?}",
                event_count - old_event_count,
                event_file_path,
            );

            if matches!(
                read_result,
                Err(SimulateError::DeserializeError(ref deser_err))
                    if matches!(
                        &**deser_err,
                        bincode::ErrorKind::Io(io_err)
                            if io_err.kind() == std::io::ErrorKind::UnexpectedEof
                    )
            ) {
                // Silence errors here as this can happen under normal operation...
                warn!(
                    "Reading {:?} failed {:?} due to file corruption or unclean validator shutdown",
                    event_file_path, read_result,
                );
            } else {
                read_result?
            }
        }

        Ok(events)
    }

    fn load_event(&mut self, TimedTracedEvent(event_time, event): TimedTracedEvent) {
        match event {
            TracedEvent::PacketBatch(label, batch) => {
                // Deserialized PacketBatches will mostly be ordered by event_time, but this
                // isn't guaranteed when traced, because time are measured by multiple _sender_
                // threads without synchronization among them to avoid overhead.
                //
                // Also, there's a possibility of system clock change. In this case,
                // the simulation is meaningless, though...
                //
                // Somewhat naively assume that event_times (nanosecond resolution) won't
                // collide.
                let is_new = self
                    .packet_batches_by_time
                    .insert(event_time, (label, batch))
                    .is_none();
                assert!(is_new);
            }
            TracedEvent::BlockAndBankHash(slot, blockhash, bank_hash) => {
                let is_new = self.freeze_time_by_slot.insert(slot, event_time).is_none();
                self.hash_overrides.add_override(slot, blockhash, bank_hash);
                assert!(is_new);
            }
        }
    }

    pub fn hash_overrides(&self) -> &HashOverrides {
        &self.hash_overrides
    }
}

struct DummyClusterInfo {
    // Artificially wrap Pubkey with RwLock to induce lock contention if any to mimic the real
    // ClusterInfo
    id: RwLock<Pubkey>,
}

impl LikeClusterInfo for Arc<DummyClusterInfo> {
    fn id(&self) -> Pubkey {
        *self.id.read().unwrap()
    }

    fn lookup_contact_info<F, Y>(&self, _id: &Pubkey, _map: F) -> Option<Y>
    where
        F: FnOnce(&ContactInfo) -> Y,
    {
        None
    }
}

struct SimulatorLoopLogger {
    simulated_leader: Pubkey,
    freeze_time_by_slot: FreezeTimeBySlot,
    base_event_time: SystemTime,
    base_simulation_time: SystemTime,
}

impl SimulatorLoopLogger {
    fn bank_costs(bank: &Bank) -> (u64, u64) {
        bank.read_cost_tracker()
            .map(|t| (t.block_cost(), t.vote_cost()))
            .unwrap()
    }

    fn log_frozen_bank_cost(&self, bank: &Bank) {
        info!(
            "bank cost: slot: {} {:?} (frozen)",
            bank.slot(),
            Self::bank_costs(bank),
        );
    }

    fn log_ongoing_bank_cost(&self, bank: &Bank) {
        debug!(
            "bank cost: slot: {} {:?} (ongoing)",
            bank.slot(),
            Self::bank_costs(bank),
        );
    }

    fn log_jitter(&self, bank: &Bank) {
        let old_slot = bank.slot();
        if let Some(event_time) = self.freeze_time_by_slot.get(&old_slot) {
            if log_enabled!(log::Level::Info) {
                let current_simulation_time = SystemTime::now();
                let elapsed_simulation_time = current_simulation_time
                    .duration_since(self.base_simulation_time)
                    .unwrap();
                let elapsed_event_time = event_time.duration_since(self.base_event_time).unwrap();
                info!(
                    "jitter(parent_slot: {}): {}{:?} (sim: {:?} event: {:?})",
                    old_slot,
                    if elapsed_simulation_time > elapsed_event_time {
                        "+"
                    } else {
                        "-"
                    },
                    if elapsed_simulation_time > elapsed_event_time {
                        elapsed_simulation_time - elapsed_event_time
                    } else {
                        elapsed_event_time - elapsed_simulation_time
                    },
                    elapsed_simulation_time,
                    elapsed_event_time,
                );
            }
        }
    }

    fn on_new_leader(&self, bank: &Bank, new_slot: Slot, new_leader: Pubkey) {
        self.log_frozen_bank_cost(bank);
        info!(
            "{} isn't leader anymore at slot {}; new leader: {}",
            self.simulated_leader, new_slot, new_leader
        );
    }
}

struct SenderLoop {
    parent_slot: Slot,
    first_simulated_slot: Slot,
    non_vote_sender: TracedSender,
    tpu_vote_sender: TracedSender,
    gossip_vote_sender: TracedSender,
    exit: Arc<AtomicBool>,
    raw_base_event_time: SystemTime,
    total_batch_count: usize,
    timed_batches_to_send: TimedBatchesToSend,
}

impl SenderLoop {
    fn log_starting(&self) {
        info!(
            "simulating events: {} (out of {}), starting at slot {} (based on {} from traced event slot: {}) (warmup: -{:?})",
            self.timed_batches_to_send.len(), self.total_batch_count, self.first_simulated_slot,
            SenderLoopLogger::format_as_timestamp(self.raw_base_event_time),
            self.parent_slot, WARMUP_DURATION,
        );
    }

    fn spawn(self, base_simulation_time: SystemTime) -> Result<EventSenderThread, SimulateError> {
        let handle = thread::Builder::new()
            .name("solSimSender".into())
            .spawn(move || self.start(base_simulation_time))?;
        Ok(handle)
    }

    fn start(
        mut self,
        base_simulation_time: SystemTime,
    ) -> (TracedSender, TracedSender, TracedSender) {
        let mut logger = SenderLoopLogger::new(
            &self.non_vote_sender,
            &self.tpu_vote_sender,
            &self.gossip_vote_sender,
        );
        let mut simulation_duration = Duration::default();
        for ((required_duration, (label, batches_with_stats)), (batch_count, tx_count)) in
            self.timed_batches_to_send.drain(..)
        {
            // Busy loop for most accurate sending timings
            while simulation_duration < required_duration {
                let current_simulation_time = SystemTime::now();
                simulation_duration = current_simulation_time
                    .duration_since(base_simulation_time)
                    .unwrap();
            }

            let sender = match label {
                ChannelLabel::NonVote => &self.non_vote_sender,
                ChannelLabel::TpuVote => &self.tpu_vote_sender,
                ChannelLabel::GossipVote => &self.gossip_vote_sender,
                ChannelLabel::Dummy => unreachable!(),
            };
            sender.send(batches_with_stats).unwrap();

            logger.on_sending_batches(&simulation_duration, label, batch_count, tx_count);
            if self.exit.load(Ordering::Relaxed) {
                break;
            }
        }
        logger.on_terminating();
        drop(self.timed_batches_to_send);
        // hold these senders in join_handle to control banking stage termination!
        (
            self.non_vote_sender,
            self.tpu_vote_sender,
            self.gossip_vote_sender,
        )
    }
}

struct SimulatorLoop {
    bank: BankWithScheduler,
    parent_slot: Slot,
    first_simulated_slot: Slot,
    freeze_time_by_slot: FreezeTimeBySlot,
    base_event_time: SystemTime,
    poh_recorder: Arc<RwLock<PohRecorder>>,
    simulated_leader: Pubkey,
    bank_forks: Arc<RwLock<BankForks>>,
    blockstore: Arc<Blockstore>,
    leader_schedule_cache: Arc<LeaderScheduleCache>,
    retransmit_slots_sender: Sender<Slot>,
    retracer: Arc<BankingTracer>,
}

impl SimulatorLoop {
    fn enter(
        self,
        base_simulation_time: SystemTime,
        sender_thread: EventSenderThread,
    ) -> (EventSenderThread, Sender<Slot>) {
        sleep(WARMUP_DURATION);
        info!("warmup done!");
        self.start(base_simulation_time, sender_thread)
    }

    fn start(
        self,
        base_simulation_time: SystemTime,
        sender_thread: EventSenderThread,
    ) -> (EventSenderThread, Sender<Slot>) {
        let logger = SimulatorLoopLogger {
            simulated_leader: self.simulated_leader,
            base_event_time: self.base_event_time,
            base_simulation_time,
            freeze_time_by_slot: self.freeze_time_by_slot,
        };
        let mut bank = self.bank;
        loop {
            if self.poh_recorder.read().unwrap().bank().is_none() {
                let next_leader_slot = self.leader_schedule_cache.next_leader_slot(
                    &self.simulated_leader,
                    bank.slot(),
                    &bank,
                    Some(&self.blockstore),
                    GRACE_TICKS_FACTOR * MAX_GRACE_SLOTS,
                );
                debug!("{next_leader_slot:?}");
                self.poh_recorder
                    .write()
                    .unwrap()
                    .reset(bank.clone_without_scheduler(), next_leader_slot);
                info!("Bank::new_from_parent()!");

                logger.log_jitter(&bank);
                bank.freeze();
                let new_slot = if bank.slot() == self.parent_slot {
                    info!("initial leader block!");
                    self.first_simulated_slot
                } else {
                    info!("next leader block!");
                    bank.slot() + 1
                };
                let new_leader = self
                    .leader_schedule_cache
                    .slot_leader_at(new_slot, None)
                    .unwrap();
                if new_leader != self.simulated_leader {
                    logger.on_new_leader(&bank, new_slot, new_leader);
                    break;
                } else if sender_thread.is_finished() {
                    warn!("sender thread existed maybe due to completion of sending traced events");
                    break;
                } else {
                    info!("new leader bank slot: {new_slot}");
                }
                let new_bank = Bank::new_from_parent(
                    bank.clone_without_scheduler(),
                    &self.simulated_leader,
                    new_slot,
                );
                // make sure parent is frozen for finalized hashes via the above
                // new()-ing of its child bank
                self.retracer
                    .hash_event(bank.slot(), &bank.last_blockhash(), &bank.hash());
                if *bank.collector_id() == self.simulated_leader {
                    logger.log_frozen_bank_cost(&bank);
                }
                self.retransmit_slots_sender.send(bank.slot()).unwrap();
                self.bank_forks.write().unwrap().insert(new_bank);
                bank = self
                    .bank_forks
                    .read()
                    .unwrap()
                    .working_bank_with_scheduler()
                    .clone_with_scheduler();
                self.poh_recorder
                    .write()
                    .unwrap()
                    .set_bank(bank.clone_with_scheduler(), false);
            } else {
                logger.log_ongoing_bank_cost(&bank);
            }

            sleep(Duration::from_millis(10));
        }

        (sender_thread, self.retransmit_slots_sender)
    }
}

struct SimulatorThreads {
    poh_service: PohService,
    banking_stage: BankingStage,
    broadcast_stage: BroadcastStage,
    retracer_thread: TracerThread,
    exit: Arc<AtomicBool>,
}

impl SimulatorThreads {
    fn finish(self, sender_thread: EventSenderThread, retransmit_slots_sender: Sender<Slot>) {
        info!("Sleeping a bit before signaling exit");
        sleep(Duration::from_millis(100));
        self.exit.store(true, Ordering::Relaxed);

        // The order is important. Consuming sender_thread by joining will drop some channels. That
        // triggers termination of banking_stage, in turn retracer thread will be terminated.
        sender_thread.join().unwrap();
        self.banking_stage.join().unwrap();
        self.poh_service.join().unwrap();
        if let Some(retracer_thread) = self.retracer_thread {
            retracer_thread.join().unwrap().unwrap();
        }

        info!("Joining broadcast stage...");
        drop(retransmit_slots_sender);
        self.broadcast_stage.join().unwrap();
    }
}

struct SenderLoopLogger<'a> {
    non_vote_sender: &'a TracedSender,
    tpu_vote_sender: &'a TracedSender,
    gossip_vote_sender: &'a TracedSender,
    last_log_duration: Duration,
    last_tx_count: usize,
    last_non_vote_batch_count: usize,
    last_tpu_vote_tx_count: usize,
    last_gossip_vote_tx_count: usize,
    non_vote_batch_count: usize,
    non_vote_tx_count: usize,
    tpu_vote_batch_count: usize,
    tpu_vote_tx_count: usize,
    gossip_vote_batch_count: usize,
    gossip_vote_tx_count: usize,
}

impl<'a> SenderLoopLogger<'a> {
    fn new(
        non_vote_sender: &'a TracedSender,
        tpu_vote_sender: &'a TracedSender,
        gossip_vote_sender: &'a TracedSender,
    ) -> Self {
        Self {
            non_vote_sender,
            tpu_vote_sender,
            gossip_vote_sender,
            last_log_duration: Duration::default(),
            last_tx_count: 0,
            last_non_vote_batch_count: 0,
            last_tpu_vote_tx_count: 0,
            last_gossip_vote_tx_count: 0,
            non_vote_batch_count: 0,
            non_vote_tx_count: 0,
            tpu_vote_batch_count: 0,
            tpu_vote_tx_count: 0,
            gossip_vote_batch_count: 0,
            gossip_vote_tx_count: 0,
        }
    }

    fn on_sending_batches(
        &mut self,
        &simulation_duration: &Duration,
        label: ChannelLabel,
        batch_count: usize,
        tx_count: usize,
    ) {
        debug!(
            "sent {:?} {} batches ({} txes)",
            label, batch_count, tx_count
        );

        use ChannelLabel::*;
        let (total_batch_count, total_tx_count) = match label {
            NonVote => (&mut self.non_vote_batch_count, &mut self.non_vote_tx_count),
            TpuVote => (&mut self.tpu_vote_batch_count, &mut self.tpu_vote_tx_count),
            GossipVote => (
                &mut self.gossip_vote_batch_count,
                &mut self.gossip_vote_tx_count,
            ),
            Dummy => unreachable!(),
        };
        *total_batch_count += batch_count;
        *total_tx_count += tx_count;

        let log_interval = simulation_duration - self.last_log_duration;
        if log_interval > Duration::from_millis(100) {
            let current_tx_count =
                self.non_vote_tx_count + self.tpu_vote_tx_count + self.gossip_vote_tx_count;
            let duration = log_interval.as_secs_f64();
            let tps = (current_tx_count - self.last_tx_count) as f64 / duration;
            let non_vote_tps =
                (self.non_vote_tx_count - self.last_non_vote_batch_count) as f64 / duration;
            let tpu_vote_tps =
                (self.tpu_vote_tx_count - self.last_tpu_vote_tx_count) as f64 / duration;
            let gossip_vote_tps =
                (self.gossip_vote_tx_count - self.last_gossip_vote_tx_count) as f64 / duration;
            info!(
                "senders(non-,tpu-,gossip-vote): tps: {:.0} (={:.0}+{:.0}+{:.0}) over {:?} not-recved: ({}+{}+{})",
                tps, non_vote_tps, tpu_vote_tps, gossip_vote_tps, log_interval,
                self.non_vote_sender.len(), self.tpu_vote_sender.len(), self.gossip_vote_sender.len(),
            );
            self.last_log_duration = simulation_duration;
            self.last_tx_count = current_tx_count;
            (
                self.last_non_vote_batch_count,
                self.last_tpu_vote_tx_count,
                self.last_gossip_vote_tx_count,
            ) = (
                self.non_vote_tx_count,
                self.tpu_vote_tx_count,
                self.gossip_vote_batch_count,
            );
        }
    }

    fn on_terminating(self) {
        info!(
            "terminating to send...: non_vote: {} ({}), tpu_vote: {} ({}), gossip_vote: {} ({})",
            self.non_vote_batch_count,
            self.non_vote_tx_count,
            self.tpu_vote_batch_count,
            self.tpu_vote_tx_count,
            self.gossip_vote_batch_count,
            self.gossip_vote_tx_count,
        );
    }

    fn format_as_timestamp(time: SystemTime) -> impl Display {
        let time: chrono::DateTime<chrono::Utc> = time.into();
        time.format("%Y-%m-%d %H:%M:%S.%f")
    }
}

impl BankingSimulator {
    pub fn new(banking_trace_events: BankingTraceEvents, first_simulated_slot: Slot) -> Self {
        Self {
            banking_trace_events,
            first_simulated_slot,
        }
    }

    pub fn parent_slot(&self) -> Option<Slot> {
        self.banking_trace_events
            .freeze_time_by_slot
            .range(..self.first_simulated_slot)
            .last()
            .map(|(slot, _time)| slot)
            .copied()
    }

    fn prepare_simulation(
        self,
        genesis_config: GenesisConfig,
        bank_forks: Arc<RwLock<BankForks>>,
        blockstore: Arc<Blockstore>,
        block_production_method: BlockProductionMethod,
    ) -> (SenderLoop, SimulatorLoop, SimulatorThreads) {
        let parent_slot = self.parent_slot().unwrap();
        let mut packet_batches_by_time = self.banking_trace_events.packet_batches_by_time;
        let freeze_time_by_slot = self.banking_trace_events.freeze_time_by_slot;
        let bank = bank_forks
            .read()
            .unwrap()
            .working_bank_with_scheduler()
            .clone_with_scheduler();

        let leader_schedule_cache = Arc::new(LeaderScheduleCache::new_from_bank(&bank));
        assert_eq!(parent_slot, bank.slot());

        let simulated_leader = leader_schedule_cache
            .slot_leader_at(self.first_simulated_slot, None)
            .unwrap();
        info!(
            "Simulated leader and slot: {}, {}",
            simulated_leader, self.first_simulated_slot,
        );

        let exit = Arc::new(AtomicBool::default());

        if let Some(end_slot) = blockstore
            .slot_meta_iterator(self.first_simulated_slot)
            .unwrap()
            .map(|(s, _)| s)
            .last()
        {
            info!("purging slots {}, {}", self.first_simulated_slot, end_slot);
            blockstore.purge_from_next_slots(self.first_simulated_slot, end_slot);
            blockstore.purge_slots(self.first_simulated_slot, end_slot, PurgeType::Exact);
            info!("done: purging");
        } else {
            info!("skipping purging...");
        }

        info!("Poh is starting!");

        let (poh_recorder, entry_receiver, record_receiver) = PohRecorder::new_with_clear_signal(
            bank.tick_height(),
            bank.last_blockhash(),
            bank.clone(),
            None,
            bank.ticks_per_slot(),
            false,
            blockstore.clone(),
            blockstore.get_new_shred_signal(0),
            &leader_schedule_cache,
            &genesis_config.poh_config,
            None,
            exit.clone(),
        );
        let poh_recorder = Arc::new(RwLock::new(poh_recorder));
        let poh_service = PohService::new(
            poh_recorder.clone(),
            &genesis_config.poh_config,
            exit.clone(),
            bank.ticks_per_slot(),
            DEFAULT_PINNED_CPU_CORE,
            DEFAULT_HASHES_PER_BATCH,
            record_receiver,
        );

        // Enable BankingTracer to approximate the real environment as close as possible because
        // it's not expected to disable BankingTracer on production environments.
        //
        // It's not likely for it to affect the banking stage performance noticeably. So, make sure
        // that assumption is held here. That said, it incurs additional channel sending,
        // SystemTime::now() and buffered seq IO, and indirectly functions as a background dropper
        // of `BankingPacketBatch`.
        //
        // Lastly, the actual retraced events can be used to evaluate simulation timing accuracy in
        // the future.
        let (retracer, retracer_thread) = BankingTracer::new(Some((
            &blockstore.banking_retracer_path(),
            exit.clone(),
            BANKING_TRACE_DIR_DEFAULT_BYTE_LIMIT,
        )))
        .unwrap();
        assert!(retracer.is_enabled());
        info!(
            "Enabled banking retracer (dir_byte_limit: {})",
            BANKING_TRACE_DIR_DEFAULT_BYTE_LIMIT,
        );

        let (non_vote_sender, non_vote_receiver) = retracer.create_channel_non_vote();
        let (tpu_vote_sender, tpu_vote_receiver) = retracer.create_channel_tpu_vote();
        let (gossip_vote_sender, gossip_vote_receiver) = retracer.create_channel_gossip_vote();

        let connection_cache = Arc::new(ConnectionCache::new("connection_cache_sim"));
        let (replay_vote_sender, _replay_vote_receiver) = unbounded();
        let (retransmit_slots_sender, retransmit_slots_receiver) = unbounded();
        let shred_version = compute_shred_version(
            &genesis_config.hash(),
            Some(&bank_forks.read().unwrap().root_bank().hard_forks()),
        );
        let (sender, _receiver) = tokio::sync::mpsc::channel(1);

        // Create a completely-dummy ClusterInfo for the broadcast stage.
        // We only need it to write shreds into the blockstore and it seems given ClusterInfo is
        // irrelevant for the neccesary minimum work for this simulation.
        let random_keypair = Arc::new(Keypair::new());
        let cluster_info = Arc::new(ClusterInfo::new(
            Node::new_localhost_with_pubkey(&random_keypair.pubkey()).info,
            random_keypair,
            SocketAddrSpace::Unspecified,
        ));
        // Broadcast stage is needed to save the simulated blocks for post-run analysis by
        // inserting produced shreds into the blockstore.
        let broadcast_stage = BroadcastStageType::Standard.new_broadcast_stage(
            vec![UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap()],
            cluster_info.clone(),
            entry_receiver,
            retransmit_slots_receiver,
            exit.clone(),
            blockstore.clone(),
            bank_forks.clone(),
            shred_version,
            sender,
        );

        info!("Start banking stage!...");
        // Create a partially-dummy ClusterInfo for the banking stage.
        let cluster_info = Arc::new(DummyClusterInfo {
            id: simulated_leader.into(),
        });
        let prioritization_fee_cache = &Arc::new(PrioritizationFeeCache::new(0u64));
        let banking_stage = BankingStage::new_num_threads(
            block_production_method.clone(),
            &cluster_info,
            &poh_recorder,
            non_vote_receiver,
            tpu_vote_receiver,
            gossip_vote_receiver,
            BankingStage::num_threads(),
            None,
            replay_vote_sender,
            None,
            connection_cache,
            bank_forks.clone(),
            prioritization_fee_cache,
            false,
        );

        let (&_slot, &raw_base_event_time) = freeze_time_by_slot
            .range(parent_slot..)
            .next()
            .expect("timed hashes");
        let base_event_time = raw_base_event_time - WARMUP_DURATION;

        let total_batch_count = packet_batches_by_time.len();
        let timed_batches_to_send = packet_batches_by_time.split_off(&base_event_time);
        let batch_and_tx_counts = timed_batches_to_send
            .values()
            .map(|(_label, batches_with_stats)| {
                let batches = &batches_with_stats.0;
                (
                    batches.len(),
                    batches.iter().map(|batch| batch.len()).sum::<usize>(),
                )
            })
            .collect::<Vec<_>>();
        // Convert to a large plain old Vec and drain on it, finally dropping it outside
        // the simulation loop to avoid jitter due to interleaved deallocs of BTreeMap.
        let timed_batches_to_send = timed_batches_to_send
            .into_iter()
            .map(|(event_time, batches)| {
                (event_time.duration_since(base_event_time).unwrap(), batches)
            })
            .zip_eq(batch_and_tx_counts)
            .collect::<Vec<_>>();

        let sender_loop = SenderLoop {
            parent_slot,
            first_simulated_slot: self.first_simulated_slot,
            non_vote_sender,
            tpu_vote_sender,
            gossip_vote_sender,
            exit: exit.clone(),
            raw_base_event_time,
            total_batch_count,
            timed_batches_to_send,
        };

        let simulator_loop = SimulatorLoop {
            bank,
            parent_slot,
            first_simulated_slot: self.first_simulated_slot,
            freeze_time_by_slot,
            base_event_time,
            poh_recorder,
            simulated_leader,
            bank_forks,
            blockstore,
            leader_schedule_cache,
            retransmit_slots_sender,
            retracer,
        };

        let simulator_threads = SimulatorThreads {
            poh_service,
            banking_stage,
            broadcast_stage,
            retracer_thread,
            exit,
        };

        (sender_loop, simulator_loop, simulator_threads)
    }

    pub fn start(
        self,
        genesis_config: GenesisConfig,
        bank_forks: Arc<RwLock<BankForks>>,
        blockstore: Arc<Blockstore>,
        block_production_method: BlockProductionMethod,
    ) -> Result<(), SimulateError> {
        let (sender_loop, simulator_loop, simulator_threads) = self.prepare_simulation(
            genesis_config,
            bank_forks,
            blockstore,
            block_production_method,
        );

        sender_loop.log_starting();
        let base_simulation_time = SystemTime::now();
        // Spawning and entering these two loops must be done at the same time as they're timed.
        // So, all the mundane setup must be done in advance.
        let sender_thread = sender_loop.spawn(base_simulation_time)?;
        let (sender_thread, retransmit_slots_sender) =
            simulator_loop.enter(base_simulation_time, sender_thread);

        simulator_threads.finish(sender_thread, retransmit_slots_sender);

        Ok(())
    }

    pub fn event_file_name(index: usize) -> String {
        if index == 0 {
            BASENAME.to_string()
        } else {
            format!("{BASENAME}.{index}")
        }
    }
}

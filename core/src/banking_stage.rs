//! The `banking_stage` processes Transaction messages. It is intended to be used
//! to construct a software pipeline. The stage uses all available CPU cores and
//! can do its processing in parallel with signature verification on the GPU.

use {
    self::{
        committer::Committer,
        consumer::Consumer,
        decision_maker::{BufferedPacketsDecision, DecisionMaker},
        forwarder::Forwarder,
        latest_unprocessed_votes::{LatestUnprocessedVotes, VoteSource},
        leader_slot_metrics::LeaderSlotMetricsTracker,
        packet_receiver::PacketReceiver,
        qos_service::QosService,
        unprocessed_packet_batches::*,
        unprocessed_transaction_storage::{ThreadType, UnprocessedTransactionStorage},
    },
    crate::{
        banking_trace::BankingPacketReceiver, tracer_packet_stats::TracerPacketStats,
        validator::BlockProductionMethod,
    },
    crossbeam_channel::RecvTimeoutError,
    histogram::Histogram,
    solana_client::connection_cache::ConnectionCache,
    solana_gossip::cluster_info::ClusterInfo,
    solana_ledger::blockstore_processor::TransactionStatusSender,
    solana_measure::{measure, measure_us},
    solana_perf::{data_budget::DataBudget, packet::PACKETS_PER_BATCH},
    solana_poh::poh_recorder::PohRecorder,
    solana_runtime::{bank_forks::BankForks, prioritization_fee_cache::PrioritizationFeeCache},
    solana_sdk::timing::AtomicInterval,
    solana_vote::vote_sender_types::ReplayVoteSender,
    std::{
        cmp, env,
        sync::{
            atomic::{AtomicU64, AtomicUsize, Ordering},
            Arc, RwLock,
        },
        thread::{self, Builder, JoinHandle},
        time::{Duration, Instant},
    },
};

// Below modules are pub to allow use by banking_stage bench
pub mod committer;
pub mod consumer;
pub mod leader_slot_metrics;
pub mod qos_service;
pub mod unprocessed_packet_batches;
pub mod unprocessed_transaction_storage;

mod consume_worker;
mod decision_maker;
mod forward_packet_batches_by_accounts;
mod forward_worker;
mod forwarder;
mod immutable_deserialized_packet;
mod latest_unprocessed_votes;
mod leader_slot_timing_metrics;
mod multi_iterator_scanner;
mod packet_deserializer;
mod packet_receiver;
mod read_write_account_set;
#[allow(dead_code)]
mod scheduler_messages;
mod transaction_scheduler;

// Fixed thread size seems to be fastest on GCP setup
pub const NUM_THREADS: u32 = 6;

const TOTAL_BUFFERED_PACKETS: usize = 700_000;

const NUM_VOTE_PROCESSING_THREADS: u32 = 2;
const MIN_THREADS_BANKING: u32 = 1;
const MIN_TOTAL_THREADS: u32 = NUM_VOTE_PROCESSING_THREADS + MIN_THREADS_BANKING;

const SLOT_BOUNDARY_CHECK_PERIOD: Duration = Duration::from_millis(10);

#[derive(Debug, Default)]
pub struct BankingStageStats {
    last_report: AtomicInterval,
    id: u32,
    receive_and_buffer_packets_count: AtomicUsize,
    dropped_packets_count: AtomicUsize,
    pub(crate) dropped_duplicated_packets_count: AtomicUsize,
    dropped_forward_packets_count: AtomicUsize,
    newly_buffered_packets_count: AtomicUsize,
    newly_buffered_forwarded_packets_count: AtomicUsize,
    current_buffered_packets_count: AtomicUsize,
    rebuffered_packets_count: AtomicUsize,
    consumed_buffered_packets_count: AtomicUsize,
    forwarded_transaction_count: AtomicUsize,
    forwarded_vote_count: AtomicUsize,
    batch_packet_indexes_len: Histogram,

    // Timing
    consume_buffered_packets_elapsed: AtomicU64,
    receive_and_buffer_packets_elapsed: AtomicU64,
    filter_pending_packets_elapsed: AtomicU64,
    pub(crate) packet_conversion_elapsed: AtomicU64,
    transaction_processing_elapsed: AtomicU64,
}

impl BankingStageStats {
    pub fn new(id: u32) -> Self {
        BankingStageStats {
            id,
            batch_packet_indexes_len: Histogram::configure()
                .max_value(PACKETS_PER_BATCH as u64)
                .build()
                .unwrap(),
            ..BankingStageStats::default()
        }
    }

    fn is_empty(&self) -> bool {
        0 == self
            .receive_and_buffer_packets_count
            .load(Ordering::Relaxed) as u64
            + self.dropped_packets_count.load(Ordering::Relaxed) as u64
            + self
                .dropped_duplicated_packets_count
                .load(Ordering::Relaxed) as u64
            + self.dropped_forward_packets_count.load(Ordering::Relaxed) as u64
            + self.newly_buffered_packets_count.load(Ordering::Relaxed) as u64
            + self.current_buffered_packets_count.load(Ordering::Relaxed) as u64
            + self.rebuffered_packets_count.load(Ordering::Relaxed) as u64
            + self.consumed_buffered_packets_count.load(Ordering::Relaxed) as u64
            + self
                .consume_buffered_packets_elapsed
                .load(Ordering::Relaxed)
            + self
                .receive_and_buffer_packets_elapsed
                .load(Ordering::Relaxed)
            + self.filter_pending_packets_elapsed.load(Ordering::Relaxed)
            + self.packet_conversion_elapsed.load(Ordering::Relaxed)
            + self.transaction_processing_elapsed.load(Ordering::Relaxed)
            + self.forwarded_transaction_count.load(Ordering::Relaxed) as u64
            + self.forwarded_vote_count.load(Ordering::Relaxed) as u64
            + self.batch_packet_indexes_len.entries()
    }

    fn report(&mut self, report_interval_ms: u64) {
        // skip reporting metrics if stats is empty
        if self.is_empty() {
            return;
        }
        if self.last_report.should_update(report_interval_ms) {
            datapoint_info!(
                "banking_stage-loop-stats",
                ("id", self.id, i64),
                (
                    "receive_and_buffer_packets_count",
                    self.receive_and_buffer_packets_count
                        .swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "dropped_packets_count",
                    self.dropped_packets_count.swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "dropped_duplicated_packets_count",
                    self.dropped_duplicated_packets_count
                        .swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "dropped_forward_packets_count",
                    self.dropped_forward_packets_count
                        .swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "newly_buffered_packets_count",
                    self.newly_buffered_packets_count.swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "newly_buffered_forwarded_packets_count",
                    self.newly_buffered_forwarded_packets_count
                        .swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "current_buffered_packets_count",
                    self.current_buffered_packets_count
                        .swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "rebuffered_packets_count",
                    self.rebuffered_packets_count.swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "consumed_buffered_packets_count",
                    self.consumed_buffered_packets_count
                        .swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "forwarded_transaction_count",
                    self.forwarded_transaction_count.swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "forwarded_vote_count",
                    self.forwarded_vote_count.swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "consume_buffered_packets_elapsed",
                    self.consume_buffered_packets_elapsed
                        .swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "receive_and_buffer_packets_elapsed",
                    self.receive_and_buffer_packets_elapsed
                        .swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "filter_pending_packets_elapsed",
                    self.filter_pending_packets_elapsed
                        .swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "packet_conversion_elapsed",
                    self.packet_conversion_elapsed.swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "transaction_processing_elapsed",
                    self.transaction_processing_elapsed
                        .swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "packet_batch_indices_len_min",
                    self.batch_packet_indexes_len.minimum().unwrap_or(0),
                    i64
                ),
                (
                    "packet_batch_indices_len_max",
                    self.batch_packet_indexes_len.maximum().unwrap_or(0),
                    i64
                ),
                (
                    "packet_batch_indices_len_mean",
                    self.batch_packet_indexes_len.mean().unwrap_or(0),
                    i64
                ),
                (
                    "packet_batch_indices_len_90pct",
                    self.batch_packet_indexes_len.percentile(90.0).unwrap_or(0),
                    i64
                )
            );
            self.batch_packet_indexes_len.clear();
        }
    }
}

#[derive(Debug, Default)]
pub struct BatchedTransactionDetails {
    pub costs: BatchedTransactionCostDetails,
    pub errors: BatchedTransactionErrorDetails,
}

#[derive(Debug, Default)]
pub struct BatchedTransactionCostDetails {
    pub batched_signature_cost: u64,
    pub batched_write_lock_cost: u64,
    pub batched_data_bytes_cost: u64,
    pub batched_builtins_execute_cost: u64,
    pub batched_bpf_execute_cost: u64,
}

#[derive(Debug, Default)]
pub struct BatchedTransactionErrorDetails {
    pub batched_retried_txs_per_block_limit_count: u64,
    pub batched_retried_txs_per_vote_limit_count: u64,
    pub batched_retried_txs_per_account_limit_count: u64,
    pub batched_retried_txs_per_account_data_block_limit_count: u64,
    pub batched_dropped_txs_per_account_data_total_limit_count: u64,
}

/// Stores the stage's thread handle and output receiver.
pub struct BankingStage {
    bank_thread_hdls: Vec<JoinHandle<()>>,
}

#[derive(Debug, Clone)]
pub enum ForwardOption {
    NotForward,
    ForwardTpuVote,
    ForwardTransaction,
}

#[derive(Debug, Default)]
pub struct FilterForwardingResults {
    pub(crate) total_forwardable_packets: usize,
    pub(crate) total_tracer_packets_in_buffer: usize,
    pub(crate) total_forwardable_tracer_packets: usize,
    pub(crate) total_dropped_packets: usize,
    pub(crate) total_packet_conversion_us: u64,
    pub(crate) total_filter_packets_us: u64,
}

impl BankingStage {
    /// Create the stage using `bank`. Exit when `verified_receiver` is dropped.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        block_production_method: BlockProductionMethod,
        cluster_info: &Arc<ClusterInfo>,
        poh_recorder: &Arc<RwLock<PohRecorder>>,
        non_vote_receiver: BankingPacketReceiver,
        tpu_vote_receiver: BankingPacketReceiver,
        gossip_vote_receiver: BankingPacketReceiver,
        transaction_status_sender: Option<TransactionStatusSender>,
        replay_vote_sender: ReplayVoteSender,
        log_messages_bytes_limit: Option<usize>,
        connection_cache: Arc<ConnectionCache>,
        bank_forks: Arc<RwLock<BankForks>>,
        prioritization_fee_cache: &Arc<PrioritizationFeeCache>,
    ) -> Self {
        Self::new_num_threads(
            block_production_method,
            cluster_info,
            poh_recorder,
            non_vote_receiver,
            tpu_vote_receiver,
            gossip_vote_receiver,
            Self::num_threads(),
            transaction_status_sender,
            replay_vote_sender,
            log_messages_bytes_limit,
            connection_cache,
            bank_forks,
            prioritization_fee_cache,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_num_threads(
        block_production_method: BlockProductionMethod,
        cluster_info: &Arc<ClusterInfo>,
        poh_recorder: &Arc<RwLock<PohRecorder>>,
        non_vote_receiver: BankingPacketReceiver,
        tpu_vote_receiver: BankingPacketReceiver,
        gossip_vote_receiver: BankingPacketReceiver,
        num_threads: u32,
        transaction_status_sender: Option<TransactionStatusSender>,
        replay_vote_sender: ReplayVoteSender,
        log_messages_bytes_limit: Option<usize>,
        connection_cache: Arc<ConnectionCache>,
        bank_forks: Arc<RwLock<BankForks>>,
        prioritization_fee_cache: &Arc<PrioritizationFeeCache>,
    ) -> Self {
        match block_production_method {
            BlockProductionMethod::ThreadLocalMultiIterator => {
                Self::new_thread_local_multi_iterator(
                    cluster_info,
                    poh_recorder,
                    non_vote_receiver,
                    tpu_vote_receiver,
                    gossip_vote_receiver,
                    num_threads,
                    transaction_status_sender,
                    replay_vote_sender,
                    log_messages_bytes_limit,
                    connection_cache,
                    bank_forks,
                    prioritization_fee_cache,
                )
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_thread_local_multi_iterator(
        cluster_info: &Arc<ClusterInfo>,
        poh_recorder: &Arc<RwLock<PohRecorder>>,
        non_vote_receiver: BankingPacketReceiver,
        tpu_vote_receiver: BankingPacketReceiver,
        gossip_vote_receiver: BankingPacketReceiver,
        num_threads: u32,
        transaction_status_sender: Option<TransactionStatusSender>,
        replay_vote_sender: ReplayVoteSender,
        log_messages_bytes_limit: Option<usize>,
        connection_cache: Arc<ConnectionCache>,
        bank_forks: Arc<RwLock<BankForks>>,
        prioritization_fee_cache: &Arc<PrioritizationFeeCache>,
    ) -> Self {
        assert!(num_threads >= MIN_TOTAL_THREADS);
        // Single thread to generate entries from many banks.
        // This thread talks to poh_service and broadcasts the entries once they have been recorded.
        // Once an entry has been recorded, its blockhash is registered with the bank.
        let data_budget = Arc::new(DataBudget::default());
        let batch_limit =
            TOTAL_BUFFERED_PACKETS / ((num_threads - NUM_VOTE_PROCESSING_THREADS) as usize);
        // Keeps track of extraneous vote transactions for the vote threads
        let latest_unprocessed_votes = Arc::new(LatestUnprocessedVotes::new());
        // Many banks that process transactions in parallel.
        let bank_thread_hdls: Vec<JoinHandle<()>> = (0..num_threads)
            .map(|id| {
                let (packet_receiver, unprocessed_transaction_storage) = match id {
                    0 => (
                        gossip_vote_receiver.clone(),
                        UnprocessedTransactionStorage::new_vote_storage(
                            latest_unprocessed_votes.clone(),
                            VoteSource::Gossip,
                        ),
                    ),
                    1 => (
                        tpu_vote_receiver.clone(),
                        UnprocessedTransactionStorage::new_vote_storage(
                            latest_unprocessed_votes.clone(),
                            VoteSource::Tpu,
                        ),
                    ),
                    _ => (
                        non_vote_receiver.clone(),
                        UnprocessedTransactionStorage::new_transaction_storage(
                            UnprocessedPacketBatches::with_capacity(batch_limit),
                            ThreadType::Transactions,
                        ),
                    ),
                };

                let mut packet_receiver =
                    PacketReceiver::new(id, packet_receiver, bank_forks.clone());
                let poh_recorder = poh_recorder.clone();

                let committer = Committer::new(
                    transaction_status_sender.clone(),
                    replay_vote_sender.clone(),
                    prioritization_fee_cache.clone(),
                );
                let decision_maker = DecisionMaker::new(cluster_info.id(), poh_recorder.clone());
                let forwarder = Forwarder::new(
                    poh_recorder.clone(),
                    bank_forks.clone(),
                    cluster_info.clone(),
                    connection_cache.clone(),
                    data_budget.clone(),
                );
                let consumer = Consumer::new(
                    committer,
                    poh_recorder.read().unwrap().new_recorder(),
                    QosService::new(id),
                    log_messages_bytes_limit,
                );

                Builder::new()
                    .name(format!("solBanknStgTx{id:02}"))
                    .spawn(move || {
                        Self::process_loop(
                            &mut packet_receiver,
                            &decision_maker,
                            &forwarder,
                            &consumer,
                            id,
                            unprocessed_transaction_storage,
                        );
                    })
                    .unwrap()
            })
            .collect();
        Self { bank_thread_hdls }
    }

    #[allow(clippy::too_many_arguments)]
    fn process_buffered_packets(
        decision_maker: &DecisionMaker,
        forwarder: &Forwarder,
        consumer: &Consumer,
        unprocessed_transaction_storage: &mut UnprocessedTransactionStorage,
        banking_stage_stats: &BankingStageStats,
        slot_metrics_tracker: &mut LeaderSlotMetricsTracker,
        tracer_packet_stats: &mut TracerPacketStats,
    ) {
        if unprocessed_transaction_storage.should_not_process() {
            return;
        }
        let (decision, make_decision_time) =
            measure!(decision_maker.make_consume_or_forward_decision());
        let metrics_action = slot_metrics_tracker.check_leader_slot_boundary(decision.bank_start());
        slot_metrics_tracker.increment_make_decision_us(make_decision_time.as_us());

        match decision {
            BufferedPacketsDecision::Consume(bank_start) => {
                // Take metrics action before consume packets (potentially resetting the
                // slot metrics tracker to the next slot) so that we don't count the
                // packet processing metrics from the next slot towards the metrics
                // of the previous slot
                slot_metrics_tracker.apply_action(metrics_action);
                let (_, consume_buffered_packets_time) = measure!(
                    consumer.consume_buffered_packets(
                        &bank_start,
                        unprocessed_transaction_storage,
                        banking_stage_stats,
                        slot_metrics_tracker,
                    ),
                    "consume_buffered_packets",
                );
                slot_metrics_tracker
                    .increment_consume_buffered_packets_us(consume_buffered_packets_time.as_us());
            }
            BufferedPacketsDecision::Forward => {
                let ((), forward_us) = measure_us!(forwarder.handle_forwarding(
                    unprocessed_transaction_storage,
                    false,
                    slot_metrics_tracker,
                    banking_stage_stats,
                    tracer_packet_stats,
                ));
                slot_metrics_tracker.increment_forward_us(forward_us);
                // Take metrics action after forwarding packets to include forwarded
                // metrics into current slot
                slot_metrics_tracker.apply_action(metrics_action);
            }
            BufferedPacketsDecision::ForwardAndHold => {
                let ((), forward_and_hold_us) = measure_us!(forwarder.handle_forwarding(
                    unprocessed_transaction_storage,
                    true,
                    slot_metrics_tracker,
                    banking_stage_stats,
                    tracer_packet_stats,
                ));
                slot_metrics_tracker.increment_forward_and_hold_us(forward_and_hold_us);
                // Take metrics action after forwarding packets
                slot_metrics_tracker.apply_action(metrics_action);
            }
            _ => (),
        }
    }

    fn process_loop(
        packet_receiver: &mut PacketReceiver,
        decision_maker: &DecisionMaker,
        forwarder: &Forwarder,
        consumer: &Consumer,
        id: u32,
        mut unprocessed_transaction_storage: UnprocessedTransactionStorage,
    ) {
        let mut banking_stage_stats = BankingStageStats::new(id);
        let mut tracer_packet_stats = TracerPacketStats::new(id);

        let mut slot_metrics_tracker = LeaderSlotMetricsTracker::new(id);
        let mut last_metrics_update = Instant::now();

        loop {
            if !unprocessed_transaction_storage.is_empty()
                || last_metrics_update.elapsed() >= SLOT_BOUNDARY_CHECK_PERIOD
            {
                let (_, process_buffered_packets_time) = measure!(
                    Self::process_buffered_packets(
                        decision_maker,
                        forwarder,
                        consumer,
                        &mut unprocessed_transaction_storage,
                        &banking_stage_stats,
                        &mut slot_metrics_tracker,
                        &mut tracer_packet_stats,
                    ),
                    "process_buffered_packets",
                );
                slot_metrics_tracker
                    .increment_process_buffered_packets_us(process_buffered_packets_time.as_us());
                last_metrics_update = Instant::now();
            }

            tracer_packet_stats.report(1000);

            match packet_receiver.receive_and_buffer_packets(
                &mut unprocessed_transaction_storage,
                &mut banking_stage_stats,
                &mut tracer_packet_stats,
                &mut slot_metrics_tracker,
            ) {
                Ok(()) | Err(RecvTimeoutError::Timeout) => (),
                Err(RecvTimeoutError::Disconnected) => break,
            }
            banking_stage_stats.report(1000);
        }
    }

    pub fn num_threads() -> u32 {
        cmp::max(
            env::var("SOLANA_BANKING_THREADS")
                .map(|x| x.parse().unwrap_or(NUM_THREADS))
                .unwrap_or(NUM_THREADS),
            MIN_TOTAL_THREADS,
        )
    }

    pub fn join(self) -> thread::Result<()> {
        for bank_thread_hdl in self.bank_thread_hdls {
            bank_thread_hdl.join()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::banking_trace::{BankingPacketBatch, BankingTracer},
        crossbeam_channel::{unbounded, Receiver},
        itertools::Itertools,
        solana_entry::entry::{Entry, EntrySlice},
        solana_gossip::cluster_info::Node,
        solana_ledger::{
            blockstore::Blockstore,
            genesis_utils::{
                create_genesis_config, create_genesis_config_with_leader, GenesisConfigInfo,
            },
            get_tmp_ledger_path_auto_delete,
            leader_schedule_cache::LeaderScheduleCache,
        },
        solana_perf::packet::{to_packet_batches, PacketBatch},
        solana_poh::{
            poh_recorder::{
                create_test_recorder, PohRecorderError, Record, RecordTransactionsSummary,
            },
            poh_service::PohService,
        },
        solana_runtime::{
            bank::Bank, bank_forks::BankForks, genesis_utils::bootstrap_validator_stake_lamports,
        },
        solana_sdk::{
            hash::Hash,
            poh_config::PohConfig,
            pubkey::Pubkey,
            signature::{Keypair, Signer},
            system_transaction,
            transaction::{SanitizedTransaction, Transaction},
        },
        solana_streamer::socket::SocketAddrSpace,
        solana_vote_program::{
            vote_state::VoteStateUpdate, vote_transaction::new_vote_state_update_transaction,
        },
        std::{
            sync::atomic::{AtomicBool, Ordering},
            thread::sleep,
        },
    };

    pub(crate) fn new_test_cluster_info(keypair: Option<Arc<Keypair>>) -> (Node, ClusterInfo) {
        let keypair = keypair.unwrap_or_else(|| Arc::new(Keypair::new()));
        let node = Node::new_localhost_with_pubkey(&keypair.pubkey());
        let cluster_info =
            ClusterInfo::new(node.info.clone(), keypair, SocketAddrSpace::Unspecified);
        (node, cluster_info)
    }

    pub(crate) fn sanitize_transactions(txs: Vec<Transaction>) -> Vec<SanitizedTransaction> {
        txs.into_iter()
            .map(SanitizedTransaction::from_transaction_for_tests)
            .collect()
    }

    #[test]
    fn test_banking_stage_shutdown1() {
        let genesis_config = create_genesis_config(2).genesis_config;
        let bank = Bank::new_no_wallclock_throttle_for_tests(&genesis_config);
        let bank_forks = BankForks::new_rw_arc(bank);
        let bank = bank_forks.read().unwrap().get(0).unwrap();
        let banking_tracer = BankingTracer::new_disabled();
        let (non_vote_sender, non_vote_receiver) = banking_tracer.create_channel_non_vote();
        let (tpu_vote_sender, tpu_vote_receiver) = banking_tracer.create_channel_tpu_vote();
        let (gossip_vote_sender, gossip_vote_receiver) =
            banking_tracer.create_channel_gossip_vote();
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        {
            let blockstore = Arc::new(
                Blockstore::open(ledger_path.path())
                    .expect("Expected to be able to open database ledger"),
            );
            let (exit, poh_recorder, poh_service, _entry_receiever) =
                create_test_recorder(bank, blockstore, None, None);
            let (_, cluster_info) = new_test_cluster_info(/*keypair:*/ None);
            let cluster_info = Arc::new(cluster_info);
            let (replay_vote_sender, _replay_vote_receiver) = unbounded();

            let banking_stage = BankingStage::new(
                BlockProductionMethod::ThreadLocalMultiIterator,
                &cluster_info,
                &poh_recorder,
                non_vote_receiver,
                tpu_vote_receiver,
                gossip_vote_receiver,
                None,
                replay_vote_sender,
                None,
                Arc::new(ConnectionCache::new("connection_cache_test")),
                bank_forks,
                &Arc::new(PrioritizationFeeCache::new(0u64)),
            );
            drop(non_vote_sender);
            drop(tpu_vote_sender);
            drop(gossip_vote_sender);
            exit.store(true, Ordering::Relaxed);
            banking_stage.join().unwrap();
            poh_service.join().unwrap();
        }
        Blockstore::destroy(ledger_path.path()).unwrap();
    }

    #[test]
    fn test_banking_stage_tick() {
        solana_logger::setup();
        let GenesisConfigInfo {
            mut genesis_config, ..
        } = create_genesis_config(2);
        genesis_config.ticks_per_slot = 4;
        let num_extra_ticks = 2;
        let bank = Bank::new_no_wallclock_throttle_for_tests(&genesis_config);
        let bank_forks = BankForks::new_rw_arc(bank);
        let bank = bank_forks.read().unwrap().get(0).unwrap();
        let start_hash = bank.last_blockhash();
        let banking_tracer = BankingTracer::new_disabled();
        let (non_vote_sender, non_vote_receiver) = banking_tracer.create_channel_non_vote();
        let (tpu_vote_sender, tpu_vote_receiver) = banking_tracer.create_channel_tpu_vote();
        let (gossip_vote_sender, gossip_vote_receiver) =
            banking_tracer.create_channel_gossip_vote();
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        {
            let blockstore = Arc::new(
                Blockstore::open(ledger_path.path())
                    .expect("Expected to be able to open database ledger"),
            );
            let poh_config = PohConfig {
                target_tick_count: Some(bank.max_tick_height() + num_extra_ticks),
                ..PohConfig::default()
            };
            let (exit, poh_recorder, poh_service, entry_receiver) =
                create_test_recorder(bank.clone(), blockstore, Some(poh_config), None);
            let (_, cluster_info) = new_test_cluster_info(/*keypair:*/ None);
            let cluster_info = Arc::new(cluster_info);
            let (replay_vote_sender, _replay_vote_receiver) = unbounded();

            let banking_stage = BankingStage::new(
                BlockProductionMethod::ThreadLocalMultiIterator,
                &cluster_info,
                &poh_recorder,
                non_vote_receiver,
                tpu_vote_receiver,
                gossip_vote_receiver,
                None,
                replay_vote_sender,
                None,
                Arc::new(ConnectionCache::new("connection_cache_test")),
                bank_forks,
                &Arc::new(PrioritizationFeeCache::new(0u64)),
            );
            trace!("sending bank");
            drop(non_vote_sender);
            drop(tpu_vote_sender);
            drop(gossip_vote_sender);
            exit.store(true, Ordering::Relaxed);
            poh_service.join().unwrap();
            drop(poh_recorder);

            trace!("getting entries");
            let entries: Vec<_> = entry_receiver
                .iter()
                .map(|(_bank, (entry, _tick_height))| entry)
                .collect();
            trace!("done");
            assert_eq!(entries.len(), genesis_config.ticks_per_slot as usize);
            assert!(entries.verify(&start_hash));
            assert_eq!(entries[entries.len() - 1].hash, bank.last_blockhash());
            banking_stage.join().unwrap();
        }
        Blockstore::destroy(ledger_path.path()).unwrap();
    }

    pub fn convert_from_old_verified(
        mut with_vers: Vec<(PacketBatch, Vec<u8>)>,
    ) -> Vec<PacketBatch> {
        with_vers.iter_mut().for_each(|(b, v)| {
            b.iter_mut()
                .zip(v)
                .for_each(|(p, f)| p.meta_mut().set_discard(*f == 0))
        });
        with_vers.into_iter().map(|(b, _)| b).collect()
    }

    #[test]
    fn test_banking_stage_entries_only() {
        solana_logger::setup();
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_slow_genesis_config(10);
        let bank = Bank::new_no_wallclock_throttle_for_tests(&genesis_config);
        let bank_forks = BankForks::new_rw_arc(bank);
        let bank = bank_forks.read().unwrap().get(0).unwrap();
        let start_hash = bank.last_blockhash();
        let banking_tracer = BankingTracer::new_disabled();
        let (non_vote_sender, non_vote_receiver) = banking_tracer.create_channel_non_vote();
        let (tpu_vote_sender, tpu_vote_receiver) = banking_tracer.create_channel_tpu_vote();
        let (gossip_vote_sender, gossip_vote_receiver) =
            banking_tracer.create_channel_gossip_vote();
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        {
            let blockstore = Arc::new(
                Blockstore::open(ledger_path.path())
                    .expect("Expected to be able to open database ledger"),
            );
            let poh_config = PohConfig {
                // limit tick count to avoid clearing working_bank at PohRecord then
                // PohRecorderError(MaxHeightReached) at BankingStage
                target_tick_count: Some(bank.max_tick_height() - 1),
                ..PohConfig::default()
            };
            let (exit, poh_recorder, poh_service, entry_receiver) =
                create_test_recorder(bank.clone(), blockstore, Some(poh_config), None);
            let (_, cluster_info) = new_test_cluster_info(/*keypair:*/ None);
            let cluster_info = Arc::new(cluster_info);
            let (replay_vote_sender, _replay_vote_receiver) = unbounded();

            let banking_stage = BankingStage::new(
                BlockProductionMethod::ThreadLocalMultiIterator,
                &cluster_info,
                &poh_recorder,
                non_vote_receiver,
                tpu_vote_receiver,
                gossip_vote_receiver,
                None,
                replay_vote_sender,
                None,
                Arc::new(ConnectionCache::new("connection_cache_test")),
                bank_forks,
                &Arc::new(PrioritizationFeeCache::new(0u64)),
            );

            // fund another account so we can send 2 good transactions in a single batch.
            let keypair = Keypair::new();
            let fund_tx =
                system_transaction::transfer(&mint_keypair, &keypair.pubkey(), 2, start_hash);
            bank.process_transaction(&fund_tx).unwrap();

            // good tx
            let to = solana_sdk::pubkey::new_rand();
            let tx = system_transaction::transfer(&mint_keypair, &to, 1, start_hash);

            // good tx, but no verify
            let to2 = solana_sdk::pubkey::new_rand();
            let tx_no_ver = system_transaction::transfer(&keypair, &to2, 2, start_hash);

            // bad tx, AccountNotFound
            let keypair = Keypair::new();
            let to3 = solana_sdk::pubkey::new_rand();
            let tx_anf = system_transaction::transfer(&keypair, &to3, 1, start_hash);

            // send 'em over
            let packet_batches = to_packet_batches(&[tx_no_ver, tx_anf, tx], 3);

            // glad they all fit
            assert_eq!(packet_batches.len(), 1);

            let packet_batches = packet_batches
                .into_iter()
                .map(|batch| (batch, vec![0u8, 1u8, 1u8]))
                .collect();
            let packet_batches = convert_from_old_verified(packet_batches);
            non_vote_sender // no_ver, anf, tx
                .send(BankingPacketBatch::new((packet_batches, None)))
                .unwrap();

            drop(non_vote_sender);
            drop(tpu_vote_sender);
            drop(gossip_vote_sender);
            // wait until banking_stage to finish up all packets
            banking_stage.join().unwrap();

            exit.store(true, Ordering::Relaxed);
            poh_service.join().unwrap();
            drop(poh_recorder);

            let mut blockhash = start_hash;
            let bank = Arc::new(Bank::new_no_wallclock_throttle_for_tests(&genesis_config));
            bank.process_transaction(&fund_tx).unwrap();
            //receive entries + ticks
            loop {
                let entries: Vec<Entry> = entry_receiver
                    .iter()
                    .map(|(_bank, (entry, _tick_height))| entry)
                    .collect();

                assert!(entries.verify(&blockhash));
                if !entries.is_empty() {
                    blockhash = entries.last().unwrap().hash;
                    for entry in entries {
                        bank.process_entry_transactions(entry.transactions)
                            .iter()
                            .for_each(|x| assert_eq!(*x, Ok(())));
                    }
                }

                if bank.get_balance(&to) == 1 {
                    break;
                }

                sleep(Duration::from_millis(200));
            }

            assert_eq!(bank.get_balance(&to), 1);
            assert_eq!(bank.get_balance(&to2), 0);

            drop(entry_receiver);
        }
        Blockstore::destroy(ledger_path.path()).unwrap();
    }

    #[test]
    fn test_banking_stage_entryfication() {
        solana_logger::setup();
        // In this attack we'll demonstrate that a verifier can interpret the ledger
        // differently if either the server doesn't signal the ledger to add an
        // Entry OR if the verifier tries to parallelize across multiple Entries.
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_slow_genesis_config(2);
        let banking_tracer = BankingTracer::new_disabled();
        let (non_vote_sender, non_vote_receiver) = banking_tracer.create_channel_non_vote();

        // Process a batch that includes a transaction that receives two lamports.
        let alice = Keypair::new();
        let tx =
            system_transaction::transfer(&mint_keypair, &alice.pubkey(), 2, genesis_config.hash());

        let packet_batches = to_packet_batches(&[tx], 1);
        let packet_batches = packet_batches
            .into_iter()
            .map(|batch| (batch, vec![1u8]))
            .collect();
        let packet_batches = convert_from_old_verified(packet_batches);
        non_vote_sender
            .send(BankingPacketBatch::new((packet_batches, None)))
            .unwrap();

        // Process a second batch that uses the same from account, so conflicts with above TX
        let tx =
            system_transaction::transfer(&mint_keypair, &alice.pubkey(), 1, genesis_config.hash());
        let packet_batches = to_packet_batches(&[tx], 1);
        let packet_batches = packet_batches
            .into_iter()
            .map(|batch| (batch, vec![1u8]))
            .collect();
        let packet_batches = convert_from_old_verified(packet_batches);
        non_vote_sender
            .send(BankingPacketBatch::new((packet_batches, None)))
            .unwrap();

        let (tpu_vote_sender, tpu_vote_receiver) = banking_tracer.create_channel_tpu_vote();
        let (gossip_vote_sender, gossip_vote_receiver) =
            banking_tracer.create_channel_gossip_vote();
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        {
            let (replay_vote_sender, _replay_vote_receiver) = unbounded();

            let entry_receiver = {
                // start a banking_stage to eat verified receiver
                let bank = Bank::new_no_wallclock_throttle_for_tests(&genesis_config);
                let bank_forks = BankForks::new_rw_arc(bank);
                let bank = bank_forks.read().unwrap().get(0).unwrap();
                let blockstore = Arc::new(
                    Blockstore::open(ledger_path.path())
                        .expect("Expected to be able to open database ledger"),
                );
                let poh_config = PohConfig {
                    // limit tick count to avoid clearing working_bank at
                    // PohRecord then PohRecorderError(MaxHeightReached) at BankingStage
                    target_tick_count: Some(bank.max_tick_height() - 1),
                    ..PohConfig::default()
                };
                let (exit, poh_recorder, poh_service, entry_receiver) =
                    create_test_recorder(bank.clone(), blockstore, Some(poh_config), None);
                let (_, cluster_info) = new_test_cluster_info(/*keypair:*/ None);
                let cluster_info = Arc::new(cluster_info);
                let _banking_stage = BankingStage::new_thread_local_multi_iterator(
                    &cluster_info,
                    &poh_recorder,
                    non_vote_receiver,
                    tpu_vote_receiver,
                    gossip_vote_receiver,
                    3,
                    None,
                    replay_vote_sender,
                    None,
                    Arc::new(ConnectionCache::new("connection_cache_test")),
                    bank_forks,
                    &Arc::new(PrioritizationFeeCache::new(0u64)),
                );

                // wait for banking_stage to eat the packets
                while bank.get_balance(&alice.pubkey()) < 1 {
                    sleep(Duration::from_millis(10));
                }
                exit.store(true, Ordering::Relaxed);
                poh_service.join().unwrap();
                entry_receiver
            };
            drop(non_vote_sender);
            drop(tpu_vote_sender);
            drop(gossip_vote_sender);

            // consume the entire entry_receiver, feed it into a new bank
            // check that the balance is what we expect.
            let entries: Vec<_> = entry_receiver
                .iter()
                .map(|(_bank, (entry, _tick_height))| entry)
                .collect();

            let bank = Bank::new_no_wallclock_throttle_for_tests(&genesis_config);
            for entry in entries {
                bank.process_entry_transactions(entry.transactions)
                    .iter()
                    .for_each(|x| assert_eq!(*x, Ok(())));
            }

            // Assert the user doesn't hold three lamports. If the stage only outputs one
            // entry, then one of the transactions will be rejected, because it drives
            // the account balance below zero before the credit is added.
            assert!(bank.get_balance(&alice.pubkey()) != 3);
        }
        Blockstore::destroy(ledger_path.path()).unwrap();
    }

    #[test]
    fn test_bank_record_transactions() {
        solana_logger::setup();

        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(10_000);
        let bank = Arc::new(Bank::new_no_wallclock_throttle_for_tests(&genesis_config));
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        {
            let blockstore = Blockstore::open(ledger_path.path())
                .expect("Expected to be able to open database ledger");
            let (poh_recorder, entry_receiver, record_receiver) = PohRecorder::new(
                // TODO use record_receiver
                bank.tick_height(),
                bank.last_blockhash(),
                bank.clone(),
                None,
                bank.ticks_per_slot(),
                &Pubkey::default(),
                Arc::new(blockstore),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
                &PohConfig::default(),
                Arc::new(AtomicBool::default()),
            );
            let recorder = poh_recorder.new_recorder();
            let poh_recorder = Arc::new(RwLock::new(poh_recorder));

            let poh_simulator = simulate_poh(record_receiver, &poh_recorder);

            poh_recorder.write().unwrap().set_bank(bank.clone(), false);
            let pubkey = solana_sdk::pubkey::new_rand();
            let keypair2 = Keypair::new();
            let pubkey2 = solana_sdk::pubkey::new_rand();

            let txs = vec![
                system_transaction::transfer(&mint_keypair, &pubkey, 1, genesis_config.hash())
                    .into(),
                system_transaction::transfer(&keypair2, &pubkey2, 1, genesis_config.hash()).into(),
            ];

            let _ = recorder.record_transactions(bank.slot(), txs.clone());
            let (_bank, (entry, _tick_height)) = entry_receiver.recv().unwrap();
            assert_eq!(entry.transactions, txs);

            // Once bank is set to a new bank (setting bank.slot() + 1 in record_transactions),
            // record_transactions should throw MaxHeightReached
            let next_slot = bank.slot() + 1;
            let RecordTransactionsSummary { result, .. } =
                recorder.record_transactions(next_slot, txs);
            assert_matches!(result, Err(PohRecorderError::MaxHeightReached));
            // Should receive nothing from PohRecorder b/c record failed
            assert!(entry_receiver.try_recv().is_err());

            poh_recorder
                .read()
                .unwrap()
                .is_exited
                .store(true, Ordering::Relaxed);
            let _ = poh_simulator.join();
        }
        Blockstore::destroy(ledger_path.path()).unwrap();
    }

    pub(crate) fn create_slow_genesis_config(lamports: u64) -> GenesisConfigInfo {
        create_slow_genesis_config_with_leader(lamports, &solana_sdk::pubkey::new_rand())
    }

    pub(crate) fn create_slow_genesis_config_with_leader(
        lamports: u64,
        validator_pubkey: &Pubkey,
    ) -> GenesisConfigInfo {
        let mut config_info = create_genesis_config_with_leader(
            lamports,
            validator_pubkey,
            // See solana_ledger::genesis_utils::create_genesis_config.
            bootstrap_validator_stake_lamports(),
        );

        // For these tests there's only 1 slot, don't want to run out of ticks
        config_info.genesis_config.ticks_per_slot *= 8;
        config_info
    }

    pub(crate) fn simulate_poh(
        record_receiver: Receiver<Record>,
        poh_recorder: &Arc<RwLock<PohRecorder>>,
    ) -> JoinHandle<()> {
        let poh_recorder = poh_recorder.clone();
        let is_exited = poh_recorder.read().unwrap().is_exited.clone();
        let tick_producer = Builder::new()
            .name("solana-simulate_poh".to_string())
            .spawn(move || loop {
                PohService::read_record_receiver_and_process(
                    &poh_recorder,
                    &record_receiver,
                    Duration::from_millis(10),
                );
                if is_exited.load(Ordering::Relaxed) {
                    break;
                }
            });
        tick_producer.unwrap()
    }

    #[test]
    fn test_unprocessed_transaction_storage_full_send() {
        solana_logger::setup();
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_slow_genesis_config(10000);
        let bank = Bank::new_no_wallclock_throttle_for_tests(&genesis_config);
        let bank_forks = BankForks::new_rw_arc(bank);
        let bank = bank_forks.read().unwrap().get(0).unwrap();
        let start_hash = bank.last_blockhash();
        let banking_tracer = BankingTracer::new_disabled();
        let (non_vote_sender, non_vote_receiver) = banking_tracer.create_channel_non_vote();
        let (tpu_vote_sender, tpu_vote_receiver) = banking_tracer.create_channel_tpu_vote();
        let (gossip_vote_sender, gossip_vote_receiver) =
            banking_tracer.create_channel_gossip_vote();
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        {
            let blockstore = Arc::new(
                Blockstore::open(ledger_path.path())
                    .expect("Expected to be able to open database ledger"),
            );
            let poh_config = PohConfig {
                // limit tick count to avoid clearing working_bank at PohRecord then
                // PohRecorderError(MaxHeightReached) at BankingStage
                target_tick_count: Some(bank.max_tick_height() - 1),
                ..PohConfig::default()
            };
            let (exit, poh_recorder, poh_service, _entry_receiver) =
                create_test_recorder(bank.clone(), blockstore, Some(poh_config), None);
            let (_, cluster_info) = new_test_cluster_info(/*keypair:*/ None);
            let cluster_info = Arc::new(cluster_info);
            let (replay_vote_sender, _replay_vote_receiver) = unbounded();

            let banking_stage = BankingStage::new(
                BlockProductionMethod::ThreadLocalMultiIterator,
                &cluster_info,
                &poh_recorder,
                non_vote_receiver,
                tpu_vote_receiver,
                gossip_vote_receiver,
                None,
                replay_vote_sender,
                None,
                Arc::new(ConnectionCache::new("connection_cache_test")),
                bank_forks,
                &Arc::new(PrioritizationFeeCache::new(0u64)),
            );

            let keypairs = (0..100).map(|_| Keypair::new()).collect_vec();
            let vote_keypairs = (0..100).map(|_| Keypair::new()).collect_vec();
            for keypair in keypairs.iter() {
                bank.process_transaction(&system_transaction::transfer(
                    &mint_keypair,
                    &keypair.pubkey(),
                    20,
                    start_hash,
                ))
                .unwrap();
            }

            // Send a bunch of votes and transfers
            let tpu_votes = (0..100_usize)
                .map(|i| {
                    new_vote_state_update_transaction(
                        VoteStateUpdate::from(vec![
                            (0, 8),
                            (1, 7),
                            (i as u64 + 10, 6),
                            (i as u64 + 11, 1),
                        ]),
                        Hash::new_unique(),
                        &keypairs[i],
                        &vote_keypairs[i],
                        &vote_keypairs[i],
                        None,
                    );
                })
                .collect_vec();
            let gossip_votes = (0..100_usize)
                .map(|i| {
                    new_vote_state_update_transaction(
                        VoteStateUpdate::from(vec![
                            (0, 8),
                            (1, 7),
                            (i as u64 + 64 + 5, 6),
                            (i as u64 + 7, 1),
                        ]),
                        Hash::new_unique(),
                        &keypairs[i],
                        &vote_keypairs[i],
                        &vote_keypairs[i],
                        None,
                    );
                })
                .collect_vec();
            let txs = (0..100_usize)
                .map(|i| {
                    system_transaction::transfer(
                        &keypairs[i],
                        &keypairs[(i + 1) % 100].pubkey(),
                        10,
                        start_hash,
                    );
                })
                .collect_vec();

            let non_vote_packet_batches = to_packet_batches(&txs, 10);
            let tpu_packet_batches = to_packet_batches(&tpu_votes, 10);
            let gossip_packet_batches = to_packet_batches(&gossip_votes, 10);

            // Send em all
            [
                (non_vote_packet_batches, non_vote_sender),
                (tpu_packet_batches, tpu_vote_sender),
                (gossip_packet_batches, gossip_vote_sender),
            ]
            .into_iter()
            .map(|(packet_batches, sender)| {
                Builder::new()
                    .spawn(move || {
                        sender
                            .send(BankingPacketBatch::new((packet_batches, None)))
                            .unwrap()
                    })
                    .unwrap()
            })
            .for_each(|handle| handle.join().unwrap());

            banking_stage.join().unwrap();
            exit.store(true, Ordering::Relaxed);
            poh_service.join().unwrap();
        }
        Blockstore::destroy(ledger_path.path()).unwrap();
    }
}

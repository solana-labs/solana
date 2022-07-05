//! The `banking_stage` processes Transaction messages. It is intended to be used
//! to construct a software pipeline. The stage uses all available CPU cores and
//! can do its processing in parallel with signature verification on the GPU.

use {
    crate::{
        forward_packet_batches_by_accounts::ForwardPacketBatchesByAccounts,
        immutable_deserialized_packet::ImmutableDeserializedPacket,
        leader_slot_banking_stage_metrics::{LeaderSlotMetricsTracker, ProcessTransactionsSummary},
        leader_slot_banking_stage_timing_metrics::{
            LeaderExecuteAndCommitTimings, RecordTransactionsTimings,
        },
        qos_service::QosService,
        sigverify::SigverifyTracerPacketStats,
        tracer_packet_stats::TracerPacketStats,
        unprocessed_packet_batches::{self, *},
    },
    core::iter::repeat,
    crossbeam_channel::{
        Receiver as CrossbeamReceiver, RecvTimeoutError, Sender as CrossbeamSender,
    },
    histogram::Histogram,
    itertools::Itertools,
    min_max_heap::MinMaxHeap,
    solana_client::{connection_cache::ConnectionCache, tpu_connection::TpuConnection},
    solana_entry::entry::hash_transactions,
    solana_gossip::{cluster_info::ClusterInfo, contact_info::ContactInfo},
    solana_ledger::{
        blockstore_processor::TransactionStatusSender, token_balances::collect_token_balances,
    },
    solana_measure::{measure, measure::Measure},
    solana_metrics::inc_new_counter_info,
    solana_perf::{
        data_budget::DataBudget,
        packet::{Packet, PacketBatch, PACKETS_PER_BATCH},
        perf_libs,
    },
    solana_poh::poh_recorder::{BankStart, PohRecorder, PohRecorderError, TransactionRecorder},
    solana_program_runtime::timings::ExecuteTimings,
    solana_runtime::{
        bank::{
            Bank, CommitTransactionCounts, LoadAndExecuteTransactionsOutput,
            TransactionBalancesSet, TransactionCheckResult,
        },
        bank_forks::BankForks,
        bank_utils,
        cost_model::{CostModel, TransactionCost},
        transaction_batch::TransactionBatch,
        transaction_error_metrics::TransactionErrorMetrics,
        vote_sender_types::ReplayVoteSender,
    },
    solana_sdk::{
        clock::{
            Slot, DEFAULT_TICKS_PER_SLOT, MAX_PROCESSING_AGE, MAX_TRANSACTION_FORWARDING_DELAY,
            MAX_TRANSACTION_FORWARDING_DELAY_GPU,
        },
        pubkey::Pubkey,
        saturating_add_assign,
        timing::{duration_as_ms, timestamp, AtomicInterval},
        transaction::{self, SanitizedTransaction, TransactionError, VersionedTransaction},
        transport::TransportError,
    },
    solana_streamer::sendmmsg::batch_send,
    solana_transaction_status::token_balances::TransactionTokenBalancesSet,
    std::{
        cmp,
        collections::HashMap,
        env,
        net::{SocketAddr, UdpSocket},
        rc::Rc,
        sync::{
            atomic::{AtomicU64, AtomicUsize, Ordering},
            Arc, RwLock,
        },
        thread::{self, Builder, JoinHandle},
        time::{Duration, Instant},
    },
};

/// Transaction forwarding
pub const FORWARD_TRANSACTIONS_TO_LEADER_AT_SLOT_OFFSET: u64 = 2;
pub const HOLD_TRANSACTIONS_SLOT_OFFSET: u64 = 20;

// Fixed thread size seems to be fastest on GCP setup
pub const NUM_THREADS: u32 = 6;

const TOTAL_BUFFERED_PACKETS: usize = 700_000;

const MAX_NUM_TRANSACTIONS_PER_BATCH: usize = 64;

const NUM_VOTE_PROCESSING_THREADS: u32 = 2;
const MIN_THREADS_BANKING: u32 = 1;
const MIN_TOTAL_THREADS: u32 = NUM_VOTE_PROCESSING_THREADS + MIN_THREADS_BANKING;
const UNPROCESSED_BUFFER_STEP_SIZE: usize = 128;

const SLOT_BOUNDARY_CHECK_PERIOD: Duration = Duration::from_millis(10);
pub type BankingPacketBatch = (Vec<PacketBatch>, Option<SigverifyTracerPacketStats>);
pub type BankingPacketSender = CrossbeamSender<BankingPacketBatch>;
pub type BankingPacketReceiver = CrossbeamReceiver<BankingPacketBatch>;

pub struct ProcessTransactionBatchOutput {
    // The number of transactions filtered out by the cost model
    cost_model_throttled_transactions_count: usize,
    // Amount of time spent running the cost model
    cost_model_us: u64,
    execute_and_commit_transactions_output: ExecuteAndCommitTransactionsOutput,
}

struct RecordTransactionsSummary {
    // Metrics describing how time was spent recording transactions
    record_transactions_timings: RecordTransactionsTimings,
    // Result of trying to record the transactions into the PoH stream
    result: Result<(), PohRecorderError>,
    // Index in the slot of the first transaction recorded
    starting_transaction_index: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommitTransactionDetails {
    Committed { compute_units: u64 },
    NotCommitted,
}

pub struct ExecuteAndCommitTransactionsOutput {
    // Total number of transactions that were passed as candidates for execution
    transactions_attempted_execution_count: usize,
    // The number of transactions of that were executed. See description of in `ProcessTransactionsSummary`
    // for possible outcomes of execution.
    executed_transactions_count: usize,
    // Total number of the executed transactions that returned success/not
    // an error.
    executed_with_successful_result_count: usize,
    // Transactions that either were not executed, or were executed and failed to be committed due
    // to the block ending.
    retryable_transaction_indexes: Vec<usize>,
    // A result that indicates whether transactions were successfully
    // committed into the Poh stream.
    commit_transactions_result: Result<Vec<CommitTransactionDetails>, PohRecorderError>,
    execute_and_commit_timings: LeaderExecuteAndCommitTimings,
    error_counters: TransactionErrorMetrics,
}

#[derive(Debug, Default)]
pub struct BankingStageStats {
    last_report: AtomicInterval,
    id: u32,
    receive_and_buffer_packets_count: AtomicUsize,
    dropped_packets_count: AtomicUsize,
    pub(crate) dropped_duplicated_packets_count: AtomicUsize,
    newly_buffered_packets_count: AtomicUsize,
    current_buffered_packets_count: AtomicUsize,
    current_buffered_packet_batches_count: AtomicUsize,
    rebuffered_packets_count: AtomicUsize,
    consumed_buffered_packets_count: AtomicUsize,
    forwarded_transaction_count: AtomicUsize,
    forwarded_vote_count: AtomicUsize,
    batch_packet_indexes_len: Histogram,

    // Timing
    consume_buffered_packets_elapsed: AtomicU64,
    receive_and_buffer_packets_elapsed: AtomicU64,
    handle_retryable_packets_elapsed: AtomicU64,
    filter_pending_packets_elapsed: AtomicU64,
    packet_conversion_elapsed: AtomicU64,
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
            + self.newly_buffered_packets_count.load(Ordering::Relaxed) as u64
            + self.current_buffered_packets_count.load(Ordering::Relaxed) as u64
            + self
                .current_buffered_packet_batches_count
                .load(Ordering::Relaxed) as u64
            + self.rebuffered_packets_count.load(Ordering::Relaxed) as u64
            + self.consumed_buffered_packets_count.load(Ordering::Relaxed) as u64
            + self
                .consume_buffered_packets_elapsed
                .load(Ordering::Relaxed)
            + self
                .receive_and_buffer_packets_elapsed
                .load(Ordering::Relaxed)
            + self
                .handle_retryable_packets_elapsed
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
                ("id", self.id as i64, i64),
                (
                    "receive_and_buffer_packets_count",
                    self.receive_and_buffer_packets_count
                        .swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "dropped_packets_count",
                    self.dropped_packets_count.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "dropped_duplicated_packets_count",
                    self.dropped_duplicated_packets_count
                        .swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "newly_buffered_packets_count",
                    self.newly_buffered_packets_count.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "current_buffered_packet_batches_count",
                    self.current_buffered_packet_batches_count
                        .swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "current_buffered_packets_count",
                    self.current_buffered_packets_count
                        .swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "rebuffered_packets_count",
                    self.rebuffered_packets_count.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "consumed_buffered_packets_count",
                    self.consumed_buffered_packets_count
                        .swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "forwarded_transaction_count",
                    self.forwarded_transaction_count.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "forwarded_vote_count",
                    self.forwarded_vote_count.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "consume_buffered_packets_elapsed",
                    self.consume_buffered_packets_elapsed
                        .swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "receive_and_buffer_packets_elapsed",
                    self.receive_and_buffer_packets_elapsed
                        .swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "handle_retryable_packets_elapsed",
                    self.handle_retryable_packets_elapsed
                        .swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "filter_pending_packets_elapsed",
                    self.filter_pending_packets_elapsed
                        .swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "packet_conversion_elapsed",
                    self.packet_conversion_elapsed.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "transaction_processing_elapsed",
                    self.transaction_processing_elapsed
                        .swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "packet_batch_indices_len_min",
                    self.batch_packet_indexes_len.minimum().unwrap_or(0) as i64,
                    i64
                ),
                (
                    "packet_batch_indices_len_max",
                    self.batch_packet_indexes_len.maximum().unwrap_or(0) as i64,
                    i64
                ),
                (
                    "packet_batch_indices_len_mean",
                    self.batch_packet_indexes_len.mean().unwrap_or(0) as i64,
                    i64
                ),
                (
                    "packet_batch_indices_len_90pct",
                    self.batch_packet_indexes_len.percentile(90.0).unwrap_or(0) as i64,
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
pub enum BufferedPacketsDecision {
    Consume(u128),
    Forward,
    ForwardAndHold,
    Hold,
}

#[derive(Debug, Clone)]
pub enum ForwardOption {
    NotForward,
    ForwardTpuVote,
    ForwardTransaction,
}

#[derive(Debug)]
pub struct FilterForwardingResults {
    total_forwardable_packets: usize,
    total_tracer_packets_in_buffer: usize,
    total_forwardable_tracer_packets: usize,
}

impl BankingStage {
    /// Create the stage using `bank`. Exit when `verified_receiver` is dropped.
    #[allow(clippy::new_ret_no_self)]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cluster_info: &Arc<ClusterInfo>,
        poh_recorder: &Arc<RwLock<PohRecorder>>,
        verified_receiver: BankingPacketReceiver,
        tpu_verified_vote_receiver: BankingPacketReceiver,
        verified_vote_receiver: BankingPacketReceiver,
        transaction_status_sender: Option<TransactionStatusSender>,
        gossip_vote_sender: ReplayVoteSender,
        cost_model: Arc<RwLock<CostModel>>,
        log_messages_bytes_limit: Option<usize>,
        connection_cache: Arc<ConnectionCache>,
        bank_forks: Arc<RwLock<BankForks>>,
    ) -> Self {
        Self::new_num_threads(
            cluster_info,
            poh_recorder,
            verified_receiver,
            tpu_verified_vote_receiver,
            verified_vote_receiver,
            Self::num_threads(),
            transaction_status_sender,
            gossip_vote_sender,
            cost_model,
            log_messages_bytes_limit,
            connection_cache,
            bank_forks,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_num_threads(
        cluster_info: &Arc<ClusterInfo>,
        poh_recorder: &Arc<RwLock<PohRecorder>>,
        verified_receiver: BankingPacketReceiver,
        tpu_verified_vote_receiver: BankingPacketReceiver,
        verified_vote_receiver: BankingPacketReceiver,
        num_threads: u32,
        transaction_status_sender: Option<TransactionStatusSender>,
        gossip_vote_sender: ReplayVoteSender,
        cost_model: Arc<RwLock<CostModel>>,
        log_messages_bytes_limit: Option<usize>,
        connection_cache: Arc<ConnectionCache>,
        bank_forks: Arc<RwLock<BankForks>>,
    ) -> Self {
        assert!(num_threads >= MIN_TOTAL_THREADS);
        // Single thread to generate entries from many banks.
        // This thread talks to poh_service and broadcasts the entries once they have been recorded.
        // Once an entry has been recorded, its blockhash is registered with the bank.
        let data_budget = Arc::new(DataBudget::default());
        let batch_limit =
            TOTAL_BUFFERED_PACKETS / ((num_threads - NUM_VOTE_PROCESSING_THREADS) as usize);
        // Many banks that process transactions in parallel.
        let bank_thread_hdls: Vec<JoinHandle<()>> = (0..num_threads)
            .map(|i| {
                let (verified_receiver, forward_option) = match i {
                    0 => {
                        // Disable forwarding of vote transactions
                        // from gossip. Note - votes can also arrive from tpu
                        (verified_vote_receiver.clone(), ForwardOption::NotForward)
                    }
                    1 => (
                        tpu_verified_vote_receiver.clone(),
                        ForwardOption::ForwardTpuVote,
                    ),
                    _ => (verified_receiver.clone(), ForwardOption::ForwardTransaction),
                };

                let poh_recorder = poh_recorder.clone();
                let cluster_info = cluster_info.clone();
                let mut recv_start = Instant::now();
                let transaction_status_sender = transaction_status_sender.clone();
                let gossip_vote_sender = gossip_vote_sender.clone();
                let data_budget = data_budget.clone();
                let cost_model = cost_model.clone();
                let connection_cache = connection_cache.clone();
                let bank_forks = bank_forks.clone();
                Builder::new()
                    .name(format!("solana-banking-stage-tx-{}", i))
                    .spawn(move || {
                        Self::process_loop(
                            &verified_receiver,
                            &poh_recorder,
                            &cluster_info,
                            &mut recv_start,
                            forward_option,
                            i,
                            batch_limit,
                            transaction_status_sender,
                            gossip_vote_sender,
                            &data_budget,
                            cost_model,
                            log_messages_bytes_limit,
                            connection_cache,
                            &bank_forks,
                        );
                    })
                    .unwrap()
            })
            .collect();
        Self { bank_thread_hdls }
    }

    // filter forwardable Rc<immutable_deserialized_packet>s that:
    // 1. are not forwarded, and
    // 2. in priority order from max to min, and
    // 3. not exceeding account bucket limit
    // returns forwarded packets count
    pub fn filter_valid_packets_for_forwarding(
        buffered_packet_batches: &mut UnprocessedPacketBatches,
        forward_packet_batches_by_accounts: &mut ForwardPacketBatchesByAccounts,
    ) -> FilterForwardingResults {
        let mut total_forwardable_tracer_packets: usize = 0;
        let mut total_tracer_packets_in_buffer: usize = 0;
        let mut total_forwardable_packets: usize = 0;
        let mut dropped_tx_before_forwarding_count: usize = 0;

        let filter_forwardable_packet = |deserialized_packet: &mut DeserializedPacket| -> bool {
            let mut result = true;
            let is_tracer_packet = deserialized_packet
                .immutable_section()
                .original_packet()
                .meta
                .is_tracer_packet();
            if is_tracer_packet {
                saturating_add_assign!(total_tracer_packets_in_buffer, 1);
            }
            if !deserialized_packet.forwarded {
                saturating_add_assign!(total_forwardable_packets, 1);
                if is_tracer_packet {
                    saturating_add_assign!(total_forwardable_tracer_packets, 1);
                }
                result = forward_packet_batches_by_accounts
                    .add_packet(deserialized_packet.immutable_section().clone());
                if !result {
                    saturating_add_assign!(dropped_tx_before_forwarding_count, 1);
                }
            }
            result
        };

        // Iterates buffered packets from high priority to low, places each packet into
        // forwarding account buckets by calling `forward_packet_batches_by_accounts.add_packet()`.
        // Iteration stops as soon as `add_packet()` returns false when a packet fails to fit into
        // buckets, ignoring remaining lower priority packets that could fit.
        // The motivation of this is during bot spamming, buffer is likely to be filled with
        // transactions have higher priority and write to same account(s), other lower priority
        // transactions will not make into buffer, therefore it shall exit as soon as first
        // transaction failed to fit in forwarding buckets.
        buffered_packet_batches.iter_desc(filter_forwardable_packet);

        inc_new_counter_info!(
            "banking_stage-dropped_tx_before_forwarding",
            dropped_tx_before_forwarding_count
        );

        FilterForwardingResults {
            total_forwardable_packets,
            total_tracer_packets_in_buffer,
            total_forwardable_tracer_packets,
        }
    }

    /// Forwards all valid, unprocessed packets in the buffer, up to a rate limit. Returns
    /// the number of successfully forwarded packets in second part of tuple
    fn forward_buffered_packets<'a>(
        connection_cache: &ConnectionCache,
        forward_option: &ForwardOption,
        cluster_info: &ClusterInfo,
        poh_recorder: &Arc<RwLock<PohRecorder>>,
        socket: &UdpSocket,
        forwardable_packets: impl Iterator<Item = &'a Packet>,
        data_budget: &DataBudget,
        banking_stage_stats: &BankingStageStats,
    ) -> (
        std::result::Result<(), TransportError>,
        usize,
        Option<Pubkey>,
    ) {
        let leader_and_addr = match forward_option {
            ForwardOption::NotForward => return (Ok(()), 0, None),
            ForwardOption::ForwardTransaction => {
                next_leader_tpu_forwards(cluster_info, poh_recorder)
            }

            ForwardOption::ForwardTpuVote => next_leader_tpu_vote(cluster_info, poh_recorder),
        };
        let (leader_pubkey, addr) = match leader_and_addr {
            Some(leader_and_addr) => leader_and_addr,
            None => return (Ok(()), 0, None),
        };

        const INTERVAL_MS: u64 = 100;
        // 12 MB outbound limit per second
        const MAX_BYTES_PER_SECOND: usize = 12_000_000;
        const MAX_BYTES_PER_INTERVAL: usize = MAX_BYTES_PER_SECOND * INTERVAL_MS as usize / 1000;
        const MAX_BYTES_BUDGET: usize = MAX_BYTES_PER_INTERVAL * 5;
        data_budget.update(INTERVAL_MS, |bytes| {
            std::cmp::min(
                bytes.saturating_add(MAX_BYTES_PER_INTERVAL),
                MAX_BYTES_BUDGET,
            )
        });

        let packet_vec: Vec<_> = forwardable_packets
            .filter_map(|p| {
                if !p.meta.forwarded() && data_budget.take(p.meta.size) {
                    Some(p.data(..)?.to_vec())
                } else {
                    None
                }
            })
            .collect();

        let packet_vec_len = packet_vec.len();
        // TODO: see https://github.com/solana-labs/solana/issues/23819
        // fix this so returns the correct number of succeeded packets
        // when there's an error sending the batch. This was left as-is for now
        // in favor of shipping Quic support, which was considered higher-priority
        if !packet_vec.is_empty() {
            inc_new_counter_info!("banking_stage-forwarded_packets", packet_vec_len);

            let mut measure = Measure::start("banking_stage-forward-us");

            let res = if let ForwardOption::ForwardTpuVote = forward_option {
                // The vote must be forwarded using only UDP.
                banking_stage_stats
                    .forwarded_vote_count
                    .fetch_add(packet_vec_len, Ordering::Relaxed);
                let pkts: Vec<_> = packet_vec.into_iter().zip(repeat(addr)).collect();
                batch_send(socket, &pkts).map_err(|err| err.into())
            } else {
                // All other transactions can be forwarded using QUIC, get_connection() will use
                // system wide setting to pick the correct connection object.
                banking_stage_stats
                    .forwarded_transaction_count
                    .fetch_add(packet_vec_len, Ordering::Relaxed);
                let conn = connection_cache.get_connection(&addr);
                conn.send_wire_transaction_batch_async(packet_vec)
            };

            measure.stop();
            inc_new_counter_info!(
                "banking_stage-forward-us",
                measure.as_us() as usize,
                1000,
                1000
            );

            if let Err(err) = res {
                inc_new_counter_info!("banking_stage-forward_packets-failed-batches", 1);
                return (Err(err), 0, Some(leader_pubkey));
            }
        }

        (Ok(()), packet_vec_len, Some(leader_pubkey))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn consume_buffered_packets(
        _my_pubkey: &Pubkey,
        max_tx_ingestion_ns: u128,
        poh_recorder: &Arc<RwLock<PohRecorder>>,
        buffered_packet_batches: &mut UnprocessedPacketBatches,
        transaction_status_sender: Option<TransactionStatusSender>,
        gossip_vote_sender: &ReplayVoteSender,
        test_fn: Option<impl Fn()>,
        banking_stage_stats: &BankingStageStats,
        recorder: &TransactionRecorder,
        qos_service: &QosService,
        slot_metrics_tracker: &mut LeaderSlotMetricsTracker,
        num_packets_to_process_per_iteration: usize,
        log_messages_bytes_limit: Option<usize>,
    ) {
        let mut rebuffered_packet_count = 0;
        let mut consumed_buffered_packets_count = 0;
        let buffered_packets_len = buffered_packet_batches.len();
        let mut proc_start = Measure::start("consume_buffered_process");
        let mut reached_end_of_slot = false;
        let mut retryable_packets = {
            let capacity = buffered_packet_batches.capacity();
            std::mem::replace(
                &mut buffered_packet_batches.packet_priority_queue,
                MinMaxHeap::with_capacity(capacity),
            )
        };
        let retryable_packets: MinMaxHeap<Rc<ImmutableDeserializedPacket>> = retryable_packets
            .drain_desc()
            .chunks(num_packets_to_process_per_iteration)
            .into_iter()
            .flat_map(|packets_to_process| {
                let packets_to_process = packets_to_process.into_iter().collect_vec();
                // TODO: Right now we iterate through buffer and try the highest weighted transaction once
                // but we should retry the highest weighted transactions more often.
                let (bank_start, poh_recorder_lock_time) = measure!(
                    poh_recorder.read().unwrap().bank_start(),
                    "poh_recorder.read",
                );
                slot_metrics_tracker.increment_consume_buffered_packets_poh_recorder_lock_us(
                    poh_recorder_lock_time.as_us(),
                );

                let packets_to_process_len = packets_to_process.len();
                if let Some(BankStart {
                    working_bank,
                    bank_creation_time,
                }) = bank_start
                {
                    let (process_transactions_summary, process_packets_transactions_time) =
                        measure!(
                                Self::process_packets_transactions(
                                    &working_bank,
                                    &bank_creation_time,
                                    recorder,
                                    packets_to_process.iter().map(|p| &**p),
                                    transaction_status_sender.clone(),
                                    gossip_vote_sender,
                                    banking_stage_stats,
                                    qos_service,
                                    slot_metrics_tracker,
                                    log_messages_bytes_limit
                                ),
                            "process_packets_transactions",
                        );
                    slot_metrics_tracker.increment_process_packets_transactions_us(
                        process_packets_transactions_time.as_us(),
                    );

                    let ProcessTransactionsSummary {
                        reached_max_poh_height,
                        retryable_transaction_indexes,
                        ..
                    } = process_transactions_summary;

                    if reached_max_poh_height
                        || !Bank::should_bank_still_be_processing_txs(
                            &bank_creation_time,
                            max_tx_ingestion_ns,
                        )
                    {
                        reached_end_of_slot = true;
                    }

                    // The difference between all transactions passed to execution and the ones that
                    // are retryable were the ones that were either:
                    // 1) Committed into the block
                    // 2) Dropped without being committed because they had some fatal error (too old,
                    // duplicate signature, etc.)
                    //
                    // Note: This assumes that every packet deserializes into one transaction!
                    consumed_buffered_packets_count +=
                        packets_to_process_len.saturating_sub(retryable_transaction_indexes.len());

                    // Out of the buffered packets just retried, collect any still unprocessed
                    // transactions in this batch for forwarding
                    rebuffered_packet_count += retryable_transaction_indexes.len();
                    if let Some(test_fn) = &test_fn {
                        test_fn();
                    }

                    slot_metrics_tracker.increment_retryable_packets_count(
                        retryable_transaction_indexes.len() as u64,
                    );

                    let result = retryable_transaction_indexes
                        .iter()
                        .map(|i| packets_to_process[*i].clone())
                        .collect_vec();

                    // Remove the non-retryable packets, packets that were either:
                    // 1) Successfully processed
                    // 2) Failed but not retryable
                    Self::filter_processed_packets(
                        retryable_transaction_indexes
                            .iter()
                            .chain(std::iter::once(&packets_to_process.len())),
                        |start, end| {
                            for processed_packet in &packets_to_process[start..end] {
                                buffered_packet_batches
                                    .message_hash_to_transaction
                                    .remove(processed_packet.message_hash());
                            }
                        },
                    );

                    result
                } else if reached_end_of_slot {
                    packets_to_process
                } else {
                    // mark as end-of-slot to avoid aggressively lock poh for the remaining for
                    // packet batches in buffer
                    reached_end_of_slot = true;

                    packets_to_process
                }
            })
            .collect();

        buffered_packet_batches.packet_priority_queue = retryable_packets;

        if reached_end_of_slot {
            slot_metrics_tracker
                .set_end_of_slot_unprocessed_buffer_len(buffered_packet_batches.len() as u64);

            // We've hit the end of this slot, no need to perform more processing,
            // Packet filtering will be done at `forward_packet_batches_by_accounts.add_packet()`
        }

        proc_start.stop();
        debug!(
            "@{:?} done processing buffered batches: {} time: {:?}ms tx count: {} tx/s: {}",
            timestamp(),
            buffered_packets_len,
            proc_start.as_ms(),
            consumed_buffered_packets_count,
            (consumed_buffered_packets_count as f32) / (proc_start.as_s())
        );

        // Assert unprocessed queue is still consistent
        assert_eq!(
            buffered_packet_batches.packet_priority_queue.len(),
            buffered_packet_batches.message_hash_to_transaction.len()
        );
        banking_stage_stats
            .consume_buffered_packets_elapsed
            .fetch_add(proc_start.as_us(), Ordering::Relaxed);
        banking_stage_stats
            .rebuffered_packets_count
            .fetch_add(rebuffered_packet_count, Ordering::Relaxed);
        banking_stage_stats
            .consumed_buffered_packets_count
            .fetch_add(consumed_buffered_packets_count, Ordering::Relaxed);
    }

    fn consume_or_forward_packets(
        my_pubkey: &Pubkey,
        leader_pubkey: Option<Pubkey>,
        bank_still_processing_txs: Option<&Arc<Bank>>,
        would_be_leader: bool,
        would_be_leader_shortly: bool,
    ) -> BufferedPacketsDecision {
        // If has active bank, then immediately process buffered packets
        // otherwise, based on leader schedule to either forward or hold packets
        if let Some(bank) = bank_still_processing_txs {
            // If the bank is available, this node is the leader
            BufferedPacketsDecision::Consume(bank.ns_per_slot)
        } else if would_be_leader_shortly {
            // If the node will be the leader soon, hold the packets for now
            BufferedPacketsDecision::Hold
        } else if would_be_leader {
            // Node will be leader within ~20 slots, hold the transactions in
            // case it is the only node which produces an accepted slot.
            BufferedPacketsDecision::ForwardAndHold
        } else if let Some(x) = leader_pubkey {
            if x != *my_pubkey {
                // If the current node is not the leader, forward the buffered packets
                BufferedPacketsDecision::Forward
            } else {
                // If the current node is the leader, return the buffered packets as is
                BufferedPacketsDecision::Hold
            }
        } else {
            // We don't know the leader. Hold the packets for now
            BufferedPacketsDecision::Hold
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn process_buffered_packets(
        my_pubkey: &Pubkey,
        socket: &UdpSocket,
        poh_recorder: &Arc<RwLock<PohRecorder>>,
        cluster_info: &ClusterInfo,
        buffered_packet_batches: &mut UnprocessedPacketBatches,
        forward_option: &ForwardOption,
        transaction_status_sender: Option<TransactionStatusSender>,
        gossip_vote_sender: &ReplayVoteSender,
        banking_stage_stats: &BankingStageStats,
        recorder: &TransactionRecorder,
        data_budget: &DataBudget,
        qos_service: &QosService,
        slot_metrics_tracker: &mut LeaderSlotMetricsTracker,
        log_messages_bytes_limit: Option<usize>,
        connection_cache: &ConnectionCache,
        tracer_packet_stats: &mut TracerPacketStats,
        bank_forks: &Arc<RwLock<BankForks>>,
    ) {
        let ((metrics_action, decision), make_decision_time) = measure!(
            {
                let bank_start;
                let (
                    leader_at_slot_offset,
                    bank_still_processing_txs,
                    would_be_leader,
                    would_be_leader_shortly,
                ) = {
                    let poh = poh_recorder.read().unwrap();
                    bank_start = poh.bank_start();
                    (
                        poh.leader_after_n_slots(FORWARD_TRANSACTIONS_TO_LEADER_AT_SLOT_OFFSET),
                        PohRecorder::get_working_bank_if_not_expired(&bank_start.as_ref()),
                        poh.would_be_leader(HOLD_TRANSACTIONS_SLOT_OFFSET * DEFAULT_TICKS_PER_SLOT),
                        poh.would_be_leader(
                            (FORWARD_TRANSACTIONS_TO_LEADER_AT_SLOT_OFFSET - 1)
                                * DEFAULT_TICKS_PER_SLOT,
                        ),
                    )
                };

                (
                    slot_metrics_tracker.check_leader_slot_boundary(&bank_start),
                    Self::consume_or_forward_packets(
                        my_pubkey,
                        leader_at_slot_offset,
                        bank_still_processing_txs,
                        would_be_leader,
                        would_be_leader_shortly,
                    ),
                )
            },
            "make_decision",
        );
        slot_metrics_tracker.increment_make_decision_us(make_decision_time.as_us());

        match decision {
            BufferedPacketsDecision::Consume(max_tx_ingestion_ns) => {
                // Take metrics action before consume packets (potentially resetting the
                // slot metrics tracker to the next slot) so that we don't count the
                // packet processing metrics from the next slot towards the metrics
                // of the previous slot
                slot_metrics_tracker.apply_action(metrics_action);
                let (_, consume_buffered_packets_time) = measure!(
                    Self::consume_buffered_packets(
                        my_pubkey,
                        max_tx_ingestion_ns,
                        poh_recorder,
                        buffered_packet_batches,
                        transaction_status_sender,
                        gossip_vote_sender,
                        None::<Box<dyn Fn()>>,
                        banking_stage_stats,
                        recorder,
                        qos_service,
                        slot_metrics_tracker,
                        UNPROCESSED_BUFFER_STEP_SIZE,
                        log_messages_bytes_limit
                    ),
                    "consume_buffered_packets",
                );
                slot_metrics_tracker
                    .increment_consume_buffered_packets_us(consume_buffered_packets_time.as_us());
            }
            BufferedPacketsDecision::Forward => {
                let (_, forward_time) = measure!(
                    Self::handle_forwarding(
                        forward_option,
                        cluster_info,
                        buffered_packet_batches,
                        poh_recorder,
                        socket,
                        false,
                        data_budget,
                        slot_metrics_tracker,
                        banking_stage_stats,
                        connection_cache,
                        tracer_packet_stats,
                        bank_forks,
                    ),
                    "forward",
                );
                slot_metrics_tracker.increment_forward_us(forward_time.as_us());
                // Take metrics action after forwarding packets to include forwarded
                // metrics into current slot
                slot_metrics_tracker.apply_action(metrics_action);
            }
            BufferedPacketsDecision::ForwardAndHold => {
                let (_, forward_and_hold_time) = measure!(
                    Self::handle_forwarding(
                        forward_option,
                        cluster_info,
                        buffered_packet_batches,
                        poh_recorder,
                        socket,
                        true,
                        data_budget,
                        slot_metrics_tracker,
                        banking_stage_stats,
                        connection_cache,
                        tracer_packet_stats,
                        bank_forks,
                    ),
                    "forward_and_hold",
                );
                slot_metrics_tracker.increment_forward_and_hold_us(forward_and_hold_time.as_us());
                // Take metrics action after forwarding packets
                slot_metrics_tracker.apply_action(metrics_action);
            }
            _ => (),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_forwarding(
        forward_option: &ForwardOption,
        cluster_info: &ClusterInfo,
        buffered_packet_batches: &mut UnprocessedPacketBatches,
        poh_recorder: &Arc<RwLock<PohRecorder>>,
        socket: &UdpSocket,
        hold: bool,
        data_budget: &DataBudget,
        slot_metrics_tracker: &mut LeaderSlotMetricsTracker,
        banking_stage_stats: &BankingStageStats,
        connection_cache: &ConnectionCache,
        tracer_packet_stats: &mut TracerPacketStats,
        bank_forks: &Arc<RwLock<BankForks>>,
    ) {
        if let ForwardOption::NotForward = forward_option {
            if !hold {
                buffered_packet_batches.clear();
            }
            return;
        }

        // get current root bank from bank_forks, use it to sanitize transaction and
        // load all accounts from address loader;
        let current_bank = bank_forks.read().unwrap().root_bank();
        let mut forward_packet_batches_by_accounts =
            ForwardPacketBatchesByAccounts::new_with_default_batch_limits(current_bank);
        let filter_forwarding_result = Self::filter_valid_packets_for_forwarding(
            buffered_packet_batches,
            &mut forward_packet_batches_by_accounts,
        );
        forward_packet_batches_by_accounts
            .iter_batches()
            .filter(|&batch| !batch.is_empty())
            .for_each(|forward_batch| {
                slot_metrics_tracker.increment_forwardable_batches_count(1);

                let batched_forwardable_packets_count = forward_batch.len();
                let (_forward_result, sucessful_forwarded_packets_count, leader_pubkey) =
                    Self::forward_buffered_packets(
                        connection_cache,
                        forward_option,
                        cluster_info,
                        poh_recorder,
                        socket,
                        forward_batch.get_forwardable_packets(),
                        data_budget,
                        banking_stage_stats,
                    );

                if let Some(leader_pubkey) = leader_pubkey {
                    tracer_packet_stats.increment_total_forwardable_tracer_packets(
                        filter_forwarding_result.total_forwardable_tracer_packets,
                        leader_pubkey,
                    );
                }
                let failed_forwarded_packets_count = batched_forwardable_packets_count
                    .saturating_sub(sucessful_forwarded_packets_count);

                if failed_forwarded_packets_count > 0 {
                    slot_metrics_tracker.increment_failed_forwarded_packets_count(
                        failed_forwarded_packets_count as u64,
                    );
                    slot_metrics_tracker.increment_packet_batch_forward_failure_count(1);
                }

                if sucessful_forwarded_packets_count > 0 {
                    slot_metrics_tracker.increment_successful_forwarded_packets_count(
                        sucessful_forwarded_packets_count as u64,
                    );
                }
            });

        if hold {
            for deserialized_packet in buffered_packet_batches.iter_mut() {
                deserialized_packet.forwarded = true;
            }
        } else {
            slot_metrics_tracker.increment_cleared_from_buffer_after_forward_count(
                filter_forwarding_result.total_forwardable_packets as u64,
            );
            tracer_packet_stats.increment_total_cleared_from_buffer_after_forward(
                filter_forwarding_result.total_tracer_packets_in_buffer,
            );
            buffered_packet_batches.clear();
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn process_loop(
        verified_receiver: &BankingPacketReceiver,
        poh_recorder: &Arc<RwLock<PohRecorder>>,
        cluster_info: &ClusterInfo,
        recv_start: &mut Instant,
        forward_option: ForwardOption,
        id: u32,
        batch_limit: usize,
        transaction_status_sender: Option<TransactionStatusSender>,
        gossip_vote_sender: ReplayVoteSender,
        data_budget: &DataBudget,
        cost_model: Arc<RwLock<CostModel>>,
        log_messages_bytes_limit: Option<usize>,
        connection_cache: Arc<ConnectionCache>,
        bank_forks: &Arc<RwLock<BankForks>>,
    ) {
        let recorder = poh_recorder.read().unwrap().recorder();
        let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let mut buffered_packet_batches = UnprocessedPacketBatches::with_capacity(batch_limit);
        let mut banking_stage_stats = BankingStageStats::new(id);
        let mut tracer_packet_stats = TracerPacketStats::new(id);
        let qos_service = QosService::new(cost_model, id);

        let mut slot_metrics_tracker = LeaderSlotMetricsTracker::new(id);
        let mut last_metrics_update = Instant::now();

        loop {
            let my_pubkey = cluster_info.id();
            if !buffered_packet_batches.is_empty()
                || last_metrics_update.elapsed() >= SLOT_BOUNDARY_CHECK_PERIOD
            {
                let (_, process_buffered_packets_time) = measure!(
                    Self::process_buffered_packets(
                        &my_pubkey,
                        &socket,
                        poh_recorder,
                        cluster_info,
                        &mut buffered_packet_batches,
                        &forward_option,
                        transaction_status_sender.clone(),
                        &gossip_vote_sender,
                        &banking_stage_stats,
                        &recorder,
                        data_budget,
                        &qos_service,
                        &mut slot_metrics_tracker,
                        log_messages_bytes_limit,
                        &connection_cache,
                        &mut tracer_packet_stats,
                        bank_forks,
                    ),
                    "process_buffered_packets",
                );
                slot_metrics_tracker
                    .increment_process_buffered_packets_us(process_buffered_packets_time.as_us());
                last_metrics_update = Instant::now();
            }

            tracer_packet_stats.report(1000);

            let recv_timeout = if !buffered_packet_batches.is_empty() {
                // If there are buffered packets, run the equivalent of try_recv to try reading more
                // packets. This prevents starving BankingStage::consume_buffered_packets due to
                // buffered_packet_batches containing transactions that exceed the cost model for
                // the current bank.
                Duration::from_millis(0)
            } else {
                // Default wait time
                Duration::from_millis(100)
            };

            let (res, receive_and_buffer_packets_time) = measure!(
                Self::receive_and_buffer_packets(
                    verified_receiver,
                    recv_start,
                    recv_timeout,
                    id,
                    &mut buffered_packet_batches,
                    &mut banking_stage_stats,
                    &mut tracer_packet_stats,
                    &mut slot_metrics_tracker,
                ),
                "receive_and_buffer_packets",
            );
            slot_metrics_tracker
                .increment_receive_and_buffer_packets_us(receive_and_buffer_packets_time.as_us());

            match res {
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

    #[allow(clippy::match_wild_err_arm)]
    fn record_transactions(
        bank_slot: Slot,
        transactions: Vec<VersionedTransaction>,
        recorder: &TransactionRecorder,
    ) -> RecordTransactionsSummary {
        let mut record_transactions_timings = RecordTransactionsTimings::default();
        let mut starting_transaction_index = None;

        if !transactions.is_empty() {
            let num_to_record = transactions.len();
            inc_new_counter_info!("banking_stage-record_count", 1);
            inc_new_counter_info!("banking_stage-record_transactions", num_to_record);

            let (hash, hash_time) = measure!(hash_transactions(&transactions), "hash");
            record_transactions_timings.hash_us = hash_time.as_us();

            let (res, poh_record_time) =
                measure!(recorder.record(bank_slot, hash, transactions), "hash");
            record_transactions_timings.poh_record_us = poh_record_time.as_us();

            match res {
                Ok(starting_index) => {
                    starting_transaction_index = starting_index;
                }
                Err(PohRecorderError::MaxHeightReached) => {
                    inc_new_counter_info!("banking_stage-max_height_reached", 1);
                    inc_new_counter_info!(
                        "banking_stage-max_height_reached_num_to_commit",
                        num_to_record
                    );
                    return RecordTransactionsSummary {
                        record_transactions_timings,
                        result: Err(PohRecorderError::MaxHeightReached),
                        starting_transaction_index: None,
                    };
                }
                Err(e) => panic!("Poh recorder returned unexpected error: {:?}", e),
            }
        }

        RecordTransactionsSummary {
            record_transactions_timings,
            result: Ok(()),
            starting_transaction_index,
        }
    }

    fn execute_and_commit_transactions_locked(
        bank: &Arc<Bank>,
        poh: &TransactionRecorder,
        batch: &TransactionBatch,
        transaction_status_sender: Option<TransactionStatusSender>,
        gossip_vote_sender: &ReplayVoteSender,
        log_messages_bytes_limit: Option<usize>,
    ) -> ExecuteAndCommitTransactionsOutput {
        let mut execute_and_commit_timings = LeaderExecuteAndCommitTimings::default();
        let mut mint_decimals: HashMap<Pubkey, u8> = HashMap::new();

        let ((pre_balances, pre_token_balances), collect_balances_time) = measure!(
            {
                // Use a shorter maximum age when adding transactions into the pipeline.  This will reduce
                // the likelihood of any single thread getting starved and processing old ids.
                // TODO: Banking stage threads should be prioritized to complete faster then this queue
                // expires.
                let pre_balances = if transaction_status_sender.is_some() {
                    bank.collect_balances(batch)
                } else {
                    vec![]
                };

                let pre_token_balances = if transaction_status_sender.is_some() {
                    collect_token_balances(bank, batch, &mut mint_decimals)
                } else {
                    vec![]
                };

                (pre_balances, pre_token_balances)
            },
            "collect_balances",
        );
        execute_and_commit_timings.collect_balances_us = collect_balances_time.as_us();

        let (load_and_execute_transactions_output, load_execute_time) = measure!(
            bank.load_and_execute_transactions(
                batch,
                MAX_PROCESSING_AGE,
                transaction_status_sender.is_some(),
                transaction_status_sender.is_some(),
                transaction_status_sender.is_some(),
                &mut execute_and_commit_timings.execute_timings,
                None, // account_overrides
                log_messages_bytes_limit
            ),
            "load_execute",
        );
        execute_and_commit_timings.load_execute_us = load_execute_time.as_us();

        let LoadAndExecuteTransactionsOutput {
            mut loaded_transactions,
            execution_results,
            mut retryable_transaction_indexes,
            executed_transactions_count,
            executed_with_successful_result_count,
            signature_count,
            error_counters,
            ..
        } = load_and_execute_transactions_output;

        let transactions_attempted_execution_count = execution_results.len();
        let (executed_transactions, execution_results_to_transactions_time): (Vec<_>, Measure) = measure!(
            execution_results
                .iter()
                .zip(batch.sanitized_transactions())
                .filter_map(|(execution_result, tx)| {
                    if execution_result.was_executed() {
                        Some(tx.to_versioned_transaction())
                    } else {
                        None
                    }
                })
                .collect(),
            "execution_results_to_transactions",
        );

        let (last_blockhash, lamports_per_signature) =
            bank.last_blockhash_and_lamports_per_signature();
        let (freeze_lock, freeze_lock_time) = measure!(bank.freeze_lock(), "freeze_lock");
        execute_and_commit_timings.freeze_lock_us = freeze_lock_time.as_us();

        let (record_transactions_summary, record_time) = measure!(
            Self::record_transactions(bank.slot(), executed_transactions, poh),
            "record_transactions",
        );
        execute_and_commit_timings.record_us = record_time.as_us();

        let RecordTransactionsSummary {
            result: record_transactions_result,
            record_transactions_timings,
            starting_transaction_index,
        } = record_transactions_summary;
        execute_and_commit_timings.record_transactions_timings = RecordTransactionsTimings {
            execution_results_to_transactions_us: execution_results_to_transactions_time.as_us(),
            ..record_transactions_timings
        };

        if let Err(recorder_err) = record_transactions_result {
            inc_new_counter_info!(
                "banking_stage-record_transactions_retryable_record_txs",
                executed_transactions_count
            );

            retryable_transaction_indexes.extend(execution_results.iter().enumerate().filter_map(
                |(index, execution_result)| execution_result.was_executed().then(|| index),
            ));

            return ExecuteAndCommitTransactionsOutput {
                transactions_attempted_execution_count,
                executed_transactions_count,
                executed_with_successful_result_count,
                retryable_transaction_indexes,
                commit_transactions_result: Err(recorder_err),
                execute_and_commit_timings,
                error_counters,
            };
        }

        let sanitized_txs = batch.sanitized_transactions();
        let (commit_time_us, commit_transaction_statuses) = if executed_transactions_count != 0 {
            inc_new_counter_info!(
                "banking_stage-record_transactions_num_to_commit",
                executed_transactions_count
            );

            let (tx_results, commit_time) = measure!(
                bank.commit_transactions(
                    sanitized_txs,
                    &mut loaded_transactions,
                    execution_results,
                    last_blockhash,
                    lamports_per_signature,
                    CommitTransactionCounts {
                        committed_transactions_count: executed_transactions_count as u64,
                        committed_with_failure_result_count: executed_transactions_count
                            .saturating_sub(executed_with_successful_result_count)
                            as u64,
                        signature_count,
                    },
                    &mut execute_and_commit_timings.execute_timings,
                ),
                "commit",
            );
            let commit_time_us = commit_time.as_us();
            execute_and_commit_timings.commit_us = commit_time_us;

            let commit_transaction_statuses = tx_results
                .execution_results
                .iter()
                .map(|execution_result| match execution_result.details() {
                    Some(details) => CommitTransactionDetails::Committed {
                        compute_units: details.executed_units,
                    },
                    None => CommitTransactionDetails::NotCommitted,
                })
                .collect();

            let (_, find_and_send_votes_time) = measure!(
                {
                    bank_utils::find_and_send_votes(
                        sanitized_txs,
                        &tx_results,
                        Some(gossip_vote_sender),
                    );
                    if let Some(transaction_status_sender) = transaction_status_sender {
                        let txs = batch.sanitized_transactions().to_vec();
                        let post_balances = bank.collect_balances(batch);
                        let post_token_balances =
                            collect_token_balances(bank, batch, &mut mint_decimals);
                        let mut transaction_index = starting_transaction_index.unwrap_or_default();
                        let batch_transaction_indexes: Vec<_> = tx_results
                            .execution_results
                            .iter()
                            .map(|result| {
                                if result.was_executed() {
                                    let this_transaction_index = transaction_index;
                                    saturating_add_assign!(transaction_index, 1);
                                    this_transaction_index
                                } else {
                                    0
                                }
                            })
                            .collect();
                        transaction_status_sender.send_transaction_status_batch(
                            bank.clone(),
                            txs,
                            tx_results.execution_results,
                            TransactionBalancesSet::new(pre_balances, post_balances),
                            TransactionTokenBalancesSet::new(
                                pre_token_balances,
                                post_token_balances,
                            ),
                            tx_results.rent_debits,
                            batch_transaction_indexes,
                        );
                    }
                },
                "find_and_send_votes",
            );
            execute_and_commit_timings.find_and_send_votes_us = find_and_send_votes_time.as_us();
            (commit_time_us, commit_transaction_statuses)
        } else {
            (
                0,
                vec![CommitTransactionDetails::NotCommitted; execution_results.len()],
            )
        };

        drop(freeze_lock);

        debug!(
            "bank: {} process_and_record_locked: {}us record: {}us commit: {}us txs_len: {}",
            bank.slot(),
            load_execute_time.as_us(),
            record_time.as_us(),
            commit_time_us,
            sanitized_txs.len(),
        );

        debug!(
            "execute_and_commit_transactions_locked: {:?}",
            execute_and_commit_timings.execute_timings,
        );

        debug_assert_eq!(
            commit_transaction_statuses.len(),
            transactions_attempted_execution_count
        );

        ExecuteAndCommitTransactionsOutput {
            transactions_attempted_execution_count,
            executed_transactions_count,
            executed_with_successful_result_count,
            retryable_transaction_indexes,
            commit_transactions_result: Ok(commit_transaction_statuses),
            execute_and_commit_timings,
            error_counters,
        }
    }

    pub fn process_and_record_transactions(
        bank: &Arc<Bank>,
        txs: &[SanitizedTransaction],
        poh: &TransactionRecorder,
        chunk_offset: usize,
        transaction_status_sender: Option<TransactionStatusSender>,
        gossip_vote_sender: &ReplayVoteSender,
        qos_service: &QosService,
        log_messages_bytes_limit: Option<usize>,
    ) -> ProcessTransactionBatchOutput {
        let mut cost_model_time = Measure::start("cost_model");

        let transaction_costs = qos_service.compute_transaction_costs(txs.iter());

        let (transactions_qos_results, num_included) =
            qos_service.select_transactions_per_cost(txs.iter(), transaction_costs.iter(), bank);

        let cost_model_throttled_transactions_count = txs.len().saturating_sub(num_included);

        qos_service.accumulate_estimated_transaction_costs(
            &Self::accumulate_batched_transaction_costs(
                transaction_costs.iter(),
                transactions_qos_results.iter(),
            ),
        );
        cost_model_time.stop();

        // Only lock accounts for those transactions are selected for the block;
        // Once accounts are locked, other threads cannot encode transactions that will modify the
        // same account state
        let mut lock_time = Measure::start("lock_time");
        let batch = bank.prepare_sanitized_batch_with_results(txs, transactions_qos_results.iter());
        lock_time.stop();

        // retryable_txs includes AccountInUse, WouldExceedMaxBlockCostLimit
        // WouldExceedMaxAccountCostLimit, WouldExceedMaxVoteCostLimit
        // and WouldExceedMaxAccountDataCostLimit
        let mut execute_and_commit_transactions_output =
            Self::execute_and_commit_transactions_locked(
                bank,
                poh,
                &batch,
                transaction_status_sender,
                gossip_vote_sender,
                log_messages_bytes_limit,
            );

        let mut unlock_time = Measure::start("unlock_time");
        // Once the accounts are new transactions can enter the pipeline to process them
        drop(batch);
        unlock_time.stop();

        let ExecuteAndCommitTransactionsOutput {
            ref mut retryable_transaction_indexes,
            ref execute_and_commit_timings,
            ref commit_transactions_result,
            ..
        } = execute_and_commit_transactions_output;

        QosService::update_or_remove_transaction_costs(
            transaction_costs.iter(),
            transactions_qos_results.iter(),
            commit_transactions_result.as_ref().ok(),
            bank,
        );

        retryable_transaction_indexes
            .iter_mut()
            .for_each(|x| *x += chunk_offset);

        let (cu, us) =
            Self::accumulate_execute_units_and_time(&execute_and_commit_timings.execute_timings);
        qos_service.accumulate_actual_execute_cu(cu);
        qos_service.accumulate_actual_execute_time(us);

        // reports qos service stats for this batch
        qos_service.report_metrics(bank.clone());

        debug!(
            "bank: {} lock: {}us unlock: {}us txs_len: {}",
            bank.slot(),
            lock_time.as_us(),
            unlock_time.as_us(),
            txs.len(),
        );

        ProcessTransactionBatchOutput {
            cost_model_throttled_transactions_count,
            cost_model_us: cost_model_time.as_us(),
            execute_and_commit_transactions_output,
        }
    }

    // rollup transaction cost details, eg signature_cost, write_lock_cost, data_bytes_cost and
    // execution_cost from the batch of transactions selected for block.
    fn accumulate_batched_transaction_costs<'a>(
        transactions_costs: impl Iterator<Item = &'a TransactionCost>,
        transaction_results: impl Iterator<Item = &'a transaction::Result<()>>,
    ) -> BatchedTransactionDetails {
        let mut batched_transaction_details = BatchedTransactionDetails::default();
        transactions_costs
            .zip(transaction_results)
            .for_each(|(cost, result)| match result {
                Ok(_) => {
                    saturating_add_assign!(
                        batched_transaction_details.costs.batched_signature_cost,
                        cost.signature_cost
                    );
                    saturating_add_assign!(
                        batched_transaction_details.costs.batched_write_lock_cost,
                        cost.write_lock_cost
                    );
                    saturating_add_assign!(
                        batched_transaction_details.costs.batched_data_bytes_cost,
                        cost.data_bytes_cost
                    );
                    saturating_add_assign!(
                        batched_transaction_details
                            .costs
                            .batched_builtins_execute_cost,
                        cost.builtins_execution_cost
                    );
                    saturating_add_assign!(
                        batched_transaction_details.costs.batched_bpf_execute_cost,
                        cost.bpf_execution_cost
                    );
                }
                Err(transaction_error) => match transaction_error {
                    TransactionError::WouldExceedMaxBlockCostLimit => {
                        saturating_add_assign!(
                            batched_transaction_details
                                .errors
                                .batched_retried_txs_per_block_limit_count,
                            1
                        );
                    }
                    TransactionError::WouldExceedMaxVoteCostLimit => {
                        saturating_add_assign!(
                            batched_transaction_details
                                .errors
                                .batched_retried_txs_per_vote_limit_count,
                            1
                        );
                    }
                    TransactionError::WouldExceedMaxAccountCostLimit => {
                        saturating_add_assign!(
                            batched_transaction_details
                                .errors
                                .batched_retried_txs_per_account_limit_count,
                            1
                        );
                    }
                    TransactionError::WouldExceedAccountDataBlockLimit => {
                        saturating_add_assign!(
                            batched_transaction_details
                                .errors
                                .batched_retried_txs_per_account_data_block_limit_count,
                            1
                        );
                    }
                    TransactionError::WouldExceedAccountDataTotalLimit => {
                        saturating_add_assign!(
                            batched_transaction_details
                                .errors
                                .batched_dropped_txs_per_account_data_total_limit_count,
                            1
                        );
                    }
                    _ => {}
                },
            });
        batched_transaction_details
    }

    fn accumulate_execute_units_and_time(execute_timings: &ExecuteTimings) -> (u64, u64) {
        let (units, times): (Vec<_>, Vec<_>) = execute_timings
            .details
            .per_program_timings
            .iter()
            .map(|(_program_id, program_timings)| {
                (
                    program_timings.accumulated_units,
                    program_timings.accumulated_us,
                )
            })
            .unzip();
        (units.iter().sum(), times.iter().sum())
    }

    /// Sends transactions to the bank.
    ///
    /// Returns the number of transactions successfully processed by the bank, which may be less
    /// than the total number if max PoH height was reached and the bank halted
    fn process_transactions(
        bank: &Arc<Bank>,
        bank_creation_time: &Instant,
        transactions: &[SanitizedTransaction],
        poh: &TransactionRecorder,
        transaction_status_sender: Option<TransactionStatusSender>,
        gossip_vote_sender: &ReplayVoteSender,
        qos_service: &QosService,
        log_messages_bytes_limit: Option<usize>,
    ) -> ProcessTransactionsSummary {
        let mut chunk_start = 0;
        let mut all_retryable_tx_indexes = vec![];
        // All the transactions that attempted execution. See description of
        // struct ProcessTransactionsSummary above for possible outcomes.
        let mut total_transactions_attempted_execution_count: usize = 0;
        // All transactions that were executed and committed
        let mut total_committed_transactions_count: usize = 0;
        // All transactions that were executed and committed with a successful result
        let mut total_committed_transactions_with_successful_result_count: usize = 0;
        // All transactions that were executed but then failed record because the
        // slot ended
        let mut total_failed_commit_count: usize = 0;
        let mut total_cost_model_throttled_transactions_count: usize = 0;
        let mut total_cost_model_us: u64 = 0;
        let mut total_execute_and_commit_timings = LeaderExecuteAndCommitTimings::default();
        let mut total_error_counters = TransactionErrorMetrics::default();
        let mut reached_max_poh_height = false;
        while chunk_start != transactions.len() {
            let chunk_end = std::cmp::min(
                transactions.len(),
                chunk_start + MAX_NUM_TRANSACTIONS_PER_BATCH,
            );
            let process_transaction_batch_output = Self::process_and_record_transactions(
                bank,
                &transactions[chunk_start..chunk_end],
                poh,
                chunk_start,
                transaction_status_sender.clone(),
                gossip_vote_sender,
                qos_service,
                log_messages_bytes_limit,
            );

            let ProcessTransactionBatchOutput {
                cost_model_throttled_transactions_count: new_cost_model_throttled_transactions_count,
                cost_model_us: new_cost_model_us,
                execute_and_commit_transactions_output,
            } = process_transaction_batch_output;
            total_cost_model_throttled_transactions_count =
                total_cost_model_throttled_transactions_count
                    .saturating_add(new_cost_model_throttled_transactions_count);
            total_cost_model_us = total_cost_model_us.saturating_add(new_cost_model_us);

            let ExecuteAndCommitTransactionsOutput {
                transactions_attempted_execution_count: new_transactions_attempted_execution_count,
                executed_transactions_count: new_executed_transactions_count,
                executed_with_successful_result_count: new_executed_with_successful_result_count,
                retryable_transaction_indexes: new_retryable_transaction_indexes,
                commit_transactions_result: new_commit_transactions_result,
                execute_and_commit_timings: new_execute_and_commit_timings,
                error_counters: new_error_counters,
                ..
            } = execute_and_commit_transactions_output;

            total_execute_and_commit_timings.accumulate(&new_execute_and_commit_timings);
            total_error_counters.accumulate(&new_error_counters);
            total_transactions_attempted_execution_count =
                total_transactions_attempted_execution_count
                    .saturating_add(new_transactions_attempted_execution_count);

            trace!(
                "process_transactions result: {:?}",
                new_commit_transactions_result
            );

            if new_commit_transactions_result.is_ok() {
                total_committed_transactions_count = total_committed_transactions_count
                    .saturating_add(new_executed_transactions_count);
                total_committed_transactions_with_successful_result_count =
                    total_committed_transactions_with_successful_result_count
                        .saturating_add(new_executed_with_successful_result_count);
            } else {
                total_failed_commit_count =
                    total_failed_commit_count.saturating_add(new_executed_transactions_count);
            }

            // Add the retryable txs (transactions that errored in a way that warrants a retry)
            // to the list of unprocessed txs.
            all_retryable_tx_indexes.extend_from_slice(&new_retryable_transaction_indexes);

            // If `bank_creation_time` is None, it's a test so ignore the option so
            // allow processing
            let should_bank_still_be_processing_txs =
                Bank::should_bank_still_be_processing_txs(bank_creation_time, bank.ns_per_slot);
            match (
                new_commit_transactions_result,
                should_bank_still_be_processing_txs,
            ) {
                (Err(PohRecorderError::MaxHeightReached), _) | (_, false) => {
                    info!(
                        "process transactions: max height reached slot: {} height: {}",
                        bank.slot(),
                        bank.tick_height()
                    );
                    // process_and_record_transactions has returned all retryable errors in
                    // transactions[chunk_start..chunk_end], so we just need to push the remaining
                    // transactions into the unprocessed queue.
                    all_retryable_tx_indexes.extend(chunk_end..transactions.len());
                    reached_max_poh_height = true;
                    break;
                }
                _ => (),
            }
            // Don't exit early on any other type of error, continue processing...
            chunk_start = chunk_end;
        }

        ProcessTransactionsSummary {
            reached_max_poh_height,
            transactions_attempted_execution_count: total_transactions_attempted_execution_count,
            committed_transactions_count: total_committed_transactions_count,
            committed_transactions_with_successful_result_count:
                total_committed_transactions_with_successful_result_count,
            failed_commit_count: total_failed_commit_count,
            retryable_transaction_indexes: all_retryable_tx_indexes,
            cost_model_throttled_transactions_count: total_cost_model_throttled_transactions_count,
            cost_model_us: total_cost_model_us,
            execute_and_commit_timings: total_execute_and_commit_timings,
            error_counters: total_error_counters,
        }
    }

    // This function creates a filter of transaction results with Ok() for every pending
    // transaction. The non-pending transactions are marked with TransactionError
    fn prepare_filter_for_pending_transactions(
        transactions_len: usize,
        pending_tx_indexes: &[usize],
    ) -> Vec<transaction::Result<()>> {
        let mut mask = vec![Err(TransactionError::BlockhashNotFound); transactions_len];
        pending_tx_indexes.iter().for_each(|x| mask[*x] = Ok(()));
        mask
    }

    // This function returns a vector containing index of all valid transactions. A valid
    // transaction has result Ok() as the value
    fn filter_valid_transaction_indexes(
        valid_txs: &[TransactionCheckResult],
        transaction_indexes: &[usize],
    ) -> Vec<usize> {
        valid_txs
            .iter()
            .enumerate()
            .filter_map(|(index, (x, _h))| if x.is_ok() { Some(index) } else { None })
            .map(|x| transaction_indexes[x])
            .collect_vec()
    }

    /// This function filters pending packets that are still valid
    /// # Arguments
    /// * `transactions` - a batch of transactions deserialized from packets
    /// * `transaction_to_packet_indexes` - maps each transaction to a packet index
    /// * `pending_indexes` - identifies which indexes in the `transactions` list are still pending
    fn filter_pending_packets_from_pending_txs(
        bank: &Arc<Bank>,
        transactions: &[SanitizedTransaction],
        transaction_to_packet_indexes: &[usize],
        pending_indexes: &[usize],
    ) -> Vec<usize> {
        let filter =
            Self::prepare_filter_for_pending_transactions(transactions.len(), pending_indexes);

        let mut error_counters = TransactionErrorMetrics::default();
        // The following code also checks if the blockhash for a transaction is too old
        // The check accounts for
        //  1. Transaction forwarding delay
        //  2. The slot at which the next leader will actually process the transaction
        // Drop the transaction if it will expire by the time the next node receives and processes it
        let api = perf_libs::api();
        let max_tx_fwd_delay = if api.is_none() {
            MAX_TRANSACTION_FORWARDING_DELAY
        } else {
            MAX_TRANSACTION_FORWARDING_DELAY_GPU
        };

        let results = bank.check_transactions(
            transactions,
            &filter,
            (MAX_PROCESSING_AGE)
                .saturating_sub(max_tx_fwd_delay)
                .saturating_sub(FORWARD_TRANSACTIONS_TO_LEADER_AT_SLOT_OFFSET as usize),
            &mut error_counters,
        );

        Self::filter_valid_transaction_indexes(&results, transaction_to_packet_indexes)
    }

    fn filter_processed_packets<'a, F>(
        retryable_transaction_indexes: impl Iterator<Item = &'a usize>,
        mut f: F,
    ) where
        F: FnMut(usize, usize),
    {
        let mut prev_retryable_index = 0;
        for (i, retryable_index) in retryable_transaction_indexes.enumerate() {
            let start = if i == 0 { 0 } else { prev_retryable_index + 1 };

            let end = *retryable_index;
            prev_retryable_index = *retryable_index;

            if start < end {
                f(start, end)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn process_packets_transactions<'a>(
        bank: &'a Arc<Bank>,
        bank_creation_time: &Instant,
        poh: &'a TransactionRecorder,
        deserialized_packets: impl Iterator<Item = &'a ImmutableDeserializedPacket>,
        transaction_status_sender: Option<TransactionStatusSender>,
        gossip_vote_sender: &'a ReplayVoteSender,
        banking_stage_stats: &'a BankingStageStats,
        qos_service: &'a QosService,
        slot_metrics_tracker: &'a mut LeaderSlotMetricsTracker,
        log_messages_bytes_limit: Option<usize>,
    ) -> ProcessTransactionsSummary {
        // Convert packets to transactions
        let ((transactions, transaction_to_packet_indexes), packet_conversion_time): (
            (Vec<SanitizedTransaction>, Vec<usize>),
            _,
        ) = measure!(
            deserialized_packets
                .enumerate()
                .filter_map(|(i, deserialized_packet)| {
                    unprocessed_packet_batches::transaction_from_deserialized_packet(
                        deserialized_packet,
                        &bank.feature_set,
                        bank.vote_only_bank(),
                        bank.as_ref(),
                    )
                    .map(|transaction| (transaction, i))
                })
                .unzip(),
            "packet_conversion",
        );

        let packet_conversion_us = packet_conversion_time.as_us();
        slot_metrics_tracker.increment_transactions_from_packets_us(packet_conversion_us);
        banking_stage_stats
            .packet_conversion_elapsed
            .fetch_add(packet_conversion_us, Ordering::Relaxed);
        inc_new_counter_info!("banking_stage-packet_conversion", 1);

        // Process transactions
        let (mut process_transactions_summary, process_transactions_time) = measure!(
            Self::process_transactions(
                bank,
                bank_creation_time,
                &transactions,
                poh,
                transaction_status_sender,
                gossip_vote_sender,
                qos_service,
                log_messages_bytes_limit,
            ),
            "process_transaction_time",
        );
        let process_transactions_us = process_transactions_time.as_us();
        slot_metrics_tracker.increment_process_transactions_us(process_transactions_us);
        banking_stage_stats
            .transaction_processing_elapsed
            .fetch_add(process_transactions_us, Ordering::Relaxed);

        let ProcessTransactionsSummary {
            ref retryable_transaction_indexes,
            ref error_counters,
            ..
        } = process_transactions_summary;

        slot_metrics_tracker.accumulate_process_transactions_summary(&process_transactions_summary);
        slot_metrics_tracker.accumulate_transaction_errors(error_counters);

        let retryable_tx_count = retryable_transaction_indexes.len();
        inc_new_counter_info!("banking_stage-unprocessed_transactions", retryable_tx_count);

        // Filter out the retryable transactions that are too old
        let (filtered_retryable_transaction_indexes, filter_retryable_packets_time) = measure!(
            Self::filter_pending_packets_from_pending_txs(
                bank,
                &transactions,
                &transaction_to_packet_indexes,
                retryable_transaction_indexes,
            ),
            "filter_pending_packets_time",
        );
        let filter_retryable_packets_us = filter_retryable_packets_time.as_us();
        slot_metrics_tracker
            .increment_filter_retryable_packets_us(filter_retryable_packets_us as u64);
        banking_stage_stats
            .filter_pending_packets_elapsed
            .fetch_add(filter_retryable_packets_us, Ordering::Relaxed);

        let retryable_packets_filtered_count = retryable_transaction_indexes
            .len()
            .saturating_sub(filtered_retryable_transaction_indexes.len());
        slot_metrics_tracker
            .increment_retryable_packets_filtered_count(retryable_packets_filtered_count as u64);

        inc_new_counter_info!(
            "banking_stage-dropped_tx_before_forwarding",
            retryable_transaction_indexes
                .len()
                .saturating_sub(filtered_retryable_transaction_indexes.len())
        );

        process_transactions_summary.retryable_transaction_indexes =
            filtered_retryable_transaction_indexes;
        process_transactions_summary
    }

    fn generate_packet_indexes(vers: &PacketBatch) -> Vec<usize> {
        vers.iter()
            .enumerate()
            .filter(|(_, pkt)| !pkt.meta.discard())
            .map(|(index, _)| index)
            .collect()
    }

    fn receive_until(
        verified_receiver: &BankingPacketReceiver,
        recv_timeout: Duration,
        packet_count_upperbound: usize,
    ) -> Result<(Vec<PacketBatch>, Option<SigverifyTracerPacketStats>), RecvTimeoutError> {
        let start = Instant::now();
        let mut aggregated_tracer_packet_stats_option: Option<SigverifyTracerPacketStats> = None;
        let (mut packet_batches, new_tracer_packet_stats_option) =
            verified_receiver.recv_timeout(recv_timeout)?;

        if let Some(new_tracer_packet_stats) = &new_tracer_packet_stats_option {
            if let Some(aggregated_tracer_packet_stats) = &mut aggregated_tracer_packet_stats_option
            {
                aggregated_tracer_packet_stats.aggregate(new_tracer_packet_stats);
            } else {
                aggregated_tracer_packet_stats_option = new_tracer_packet_stats_option;
            }
        }

        let mut num_packets_received: usize = packet_batches.iter().map(|batch| batch.len()).sum();
        while let Ok((packet_batch, _tracer_packet_stats_option)) = verified_receiver.try_recv() {
            trace!("got more packet batches in banking stage");
            let (packets_received, packet_count_overflowed) = num_packets_received
                .overflowing_add(packet_batch.iter().map(|batch| batch.len()).sum());
            packet_batches.extend(packet_batch);

            // Spend any leftover receive time budget to greedily receive more packet batches,
            // until the upperbound of the packet count is reached.
            if start.elapsed() >= recv_timeout
                || packet_count_overflowed
                || packets_received >= packet_count_upperbound
            {
                break;
            }
            num_packets_received = packets_received;
        }
        Ok((packet_batches, aggregated_tracer_packet_stats_option))
    }

    #[allow(clippy::too_many_arguments)]
    /// Receive incoming packets, push into unprocessed buffer with packet indexes
    fn receive_and_buffer_packets(
        verified_receiver: &BankingPacketReceiver,
        recv_start: &mut Instant,
        recv_timeout: Duration,
        id: u32,
        buffered_packet_batches: &mut UnprocessedPacketBatches,
        banking_stage_stats: &mut BankingStageStats,
        tracer_packet_stats: &mut TracerPacketStats,
        slot_metrics_tracker: &mut LeaderSlotMetricsTracker,
    ) -> Result<(), RecvTimeoutError> {
        let mut recv_time = Measure::start("receive_and_buffer_packets_recv");
        let (packet_batches, new_sigverify_tracer_packet_stats_option) = Self::receive_until(
            verified_receiver,
            recv_timeout,
            buffered_packet_batches.capacity() - buffered_packet_batches.len(),
        )?;

        if let Some(new_sigverify_tracer_packet_stats) = &new_sigverify_tracer_packet_stats_option {
            tracer_packet_stats
                .aggregate_sigverify_tracer_packet_stats(new_sigverify_tracer_packet_stats);
        }

        let packet_count: usize = packet_batches.iter().map(|x| x.len()).sum();
        debug!(
            "@{:?} process start stalled for: {:?}ms txs: {} id: {}",
            timestamp(),
            duration_as_ms(&recv_start.elapsed()),
            packet_count,
            id,
        );

        let packet_batch_iter = packet_batches.into_iter();
        let mut dropped_packets_count = 0;
        let mut newly_buffered_packets_count = 0;
        for packet_batch in packet_batch_iter {
            let packet_indexes = Self::generate_packet_indexes(&packet_batch);
            // Track all the packets incoming from sigverify, both valid and invalid
            slot_metrics_tracker.increment_total_new_valid_packets(packet_indexes.len() as u64);
            slot_metrics_tracker.increment_newly_failed_sigverify_count(
                packet_batch.len().saturating_sub(packet_indexes.len()) as u64,
            );

            Self::push_unprocessed(
                buffered_packet_batches,
                &packet_batch,
                &packet_indexes,
                &mut dropped_packets_count,
                &mut newly_buffered_packets_count,
                banking_stage_stats,
                slot_metrics_tracker,
                tracer_packet_stats,
            )
        }
        recv_time.stop();

        banking_stage_stats
            .receive_and_buffer_packets_elapsed
            .fetch_add(recv_time.as_us(), Ordering::Relaxed);
        banking_stage_stats
            .receive_and_buffer_packets_count
            .fetch_add(packet_count, Ordering::Relaxed);
        banking_stage_stats
            .dropped_packets_count
            .fetch_add(dropped_packets_count, Ordering::Relaxed);
        banking_stage_stats
            .newly_buffered_packets_count
            .fetch_add(newly_buffered_packets_count, Ordering::Relaxed);
        banking_stage_stats
            .current_buffered_packet_batches_count
            .swap(buffered_packet_batches.len(), Ordering::Relaxed);
        banking_stage_stats
            .current_buffered_packets_count
            .swap(buffered_packet_batches.len(), Ordering::Relaxed);
        *recv_start = Instant::now();
        Ok(())
    }

    fn push_unprocessed(
        unprocessed_packet_batches: &mut UnprocessedPacketBatches,
        packet_batch: &PacketBatch,
        packet_indexes: &[usize],
        dropped_packets_count: &mut usize,
        newly_buffered_packets_count: &mut usize,
        banking_stage_stats: &mut BankingStageStats,
        slot_metrics_tracker: &mut LeaderSlotMetricsTracker,
        tracer_packet_stats: &mut TracerPacketStats,
    ) {
        if !packet_indexes.is_empty() {
            let _ = banking_stage_stats
                .batch_packet_indexes_len
                .increment(packet_indexes.len() as u64);

            *newly_buffered_packets_count += packet_indexes.len();
            slot_metrics_tracker
                .increment_newly_buffered_packets_count(packet_indexes.len() as u64);

            let (number_of_dropped_packets, number_of_dropped_tracer_packets) =
                unprocessed_packet_batches.insert_batch(
                    unprocessed_packet_batches::deserialize_packets(packet_batch, packet_indexes),
                );

            saturating_add_assign!(*dropped_packets_count, number_of_dropped_packets);
            slot_metrics_tracker.increment_exceeded_buffer_limit_dropped_packets_count(
                number_of_dropped_packets as u64,
            );

            tracer_packet_stats
                .increment_total_exceeded_banking_stage_buffer(number_of_dropped_tracer_packets);
        }
    }

    pub fn join(self) -> thread::Result<()> {
        for bank_thread_hdl in self.bank_thread_hdls {
            bank_thread_hdl.join()?;
        }
        Ok(())
    }
}

pub(crate) fn next_leader_tpu(
    cluster_info: &ClusterInfo,
    poh_recorder: &RwLock<PohRecorder>,
) -> Option<(Pubkey, std::net::SocketAddr)> {
    next_leader_x(cluster_info, poh_recorder, |leader| leader.tpu)
}

fn next_leader_tpu_forwards(
    cluster_info: &ClusterInfo,
    poh_recorder: &RwLock<PohRecorder>,
) -> Option<(Pubkey, std::net::SocketAddr)> {
    next_leader_x(cluster_info, poh_recorder, |leader| leader.tpu_forwards)
}

pub(crate) fn next_leader_tpu_vote(
    cluster_info: &ClusterInfo,
    poh_recorder: &RwLock<PohRecorder>,
) -> Option<(Pubkey, std::net::SocketAddr)> {
    next_leader_x(cluster_info, poh_recorder, |leader| leader.tpu_vote)
}

fn next_leader_x<F>(
    cluster_info: &ClusterInfo,
    poh_recorder: &RwLock<PohRecorder>,
    port_selector: F,
) -> Option<(Pubkey, std::net::SocketAddr)>
where
    F: FnOnce(&ContactInfo) -> SocketAddr,
{
    let leader_pubkey = poh_recorder
        .read()
        .unwrap()
        .leader_after_n_slots(FORWARD_TRANSACTIONS_TO_LEADER_AT_SLOT_OFFSET);
    if let Some(leader_pubkey) = leader_pubkey {
        cluster_info
            .lookup_contact_info(&leader_pubkey, port_selector)
            .map(|addr| (leader_pubkey, addr))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crossbeam_channel::{unbounded, Receiver},
        solana_address_lookup_table_program::state::{AddressLookupTable, LookupTableMeta},
        solana_entry::entry::{next_entry, next_versioned_entry, Entry, EntrySlice},
        solana_gossip::{cluster_info::Node, contact_info::ContactInfo},
        solana_ledger::{
            blockstore::{entries_to_test_shreds, Blockstore},
            genesis_utils::{create_genesis_config, GenesisConfigInfo},
            get_tmp_ledger_path_auto_delete,
            leader_schedule_cache::LeaderScheduleCache,
        },
        solana_perf::packet::{to_packet_batches, PacketFlags},
        solana_poh::{
            poh_recorder::{create_test_recorder, Record, WorkingBankEntry},
            poh_service::PohService,
        },
        solana_program_runtime::timings::ProgramTiming,
        solana_rpc::transaction_status_service::TransactionStatusService,
        solana_runtime::bank_forks::BankForks,
        solana_sdk::{
            account::AccountSharedData,
            hash::Hash,
            instruction::InstructionError,
            message::{
                v0::{self, MessageAddressTableLookup},
                MessageHeader, VersionedMessage,
            },
            poh_config::PohConfig,
            signature::{Keypair, Signer},
            system_transaction,
            transaction::{MessageHash, Transaction, TransactionError, VersionedTransaction},
        },
        solana_streamer::{recvmmsg::recv_mmsg, socket::SocketAddrSpace},
        solana_transaction_status::{TransactionStatusMeta, VersionedTransactionWithStatusMeta},
        std::{
            borrow::Cow,
            collections::HashSet,
            path::Path,
            sync::atomic::{AtomicBool, Ordering},
            thread::sleep,
        },
        unprocessed_packet_batches::DeserializedPacket,
    };

    fn new_test_cluster_info(contact_info: ContactInfo) -> ClusterInfo {
        ClusterInfo::new(
            contact_info,
            Arc::new(Keypair::new()),
            SocketAddrSpace::Unspecified,
        )
    }

    #[test]
    fn test_banking_stage_shutdown1() {
        let genesis_config = create_genesis_config(2).genesis_config;
        let bank = Bank::new_no_wallclock_throttle_for_tests(&genesis_config);
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let bank = Arc::new(bank_forks.read().unwrap().get(0).unwrap());
        let (verified_sender, verified_receiver) = unbounded();
        let (gossip_verified_vote_sender, gossip_verified_vote_receiver) = unbounded();
        let (tpu_vote_sender, tpu_vote_receiver) = unbounded();
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        {
            let blockstore = Arc::new(
                Blockstore::open(ledger_path.path())
                    .expect("Expected to be able to open database ledger"),
            );
            let (exit, poh_recorder, poh_service, _entry_receiever) =
                create_test_recorder(&bank, &blockstore, None, None);
            let cluster_info = new_test_cluster_info(Node::new_localhost().info);
            let cluster_info = Arc::new(cluster_info);
            let (gossip_vote_sender, _gossip_vote_receiver) = unbounded();

            let banking_stage = BankingStage::new(
                &cluster_info,
                &poh_recorder,
                verified_receiver,
                tpu_vote_receiver,
                gossip_verified_vote_receiver,
                None,
                gossip_vote_sender,
                Arc::new(RwLock::new(CostModel::default())),
                None,
                Arc::new(ConnectionCache::default()),
                bank_forks,
            );
            drop(verified_sender);
            drop(gossip_verified_vote_sender);
            drop(tpu_vote_sender);
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
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let bank = Arc::new(bank_forks.read().unwrap().get(0).unwrap());
        let start_hash = bank.last_blockhash();
        let (verified_sender, verified_receiver) = unbounded();
        let (tpu_vote_sender, tpu_vote_receiver) = unbounded();
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
                create_test_recorder(&bank, &blockstore, Some(poh_config), None);
            let cluster_info = new_test_cluster_info(Node::new_localhost().info);
            let cluster_info = Arc::new(cluster_info);
            let (verified_gossip_vote_sender, verified_gossip_vote_receiver) = unbounded();
            let (gossip_vote_sender, _gossip_vote_receiver) = unbounded();

            let banking_stage = BankingStage::new(
                &cluster_info,
                &poh_recorder,
                verified_receiver,
                tpu_vote_receiver,
                verified_gossip_vote_receiver,
                None,
                gossip_vote_sender,
                Arc::new(RwLock::new(CostModel::default())),
                None,
                Arc::new(ConnectionCache::default()),
                bank_forks,
            );
            trace!("sending bank");
            drop(verified_sender);
            drop(verified_gossip_vote_sender);
            drop(tpu_vote_sender);
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
                .for_each(|(p, f)| p.meta.set_discard(*f == 0))
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
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let bank = Arc::new(bank_forks.read().unwrap().get(0).unwrap());
        let start_hash = bank.last_blockhash();
        let (verified_sender, verified_receiver) = unbounded();
        let (tpu_vote_sender, tpu_vote_receiver) = unbounded();
        let (gossip_verified_vote_sender, gossip_verified_vote_receiver) = unbounded();
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
                create_test_recorder(&bank, &blockstore, Some(poh_config), None);
            let cluster_info = new_test_cluster_info(Node::new_localhost().info);
            let cluster_info = Arc::new(cluster_info);
            let (gossip_vote_sender, _gossip_vote_receiver) = unbounded();

            let banking_stage = BankingStage::new(
                &cluster_info,
                &poh_recorder,
                verified_receiver,
                tpu_vote_receiver,
                gossip_verified_vote_receiver,
                None,
                gossip_vote_sender,
                Arc::new(RwLock::new(CostModel::default())),
                None,
                Arc::new(ConnectionCache::default()),
                bank_forks,
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
            verified_sender // no_ver, anf, tx
                .send((packet_batches, None))
                .unwrap();

            drop(verified_sender);
            drop(tpu_vote_sender);
            drop(gossip_verified_vote_sender);
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
        let (verified_sender, verified_receiver) = unbounded();

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
        verified_sender.send((packet_batches, None)).unwrap();

        // Process a second batch that uses the same from account, so conflicts with above TX
        let tx =
            system_transaction::transfer(&mint_keypair, &alice.pubkey(), 1, genesis_config.hash());
        let packet_batches = to_packet_batches(&[tx], 1);
        let packet_batches = packet_batches
            .into_iter()
            .map(|batch| (batch, vec![1u8]))
            .collect();
        let packet_batches = convert_from_old_verified(packet_batches);
        verified_sender.send((packet_batches, None)).unwrap();

        let (vote_sender, vote_receiver) = unbounded();
        let (tpu_vote_sender, tpu_vote_receiver) = unbounded();
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        {
            let (gossip_vote_sender, _gossip_vote_receiver) = unbounded();

            let entry_receiver = {
                // start a banking_stage to eat verified receiver
                let bank = Bank::new_no_wallclock_throttle_for_tests(&genesis_config);
                let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
                let bank = Arc::new(bank_forks.read().unwrap().get(0).unwrap());
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
                    create_test_recorder(&bank, &blockstore, Some(poh_config), None);
                let cluster_info = new_test_cluster_info(Node::new_localhost().info);
                let cluster_info = Arc::new(cluster_info);
                let _banking_stage = BankingStage::new_num_threads(
                    &cluster_info,
                    &poh_recorder,
                    verified_receiver,
                    tpu_vote_receiver,
                    vote_receiver,
                    3,
                    None,
                    gossip_vote_sender,
                    Arc::new(RwLock::new(CostModel::default())),
                    None,
                    Arc::new(ConnectionCache::default()),
                    bank_forks,
                );

                // wait for banking_stage to eat the packets
                while bank.get_balance(&alice.pubkey()) < 1 {
                    sleep(Duration::from_millis(10));
                }
                exit.store(true, Ordering::Relaxed);
                poh_service.join().unwrap();
                entry_receiver
            };
            drop(verified_sender);
            drop(vote_sender);
            drop(tpu_vote_sender);

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

    fn sanitize_transactions(txs: Vec<Transaction>) -> Vec<SanitizedTransaction> {
        txs.into_iter()
            .map(SanitizedTransaction::from_transaction_for_tests)
            .collect()
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
                &Arc::new(blockstore),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
                &Arc::new(PohConfig::default()),
            );
            let recorder = poh_recorder.recorder();
            let poh_recorder = Arc::new(RwLock::new(poh_recorder));

            let (poh_simulator, exit) = simulate_poh(record_receiver, &poh_recorder);

            poh_recorder.write().unwrap().set_bank(&bank, false);
            let pubkey = solana_sdk::pubkey::new_rand();
            let keypair2 = Keypair::new();
            let pubkey2 = solana_sdk::pubkey::new_rand();

            let txs = vec![
                system_transaction::transfer(&mint_keypair, &pubkey, 1, genesis_config.hash())
                    .into(),
                system_transaction::transfer(&keypair2, &pubkey2, 1, genesis_config.hash()).into(),
            ];

            let _ = BankingStage::record_transactions(bank.slot(), txs.clone(), &recorder);
            let (_bank, (entry, _tick_height)) = entry_receiver.recv().unwrap();
            assert_eq!(entry.transactions, txs);

            // Once bank is set to a new bank (setting bank.slot() + 1 in record_transactions),
            // record_transactions should throw MaxHeightReached
            let next_slot = bank.slot() + 1;
            let RecordTransactionsSummary { result, .. } =
                BankingStage::record_transactions(next_slot, txs, &recorder);
            assert_matches!(result, Err(PohRecorderError::MaxHeightReached));
            // Should receive nothing from PohRecorder b/c record failed
            assert!(entry_receiver.try_recv().is_err());

            exit.store(true, Ordering::Relaxed);
            let _ = poh_simulator.join();
        }
        Blockstore::destroy(ledger_path.path()).unwrap();
    }

    #[test]
    fn test_bank_prepare_filter_for_pending_transaction() {
        assert_eq!(
            BankingStage::prepare_filter_for_pending_transactions(6, &[2, 4, 5]),
            vec![
                Err(TransactionError::BlockhashNotFound),
                Err(TransactionError::BlockhashNotFound),
                Ok(()),
                Err(TransactionError::BlockhashNotFound),
                Ok(()),
                Ok(())
            ]
        );

        assert_eq!(
            BankingStage::prepare_filter_for_pending_transactions(6, &[0, 2, 3]),
            vec![
                Ok(()),
                Err(TransactionError::BlockhashNotFound),
                Ok(()),
                Ok(()),
                Err(TransactionError::BlockhashNotFound),
                Err(TransactionError::BlockhashNotFound),
            ]
        );
    }

    #[test]
    fn test_bank_filter_valid_transaction_indexes() {
        assert_eq!(
            BankingStage::filter_valid_transaction_indexes(
                &[
                    (Err(TransactionError::BlockhashNotFound), None),
                    (Err(TransactionError::BlockhashNotFound), None),
                    (Ok(()), None),
                    (Err(TransactionError::BlockhashNotFound), None),
                    (Ok(()), None),
                    (Ok(()), None),
                ],
                &[2, 4, 5, 9, 11, 13]
            ),
            [5, 11, 13]
        );

        assert_eq!(
            BankingStage::filter_valid_transaction_indexes(
                &[
                    (Ok(()), None),
                    (Err(TransactionError::BlockhashNotFound), None),
                    (Err(TransactionError::BlockhashNotFound), None),
                    (Ok(()), None),
                    (Ok(()), None),
                    (Ok(()), None),
                ],
                &[1, 6, 7, 9, 31, 43]
            ),
            [1, 9, 31, 43]
        );
    }

    #[test]
    fn test_should_process_or_forward_packets() {
        let my_pubkey = solana_sdk::pubkey::new_rand();
        let my_pubkey1 = solana_sdk::pubkey::new_rand();
        let bank = Arc::new(Bank::default_for_tests());
        // having active bank allows to consume immediately
        assert_matches!(
            BankingStage::consume_or_forward_packets(&my_pubkey, None, Some(&bank), false, false),
            BufferedPacketsDecision::Consume(_)
        );
        assert_matches!(
            BankingStage::consume_or_forward_packets(&my_pubkey, None, None, false, false),
            BufferedPacketsDecision::Hold
        );
        assert_matches!(
            BankingStage::consume_or_forward_packets(&my_pubkey1, None, None, false, false),
            BufferedPacketsDecision::Hold
        );

        assert_matches!(
            BankingStage::consume_or_forward_packets(
                &my_pubkey,
                Some(my_pubkey1),
                None,
                false,
                false
            ),
            BufferedPacketsDecision::Forward
        );

        assert_matches!(
            BankingStage::consume_or_forward_packets(
                &my_pubkey,
                Some(my_pubkey1),
                None,
                true,
                true
            ),
            BufferedPacketsDecision::Hold
        );
        assert_matches!(
            BankingStage::consume_or_forward_packets(
                &my_pubkey,
                Some(my_pubkey1),
                None,
                true,
                false
            ),
            BufferedPacketsDecision::ForwardAndHold
        );
        assert_matches!(
            BankingStage::consume_or_forward_packets(
                &my_pubkey,
                Some(my_pubkey1),
                Some(&bank),
                false,
                false
            ),
            BufferedPacketsDecision::Consume(_)
        );
        assert_matches!(
            BankingStage::consume_or_forward_packets(
                &my_pubkey1,
                Some(my_pubkey1),
                None,
                false,
                false
            ),
            BufferedPacketsDecision::Hold
        );
        assert_matches!(
            BankingStage::consume_or_forward_packets(
                &my_pubkey1,
                Some(my_pubkey1),
                Some(&bank),
                false,
                false
            ),
            BufferedPacketsDecision::Consume(_)
        );
    }

    fn create_slow_genesis_config(lamports: u64) -> GenesisConfigInfo {
        let mut config_info = create_genesis_config(lamports);
        // For these tests there's only 1 slot, don't want to run out of ticks
        config_info.genesis_config.ticks_per_slot *= 8;
        config_info
    }

    #[test]
    fn test_bank_process_and_record_transactions() {
        solana_logger::setup();
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_slow_genesis_config(10_000);
        let bank = Arc::new(Bank::new_no_wallclock_throttle_for_tests(&genesis_config));
        let pubkey = solana_sdk::pubkey::new_rand();

        let transactions = sanitize_transactions(vec![system_transaction::transfer(
            &mint_keypair,
            &pubkey,
            1,
            genesis_config.hash(),
        )]);

        let ledger_path = get_tmp_ledger_path_auto_delete!();
        {
            let blockstore = Blockstore::open(ledger_path.path())
                .expect("Expected to be able to open database ledger");
            let (poh_recorder, entry_receiver, record_receiver) = PohRecorder::new(
                bank.tick_height(),
                bank.last_blockhash(),
                bank.clone(),
                Some((4, 4)),
                bank.ticks_per_slot(),
                &pubkey,
                &Arc::new(blockstore),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
                &Arc::new(PohConfig::default()),
            );
            let recorder = poh_recorder.recorder();
            let poh_recorder = Arc::new(RwLock::new(poh_recorder));

            let (poh_simulator, exit) = simulate_poh(record_receiver, &poh_recorder);

            poh_recorder.write().unwrap().set_bank(&bank, false);
            let (gossip_vote_sender, _gossip_vote_receiver) = unbounded();

            let process_transactions_batch_output = BankingStage::process_and_record_transactions(
                &bank,
                &transactions,
                &recorder,
                0,
                None,
                &gossip_vote_sender,
                &QosService::new(Arc::new(RwLock::new(CostModel::default())), 1),
                None,
            );

            let ExecuteAndCommitTransactionsOutput {
                transactions_attempted_execution_count,
                executed_transactions_count,
                executed_with_successful_result_count,
                commit_transactions_result,
                ..
            } = process_transactions_batch_output.execute_and_commit_transactions_output;

            assert_eq!(transactions_attempted_execution_count, 1);
            assert_eq!(executed_transactions_count, 1);
            assert_eq!(executed_with_successful_result_count, 1);
            assert!(commit_transactions_result.is_ok());

            // Tick up to max tick height
            while poh_recorder.read().unwrap().tick_height() != bank.max_tick_height() {
                poh_recorder.write().unwrap().tick();
            }

            let mut done = false;
            // read entries until I find mine, might be ticks...
            while let Ok((_bank, (entry, _tick_height))) = entry_receiver.recv() {
                if !entry.is_tick() {
                    trace!("got entry");
                    assert_eq!(entry.transactions.len(), transactions.len());
                    assert_eq!(bank.get_balance(&pubkey), 1);
                    done = true;
                }
                if done {
                    break;
                }
            }
            trace!("done ticking");

            assert!(done);

            let transactions = sanitize_transactions(vec![system_transaction::transfer(
                &mint_keypair,
                &pubkey,
                2,
                genesis_config.hash(),
            )]);

            let process_transactions_batch_output = BankingStage::process_and_record_transactions(
                &bank,
                &transactions,
                &recorder,
                0,
                None,
                &gossip_vote_sender,
                &QosService::new(Arc::new(RwLock::new(CostModel::default())), 1),
                None,
            );

            let ExecuteAndCommitTransactionsOutput {
                transactions_attempted_execution_count,
                executed_transactions_count,
                executed_with_successful_result_count,
                retryable_transaction_indexes,
                commit_transactions_result,
                ..
            } = process_transactions_batch_output.execute_and_commit_transactions_output;
            assert_eq!(transactions_attempted_execution_count, 1);
            // Transactions was still executed, just wasn't committed, so should be counted here.
            assert_eq!(executed_transactions_count, 1);
            assert_eq!(executed_with_successful_result_count, 1);
            assert_eq!(retryable_transaction_indexes, vec![0]);
            assert_matches!(
                commit_transactions_result,
                Err(PohRecorderError::MaxHeightReached)
            );

            exit.store(true, Ordering::Relaxed);
            let _ = poh_simulator.join();

            assert_eq!(bank.get_balance(&pubkey), 1);
        }
        Blockstore::destroy(ledger_path.path()).unwrap();
    }

    #[test]
    fn test_bank_process_and_record_transactions_all_unexecuted() {
        solana_logger::setup();
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_slow_genesis_config(10_000);
        let bank = Arc::new(Bank::new_no_wallclock_throttle_for_tests(&genesis_config));
        let pubkey = solana_sdk::pubkey::new_rand();

        let transactions = {
            let mut tx =
                system_transaction::transfer(&mint_keypair, &pubkey, 1, genesis_config.hash());
            // Add duplicate account key
            tx.message.account_keys.push(pubkey);
            sanitize_transactions(vec![tx])
        };

        let ledger_path = get_tmp_ledger_path_auto_delete!();
        {
            let blockstore = Blockstore::open(ledger_path.path())
                .expect("Expected to be able to open database ledger");
            let (poh_recorder, _entry_receiver, record_receiver) = PohRecorder::new(
                bank.tick_height(),
                bank.last_blockhash(),
                bank.clone(),
                Some((4, 4)),
                bank.ticks_per_slot(),
                &pubkey,
                &Arc::new(blockstore),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
                &Arc::new(PohConfig::default()),
            );
            let recorder = poh_recorder.recorder();
            let poh_recorder = Arc::new(RwLock::new(poh_recorder));

            let (poh_simulator, exit) = simulate_poh(record_receiver, &poh_recorder);

            poh_recorder.write().unwrap().set_bank(&bank, false);
            let (gossip_vote_sender, _gossip_vote_receiver) = unbounded();

            let process_transactions_batch_output = BankingStage::process_and_record_transactions(
                &bank,
                &transactions,
                &recorder,
                0,
                None,
                &gossip_vote_sender,
                &QosService::new(Arc::new(RwLock::new(CostModel::default())), 1),
                None,
            );

            let ExecuteAndCommitTransactionsOutput {
                transactions_attempted_execution_count,
                executed_transactions_count,
                executed_with_successful_result_count,
                commit_transactions_result,
                retryable_transaction_indexes,
                ..
            } = process_transactions_batch_output.execute_and_commit_transactions_output;

            assert_eq!(transactions_attempted_execution_count, 1);
            assert_eq!(executed_transactions_count, 0);
            assert_eq!(executed_with_successful_result_count, 0);
            assert!(retryable_transaction_indexes.is_empty());
            assert_eq!(
                commit_transactions_result.ok(),
                Some(vec![CommitTransactionDetails::NotCommitted; 1])
            );

            exit.store(true, Ordering::Relaxed);
            let _ = poh_simulator.join();
        }
        Blockstore::destroy(ledger_path.path()).unwrap();
    }

    #[test]
    fn test_bank_process_and_record_transactions_cost_tracker() {
        solana_logger::setup();
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_slow_genesis_config(10_000);
        let bank = Arc::new(Bank::new_no_wallclock_throttle_for_tests(&genesis_config));
        let pubkey = solana_sdk::pubkey::new_rand();

        let ledger_path = get_tmp_ledger_path_auto_delete!();
        {
            let blockstore = Blockstore::open(ledger_path.path())
                .expect("Expected to be able to open database ledger");
            let (poh_recorder, _entry_receiver, record_receiver) = PohRecorder::new(
                bank.tick_height(),
                bank.last_blockhash(),
                bank.clone(),
                Some((4, 4)),
                bank.ticks_per_slot(),
                &pubkey,
                &Arc::new(blockstore),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
                &Arc::new(PohConfig::default()),
            );
            let recorder = poh_recorder.recorder();
            let poh_recorder = Arc::new(RwLock::new(poh_recorder));

            let (poh_simulator, exit) = simulate_poh(record_receiver, &poh_recorder);

            poh_recorder.write().unwrap().set_bank(&bank, false);
            let (gossip_vote_sender, _gossip_vote_receiver) = unbounded();

            let qos_service = QosService::new(Arc::new(RwLock::new(CostModel::default())), 1);

            let get_block_cost = || bank.read_cost_tracker().unwrap().block_cost();
            let get_tx_count = || bank.read_cost_tracker().unwrap().transaction_count();
            assert_eq!(get_block_cost(), 0);
            assert_eq!(get_tx_count(), 0);

            //
            // TEST: cost tracker's block cost increases when successfully processing a tx
            //

            let transactions = sanitize_transactions(vec![system_transaction::transfer(
                &mint_keypair,
                &pubkey,
                1,
                genesis_config.hash(),
            )]);

            let process_transactions_batch_output = BankingStage::process_and_record_transactions(
                &bank,
                &transactions,
                &recorder,
                0,
                None,
                &gossip_vote_sender,
                &qos_service,
                None,
            );

            let ExecuteAndCommitTransactionsOutput {
                executed_with_successful_result_count,
                commit_transactions_result,
                ..
            } = process_transactions_batch_output.execute_and_commit_transactions_output;
            assert_eq!(executed_with_successful_result_count, 1);
            assert!(commit_transactions_result.is_ok());

            let single_transfer_cost = get_block_cost();
            assert_ne!(single_transfer_cost, 0);
            assert_eq!(get_tx_count(), 1);

            //
            // TEST: When a tx in a batch can't be executed (here because of account
            // locks), then its cost does not affect the cost tracker.
            //

            let allocate_keypair = Keypair::new();
            let transactions = sanitize_transactions(vec![
                system_transaction::transfer(&mint_keypair, &pubkey, 2, genesis_config.hash()),
                // intentionally use a tx that has a different cost
                system_transaction::allocate(
                    &mint_keypair,
                    &allocate_keypair,
                    genesis_config.hash(),
                    1,
                ),
            ]);

            let process_transactions_batch_output = BankingStage::process_and_record_transactions(
                &bank,
                &transactions,
                &recorder,
                0,
                None,
                &gossip_vote_sender,
                &qos_service,
                None,
            );

            let ExecuteAndCommitTransactionsOutput {
                executed_with_successful_result_count,
                commit_transactions_result,
                retryable_transaction_indexes,
                ..
            } = process_transactions_batch_output.execute_and_commit_transactions_output;
            assert_eq!(executed_with_successful_result_count, 1);
            assert!(commit_transactions_result.is_ok());
            assert_eq!(retryable_transaction_indexes, vec![1]);

            assert_eq!(get_block_cost(), 2 * single_transfer_cost);
            assert_eq!(get_tx_count(), 2);

            exit.store(true, Ordering::Relaxed);
            let _ = poh_simulator.join();
        }
        Blockstore::destroy(ledger_path.path()).unwrap();
    }

    fn simulate_poh(
        record_receiver: CrossbeamReceiver<Record>,
        poh_recorder: &Arc<RwLock<PohRecorder>>,
    ) -> (JoinHandle<()>, Arc<AtomicBool>) {
        let exit = Arc::<AtomicBool>::default();
        let poh_recorder = poh_recorder.clone();
        let tick_producer = {
            let exit = exit.clone();
            Builder::new()
                .name("solana-simulate_poh".to_string())
                .spawn(move || loop {
                    PohService::read_record_receiver_and_process(
                        &poh_recorder,
                        &record_receiver,
                        Duration::from_millis(10),
                    );
                    if exit.load(Ordering::Relaxed) {
                        break;
                    }
                })
        };
        (tick_producer.unwrap(), exit)
    }

    #[test]
    fn test_bank_process_and_record_transactions_account_in_use() {
        solana_logger::setup();
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_slow_genesis_config(10_000);
        let bank = Arc::new(Bank::new_no_wallclock_throttle_for_tests(&genesis_config));
        let pubkey = solana_sdk::pubkey::new_rand();
        let pubkey1 = solana_sdk::pubkey::new_rand();

        let transactions = sanitize_transactions(vec![
            system_transaction::transfer(&mint_keypair, &pubkey, 1, genesis_config.hash()),
            system_transaction::transfer(&mint_keypair, &pubkey1, 1, genesis_config.hash()),
        ]);

        let ledger_path = get_tmp_ledger_path_auto_delete!();
        {
            let blockstore = Blockstore::open(ledger_path.path())
                .expect("Expected to be able to open database ledger");
            let (poh_recorder, _entry_receiver, record_receiver) = PohRecorder::new(
                bank.tick_height(),
                bank.last_blockhash(),
                bank.clone(),
                Some((4, 4)),
                bank.ticks_per_slot(),
                &pubkey,
                &Arc::new(blockstore),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
                &Arc::new(PohConfig::default()),
            );
            let recorder = poh_recorder.recorder();
            let poh_recorder = Arc::new(RwLock::new(poh_recorder));

            poh_recorder.write().unwrap().set_bank(&bank, false);

            let (poh_simulator, exit) = simulate_poh(record_receiver, &poh_recorder);

            let (gossip_vote_sender, _gossip_vote_receiver) = unbounded();

            let process_transactions_batch_output = BankingStage::process_and_record_transactions(
                &bank,
                &transactions,
                &recorder,
                0,
                None,
                &gossip_vote_sender,
                &QosService::new(Arc::new(RwLock::new(CostModel::default())), 1),
                None,
            );

            exit.store(true, Ordering::Relaxed);
            let _ = poh_simulator.join();

            let ExecuteAndCommitTransactionsOutput {
                transactions_attempted_execution_count,
                executed_transactions_count,
                retryable_transaction_indexes,
                commit_transactions_result,
                ..
            } = process_transactions_batch_output.execute_and_commit_transactions_output;

            assert_eq!(transactions_attempted_execution_count, 2);
            assert_eq!(executed_transactions_count, 1);
            assert_eq!(retryable_transaction_indexes, vec![1],);
            assert!(commit_transactions_result.is_ok());
        }
        Blockstore::destroy(ledger_path.path()).unwrap();
    }

    #[test]
    fn test_filter_valid_packets() {
        solana_logger::setup();
        let GenesisConfigInfo { genesis_config, .. } = create_slow_genesis_config(10);
        let bank = Bank::new_no_wallclock_throttle_for_tests(&genesis_config);
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let current_bank = bank_forks.read().unwrap().root_bank();

        let mut packets: Vec<DeserializedPacket> = (0..256)
            .map(|packets_id| {
                // packets are deserialized upon receiving, failed packets will not be
                // forwarded; Therefore we need to create real packets here.
                let keypair = Keypair::new();
                let pubkey = solana_sdk::pubkey::new_rand();
                let blockhash = Hash::new_unique();
                let transaction = system_transaction::transfer(&keypair, &pubkey, 1, blockhash);
                let mut p = Packet::from_data(None, &transaction).unwrap();
                p.meta.port = packets_id;
                p.meta.set_tracer(true);
                DeserializedPacket::new(p).unwrap()
            })
            .collect_vec();

        // all packets are forwarded
        {
            let mut buffered_packet_batches: UnprocessedPacketBatches =
                UnprocessedPacketBatches::from_iter(packets.clone().into_iter(), packets.len());
            let mut forward_packet_batches_by_accounts =
                ForwardPacketBatchesByAccounts::new(current_bank.clone(), 1, 2);

            let FilterForwardingResults {
                total_forwardable_packets,
                total_tracer_packets_in_buffer,
                total_forwardable_tracer_packets,
            } = BankingStage::filter_valid_packets_for_forwarding(
                &mut buffered_packet_batches,
                &mut forward_packet_batches_by_accounts,
            );
            assert_eq!(total_forwardable_packets, 256);
            assert_eq!(total_tracer_packets_in_buffer, 256);
            assert_eq!(total_forwardable_tracer_packets, 256);

            // packets in a batch are forwarded in arbitrary order; verify the ports match after
            // sorting
            let expected_ports: Vec<_> = (0..256).collect();
            let mut forwarded_ports: Vec<_> = forward_packet_batches_by_accounts
                .iter_batches()
                .flat_map(|batch| {
                    batch
                        .get_forwardable_packets()
                        .into_iter()
                        .map(|p| p.meta.port)
                })
                .collect();
            forwarded_ports.sort_unstable();
            assert_eq!(expected_ports, forwarded_ports);
        }

        // some packets are forwarded
        {
            let num_already_forwarded = 16;
            for packet in &mut packets[0..num_already_forwarded] {
                packet.forwarded = true;
            }
            let mut buffered_packet_batches: UnprocessedPacketBatches =
                UnprocessedPacketBatches::from_iter(packets.clone().into_iter(), packets.len());
            let mut forward_packet_batches_by_accounts =
                ForwardPacketBatchesByAccounts::new(current_bank, 1, 2);
            let FilterForwardingResults {
                total_forwardable_packets,
                total_tracer_packets_in_buffer,
                total_forwardable_tracer_packets,
            } = BankingStage::filter_valid_packets_for_forwarding(
                &mut buffered_packet_batches,
                &mut forward_packet_batches_by_accounts,
            );
            assert_eq!(
                total_forwardable_packets,
                packets.len() - num_already_forwarded
            );
            assert_eq!(total_tracer_packets_in_buffer, packets.len());
            assert_eq!(
                total_forwardable_tracer_packets,
                packets.len() - num_already_forwarded
            );
        }
    }

    #[test]
    fn test_process_transactions_returns_unprocessed_txs() {
        solana_logger::setup();
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_slow_genesis_config(10_000);
        let bank = Arc::new(Bank::new_no_wallclock_throttle_for_tests(&genesis_config));

        let pubkey = solana_sdk::pubkey::new_rand();

        let transactions = sanitize_transactions(vec![system_transaction::transfer(
            &mint_keypair,
            &pubkey,
            1,
            genesis_config.hash(),
        )]);

        let ledger_path = get_tmp_ledger_path_auto_delete!();
        {
            let blockstore = Blockstore::open(ledger_path.path())
                .expect("Expected to be able to open database ledger");
            let (poh_recorder, _entry_receiver, record_receiver) = PohRecorder::new(
                bank.tick_height(),
                bank.last_blockhash(),
                bank.clone(),
                Some((4, 4)),
                bank.ticks_per_slot(),
                &solana_sdk::pubkey::new_rand(),
                &Arc::new(blockstore),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
                &Arc::new(PohConfig::default()),
            );

            // Poh Recorder has no working bank, so should throw MaxHeightReached error on
            // record
            let recorder = poh_recorder.recorder();

            let (poh_simulator, exit) =
                simulate_poh(record_receiver, &Arc::new(RwLock::new(poh_recorder)));

            let (gossip_vote_sender, _gossip_vote_receiver) = unbounded();

            let process_transactions_summary = BankingStage::process_transactions(
                &bank,
                &Instant::now(),
                &transactions,
                &recorder,
                None,
                &gossip_vote_sender,
                &QosService::new(Arc::new(RwLock::new(CostModel::default())), 1),
                None,
            );

            let ProcessTransactionsSummary {
                reached_max_poh_height,
                transactions_attempted_execution_count,
                committed_transactions_count,
                committed_transactions_with_successful_result_count,
                failed_commit_count,
                mut retryable_transaction_indexes,
                ..
            } = process_transactions_summary;
            assert!(reached_max_poh_height);
            assert_eq!(transactions_attempted_execution_count, 1);
            assert_eq!(failed_commit_count, 1);
            // MaxHeightReached error does not commit, should be zero here
            assert_eq!(committed_transactions_count, 0);
            assert_eq!(committed_transactions_with_successful_result_count, 0);

            retryable_transaction_indexes.sort_unstable();
            let expected: Vec<usize> = (0..transactions.len()).collect();
            assert_eq!(retryable_transaction_indexes, expected);

            exit.store(true, Ordering::Relaxed);
            let _ = poh_simulator.join();
        }

        Blockstore::destroy(ledger_path.path()).unwrap();
    }

    fn execute_transactions_with_dummy_poh_service(
        bank: Arc<Bank>,
        transactions: Vec<Transaction>,
    ) -> ProcessTransactionsSummary {
        let transactions = sanitize_transactions(transactions);
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path())
            .expect("Expected to be able to open database ledger");
        let (poh_recorder, _entry_receiver, record_receiver) = PohRecorder::new(
            bank.tick_height(),
            bank.last_blockhash(),
            bank.clone(),
            Some((4, 4)),
            bank.ticks_per_slot(),
            &Pubkey::new_unique(),
            &Arc::new(blockstore),
            &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
            &Arc::new(PohConfig::default()),
        );
        let recorder = poh_recorder.recorder();
        let poh_recorder = Arc::new(RwLock::new(poh_recorder));

        poh_recorder.write().unwrap().set_bank(&bank, false);

        let (poh_simulator, exit) = simulate_poh(record_receiver, &poh_recorder);

        let (gossip_vote_sender, _gossip_vote_receiver) = unbounded();

        let process_transactions_summary = BankingStage::process_transactions(
            &bank,
            &Instant::now(),
            &transactions,
            &recorder,
            None,
            &gossip_vote_sender,
            &QosService::new(Arc::new(RwLock::new(CostModel::default())), 1),
            None,
        );

        exit.store(true, Ordering::Relaxed);
        let _ = poh_simulator.join();

        process_transactions_summary
    }

    #[test]
    fn test_process_transactions_instruction_error() {
        solana_logger::setup();
        let lamports = 10_000;
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_slow_genesis_config(lamports);
        let bank = Arc::new(Bank::new_no_wallclock_throttle_for_tests(&genesis_config));
        // set cost tracker limits to MAX so it will not filter out TXs
        bank.write_cost_tracker()
            .unwrap()
            .set_limits(std::u64::MAX, std::u64::MAX, std::u64::MAX);

        // Transfer more than the balance of the mint keypair, should cause a
        // InstructionError::InsufficientFunds that is then committed. Needs to be
        // MAX_NUM_TRANSACTIONS_PER_BATCH at least so it doesn't conflict on account locks
        // with the below transaction
        let mut transactions = vec![
            system_transaction::transfer(
                &mint_keypair,
                &Pubkey::new_unique(),
                lamports + 1,
                genesis_config.hash(),
            );
            MAX_NUM_TRANSACTIONS_PER_BATCH
        ];

        // Make one transaction that will succeed.
        transactions.push(system_transaction::transfer(
            &mint_keypair,
            &Pubkey::new_unique(),
            1,
            genesis_config.hash(),
        ));

        let transactions_count = transactions.len();
        let ProcessTransactionsSummary {
            reached_max_poh_height,
            transactions_attempted_execution_count,
            committed_transactions_count,
            committed_transactions_with_successful_result_count,
            failed_commit_count,
            retryable_transaction_indexes,
            ..
        } = execute_transactions_with_dummy_poh_service(bank, transactions);

        // All the transactions should have been replayed, but only 1 committed
        assert!(!reached_max_poh_height);
        assert_eq!(transactions_attempted_execution_count, transactions_count);
        // Both transactions should have been committed, even though one was an error,
        // because InstructionErrors are committed
        assert_eq!(committed_transactions_count, 2);
        assert_eq!(committed_transactions_with_successful_result_count, 1);
        assert_eq!(failed_commit_count, 0);
        assert_eq!(
            retryable_transaction_indexes,
            (1..transactions_count - 1).collect::<Vec<usize>>()
        );
    }
    #[test]
    fn test_process_transactions_account_in_use() {
        solana_logger::setup();
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_slow_genesis_config(10_000);
        let bank = Arc::new(Bank::new_no_wallclock_throttle_for_tests(&genesis_config));
        // set cost tracker limits to MAX so it will not filter out TXs
        bank.write_cost_tracker()
            .unwrap()
            .set_limits(std::u64::MAX, std::u64::MAX, std::u64::MAX);

        // Make all repetitive transactions that conflict on the `mint_keypair`, so only 1 should be executed
        let mut transactions = vec![
            system_transaction::transfer(
                &mint_keypair,
                &Pubkey::new_unique(),
                1,
                genesis_config.hash()
            );
            MAX_NUM_TRANSACTIONS_PER_BATCH
        ];

        // Make one more in separate batch that also conflicts, but because it's in a separate batch, it
        // should be executed
        transactions.push(system_transaction::transfer(
            &mint_keypair,
            &Pubkey::new_unique(),
            1,
            genesis_config.hash(),
        ));

        let transactions_count = transactions.len();
        let ProcessTransactionsSummary {
            reached_max_poh_height,
            transactions_attempted_execution_count,
            committed_transactions_count,
            committed_transactions_with_successful_result_count,
            failed_commit_count,
            retryable_transaction_indexes,
            ..
        } = execute_transactions_with_dummy_poh_service(bank, transactions);

        // All the transactions should have been replayed, but only 2 committed (first and last)
        assert!(!reached_max_poh_height);
        assert_eq!(transactions_attempted_execution_count, transactions_count);
        assert_eq!(committed_transactions_count, 2);
        assert_eq!(committed_transactions_with_successful_result_count, 2);
        assert_eq!(failed_commit_count, 0,);

        // Everything except first and last index of the transactions failed and are last retryable
        assert_eq!(
            retryable_transaction_indexes,
            (1..transactions_count - 1).collect::<Vec<usize>>()
        );
    }

    #[test]
    fn test_write_persist_transaction_status() {
        solana_logger::setup();
        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair,
            ..
        } = create_slow_genesis_config(solana_sdk::native_token::sol_to_lamports(1000.0));
        genesis_config.rent.lamports_per_byte_year = 50;
        genesis_config.rent.exemption_threshold = 2.0;
        let bank = Arc::new(Bank::new_no_wallclock_throttle_for_tests(&genesis_config));
        let pubkey = solana_sdk::pubkey::new_rand();
        let pubkey1 = solana_sdk::pubkey::new_rand();
        let keypair1 = Keypair::new();

        let rent_exempt_amount = bank.get_minimum_balance_for_rent_exemption(0);

        let success_tx = system_transaction::transfer(
            &mint_keypair,
            &pubkey,
            rent_exempt_amount,
            genesis_config.hash(),
        );
        let success_signature = success_tx.signatures[0];
        let entry_1 = next_entry(&genesis_config.hash(), 1, vec![success_tx.clone()]);
        let ix_error_tx = system_transaction::transfer(
            &keypair1,
            &pubkey1,
            2 * rent_exempt_amount,
            genesis_config.hash(),
        );
        let ix_error_signature = ix_error_tx.signatures[0];
        let entry_2 = next_entry(&entry_1.hash, 1, vec![ix_error_tx.clone()]);
        let entries = vec![entry_1, entry_2];

        let transactions = sanitize_transactions(vec![success_tx, ix_error_tx]);
        bank.transfer(rent_exempt_amount, &mint_keypair, &keypair1.pubkey())
            .unwrap();

        let ledger_path = get_tmp_ledger_path_auto_delete!();
        {
            let blockstore = Blockstore::open(ledger_path.path())
                .expect("Expected to be able to open database ledger");
            let blockstore = Arc::new(blockstore);
            let (poh_recorder, _entry_receiver, record_receiver) = PohRecorder::new(
                bank.tick_height(),
                bank.last_blockhash(),
                bank.clone(),
                Some((4, 4)),
                bank.ticks_per_slot(),
                &pubkey,
                &blockstore,
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
                &Arc::new(PohConfig::default()),
            );
            let recorder = poh_recorder.recorder();
            let poh_recorder = Arc::new(RwLock::new(poh_recorder));

            let (poh_simulator, exit) = simulate_poh(record_receiver, &poh_recorder);

            poh_recorder.write().unwrap().set_bank(&bank, false);

            let shreds = entries_to_test_shreds(&entries, bank.slot(), 0, true, 0);
            blockstore.insert_shreds(shreds, None, false).unwrap();
            blockstore.set_roots(std::iter::once(&bank.slot())).unwrap();

            let (transaction_status_sender, transaction_status_receiver) = unbounded();
            let transaction_status_service = TransactionStatusService::new(
                transaction_status_receiver,
                Arc::new(AtomicU64::default()),
                true,
                None,
                blockstore.clone(),
                false,
                &Arc::new(AtomicBool::new(false)),
            );

            let (gossip_vote_sender, _gossip_vote_receiver) = unbounded();

            let _ = BankingStage::process_and_record_transactions(
                &bank,
                &transactions,
                &recorder,
                0,
                Some(TransactionStatusSender {
                    sender: transaction_status_sender,
                }),
                &gossip_vote_sender,
                &QosService::new(Arc::new(RwLock::new(CostModel::default())), 1),
                None,
            );

            transaction_status_service.join().unwrap();

            let confirmed_block = blockstore.get_rooted_block(bank.slot(), false).unwrap();
            let actual_tx_results: Vec<_> = confirmed_block
                .transactions
                .into_iter()
                .map(|VersionedTransactionWithStatusMeta { transaction, meta }| {
                    (transaction.signatures[0], meta.status)
                })
                .collect();
            let expected_tx_results = vec![
                (success_signature, Ok(())),
                (
                    ix_error_signature,
                    Err(TransactionError::InstructionError(
                        0,
                        InstructionError::Custom(1),
                    )),
                ),
            ];
            assert_eq!(actual_tx_results, expected_tx_results);

            exit.store(true, Ordering::Relaxed);
            let _ = poh_simulator.join();
        }
        Blockstore::destroy(ledger_path.path()).unwrap();
    }

    fn generate_new_address_lookup_table(
        authority: Option<Pubkey>,
        num_addresses: usize,
    ) -> AddressLookupTable<'static> {
        let mut addresses = Vec::with_capacity(num_addresses);
        addresses.resize_with(num_addresses, Pubkey::new_unique);
        AddressLookupTable {
            meta: LookupTableMeta {
                authority,
                ..LookupTableMeta::default()
            },
            addresses: Cow::Owned(addresses),
        }
    }

    fn store_address_lookup_table(
        bank: &Bank,
        account_address: Pubkey,
        address_lookup_table: AddressLookupTable<'static>,
    ) -> AccountSharedData {
        let data = address_lookup_table.serialize_for_tests().unwrap();
        let mut account =
            AccountSharedData::new(1, data.len(), &solana_address_lookup_table_program::id());
        account.set_data(data);
        bank.store_account(&account_address, &account);

        account
    }

    #[test]
    fn test_write_persist_loaded_addresses() {
        solana_logger::setup();
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_slow_genesis_config(10_000);
        let bank = Arc::new(Bank::new_no_wallclock_throttle_for_tests(&genesis_config));
        let keypair = Keypair::new();

        let address_table_key = Pubkey::new_unique();
        let address_table_state = generate_new_address_lookup_table(None, 2);
        store_address_lookup_table(&bank, address_table_key, address_table_state);

        let bank = Arc::new(Bank::new_from_parent(&bank, &Pubkey::new_unique(), 1));
        let message = VersionedMessage::V0(v0::Message {
            header: MessageHeader {
                num_required_signatures: 1,
                num_readonly_signed_accounts: 0,
                num_readonly_unsigned_accounts: 0,
            },
            recent_blockhash: genesis_config.hash(),
            account_keys: vec![keypair.pubkey()],
            address_table_lookups: vec![MessageAddressTableLookup {
                account_key: address_table_key,
                writable_indexes: vec![0],
                readonly_indexes: vec![1],
            }],
            instructions: vec![],
        });

        let tx = VersionedTransaction::try_new(message, &[&keypair]).unwrap();
        let sanitized_tx = SanitizedTransaction::try_create(
            tx.clone(),
            MessageHash::Compute,
            Some(false),
            bank.as_ref(),
            true, // require_static_program_ids
        )
        .unwrap();

        let entry = next_versioned_entry(&genesis_config.hash(), 1, vec![tx]);
        let entries = vec![entry];

        bank.transfer(1, &mint_keypair, &keypair.pubkey()).unwrap();

        let ledger_path = get_tmp_ledger_path_auto_delete!();
        {
            let blockstore = Blockstore::open(ledger_path.path())
                .expect("Expected to be able to open database ledger");
            let blockstore = Arc::new(blockstore);
            let (poh_recorder, _entry_receiver, record_receiver) = PohRecorder::new(
                bank.tick_height(),
                bank.last_blockhash(),
                bank.clone(),
                Some((4, 4)),
                bank.ticks_per_slot(),
                &Pubkey::new_unique(),
                &blockstore,
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
                &Arc::new(PohConfig::default()),
            );
            let recorder = poh_recorder.recorder();
            let poh_recorder = Arc::new(RwLock::new(poh_recorder));

            let (poh_simulator, exit) = simulate_poh(record_receiver, &poh_recorder);

            poh_recorder.write().unwrap().set_bank(&bank, false);

            let shreds = entries_to_test_shreds(&entries, bank.slot(), 0, true, 0);
            blockstore.insert_shreds(shreds, None, false).unwrap();
            blockstore.set_roots(std::iter::once(&bank.slot())).unwrap();

            let (transaction_status_sender, transaction_status_receiver) = unbounded();
            let transaction_status_service = TransactionStatusService::new(
                transaction_status_receiver,
                Arc::new(AtomicU64::default()),
                true,
                None,
                blockstore.clone(),
                false,
                &Arc::new(AtomicBool::new(false)),
            );

            let (gossip_vote_sender, _gossip_vote_receiver) = unbounded();

            let _ = BankingStage::process_and_record_transactions(
                &bank,
                &[sanitized_tx.clone()],
                &recorder,
                0,
                Some(TransactionStatusSender {
                    sender: transaction_status_sender,
                }),
                &gossip_vote_sender,
                &QosService::new(Arc::new(RwLock::new(CostModel::default())), 1),
                None,
            );

            transaction_status_service.join().unwrap();

            let mut confirmed_block = blockstore.get_rooted_block(bank.slot(), false).unwrap();
            assert_eq!(confirmed_block.transactions.len(), 1);

            let recorded_meta = confirmed_block.transactions.pop().unwrap().meta;
            assert_eq!(
                recorded_meta,
                TransactionStatusMeta {
                    status: Ok(()),
                    pre_balances: vec![1, 0, 0],
                    post_balances: vec![1, 0, 0],
                    pre_token_balances: Some(vec![]),
                    post_token_balances: Some(vec![]),
                    rewards: Some(vec![]),
                    loaded_addresses: sanitized_tx.get_loaded_addresses(),
                    compute_units_consumed: Some(0),
                    ..TransactionStatusMeta::default()
                }
            );
            exit.store(true, Ordering::Relaxed);
            let _ = poh_simulator.join();
        }
        Blockstore::destroy(ledger_path.path()).unwrap();
    }

    #[allow(clippy::type_complexity)]
    fn setup_conflicting_transactions(
        ledger_path: &Path,
    ) -> (
        Vec<Transaction>,
        Arc<Bank>,
        Arc<RwLock<PohRecorder>>,
        Receiver<WorkingBankEntry>,
        JoinHandle<()>,
        Arc<AtomicBool>,
    ) {
        Blockstore::destroy(ledger_path).unwrap();
        let genesis_config_info = create_slow_genesis_config(10_000);
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = &genesis_config_info;
        let blockstore =
            Blockstore::open(ledger_path).expect("Expected to be able to open database ledger");
        let bank = Arc::new(Bank::new_no_wallclock_throttle_for_tests(genesis_config));
        let (poh_recorder, entry_receiver, record_receiver) = PohRecorder::new(
            bank.tick_height(),
            bank.last_blockhash(),
            bank.clone(),
            Some((4, 4)),
            bank.ticks_per_slot(),
            &solana_sdk::pubkey::new_rand(),
            &Arc::new(blockstore),
            &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
            &Arc::new(PohConfig::default()),
        );
        let poh_recorder = Arc::new(RwLock::new(poh_recorder));

        // Set up unparallelizable conflicting transactions
        let pubkey0 = solana_sdk::pubkey::new_rand();
        let pubkey1 = solana_sdk::pubkey::new_rand();
        let pubkey2 = solana_sdk::pubkey::new_rand();
        let transactions = vec![
            system_transaction::transfer(mint_keypair, &pubkey0, 1, genesis_config.hash()),
            system_transaction::transfer(mint_keypair, &pubkey1, 1, genesis_config.hash()),
            system_transaction::transfer(mint_keypair, &pubkey2, 1, genesis_config.hash()),
        ];
        let (poh_simulator, exit) = simulate_poh(record_receiver, &poh_recorder);

        (
            transactions,
            bank,
            poh_recorder,
            entry_receiver,
            poh_simulator,
            exit,
        )
    }

    #[test]
    fn test_consume_buffered_packets() {
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        {
            let (transactions, bank, poh_recorder, _entry_receiver, poh_simulator, exit) =
                setup_conflicting_transactions(ledger_path.path());
            let recorder = poh_recorder.read().unwrap().recorder();
            let num_conflicting_transactions = transactions.len();
            let deserialized_packets =
                unprocessed_packet_batches::transactions_to_deserialized_packets(&transactions)
                    .unwrap();
            assert_eq!(deserialized_packets.len(), num_conflicting_transactions);
            let mut buffered_packet_batches: UnprocessedPacketBatches =
                UnprocessedPacketBatches::from_iter(
                    deserialized_packets.into_iter(),
                    num_conflicting_transactions,
                );

            let (gossip_vote_sender, _gossip_vote_receiver) = unbounded();

            // When the working bank in poh_recorder is None, no packets should be processed
            assert!(!poh_recorder.read().unwrap().has_bank());
            let max_tx_processing_ns = std::u128::MAX;
            BankingStage::consume_buffered_packets(
                &Pubkey::default(),
                max_tx_processing_ns,
                &poh_recorder,
                &mut buffered_packet_batches,
                None,
                &gossip_vote_sender,
                None::<Box<dyn Fn()>>,
                &BankingStageStats::default(),
                &recorder,
                &QosService::new(Arc::new(RwLock::new(CostModel::default())), 1),
                &mut LeaderSlotMetricsTracker::new(0),
                num_conflicting_transactions,
                None,
            );
            assert_eq!(buffered_packet_batches.len(), num_conflicting_transactions);
            // When the poh recorder has a bank, should process all non conflicting buffered packets.
            // Processes one packet per iteration of the loop
            let num_packets_to_process_per_iteration = num_conflicting_transactions;
            for num_expected_unprocessed in (0..num_conflicting_transactions).rev() {
                poh_recorder.write().unwrap().set_bank(&bank, false);
                BankingStage::consume_buffered_packets(
                    &Pubkey::default(),
                    max_tx_processing_ns,
                    &poh_recorder,
                    &mut buffered_packet_batches,
                    None,
                    &gossip_vote_sender,
                    None::<Box<dyn Fn()>>,
                    &BankingStageStats::default(),
                    &recorder,
                    &QosService::new(Arc::new(RwLock::new(CostModel::default())), 1),
                    &mut LeaderSlotMetricsTracker::new(0),
                    num_packets_to_process_per_iteration,
                    None,
                );
                if num_expected_unprocessed == 0 {
                    assert!(buffered_packet_batches.is_empty())
                } else {
                    assert_eq!(buffered_packet_batches.len(), num_expected_unprocessed);
                }
            }
            exit.store(true, Ordering::Relaxed);
            let _ = poh_simulator.join();
        }
        Blockstore::destroy(ledger_path.path()).unwrap();
    }

    #[test]
    fn test_consume_buffered_packets_interrupted() {
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        {
            let (continue_sender, continue_receiver) = unbounded();
            let (finished_packet_sender, finished_packet_receiver) = unbounded();
            let (transactions, bank, poh_recorder, _entry_receiver, poh_simulator, exit) =
                setup_conflicting_transactions(ledger_path.path());

            let test_fn = Some(move || {
                finished_packet_sender.send(()).unwrap();
                continue_receiver.recv().unwrap();
            });
            // When the poh recorder has a bank, it should process all non conflicting buffered packets.
            // Because each conflicting transaction is in it's own `Packet` within a `PacketBatch`, then
            // each iteration of this loop will process one element of the batch per iteration of the
            // loop.
            let interrupted_iteration = 1;
            poh_recorder.write().unwrap().set_bank(&bank, false);
            let poh_recorder_ = poh_recorder.clone();
            let recorder = poh_recorder_.read().unwrap().recorder();
            let (gossip_vote_sender, _gossip_vote_receiver) = unbounded();
            // Start up thread to process the banks
            let t_consume = Builder::new()
                .name("consume-buffered-packets".to_string())
                .spawn(move || {
                    let num_conflicting_transactions = transactions.len();
                    let deserialized_packets =
                        unprocessed_packet_batches::transactions_to_deserialized_packets(
                            &transactions,
                        )
                        .unwrap();
                    assert_eq!(deserialized_packets.len(), num_conflicting_transactions);
                    let num_packets_to_process_per_iteration = 1;
                    let mut buffered_packet_batches: UnprocessedPacketBatches =
                        UnprocessedPacketBatches::from_iter(
                            deserialized_packets.clone().into_iter(),
                            num_conflicting_transactions,
                        );
                    let all_packet_message_hashes: HashSet<Hash> = buffered_packet_batches
                        .iter()
                        .map(|packet| *packet.immutable_section().message_hash())
                        .collect();
                    BankingStage::consume_buffered_packets(
                        &Pubkey::default(),
                        std::u128::MAX,
                        &poh_recorder_,
                        &mut buffered_packet_batches,
                        None,
                        &gossip_vote_sender,
                        test_fn,
                        &BankingStageStats::default(),
                        &recorder,
                        &QosService::new(Arc::new(RwLock::new(CostModel::default())), 1),
                        &mut LeaderSlotMetricsTracker::new(0),
                        num_packets_to_process_per_iteration,
                        None,
                    );

                    // Check everything is correct. All indexes after `interrupted_iteration`
                    // should still be unprocessed
                    assert_eq!(
                        buffered_packet_batches.len(),
                        deserialized_packets[interrupted_iteration + 1..].len()
                    );
                    for packet in buffered_packet_batches.iter() {
                        assert!(all_packet_message_hashes
                            .contains(packet.immutable_section().message_hash()));
                    }
                })
                .unwrap();

            for i in 0..=interrupted_iteration {
                finished_packet_receiver.recv().unwrap();
                if i == interrupted_iteration {
                    poh_recorder
                        .write()
                        .unwrap()
                        .schedule_dummy_max_height_reached_failure();
                }
                continue_sender.send(()).unwrap();
            }

            t_consume.join().unwrap();
            exit.store(true, Ordering::Relaxed);
            let _ = poh_simulator.join();
        }
        Blockstore::destroy(ledger_path.path()).unwrap();
    }

    #[test]
    fn test_forwarder_budget() {
        solana_logger::setup();
        // Create `PacketBatch` with 1 unprocessed packet
        let tx = system_transaction::transfer(
            &Keypair::new(),
            &solana_sdk::pubkey::new_rand(),
            1,
            Hash::new_unique(),
        );
        let packet = Packet::from_data(None, &tx).unwrap();
        let deserialized_packet = DeserializedPacket::new(packet).unwrap();

        let genesis_config_info = create_slow_genesis_config(10_000);
        let GenesisConfigInfo {
            genesis_config,
            validator_pubkey,
            ..
        } = &genesis_config_info;

        let bank = Bank::new_no_wallclock_throttle_for_tests(genesis_config);
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let bank = Arc::new(bank_forks.read().unwrap().get(0).unwrap());
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        {
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

            let (exit, poh_recorder, poh_service, _entry_receiver) =
                create_test_recorder(&bank, &blockstore, Some(poh_config), None);

            let local_node = Node::new_localhost_with_pubkey(validator_pubkey);
            let cluster_info = new_test_cluster_info(local_node.info);
            let recv_socket = &local_node.sockets.tpu_forwards[0];

            let test_cases = vec![
                ("budget-restricted", DataBudget::restricted(), 0),
                ("budget-available", DataBudget::default(), 1),
            ];

            let connection_cache = ConnectionCache::default();
            let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
            for (name, data_budget, expected_num_forwarded) in test_cases {
                let mut unprocessed_packet_batches: UnprocessedPacketBatches =
                    UnprocessedPacketBatches::from_iter(
                        vec![deserialized_packet.clone()].into_iter(),
                        1,
                    );
                let stats = BankingStageStats::default();
                BankingStage::handle_forwarding(
                    &ForwardOption::ForwardTransaction,
                    &cluster_info,
                    &mut unprocessed_packet_batches,
                    &poh_recorder,
                    &socket,
                    true,
                    &data_budget,
                    &mut LeaderSlotMetricsTracker::new(0),
                    &stats,
                    &connection_cache,
                    &mut TracerPacketStats::new(0),
                    &bank_forks,
                );

                recv_socket
                    .set_nonblocking(expected_num_forwarded == 0)
                    .unwrap();

                let mut packets = vec![Packet::default(); 2];
                let num_received = recv_mmsg(recv_socket, &mut packets[..]).unwrap_or_default();
                assert_eq!(num_received, expected_num_forwarded, "{}", name);
            }

            exit.store(true, Ordering::Relaxed);
            poh_service.join().unwrap();
        }
        Blockstore::destroy(ledger_path.path()).unwrap();
    }

    #[test]
    fn test_handle_forwarding() {
        solana_logger::setup();
        // packets are deserialized upon receiving, failed packets will not be
        // forwarded; Therefore need to create real packets here.
        let keypair = Keypair::new();
        let pubkey = solana_sdk::pubkey::new_rand();

        let fwd_block_hash = Hash::new_unique();
        let forwarded_packet = {
            let transaction = system_transaction::transfer(&keypair, &pubkey, 1, fwd_block_hash);
            let mut packet = Packet::from_data(None, &transaction).unwrap();
            packet.meta.flags |= PacketFlags::FORWARDED;
            DeserializedPacket::new(packet).unwrap()
        };

        let normal_block_hash = Hash::new_unique();
        let normal_packet = {
            let transaction = system_transaction::transfer(&keypair, &pubkey, 1, normal_block_hash);
            let packet = Packet::from_data(None, &transaction).unwrap();
            DeserializedPacket::new(packet).unwrap()
        };

        let mut unprocessed_packet_batches: UnprocessedPacketBatches =
            UnprocessedPacketBatches::from_iter(
                vec![forwarded_packet, normal_packet].into_iter(),
                2,
            );

        let genesis_config_info = create_slow_genesis_config(10_000);
        let GenesisConfigInfo {
            genesis_config,
            validator_pubkey,
            ..
        } = &genesis_config_info;
        let bank = Bank::new_no_wallclock_throttle_for_tests(genesis_config);
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let bank = Arc::new(bank_forks.read().unwrap().get(0).unwrap());
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        {
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

            let (exit, poh_recorder, poh_service, _entry_receiver) =
                create_test_recorder(&bank, &blockstore, Some(poh_config), None);

            let local_node = Node::new_localhost_with_pubkey(validator_pubkey);
            let cluster_info = new_test_cluster_info(local_node.info);
            let recv_socket = &local_node.sockets.tpu_forwards[0];
            let connection_cache = ConnectionCache::default();

            let test_cases = vec![
                ("not-forward", ForwardOption::NotForward, true, vec![], 2),
                (
                    "fwd-normal",
                    ForwardOption::ForwardTransaction,
                    true,
                    vec![normal_block_hash],
                    2,
                ),
                (
                    "fwd-no-op",
                    ForwardOption::ForwardTransaction,
                    true,
                    vec![],
                    2,
                ),
                (
                    "fwd-no-hold",
                    ForwardOption::ForwardTransaction,
                    false,
                    vec![],
                    0,
                ),
            ];

            let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
            for (name, forward_option, hold, expected_ids, expected_num_unprocessed) in test_cases {
                let stats = BankingStageStats::default();
                BankingStage::handle_forwarding(
                    &forward_option,
                    &cluster_info,
                    &mut unprocessed_packet_batches,
                    &poh_recorder,
                    &socket,
                    hold,
                    &DataBudget::default(),
                    &mut LeaderSlotMetricsTracker::new(0),
                    &stats,
                    &connection_cache,
                    &mut TracerPacketStats::new(0),
                    &bank_forks,
                );

                recv_socket
                    .set_nonblocking(expected_ids.is_empty())
                    .unwrap();

                let mut packets = vec![Packet::default(); 2];
                let num_received = recv_mmsg(recv_socket, &mut packets[..]).unwrap_or_default();
                assert_eq!(num_received, expected_ids.len(), "{}", name);
                for (i, expected_id) in expected_ids.iter().enumerate() {
                    assert_eq!(packets[i].meta.size, 215);
                    let recv_transaction: VersionedTransaction =
                        packets[i].deserialize_slice(..).unwrap();
                    assert_eq!(
                        recv_transaction.message.recent_blockhash(),
                        expected_id,
                        "{}",
                        name
                    );
                }

                let num_unprocessed_packets: usize = unprocessed_packet_batches.len();
                assert_eq!(
                    num_unprocessed_packets, expected_num_unprocessed,
                    "{}",
                    name
                );
            }

            exit.store(true, Ordering::Relaxed);
            poh_service.join().unwrap();
        }
        Blockstore::destroy(ledger_path.path()).unwrap();
    }

    #[test]
    fn test_accumulate_batched_transaction_costs() {
        let signature_cost = 1;
        let write_lock_cost = 2;
        let data_bytes_cost = 3;
        let builtins_execution_cost = 4;
        let bpf_execution_cost = 10;
        let num_txs = 4;

        let tx_costs: Vec<_> = (0..num_txs)
            .map(|_| TransactionCost {
                signature_cost,
                write_lock_cost,
                data_bytes_cost,
                builtins_execution_cost,
                bpf_execution_cost,
                ..TransactionCost::default()
            })
            .collect();
        let tx_results: Vec<_> = (0..num_txs)
            .map(|n| {
                if n % 2 == 0 {
                    Ok(())
                } else {
                    Err(TransactionError::WouldExceedMaxBlockCostLimit)
                }
            })
            .collect();
        // should only accumulate half of the costs that are OK
        let expected_signatures = signature_cost * (num_txs / 2);
        let expected_write_locks = write_lock_cost * (num_txs / 2);
        let expected_data_bytes = data_bytes_cost * (num_txs / 2);
        let expected_builtins_execution_costs = builtins_execution_cost * (num_txs / 2);
        let expected_bpf_execution_costs = bpf_execution_cost * (num_txs / 2);
        let batched_transaction_details =
            BankingStage::accumulate_batched_transaction_costs(tx_costs.iter(), tx_results.iter());
        assert_eq!(
            expected_signatures,
            batched_transaction_details.costs.batched_signature_cost
        );
        assert_eq!(
            expected_write_locks,
            batched_transaction_details.costs.batched_write_lock_cost
        );
        assert_eq!(
            expected_data_bytes,
            batched_transaction_details.costs.batched_data_bytes_cost
        );
        assert_eq!(
            expected_builtins_execution_costs,
            batched_transaction_details
                .costs
                .batched_builtins_execute_cost
        );
        assert_eq!(
            expected_bpf_execution_costs,
            batched_transaction_details.costs.batched_bpf_execute_cost
        );
    }

    #[test]
    fn test_accumulate_execute_units_and_time() {
        let mut execute_timings = ExecuteTimings::default();
        let mut expected_units = 0;
        let mut expected_us = 0;

        for n in 0..10 {
            execute_timings.details.per_program_timings.insert(
                Pubkey::new_unique(),
                ProgramTiming {
                    accumulated_us: n * 100,
                    accumulated_units: n * 1000,
                    count: n as u32,
                    errored_txs_compute_consumed: vec![],
                    total_errored_units: 0,
                },
            );
            expected_us += n * 100;
            expected_units += n * 1000;
        }

        let (units, us) = BankingStage::accumulate_execute_units_and_time(&execute_timings);

        assert_eq!(expected_units, units);
        assert_eq!(expected_us, us);
    }

    #[test]
    fn test_filter_processed_packets() {
        let retryable_indexes = [0, 1, 2, 3];
        let mut non_retryable_indexes = vec![];
        let f = |start, end| {
            non_retryable_indexes.push((start, end));
        };
        BankingStage::filter_processed_packets(retryable_indexes.iter(), f);
        assert!(non_retryable_indexes.is_empty());

        let retryable_indexes = [0, 1, 2, 3, 5];
        let mut non_retryable_indexes = vec![];
        let f = |start, end| {
            non_retryable_indexes.push((start, end));
        };
        BankingStage::filter_processed_packets(retryable_indexes.iter(), f);
        assert_eq!(non_retryable_indexes, vec![(4, 5)]);

        let retryable_indexes = [1, 2, 3];
        let mut non_retryable_indexes = vec![];
        let f = |start, end| {
            non_retryable_indexes.push((start, end));
        };
        BankingStage::filter_processed_packets(retryable_indexes.iter(), f);
        assert_eq!(non_retryable_indexes, vec![(0, 1)]);

        let retryable_indexes = [1, 2, 3, 5];
        let mut non_retryable_indexes = vec![];
        let f = |start, end| {
            non_retryable_indexes.push((start, end));
        };
        BankingStage::filter_processed_packets(retryable_indexes.iter(), f);
        assert_eq!(non_retryable_indexes, vec![(0, 1), (4, 5)]);

        let retryable_indexes = [1, 2, 3, 5, 8];
        let mut non_retryable_indexes = vec![];
        let f = |start, end| {
            non_retryable_indexes.push((start, end));
        };
        BankingStage::filter_processed_packets(retryable_indexes.iter(), f);
        assert_eq!(non_retryable_indexes, vec![(0, 1), (4, 5), (6, 8)]);

        let retryable_indexes = [1, 2, 3, 5, 8, 8];
        let mut non_retryable_indexes = vec![];
        let f = |start, end| {
            non_retryable_indexes.push((start, end));
        };
        BankingStage::filter_processed_packets(retryable_indexes.iter(), f);
        assert_eq!(non_retryable_indexes, vec![(0, 1), (4, 5), (6, 8)]);
    }
}

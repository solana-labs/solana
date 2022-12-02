use {
    crate::{
        leader_slot_banking_stage_timing_metrics::*,
        unprocessed_transaction_storage::InsertPacketBatchSummary,
    },
    solana_poh::poh_recorder::BankStart,
    solana_runtime::transaction_error_metrics::*,
    solana_sdk::{clock::Slot, saturating_add_assign},
    std::time::Instant,
};

/// A summary of what happened to transactions passed to the execution pipeline.
/// Transactions can
/// 1) Did not even make it to execution due to being filtered out by things like AccountInUse
/// lock conflicts or CostModel compute limits. These types of errors are retryable and
/// counted in `Self::retryable_transaction_indexes`.
/// 2) Did not execute due to some fatal error like too old, or duplicate signature. These
/// will be dropped from the transactions queue and not counted in `Self::retryable_transaction_indexes`
/// 3) Were executed and committed, captured by `committed_transactions_count` below.
/// 4) Were executed and failed commit, captured by `failed_commit_count` below.
pub(crate) struct ProcessTransactionsSummary {
    // Returns true if we hit the end of the block/max PoH height for the block before
    // processing all the transactions in the batch.
    pub reached_max_poh_height: bool,

    // Total number of transactions that were passed as candidates for execution. See description
    // of struct above for possible outcomes for these transactions
    pub transactions_attempted_execution_count: usize,

    // Total number of transactions that made it into the block
    pub committed_transactions_count: usize,

    // Total number of transactions that made it into the block where the transactions
    // output from execution was success/no error.
    pub committed_transactions_with_successful_result_count: usize,

    // All transactions that were executed but then failed record because the
    // slot ended
    pub failed_commit_count: usize,

    // Indexes of transactions in the transactions slice that were not committed but are retryable
    pub retryable_transaction_indexes: Vec<usize>,

    // The number of transactions filtered out by the cost model
    pub cost_model_throttled_transactions_count: usize,

    // Total amount of time spent running the cost model
    pub cost_model_us: u64,

    // Breakdown of time spent executing and comitting transactions
    pub execute_and_commit_timings: LeaderExecuteAndCommitTimings,

    // Breakdown of all the transaction errors from transactions passed for execution
    pub error_counters: TransactionErrorMetrics,
}

// Metrics describing packets ingested/processed in various parts of BankingStage during this
// validator's leader slot
#[derive(Debug, Default)]
struct LeaderSlotPacketCountMetrics {
    // total number of live packets TPU received from verified receiver for processing.
    total_new_valid_packets: u64,

    // total number of packets TPU received from sigverify that failed signature verification.
    newly_failed_sigverify_count: u64,

    // total number of dropped packet due to the thread's buffered packets capacity being reached.
    exceeded_buffer_limit_dropped_packets_count: u64,

    // total number of packets that got added to the pending buffer after arriving to BankingStage
    newly_buffered_packets_count: u64,

    // total number of transactions in the buffer that were filtered out due to things like age and
    // duplicate signature checks
    retryable_packets_filtered_count: u64,

    // total number of transactions that attempted execution in this slot. Should equal the sum
    // of `committed_transactions_count`, `retryable_errored_transaction_count`, and
    // `nonretryable_errored_transactions_count`.
    transactions_attempted_execution_count: u64,

    // total number of transactions that were executed and committed into the block
    // on this thread
    committed_transactions_count: u64,

    // total number of transactions that were executed, got a successful execution output/no error,
    // and were then committed into the block
    committed_transactions_with_successful_result_count: u64,

    // total number of transactions that were not executed or failed commit, BUT were added back to the buffered
    // queue becaus they were retryable errors
    retryable_errored_transaction_count: u64,

    // The size of the unprocessed buffer at the end of the slot
    end_of_slot_unprocessed_buffer_len: u64,

    // total number of transactions that were rebuffered into the queue after not being
    // executed on a previous pass
    retryable_packets_count: u64,

    // total number of transactions that attempted execution due to some fatal error (too old, duplicate signature, etc.)
    // AND were dropped from the buffered queue
    nonretryable_errored_transactions_count: u64,

    // total number of transactions that were executed, but failed to be committed into the Poh stream because
    // the block ended. Some of these may be already counted in `nonretryable_errored_transactions_count` if they
    // then hit the age limit after failing to be comitted.
    executed_transactions_failed_commit_count: u64,

    // total number of transactions that were excluded from the block because there were concurrent write locks active.
    // These transactions are added back to the buffered queue and are already counted in
    // `self.retrayble_errored_transaction_count`.
    account_lock_throttled_transactions_count: u64,

    // total number of transactions that were excluded from the block because their write
    // account locks exceed the limit.
    // These transactions are not retried.
    account_locks_limit_throttled_transactions_count: u64,

    // total number of transactions that were excluded from the block because they were too expensive
    // according to the cost model. These transactions are added back to the buffered queue and are
    // already counted in `self.retrayble_errored_transaction_count`.
    cost_model_throttled_transactions_count: u64,

    // total number of forwardsable packets that failed forwarding
    failed_forwarded_packets_count: u64,

    // total number of forwardsable packets that were successfully forwarded
    successful_forwarded_packets_count: u64,

    // total number of attempted forwards that failed. Note this is not a count of the number of packets
    // that failed, just the total number of batches of packets that failed forwarding
    packet_batch_forward_failure_count: u64,

    // total number of valid unprocessed packets in the buffer that were removed after being forwarded
    cleared_from_buffer_after_forward_count: u64,

    // total number of forwardable batches that were attempted for forwarding. A forwardable batch
    // is defined in `ForwardPacketBatchesByAccounts` in `forward_packet_batches_by_accounts.rs`
    forwardable_batches_count: u64,
}

impl LeaderSlotPacketCountMetrics {
    fn new() -> Self {
        Self { ..Self::default() }
    }

    fn report(&self, id: u32, slot: Slot) {
        datapoint_info!(
            "banking_stage-leader_slot_packet_counts",
            ("id", id as i64, i64),
            ("slot", slot as i64, i64),
            (
                "total_new_valid_packets",
                self.total_new_valid_packets as i64,
                i64
            ),
            (
                "newly_failed_sigverify_count",
                self.newly_failed_sigverify_count as i64,
                i64
            ),
            (
                "exceeded_buffer_limit_dropped_packets_count",
                self.exceeded_buffer_limit_dropped_packets_count as i64,
                i64
            ),
            (
                "newly_buffered_packets_count",
                self.newly_buffered_packets_count as i64,
                i64
            ),
            (
                "retryable_packets_filtered_count",
                self.retryable_packets_filtered_count as i64,
                i64
            ),
            (
                "transactions_attempted_execution_count",
                self.transactions_attempted_execution_count as i64,
                i64
            ),
            (
                "committed_transactions_count",
                self.committed_transactions_count as i64,
                i64
            ),
            (
                "committed_transactions_with_successful_result_count",
                self.committed_transactions_with_successful_result_count as i64,
                i64
            ),
            (
                "retryable_errored_transaction_count",
                self.retryable_errored_transaction_count as i64,
                i64
            ),
            (
                "retryable_packets_count",
                self.retryable_packets_count as i64,
                i64
            ),
            (
                "nonretryable_errored_transactions_count",
                self.nonretryable_errored_transactions_count as i64,
                i64
            ),
            (
                "executed_transactions_failed_commit_count",
                self.executed_transactions_failed_commit_count as i64,
                i64
            ),
            (
                "account_lock_throttled_transactions_count",
                self.account_lock_throttled_transactions_count as i64,
                i64
            ),
            (
                "account_locks_limit_throttled_transactions_count",
                self.account_locks_limit_throttled_transactions_count as i64,
                i64
            ),
            (
                "cost_model_throttled_transactions_count",
                self.cost_model_throttled_transactions_count as i64,
                i64
            ),
            (
                "failed_forwarded_packets_count",
                self.failed_forwarded_packets_count as i64,
                i64
            ),
            (
                "successful_forwarded_packets_count",
                self.successful_forwarded_packets_count as i64,
                i64
            ),
            (
                "packet_batch_forward_failure_count",
                self.packet_batch_forward_failure_count as i64,
                i64
            ),
            (
                "cleared_from_buffer_after_forward_count",
                self.cleared_from_buffer_after_forward_count as i64,
                i64
            ),
            (
                "forwardable_batches_count",
                self.forwardable_batches_count as i64,
                i64
            ),
            (
                "end_of_slot_unprocessed_buffer_len",
                self.end_of_slot_unprocessed_buffer_len as i64,
                i64
            ),
        );
    }
}

#[derive(Debug)]
pub(crate) struct LeaderSlotMetrics {
    // banking_stage creates one QosService instance per working threads, that is uniquely
    // identified by id. This field allows to categorize metrics for gossip votes, TPU votes
    // and other transactions.
    id: u32,

    // aggregate metrics per slot
    slot: Slot,

    packet_count_metrics: LeaderSlotPacketCountMetrics,

    transaction_error_metrics: TransactionErrorMetrics,

    vote_packet_count_metrics: VotePacketCountMetrics,

    timing_metrics: LeaderSlotTimingMetrics,

    // Used by tests to check if the `self.report()` method was called
    is_reported: bool,
}

impl LeaderSlotMetrics {
    pub(crate) fn new(id: u32, slot: Slot, bank_creation_time: &Instant) -> Self {
        Self {
            id,
            slot,
            packet_count_metrics: LeaderSlotPacketCountMetrics::new(),
            transaction_error_metrics: TransactionErrorMetrics::new(),
            vote_packet_count_metrics: VotePacketCountMetrics::new(),
            timing_metrics: LeaderSlotTimingMetrics::new(bank_creation_time),
            is_reported: false,
        }
    }

    pub(crate) fn report(&mut self) {
        self.is_reported = true;

        self.timing_metrics.report(self.id, self.slot);
        self.transaction_error_metrics.report(self.id, self.slot);
        self.packet_count_metrics.report(self.id, self.slot);
        self.vote_packet_count_metrics.report(self.id, self.slot);
    }

    /// Returns `Some(self.slot)` if the metrics have been reported, otherwise returns None
    fn reported_slot(&self) -> Option<Slot> {
        if self.is_reported {
            Some(self.slot)
        } else {
            None
        }
    }

    fn mark_slot_end_detected(&mut self) {
        self.timing_metrics.mark_slot_end_detected();
    }
}

// Metrics describing vote tx packets that were processed in the tpu vote thread as well as
// extraneous votes that were filtered out
#[derive(Debug, Default)]
pub(crate) struct VotePacketCountMetrics {
    // How many votes ingested from gossip were dropped
    dropped_gossip_votes: u64,

    // How many votes ingested from tpu were dropped
    dropped_tpu_votes: u64,
}

impl VotePacketCountMetrics {
    fn new() -> Self {
        Self { ..Self::default() }
    }

    fn report(&self, id: u32, slot: Slot) {
        datapoint_info!(
            "banking_stage-vote_packet_counts",
            ("id", id, i64),
            ("slot", slot, i64),
            ("dropped_gossip_votes", self.dropped_gossip_votes, i64),
            ("dropped_tpu_votes", self.dropped_tpu_votes, i64)
        );
    }
}

#[derive(Debug)]
pub(crate) enum MetricsTrackerAction {
    Noop,
    ReportAndResetTracker,
    NewTracker(Option<LeaderSlotMetrics>),
    ReportAndNewTracker(Option<LeaderSlotMetrics>),
}

#[derive(Debug)]
pub struct LeaderSlotMetricsTracker {
    // Only `Some` if BankingStage detects it's time to construct our leader slot,
    // otherwise `None`
    leader_slot_metrics: Option<LeaderSlotMetrics>,
    id: u32,
}

impl LeaderSlotMetricsTracker {
    pub fn new(id: u32) -> Self {
        Self {
            leader_slot_metrics: None,
            id,
        }
    }

    // Check leader slot, return MetricsTrackerAction to be applied by apply_action()
    pub(crate) fn check_leader_slot_boundary(
        &mut self,
        bank_start: &Option<BankStart>,
    ) -> MetricsTrackerAction {
        match (self.leader_slot_metrics.as_mut(), bank_start) {
            (None, None) => MetricsTrackerAction::Noop,

            (Some(leader_slot_metrics), None) => {
                leader_slot_metrics.mark_slot_end_detected();
                MetricsTrackerAction::ReportAndResetTracker
            }

            // Our leader slot has begain, time to create a new slot tracker
            (None, Some(bank_start)) => {
                MetricsTrackerAction::NewTracker(Some(LeaderSlotMetrics::new(
                    self.id,
                    bank_start.working_bank.slot(),
                    &bank_start.bank_creation_time,
                )))
            }

            (Some(leader_slot_metrics), Some(bank_start)) => {
                if leader_slot_metrics.slot != bank_start.working_bank.slot() {
                    // Last slot has ended, new slot has began
                    leader_slot_metrics.mark_slot_end_detected();
                    MetricsTrackerAction::ReportAndNewTracker(Some(LeaderSlotMetrics::new(
                        self.id,
                        bank_start.working_bank.slot(),
                        &bank_start.bank_creation_time,
                    )))
                } else {
                    MetricsTrackerAction::Noop
                }
            }
        }
    }

    pub(crate) fn apply_action(&mut self, action: MetricsTrackerAction) -> Option<Slot> {
        match action {
            MetricsTrackerAction::Noop => None,
            MetricsTrackerAction::ReportAndResetTracker => {
                let mut reported_slot = None;
                if let Some(leader_slot_metrics) = self.leader_slot_metrics.as_mut() {
                    leader_slot_metrics.report();
                    reported_slot = leader_slot_metrics.reported_slot();
                }
                self.leader_slot_metrics = None;
                reported_slot
            }
            MetricsTrackerAction::NewTracker(new_slot_metrics) => {
                self.leader_slot_metrics = new_slot_metrics;
                self.leader_slot_metrics.as_ref().unwrap().reported_slot()
            }
            MetricsTrackerAction::ReportAndNewTracker(new_slot_metrics) => {
                let mut reported_slot = None;
                if let Some(leader_slot_metrics) = self.leader_slot_metrics.as_mut() {
                    leader_slot_metrics.report();
                    reported_slot = leader_slot_metrics.reported_slot();
                }
                self.leader_slot_metrics = new_slot_metrics;
                reported_slot
            }
        }
    }

    pub(crate) fn accumulate_process_transactions_summary(
        &mut self,
        process_transactions_summary: &ProcessTransactionsSummary,
    ) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            let ProcessTransactionsSummary {
                transactions_attempted_execution_count,
                committed_transactions_count,
                committed_transactions_with_successful_result_count,
                failed_commit_count,
                ref retryable_transaction_indexes,
                cost_model_throttled_transactions_count,
                cost_model_us,
                ref execute_and_commit_timings,
                error_counters,
                ..
            } = process_transactions_summary;

            saturating_add_assign!(
                leader_slot_metrics
                    .packet_count_metrics
                    .transactions_attempted_execution_count,
                *transactions_attempted_execution_count as u64
            );

            saturating_add_assign!(
                leader_slot_metrics
                    .packet_count_metrics
                    .committed_transactions_count,
                *committed_transactions_count as u64
            );

            saturating_add_assign!(
                leader_slot_metrics
                    .packet_count_metrics
                    .committed_transactions_with_successful_result_count,
                *committed_transactions_with_successful_result_count as u64
            );

            saturating_add_assign!(
                leader_slot_metrics
                    .packet_count_metrics
                    .executed_transactions_failed_commit_count,
                *failed_commit_count as u64
            );

            saturating_add_assign!(
                leader_slot_metrics
                    .packet_count_metrics
                    .retryable_errored_transaction_count,
                retryable_transaction_indexes.len() as u64
            );

            saturating_add_assign!(
                leader_slot_metrics
                    .packet_count_metrics
                    .nonretryable_errored_transactions_count,
                transactions_attempted_execution_count
                    .saturating_sub(*committed_transactions_count)
                    .saturating_sub(retryable_transaction_indexes.len()) as u64
            );

            saturating_add_assign!(
                leader_slot_metrics
                    .packet_count_metrics
                    .account_lock_throttled_transactions_count,
                error_counters.account_in_use as u64
            );

            saturating_add_assign!(
                leader_slot_metrics
                    .packet_count_metrics
                    .account_locks_limit_throttled_transactions_count,
                error_counters.too_many_account_locks as u64
            );

            saturating_add_assign!(
                leader_slot_metrics
                    .packet_count_metrics
                    .cost_model_throttled_transactions_count,
                *cost_model_throttled_transactions_count as u64
            );

            saturating_add_assign!(
                leader_slot_metrics
                    .timing_metrics
                    .process_packets_timings
                    .cost_model_us,
                *cost_model_us
            );

            leader_slot_metrics
                .timing_metrics
                .execute_and_commit_timings
                .accumulate(execute_and_commit_timings);
        }
    }

    pub(crate) fn accumulate_insert_packet_batches_summary(
        &mut self,
        insert_packet_batches_summary: &InsertPacketBatchSummary,
    ) {
        self.increment_exceeded_buffer_limit_dropped_packets_count(
            insert_packet_batches_summary.total_dropped_packets() as u64,
        );
        self.increment_dropped_gossip_vote_count(
            insert_packet_batches_summary.dropped_gossip_packets() as u64,
        );
        self.increment_dropped_tpu_vote_count(
            insert_packet_batches_summary.dropped_tpu_packets() as u64
        );
    }

    pub(crate) fn accumulate_transaction_errors(
        &mut self,
        error_metrics: &TransactionErrorMetrics,
    ) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            leader_slot_metrics
                .transaction_error_metrics
                .accumulate(error_metrics);
        }
    }

    // Packet inflow/outflow/processing metrics
    pub(crate) fn increment_total_new_valid_packets(&mut self, count: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            saturating_add_assign!(
                leader_slot_metrics
                    .packet_count_metrics
                    .total_new_valid_packets,
                count
            );
        }
    }

    pub(crate) fn increment_newly_failed_sigverify_count(&mut self, count: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            saturating_add_assign!(
                leader_slot_metrics
                    .packet_count_metrics
                    .newly_failed_sigverify_count,
                count
            );
        }
    }

    pub(crate) fn increment_exceeded_buffer_limit_dropped_packets_count(&mut self, count: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            saturating_add_assign!(
                leader_slot_metrics
                    .packet_count_metrics
                    .exceeded_buffer_limit_dropped_packets_count,
                count
            );
        }
    }

    pub(crate) fn increment_newly_buffered_packets_count(&mut self, count: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            saturating_add_assign!(
                leader_slot_metrics
                    .packet_count_metrics
                    .newly_buffered_packets_count,
                count
            );
        }
    }

    pub(crate) fn increment_retryable_packets_filtered_count(&mut self, count: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            saturating_add_assign!(
                leader_slot_metrics
                    .packet_count_metrics
                    .retryable_packets_filtered_count,
                count
            );
        }
    }

    pub(crate) fn increment_failed_forwarded_packets_count(&mut self, count: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            saturating_add_assign!(
                leader_slot_metrics
                    .packet_count_metrics
                    .failed_forwarded_packets_count,
                count
            );
        }
    }

    pub(crate) fn increment_successful_forwarded_packets_count(&mut self, count: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            saturating_add_assign!(
                leader_slot_metrics
                    .packet_count_metrics
                    .successful_forwarded_packets_count,
                count
            );
        }
    }

    pub(crate) fn increment_packet_batch_forward_failure_count(&mut self, count: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            saturating_add_assign!(
                leader_slot_metrics
                    .packet_count_metrics
                    .packet_batch_forward_failure_count,
                count
            );
        }
    }

    pub(crate) fn increment_cleared_from_buffer_after_forward_count(&mut self, count: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            saturating_add_assign!(
                leader_slot_metrics
                    .packet_count_metrics
                    .cleared_from_buffer_after_forward_count,
                count
            );
        }
    }

    pub(crate) fn increment_forwardable_batches_count(&mut self, count: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            saturating_add_assign!(
                leader_slot_metrics
                    .packet_count_metrics
                    .forwardable_batches_count,
                count
            );
        }
    }

    pub(crate) fn increment_retryable_packets_count(&mut self, count: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            saturating_add_assign!(
                leader_slot_metrics
                    .packet_count_metrics
                    .retryable_packets_count,
                count
            );
        }
    }

    pub(crate) fn set_end_of_slot_unprocessed_buffer_len(&mut self, len: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            leader_slot_metrics
                .packet_count_metrics
                .end_of_slot_unprocessed_buffer_len = len;
        }
    }

    // Outermost banking thread's loop timing metrics
    pub(crate) fn increment_process_buffered_packets_us(&mut self, us: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            saturating_add_assign!(
                leader_slot_metrics
                    .timing_metrics
                    .outer_loop_timings
                    .process_buffered_packets_us,
                us
            );
        }
    }

    pub(crate) fn increment_receive_and_buffer_packets_us(&mut self, us: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            saturating_add_assign!(
                leader_slot_metrics
                    .timing_metrics
                    .outer_loop_timings
                    .receive_and_buffer_packets_us,
                us
            );
            saturating_add_assign!(
                leader_slot_metrics
                    .timing_metrics
                    .outer_loop_timings
                    .receive_and_buffer_packets_invoked_count,
                1
            );
        }
    }

    // Processing buffer timing metrics
    pub(crate) fn increment_make_decision_us(&mut self, us: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            saturating_add_assign!(
                leader_slot_metrics
                    .timing_metrics
                    .process_buffered_packets_timings
                    .make_decision_us,
                us
            );
        }
    }

    pub(crate) fn increment_consume_buffered_packets_us(&mut self, us: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            saturating_add_assign!(
                leader_slot_metrics
                    .timing_metrics
                    .process_buffered_packets_timings
                    .consume_buffered_packets_us,
                us
            );
        }
    }

    pub(crate) fn increment_forward_us(&mut self, us: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            saturating_add_assign!(
                leader_slot_metrics
                    .timing_metrics
                    .process_buffered_packets_timings
                    .forward_us,
                us
            );
        }
    }

    pub(crate) fn increment_forward_and_hold_us(&mut self, us: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            saturating_add_assign!(
                leader_slot_metrics
                    .timing_metrics
                    .process_buffered_packets_timings
                    .forward_and_hold_us,
                us
            );
        }
    }

    pub(crate) fn increment_process_packets_transactions_us(&mut self, us: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            saturating_add_assign!(
                leader_slot_metrics
                    .timing_metrics
                    .consume_buffered_packets_timings
                    .process_packets_transactions_us,
                us
            );
        }
    }

    // Processing packets timing metrics
    pub(crate) fn increment_transactions_from_packets_us(&mut self, us: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            saturating_add_assign!(
                leader_slot_metrics
                    .timing_metrics
                    .process_packets_timings
                    .transactions_from_packets_us,
                us
            );
        }
    }

    pub(crate) fn increment_process_transactions_us(&mut self, us: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            saturating_add_assign!(
                leader_slot_metrics
                    .timing_metrics
                    .process_packets_timings
                    .process_transactions_us,
                us
            );
        }
    }

    pub(crate) fn increment_filter_retryable_packets_us(&mut self, us: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            saturating_add_assign!(
                leader_slot_metrics
                    .timing_metrics
                    .process_packets_timings
                    .filter_retryable_packets_us,
                us
            );
        }
    }

    pub(crate) fn increment_dropped_gossip_vote_count(&mut self, count: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            saturating_add_assign!(
                leader_slot_metrics
                    .vote_packet_count_metrics
                    .dropped_gossip_votes,
                count
            );
        }
    }

    pub(crate) fn increment_dropped_tpu_vote_count(&mut self, count: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            saturating_add_assign!(
                leader_slot_metrics
                    .vote_packet_count_metrics
                    .dropped_tpu_votes,
                count
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_runtime::{bank::Bank, genesis_utils::create_genesis_config},
        solana_sdk::pubkey::Pubkey,
        std::{mem, sync::Arc},
    };

    struct TestSlotBoundaryComponents {
        first_bank: Arc<Bank>,
        first_poh_recorder_bank: BankStart,
        next_bank: Arc<Bank>,
        next_poh_recorder_bank: BankStart,
        leader_slot_metrics_tracker: LeaderSlotMetricsTracker,
    }

    fn setup_test_slot_boundary_banks() -> TestSlotBoundaryComponents {
        let genesis = create_genesis_config(10);
        let first_bank = Arc::new(Bank::new_for_tests(&genesis.genesis_config));
        let first_poh_recorder_bank = BankStart {
            working_bank: first_bank.clone(),
            bank_creation_time: Arc::new(Instant::now()),
        };

        // Create a child descended from the first bank
        let next_bank = Arc::new(Bank::new_from_parent(
            &first_bank,
            &Pubkey::new_unique(),
            first_bank.slot() + 1,
        ));
        let next_poh_recorder_bank = BankStart {
            working_bank: next_bank.clone(),
            bank_creation_time: Arc::new(Instant::now()),
        };

        let banking_stage_thread_id = 0;
        let leader_slot_metrics_tracker = LeaderSlotMetricsTracker::new(banking_stage_thread_id);

        TestSlotBoundaryComponents {
            first_bank,
            first_poh_recorder_bank,
            next_bank,
            next_poh_recorder_bank,
            leader_slot_metrics_tracker,
        }
    }

    #[test]
    pub fn test_update_on_leader_slot_boundary_not_leader_to_not_leader() {
        let TestSlotBoundaryComponents {
            mut leader_slot_metrics_tracker,
            ..
        } = setup_test_slot_boundary_banks();
        // Test that with no bank being tracked, and no new bank being tracked, nothing is reported
        let action = leader_slot_metrics_tracker.check_leader_slot_boundary(&None);
        assert_eq!(
            mem::discriminant(&MetricsTrackerAction::Noop),
            mem::discriminant(&action)
        );
        assert!(leader_slot_metrics_tracker.apply_action(action).is_none());
        assert!(leader_slot_metrics_tracker.leader_slot_metrics.is_none());
    }

    #[test]
    pub fn test_update_on_leader_slot_boundary_not_leader_to_leader() {
        let TestSlotBoundaryComponents {
            first_poh_recorder_bank,
            mut leader_slot_metrics_tracker,
            ..
        } = setup_test_slot_boundary_banks();

        // Test case where the thread has not detected a leader bank, and now sees a leader bank.
        // Metrics should not be reported because leader slot has not ended
        assert!(leader_slot_metrics_tracker.leader_slot_metrics.is_none());
        let action =
            leader_slot_metrics_tracker.check_leader_slot_boundary(&Some(first_poh_recorder_bank));
        assert_eq!(
            mem::discriminant(&MetricsTrackerAction::NewTracker(None)),
            mem::discriminant(&action)
        );
        assert!(leader_slot_metrics_tracker.apply_action(action).is_none());
        assert!(leader_slot_metrics_tracker.leader_slot_metrics.is_some());
    }

    #[test]
    pub fn test_update_on_leader_slot_boundary_leader_to_not_leader() {
        let TestSlotBoundaryComponents {
            first_bank,
            first_poh_recorder_bank,
            mut leader_slot_metrics_tracker,
            ..
        } = setup_test_slot_boundary_banks();

        // Test case where the thread has a leader bank, and now detects there's no more leader bank,
        // implying the slot has ended. Metrics should be reported for `first_bank.slot()`,
        // because that leader slot has just ended.
        {
            // Setup first_bank
            let action = leader_slot_metrics_tracker
                .check_leader_slot_boundary(&Some(first_poh_recorder_bank));
            assert!(leader_slot_metrics_tracker.apply_action(action).is_none());
        }
        {
            // Assert reporting if slot has ended
            let action = leader_slot_metrics_tracker.check_leader_slot_boundary(&None);
            assert_eq!(
                mem::discriminant(&MetricsTrackerAction::ReportAndResetTracker),
                mem::discriminant(&action)
            );
            assert_eq!(
                leader_slot_metrics_tracker.apply_action(action).unwrap(),
                first_bank.slot()
            );
            assert!(leader_slot_metrics_tracker.leader_slot_metrics.is_none());
        }
        {
            // Assert no-op if still no new bank
            let action = leader_slot_metrics_tracker.check_leader_slot_boundary(&None);
            assert_eq!(
                mem::discriminant(&MetricsTrackerAction::Noop),
                mem::discriminant(&action)
            );
        }
    }

    #[test]
    pub fn test_update_on_leader_slot_boundary_leader_to_leader_same_slot() {
        let TestSlotBoundaryComponents {
            first_bank,
            first_poh_recorder_bank,
            mut leader_slot_metrics_tracker,
            ..
        } = setup_test_slot_boundary_banks();

        // Test case where the thread has a leader bank, and now detects the same leader bank,
        // implying the slot is still running. Metrics should not be reported
        {
            // Setup with first_bank
            let action = leader_slot_metrics_tracker
                .check_leader_slot_boundary(&Some(first_poh_recorder_bank.clone()));
            assert!(leader_slot_metrics_tracker.apply_action(action).is_none());
        }
        {
            // Assert nop-op if same bank
            let action = leader_slot_metrics_tracker
                .check_leader_slot_boundary(&Some(first_poh_recorder_bank));
            assert_eq!(
                mem::discriminant(&MetricsTrackerAction::Noop),
                mem::discriminant(&action)
            );
            assert!(leader_slot_metrics_tracker.apply_action(action).is_none());
        }
        {
            // Assert reporting if slot has ended
            let action = leader_slot_metrics_tracker.check_leader_slot_boundary(&None);
            assert_eq!(
                mem::discriminant(&MetricsTrackerAction::ReportAndResetTracker),
                mem::discriminant(&action)
            );
            assert_eq!(
                leader_slot_metrics_tracker.apply_action(action).unwrap(),
                first_bank.slot()
            );
            assert!(leader_slot_metrics_tracker.leader_slot_metrics.is_none());
        }
    }

    #[test]
    pub fn test_update_on_leader_slot_boundary_leader_to_leader_bigger_slot() {
        let TestSlotBoundaryComponents {
            first_bank,
            first_poh_recorder_bank,
            next_bank,
            next_poh_recorder_bank,
            mut leader_slot_metrics_tracker,
        } = setup_test_slot_boundary_banks();

        // Test case where the thread has a leader bank, and now detects there's a new leader bank
        // for a bigger slot, implying the slot has ended. Metrics should be reported for the
        // smaller slot
        {
            // Setup with first_bank
            let action = leader_slot_metrics_tracker
                .check_leader_slot_boundary(&Some(first_poh_recorder_bank));
            assert!(leader_slot_metrics_tracker.apply_action(action).is_none());
        }
        {
            // Assert reporting if new bank
            let action = leader_slot_metrics_tracker
                .check_leader_slot_boundary(&Some(next_poh_recorder_bank));
            assert_eq!(
                mem::discriminant(&MetricsTrackerAction::ReportAndNewTracker(None)),
                mem::discriminant(&action)
            );
            assert_eq!(
                leader_slot_metrics_tracker.apply_action(action).unwrap(),
                first_bank.slot()
            );
            assert!(leader_slot_metrics_tracker.leader_slot_metrics.is_some());
        }
        {
            // Assert reporting if slot has ended
            let action = leader_slot_metrics_tracker.check_leader_slot_boundary(&None);
            assert_eq!(
                mem::discriminant(&MetricsTrackerAction::ReportAndResetTracker),
                mem::discriminant(&action)
            );
            assert_eq!(
                leader_slot_metrics_tracker.apply_action(action).unwrap(),
                next_bank.slot()
            );
            assert!(leader_slot_metrics_tracker.leader_slot_metrics.is_none());
        }
    }

    #[test]
    pub fn test_update_on_leader_slot_boundary_leader_to_leader_smaller_slot() {
        let TestSlotBoundaryComponents {
            first_bank,
            first_poh_recorder_bank,
            next_bank,
            next_poh_recorder_bank,
            mut leader_slot_metrics_tracker,
        } = setup_test_slot_boundary_banks();
        // Test case where the thread has a leader bank, and now detects there's a new leader bank
        // for a samller slot, implying the slot has ended. Metrics should be reported for the
        // bigger slot
        {
            // Setup with next_bank
            let action = leader_slot_metrics_tracker
                .check_leader_slot_boundary(&Some(next_poh_recorder_bank));
            assert!(leader_slot_metrics_tracker.apply_action(action).is_none());
        }
        {
            // Assert reporting if new bank
            let action = leader_slot_metrics_tracker
                .check_leader_slot_boundary(&Some(first_poh_recorder_bank));
            assert_eq!(
                mem::discriminant(&MetricsTrackerAction::ReportAndNewTracker(None)),
                mem::discriminant(&action)
            );
            assert_eq!(
                leader_slot_metrics_tracker.apply_action(action).unwrap(),
                next_bank.slot()
            );
            assert!(leader_slot_metrics_tracker.leader_slot_metrics.is_some());
        }
        {
            // Assert reporting if slot has ended
            let action = leader_slot_metrics_tracker.check_leader_slot_boundary(&None);
            assert_eq!(
                mem::discriminant(&MetricsTrackerAction::ReportAndResetTracker),
                mem::discriminant(&action)
            );
            assert_eq!(
                leader_slot_metrics_tracker.apply_action(action).unwrap(),
                first_bank.slot()
            );
            assert!(leader_slot_metrics_tracker.leader_slot_metrics.is_none());
        }
    }
}

use {solana_poh::poh_recorder::BankStart, solana_sdk::clock::Slot, std::time::Instant};

// Metrics capturing wallclock time spent in various parts of BankingStage during this
// validator's leader slot
#[derive(Debug, Default)]
struct LeaderSlotTimingMetrics {}

// Metrics describing packets ingested/processed in various parts of BankingStage during this
// validator's leader slot
#[derive(Debug, Default)]
struct LeaderSlotPacketCountMetrics {
    // total number of packets in the buffer when the leader slot began
    buffered_packets_starting_count: u64,

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

    // total number of transactions that attempted execution due to some fatal error (too old, duplicate signature, etc.)
    // AND were dropped from the buffered queue
    nonretryable_errored_transactions_count: u64,

    // total number of transactions that were executed, but failed to be committed into the Poh stream because
    // the block ended. Some of these may be already counted in `nonretryable_errored_transactions_count` if they
    // then hit the age limit after failing to be comitted.
    executed_transactions_failed_commit_count: u64,

    // total number of transactions that were excluded from the block because they were too expensive
    // according to the cost model. These transactions are added back to the buffered queue and are
    // already counted in `self.retrayble_errored_transaction_count`.
    cost_model_throttled_transactions_count: u64,

    // total number of unprocessed packets that were valid for forwarding
    forwardable_packet_candidates_count: u64,

    // total number of the `forwardable_packet_candidates_count` packets that were successfully
    // forwarded
    successful_forwarded_packets_count: u64,

    // total number of attempted forwards that failed. Note this is not a count of the number of packets
    // that failed, just the total number of batches of packets that failed forwarding
    packet_batch_forward_failure_count: u64,

    // total number of valid unprocessed packets in the buffer that were removed after being forwarded
    cleared_from_buffer_after_forward_count: u64,
}

impl LeaderSlotPacketCountMetrics {
    fn new(buffered_packets_starting_count: u64) -> Self {
        Self {
            buffered_packets_starting_count,
            ..Self::default()
        }
    }
}

impl LeaderSlotMetricsTracker {
    pub(crate) fn new(id: u32) -> Self {
        Self {
            leader_slot_metrics: None,
            id,
        }
    }

    pub(crate) fn update_on_leader_slot_boundary(&mut self, bank_start: &Option<BankStart>) {
        match (self.leader_slot_metrics.as_mut(), bank_start) {
            (None, None) => {}

            (Some(leader_slot_metrics), None) => {
                // Slot has ended, time to report metrics
                leader_slot_metrics.report();
            }

            (None, Some(bank_start)) => {
                // Our leader slot has begain, time to create a new slot tracker
                self.leader_slot_metrics = Some(LeaderSlotMetrics::new(
                    self.id,
                    bank_start.working_bank.slot(),
                    &bank_start.bank_creation_time,
                    // TODO: track the number of packets in buffered queue
                    0,
                ));
            }

            (Some(leader_slot_metrics), Some(bank_start)) => {
                if leader_slot_metrics.slot != bank_start.working_bank.slot() {
                    // Last slot has ended, new slot has began
                    leader_slot_metrics.report();
                    self.leader_slot_metrics = Some(LeaderSlotMetrics::new(
                        self.id,
                        bank_start.working_bank.slot(),
                        &bank_start.bank_creation_time,
                        // TODO: track the number of packets in buffered queue,
                        0,
                    ));
                }
            }
        }
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

    // when the bank was detected
    bank_detected_time: Instant,

    // delay from when the bank was created to when this thread detected it
    bank_detected_delay_us: u64,

    packet_count_metrics: LeaderSlotPacketCountMetrics,

    timing_metrics: LeaderSlotTimingMetrics,
}

impl LeaderSlotMetrics {
    pub(crate) fn new(
        id: u32,
        slot: Slot,
        bank_creation_time: &Instant,
        buffered_packets_starting_count: u64,
    ) -> Self {
        Self {
            id,
            slot,
            bank_detected_time: Instant::now(),
            bank_detected_delay_us: bank_creation_time.elapsed().as_micros() as u64,
            packet_count_metrics: LeaderSlotPacketCountMetrics::new(
                buffered_packets_starting_count,
            ),
            timing_metrics: LeaderSlotTimingMetrics::default(),
        }
    }

    pub(crate) fn report(&self) {
        let bank_detected_to_now = self.bank_detected_time.elapsed().as_micros() as u64;
        datapoint_info!(
            "banking_stage-leader_slot_timing_metrics",
            ("id", self.id as i64, i64),
            ("slot", self.slot as i64, i64),
            ("bank_detected_to_now_us", bank_detected_to_now, i64),
            (
                "bank_creation_to_now_us",
                bank_detected_to_now + self.bank_detected_delay_us,
                i64
            ),
            ("bank_detected_delay_us", self.bank_detected_delay_us, i64),
        );

        datapoint_info!(
            "banking_stage-leader_slot_packet_count_metrics",
            ("id", self.id as i64, i64),
            ("slot", self.slot as i64, i64),
            (
                "total_new_valid_packets",
                self.packet_count_metrics.total_new_valid_packets as i64,
                i64
            ),
            (
                "newly_failed_sigverify_count",
                self.packet_count_metrics.newly_failed_sigverify_count as i64,
                i64
            ),
            (
                "exceeded_buffer_limit_dropped_packets_count",
                self.packet_count_metrics
                    .exceeded_buffer_limit_dropped_packets_count as i64,
                i64
            ),
            (
                "newly_buffered_packets_count",
                self.packet_count_metrics.newly_buffered_packets_count as i64,
                i64
            ),
            (
                "retryable_packets_filtered_count",
                self.packet_count_metrics.retryable_packets_filtered_count as i64,
                i64
            ),
            (
                "transactions_attempted_execution_count",
                self.packet_count_metrics
                    .transactions_attempted_execution_count as i64,
                i64
            ),
            (
                "committed_transactions_count",
                self.packet_count_metrics.committed_transactions_count as i64,
                i64
            ),
            (
                "committed_transactions_with_successful_result_count",
                self.packet_count_metrics
                    .committed_transactions_with_successful_result_count as i64,
                i64
            ),
            (
                "retryable_errored_transaction_count",
                self.packet_count_metrics
                    .retryable_errored_transaction_count as i64,
                i64
            ),
            (
                "nonretryable_errored_transactions_count",
                self.packet_count_metrics
                    .nonretryable_errored_transactions_count as i64,
                i64
            ),
            (
                "executed_transactions_failed_commit_count",
                self.packet_count_metrics
                    .executed_transactions_failed_commit_count as i64,
                i64
            ),
            (
                "cost_model_throttled_transactions_count",
                self.packet_count_metrics
                    .cost_model_throttled_transactions_count as i64,
                i64
            ),
            (
                "forwardable_packet_candidates_count",
                self.packet_count_metrics
                    .forwardable_packet_candidates_count as i64,
                i64
            ),
            (
                "successful_forwarded_packets_count",
                self.packet_count_metrics.successful_forwarded_packets_count as i64,
                i64
            ),
            (
                "packet_batch_forward_failure_count",
                self.packet_count_metrics.packet_batch_forward_failure_count as i64,
                i64
            ),
            (
                "cleared_from_buffer_after_forward_count",
                self.packet_count_metrics
                    .cleared_from_buffer_after_forward_count as i64,
                i64
            ),
        );
    }
}

#[derive(Debug)]
pub struct LeaderSlotMetricsTracker {
    // Only `Some` if BankingStage detects it's time to construct our leader slot,
    // otherwise `None`
    leader_slot_metrics: Option<LeaderSlotMetrics>,
    id: u32,
}

impl LeaderSlotMetricsTracker {
    pub(crate) fn increment_total_new_valid_packets(&mut self, count: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            leader_slot_metrics
                .packet_count_metrics
                .total_new_valid_packets = leader_slot_metrics
                .packet_count_metrics
                .total_new_valid_packets
                .saturating_add(count);
        }
    }

    pub(crate) fn increment_newly_failed_sigverify_count(&mut self, count: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            leader_slot_metrics
                .packet_count_metrics
                .newly_failed_sigverify_count = leader_slot_metrics
                .packet_count_metrics
                .newly_failed_sigverify_count
                .saturating_add(count);
        }
    }

    pub(crate) fn increment_exceeded_buffer_limit_dropped_packets_count(&mut self, count: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            leader_slot_metrics
                .packet_count_metrics
                .exceeded_buffer_limit_dropped_packets_count = leader_slot_metrics
                .packet_count_metrics
                .exceeded_buffer_limit_dropped_packets_count
                .saturating_add(count);
        }
    }

    pub(crate) fn increment_newly_buffered_packets_count(&mut self, count: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            leader_slot_metrics
                .packet_count_metrics
                .newly_buffered_packets_count = leader_slot_metrics
                .packet_count_metrics
                .newly_buffered_packets_count
                .saturating_add(count);
        }
    }

    pub(crate) fn increment_retryable_packets_filtered_count(&mut self, count: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            leader_slot_metrics
                .packet_count_metrics
                .retryable_packets_filtered_count = leader_slot_metrics
                .packet_count_metrics
                .retryable_packets_filtered_count
                .saturating_add(count);
        }
    }

    pub(crate) fn increment_transactions_attempted_execution_count(&mut self, count: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            leader_slot_metrics
                .packet_count_metrics
                .transactions_attempted_execution_count = leader_slot_metrics
                .packet_count_metrics
                .transactions_attempted_execution_count
                .saturating_add(count);
        }
    }

    pub(crate) fn increment_committed_transactions_count(&mut self, count: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            leader_slot_metrics
                .packet_count_metrics
                .committed_transactions_count = leader_slot_metrics
                .packet_count_metrics
                .committed_transactions_count
                .saturating_add(count);
        }
    }

    pub(crate) fn increment_committed_transactions_with_successful_result_count(
        &mut self,
        count: u64,
    ) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            leader_slot_metrics
                .packet_count_metrics
                .committed_transactions_with_successful_result_count = leader_slot_metrics
                .packet_count_metrics
                .committed_transactions_with_successful_result_count
                .saturating_add(count);
        }
    }

    pub(crate) fn increment_retryable_transactions_count(&mut self, count: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            leader_slot_metrics
                .packet_count_metrics
                .retryable_errored_transaction_count = leader_slot_metrics
                .packet_count_metrics
                .retryable_errored_transaction_count
                .saturating_add(count);
        }
    }

    pub(crate) fn increment_nonretryable_errored_transactions_count(&mut self, count: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            leader_slot_metrics
                .packet_count_metrics
                .nonretryable_errored_transactions_count = leader_slot_metrics
                .packet_count_metrics
                .nonretryable_errored_transactions_count
                .saturating_add(count);
        }
    }

    pub(crate) fn increment_cost_model_throttled_transactions_count(&mut self, count: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            leader_slot_metrics
                .packet_count_metrics
                .cost_model_throttled_transactions_count = leader_slot_metrics
                .packet_count_metrics
                .cost_model_throttled_transactions_count
                .saturating_add(count);
        }
    }

    pub(crate) fn increment_forwardable_packet_candidates_count(&mut self, count: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            leader_slot_metrics
                .packet_count_metrics
                .forwardable_packet_candidates_count = leader_slot_metrics
                .packet_count_metrics
                .forwardable_packet_candidates_count
                .saturating_add(count);
        }
    }

    pub(crate) fn increment_successful_forwarded_packets_count(&mut self, count: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            leader_slot_metrics
                .packet_count_metrics
                .successful_forwarded_packets_count = leader_slot_metrics
                .packet_count_metrics
                .successful_forwarded_packets_count
                .saturating_add(count);
        }
    }

    pub(crate) fn increment_packet_batch_forward_failure_count(&mut self, count: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            leader_slot_metrics
                .packet_count_metrics
                .packet_batch_forward_failure_count = leader_slot_metrics
                .packet_count_metrics
                .packet_batch_forward_failure_count
                .saturating_add(count);
        }
    }

    pub(crate) fn increment_cleared_from_buffer_after_forward_count(&mut self, count: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            leader_slot_metrics
                .packet_count_metrics
                .cleared_from_buffer_after_forward_count = leader_slot_metrics
                .packet_count_metrics
                .cleared_from_buffer_after_forward_count
                .saturating_add(count);
        }
    }

    pub(crate) fn increment_executed_transactions_failed_commit_count(&mut self, count: u64) {
        if let Some(leader_slot_metrics) = &mut self.leader_slot_metrics {
            leader_slot_metrics
                .packet_count_metrics
                .executed_transactions_failed_commit_count = leader_slot_metrics
                .packet_count_metrics
                .executed_transactions_failed_commit_count
                .saturating_add(count);
        }
    }
}

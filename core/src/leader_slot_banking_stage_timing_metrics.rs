use {
    solana_poh::poh_recorder::RecordTransactionsTimings,
    solana_program_runtime::timings::ExecuteTimings,
    solana_sdk::{clock::Slot, saturating_add_assign},
    std::time::Instant,
};

#[derive(Default, Debug)]
pub struct LeaderExecuteAndCommitTimings {
    pub collect_balances_us: u64,
    pub load_execute_us: u64,
    pub freeze_lock_us: u64,
    pub last_blockhash_us: u64,
    pub record_us: u64,
    pub commit_us: u64,
    pub find_and_send_votes_us: u64,
    pub record_transactions_timings: RecordTransactionsTimings,
    pub execute_timings: ExecuteTimings,
}

impl LeaderExecuteAndCommitTimings {
    pub fn accumulate(&mut self, other: &LeaderExecuteAndCommitTimings) {
        saturating_add_assign!(self.collect_balances_us, other.collect_balances_us);
        saturating_add_assign!(self.load_execute_us, other.load_execute_us);
        saturating_add_assign!(self.freeze_lock_us, other.freeze_lock_us);
        saturating_add_assign!(self.last_blockhash_us, other.last_blockhash_us);
        saturating_add_assign!(self.record_us, other.record_us);
        saturating_add_assign!(self.commit_us, other.commit_us);
        saturating_add_assign!(self.find_and_send_votes_us, other.find_and_send_votes_us);
        self.record_transactions_timings
            .accumulate(&other.record_transactions_timings);
        self.execute_timings.accumulate(&other.execute_timings);
    }

    pub fn report(&self, id: u32, slot: Slot) {
        datapoint_info!(
            "banking_stage-leader_slot_execute_and_commit_timings",
            ("id", id as i64, i64),
            ("slot", slot as i64, i64),
            ("collect_balances_us", self.collect_balances_us as i64, i64),
            ("load_execute_us", self.load_execute_us as i64, i64),
            ("freeze_lock_us", self.freeze_lock_us as i64, i64),
            ("last_blockhash_us", self.last_blockhash_us as i64, i64),
            ("record_us", self.record_us as i64, i64),
            ("commit_us", self.commit_us as i64, i64),
            (
                "find_and_send_votes_us",
                self.find_and_send_votes_us as i64,
                i64
            ),
        );

        datapoint_info!(
            "banking_stage-leader_slot_record_timings",
            ("id", id as i64, i64),
            ("slot", slot as i64, i64),
            (
                "execution_results_to_transactions_us",
                self.record_transactions_timings
                    .execution_results_to_transactions_us as i64,
                i64
            ),
            (
                "hash_us",
                self.record_transactions_timings.hash_us as i64,
                i64
            ),
            (
                "poh_record_us",
                self.record_transactions_timings.poh_record_us as i64,
                i64
            ),
        );
    }
}

// Metrics capturing wallclock time spent in various parts of BankingStage during this
// validator's leader slot
#[derive(Debug)]
pub(crate) struct LeaderSlotTimingMetrics {
    pub outer_loop_timings: OuterLoopTimings,
    pub process_buffered_packets_timings: ProcessBufferedPacketsTimings,
    pub consume_buffered_packets_timings: ConsumeBufferedPacketsTimings,
    pub process_packets_timings: ProcessPacketsTimings,
    pub execute_and_commit_timings: LeaderExecuteAndCommitTimings,
}

impl LeaderSlotTimingMetrics {
    pub(crate) fn new(bank_creation_time: &Instant) -> Self {
        Self {
            outer_loop_timings: OuterLoopTimings::new(bank_creation_time),
            process_buffered_packets_timings: ProcessBufferedPacketsTimings::default(),
            consume_buffered_packets_timings: ConsumeBufferedPacketsTimings::default(),
            process_packets_timings: ProcessPacketsTimings::default(),
            execute_and_commit_timings: LeaderExecuteAndCommitTimings::default(),
        }
    }

    pub(crate) fn report(&self, id: u32, slot: Slot) {
        self.outer_loop_timings.report(id, slot);
        self.process_buffered_packets_timings.report(id, slot);
        self.consume_buffered_packets_timings.report(id, slot);
        self.process_packets_timings.report(id, slot);
        self.execute_and_commit_timings.report(id, slot);
    }

    pub(crate) fn mark_slot_end_detected(&mut self) {
        self.outer_loop_timings.mark_slot_end_detected();
    }
}

#[derive(Debug)]
pub(crate) struct OuterLoopTimings {
    pub bank_detected_time: Instant,

    // Delay from when the bank was created to when this thread detected it
    pub bank_detected_delay_us: u64,

    // Time spent processing buffered packets
    pub process_buffered_packets_us: u64,

    // Time spent processing new incoming packets to the banking thread
    pub receive_and_buffer_packets_us: u64,

    // The number of times the function to receive and buffer new packets
    // was called
    pub receive_and_buffer_packets_invoked_count: u64,

    // Elapsed time between bank was detected and slot end was detected
    pub bank_detected_to_slot_end_detected_us: u64,
}

impl OuterLoopTimings {
    fn new(bank_creation_time: &Instant) -> Self {
        Self {
            bank_detected_time: Instant::now(),
            bank_detected_delay_us: bank_creation_time.elapsed().as_micros() as u64,
            process_buffered_packets_us: 0,
            receive_and_buffer_packets_us: 0,
            receive_and_buffer_packets_invoked_count: 0,
            bank_detected_to_slot_end_detected_us: 0,
        }
    }

    /// Call when detected slot end to capture elapsed time, which might be reported later
    fn mark_slot_end_detected(&mut self) {
        self.bank_detected_to_slot_end_detected_us =
            self.bank_detected_time.elapsed().as_micros() as u64;
    }

    fn report(&self, id: u32, slot: Slot) {
        datapoint_info!(
            "banking_stage-leader_slot_loop_timings",
            ("id", id as i64, i64),
            ("slot", slot as i64, i64),
            (
                "bank_detected_to_slot_end_detected_us",
                self.bank_detected_to_slot_end_detected_us,
                i64
            ),
            (
                "bank_creation_to_slot_end_detected_us",
                self.bank_detected_to_slot_end_detected_us + self.bank_detected_delay_us,
                i64
            ),
            ("bank_detected_delay_us", self.bank_detected_delay_us, i64),
            (
                "process_buffered_packets_us",
                self.process_buffered_packets_us,
                i64
            ),
            (
                "receive_and_buffer_packets_us",
                self.receive_and_buffer_packets_us,
                i64
            ),
            (
                "receive_and_buffer_packets_invoked_count",
                self.receive_and_buffer_packets_invoked_count,
                i64
            )
        );
    }
}

#[derive(Debug, Default)]
pub(crate) struct ProcessBufferedPacketsTimings {
    pub make_decision_us: u64,
    pub consume_buffered_packets_us: u64,
    pub forward_us: u64,
    pub forward_and_hold_us: u64,
}
impl ProcessBufferedPacketsTimings {
    fn report(&self, id: u32, slot: Slot) {
        datapoint_info!(
            "banking_stage-leader_slot_process_buffered_packets_timings",
            ("id", id as i64, i64),
            ("slot", slot as i64, i64),
            ("make_decision_us", self.make_decision_us as i64, i64),
            (
                "consume_buffered_packets_us",
                self.consume_buffered_packets_us as i64,
                i64
            ),
            ("forward_us", self.forward_us as i64, i64),
            ("forward_and_hold_us", self.forward_and_hold_us as i64, i64),
        );
    }
}

#[derive(Debug, Default)]
pub(crate) struct ConsumeBufferedPacketsTimings {
    // Time spent processing transactions
    pub process_packets_transactions_us: u64,
}

impl ConsumeBufferedPacketsTimings {
    fn report(&self, id: u32, slot: Slot) {
        datapoint_info!(
            "banking_stage-leader_slot_consume_buffered_packets_timings",
            ("id", id as i64, i64),
            ("slot", slot as i64, i64),
            (
                "process_packets_transactions_us",
                self.process_packets_transactions_us as i64,
                i64
            ),
        );
    }
}

#[derive(Debug, Default)]
pub(crate) struct ProcessPacketsTimings {
    // Time spent converting packets to transactions
    pub transactions_from_packets_us: u64,

    // Time spent processing transactions
    pub process_transactions_us: u64,

    // Time spent filtering retryable packets that were returned after transaction
    // processing
    pub filter_retryable_packets_us: u64,

    // Time spent running the cost model in processing transactions before executing
    // transactions
    pub cost_model_us: u64,
}

impl ProcessPacketsTimings {
    fn report(&self, id: u32, slot: Slot) {
        datapoint_info!(
            "banking_stage-leader_slot_process_packets_timings",
            ("id", id as i64, i64),
            ("slot", slot as i64, i64),
            (
                "transactions_from_packets_us",
                self.transactions_from_packets_us,
                i64
            ),
            ("process_transactions_us", self.process_transactions_us, i64),
            (
                "filter_retryable_packets_us",
                self.filter_retryable_packets_us,
                i64
            ),
            ("cost_model_us", self.cost_model_us, i64),
        );
    }
}

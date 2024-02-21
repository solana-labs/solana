use {
    itertools::MinMaxResult,
    solana_sdk::timing::AtomicInterval,
    std::ops::{Deref, DerefMut},
};

#[derive(Default)]
pub struct SchedulerCountMetrics {
    interval: AtomicInterval,
    metrics: SchedulerCountMetricsInner,
}

#[derive(Default)]
pub struct SchedulerCountMetricsInner {
    /// Number of packets received.
    pub num_received: usize,
    /// Number of packets buffered.
    pub num_buffered: usize,

    /// Number of transactions scheduled.
    pub num_scheduled: usize,
    /// Number of transactions that were unschedulable.
    pub num_unschedulable: usize,
    /// Number of transactions that were filtered out during scheduling.
    pub num_schedule_filtered_out: usize,
    /// Number of completed transactions received from workers.
    pub num_finished: usize,
    /// Number of transactions that were retryable.
    pub num_retryable: usize,

    /// Number of transactions that were immediately dropped on receive.
    pub num_dropped_on_receive: usize,
    /// Number of transactions that were dropped due to sanitization failure.
    pub num_dropped_on_sanitization: usize,
    /// Number of transactions that were dropped due to failed lock validation.
    pub num_dropped_on_validate_locks: usize,
    /// Number of transactions that were dropped due to failed transaction
    /// checks during receive.
    pub num_dropped_on_receive_transaction_checks: usize,
    /// Number of transactions that were dropped due to clearing.
    pub num_dropped_on_clear: usize,
    /// Number of transactions that were dropped due to age and status checks.
    pub num_dropped_on_age_and_status: usize,
    /// Number of transactions that were dropped due to exceeded capacity.
    pub num_dropped_on_capacity: usize,
    /// Min prioritization fees in the transaction container
    pub min_prioritization_fees: u64,
    /// Max prioritization fees in the transaction container
    pub max_prioritization_fees: u64,
}

impl Deref for SchedulerCountMetrics {
    type Target = SchedulerCountMetricsInner;
    fn deref(&self) -> &Self::Target {
        &self.metrics
    }
}

impl DerefMut for SchedulerCountMetrics {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.metrics
    }
}

impl SchedulerCountMetrics {
    pub fn maybe_report_and_reset(&mut self, should_report: bool) {
        const REPORT_INTERVAL_MS: u64 = 1000;
        if self.interval.should_update(REPORT_INTERVAL_MS) {
            if should_report {
                self.report("banking_stage_scheduler_counts");
            }
            self.reset();
        }
    }
}

impl SchedulerCountMetricsInner {
    fn report(&self, name: &'static str) {
        datapoint_info!(
            name,
            ("num_received", self.num_received, i64),
            ("num_buffered", self.num_buffered, i64),
            ("num_scheduled", self.num_scheduled, i64),
            ("num_unschedulable", self.num_unschedulable, i64),
            (
                "num_schedule_filtered_out",
                self.num_schedule_filtered_out,
                i64
            ),
            ("num_finished", self.num_finished, i64),
            ("num_retryable", self.num_retryable, i64),
            ("num_dropped_on_receive", self.num_dropped_on_receive, i64),
            (
                "num_dropped_on_sanitization",
                self.num_dropped_on_sanitization,
                i64
            ),
            (
                "num_dropped_on_validate_locks",
                self.num_dropped_on_validate_locks,
                i64
            ),
            (
                "num_dropped_on_receive_transaction_checks",
                self.num_dropped_on_receive_transaction_checks,
                i64
            ),
            ("num_dropped_on_clear", self.num_dropped_on_clear, i64),
            (
                "num_dropped_on_age_and_status",
                self.num_dropped_on_age_and_status,
                i64
            ),
            ("num_dropped_on_capacity", self.num_dropped_on_capacity, i64),
            ("min_priority", self.get_min_priority(), i64),
            ("max_priority", self.get_max_priority(), i64)
        );
    }

    pub fn has_data(&self) -> bool {
        self.num_received != 0
            || self.num_buffered != 0
            || self.num_scheduled != 0
            || self.num_unschedulable != 0
            || self.num_schedule_filtered_out != 0
            || self.num_finished != 0
            || self.num_retryable != 0
            || self.num_dropped_on_receive != 0
            || self.num_dropped_on_sanitization != 0
            || self.num_dropped_on_validate_locks != 0
            || self.num_dropped_on_receive_transaction_checks != 0
            || self.num_dropped_on_clear != 0
            || self.num_dropped_on_age_and_status != 0
            || self.num_dropped_on_capacity != 0
    }

    fn reset(&mut self) {
        self.num_received = 0;
        self.num_buffered = 0;
        self.num_scheduled = 0;
        self.num_unschedulable = 0;
        self.num_schedule_filtered_out = 0;
        self.num_finished = 0;
        self.num_retryable = 0;
        self.num_dropped_on_receive = 0;
        self.num_dropped_on_sanitization = 0;
        self.num_dropped_on_validate_locks = 0;
        self.num_dropped_on_receive_transaction_checks = 0;
        self.num_dropped_on_clear = 0;
        self.num_dropped_on_age_and_status = 0;
        self.num_dropped_on_capacity = 0;
        self.min_prioritization_fees = u64::MAX;
        self.max_prioritization_fees = 0;
    }

    pub fn update_priority_stats(&mut self, min_max_fees: MinMaxResult<u64>) {
        // update min/max priority
        match min_max_fees {
            itertools::MinMaxResult::NoElements => {
                // do nothing
            }
            itertools::MinMaxResult::OneElement(e) => {
                self.min_prioritization_fees = e;
                self.max_prioritization_fees = e;
            }
            itertools::MinMaxResult::MinMax(min, max) => {
                self.min_prioritization_fees = min;
                self.max_prioritization_fees = max;
            }
        }
    }

    pub fn get_min_priority(&self) -> u64 {
        // to avoid getting u64::max recorded by metrics / in case of edge cases
        if self.min_prioritization_fees != u64::MAX {
            self.min_prioritization_fees
        } else {
            0
        }
    }

    pub fn get_max_priority(&self) -> u64 {
        self.max_prioritization_fees
    }
}

#[derive(Default)]
pub struct SchedulerTimingMetrics {
    interval: AtomicInterval,
    metrics: SchedulerTimingMetricsInner,
}

#[derive(Default)]
pub struct SchedulerTimingMetricsInner {
    /// Time spent making processing decisions.
    pub decision_time_us: u64,
    /// Time spent receiving packets.
    pub receive_time_us: u64,
    /// Time spent buffering packets.
    pub buffer_time_us: u64,
    /// Time spent filtering transactions during scheduling.
    pub schedule_filter_time_us: u64,
    /// Time spent scheduling transactions.
    pub schedule_time_us: u64,
    /// Time spent clearing transactions from the container.
    pub clear_time_us: u64,
    /// Time spent cleaning expired or processed transactions from the container.
    pub clean_time_us: u64,
    /// Time spent receiving completed transactions.
    pub receive_completed_time_us: u64,
}

impl Deref for SchedulerTimingMetrics {
    type Target = SchedulerTimingMetricsInner;
    fn deref(&self) -> &Self::Target {
        &self.metrics
    }
}

impl DerefMut for SchedulerTimingMetrics {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.metrics
    }
}

impl SchedulerTimingMetrics {
    pub fn maybe_report_and_reset(&mut self, should_report: bool) {
        const REPORT_INTERVAL_MS: u64 = 1000;
        if self.interval.should_update(REPORT_INTERVAL_MS) {
            if should_report {
                self.report("banking_stage_scheduler_timing");
            }
            self.reset();
        }
    }
}

impl SchedulerTimingMetricsInner {
    fn report(&self, name: &'static str) {
        datapoint_info!(
            name,
            ("decision_time_us", self.decision_time_us, i64),
            ("receive_time_us", self.receive_time_us, i64),
            ("buffer_time_us", self.buffer_time_us, i64),
            ("schedule_filter_time_us", self.schedule_filter_time_us, i64),
            ("schedule_time_us", self.schedule_time_us, i64),
            ("clear_time_us", self.clear_time_us, i64),
            ("clean_time_us", self.clean_time_us, i64),
            (
                "receive_completed_time_us",
                self.receive_completed_time_us,
                i64
            )
        );
    }

    fn reset(&mut self) {
        self.decision_time_us = 0;
        self.receive_time_us = 0;
        self.buffer_time_us = 0;
        self.schedule_filter_time_us = 0;
        self.schedule_time_us = 0;
        self.clear_time_us = 0;
        self.clean_time_us = 0;
        self.receive_completed_time_us = 0;
    }
}

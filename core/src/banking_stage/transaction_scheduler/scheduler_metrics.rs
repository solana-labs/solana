use {
    itertools::MinMaxResult,
    solana_sdk::{clock::Slot, timing::AtomicInterval},
};

#[derive(Default)]
pub struct SchedulerCountMetrics {
    interval: IntervalSchedulerCountMetrics,
    slot: SlotSchedulerCountMetrics,
}

impl SchedulerCountMetrics {
    pub fn update(&mut self, update: impl Fn(&mut SchedulerCountMetricsInner)) {
        update(&mut self.interval.metrics);
        update(&mut self.slot.metrics);
    }

    pub fn maybe_report_and_reset_slot(&mut self, slot: Option<Slot>) {
        self.slot.maybe_report_and_reset(slot);
    }

    pub fn maybe_report_and_reset_interval(&mut self, should_report: bool) {
        self.interval.maybe_report_and_reset(should_report);
    }

    pub fn interval_has_data(&self) -> bool {
        self.interval.metrics.has_data()
    }
}

#[derive(Default)]
struct IntervalSchedulerCountMetrics {
    interval: AtomicInterval,
    metrics: SchedulerCountMetricsInner,
}

#[derive(Default)]
struct SlotSchedulerCountMetrics {
    slot: Option<Slot>,
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

impl IntervalSchedulerCountMetrics {
    fn maybe_report_and_reset(&mut self, should_report: bool) {
        const REPORT_INTERVAL_MS: u64 = 1000;
        if self.interval.should_update(REPORT_INTERVAL_MS) {
            if should_report {
                self.metrics.report("banking_stage_scheduler_counts", None);
            }
            self.metrics.reset();
        }
    }
}

impl SlotSchedulerCountMetrics {
    fn maybe_report_and_reset(&mut self, slot: Option<Slot>) {
        if self.slot != slot {
            // Only report if there was an assigned slot.
            if self.slot.is_some() {
                self.metrics
                    .report("banking_stage_scheduler_slot_counts", self.slot);
            }
            self.metrics.reset();
            self.slot = slot;
        }
    }
}

impl SchedulerCountMetricsInner {
    fn report(&self, name: &'static str, slot: Option<Slot>) {
        let mut datapoint = create_datapoint!(
            @point name,
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
        if let Some(slot) = slot {
            datapoint.add_field_i64("slot", slot as i64);
        }
        solana_metrics::submit(datapoint, log::Level::Info);
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
    interval: IntervalSchedulerTimingMetrics,
    slot: SlotSchedulerTimingMetrics,
}

impl SchedulerTimingMetrics {
    pub fn update(&mut self, update: impl Fn(&mut SchedulerTimingMetricsInner)) {
        update(&mut self.interval.metrics);
        update(&mut self.slot.metrics);
    }

    pub fn maybe_report_and_reset_slot(&mut self, slot: Option<Slot>) {
        self.slot.maybe_report_and_reset(slot);
    }

    pub fn maybe_report_and_reset_interval(&mut self, should_report: bool) {
        self.interval.maybe_report_and_reset(should_report);
    }
}

#[derive(Default)]
struct IntervalSchedulerTimingMetrics {
    interval: AtomicInterval,
    metrics: SchedulerTimingMetricsInner,
}

#[derive(Default)]
struct SlotSchedulerTimingMetrics {
    slot: Option<Slot>,
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

impl IntervalSchedulerTimingMetrics {
    fn maybe_report_and_reset(&mut self, should_report: bool) {
        const REPORT_INTERVAL_MS: u64 = 1000;
        if self.interval.should_update(REPORT_INTERVAL_MS) {
            if should_report {
                self.metrics.report("banking_stage_scheduler_timing", None);
            }
            self.metrics.reset();
        }
    }
}

impl SlotSchedulerTimingMetrics {
    fn maybe_report_and_reset(&mut self, slot: Option<Slot>) {
        if self.slot != slot {
            // Only report if there was an assigned slot.
            if self.slot.is_some() {
                self.metrics
                    .report("banking_stage_scheduler_slot_counts", self.slot);
            }
            self.metrics.reset();
            self.slot = slot;
        }
    }
}

impl SchedulerTimingMetricsInner {
    fn report(&self, name: &'static str, slot: Option<Slot>) {
        let mut datapoint = create_datapoint!(
            @point name,
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
        if let Some(slot) = slot {
            datapoint.add_field_i64("slot", slot as i64);
        }
        solana_metrics::submit(datapoint, log::Level::Info);
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

use {
    solana_program_runtime::timings::ProgramTiming,
    solana_sdk::pubkey::Pubkey,
    std::time::{Duration, Instant},
};

/// To prevent too many writes to influx too often, MetricsMeter meters
/// up to `max_number_of_writes` of writes within `time_window_duration`.
/// New time window starts from the first write attempt outside of previous
/// time window. See tests for examples.
#[derive(Debug)]
struct MetricsMeter {
    max_number_of_writes: usize,
    time_window_duration: Duration,
    accumulated_number_of_writes: usize,
    time_window_start: Instant,
}

impl MetricsMeter {
    pub fn new(max_number_of_writes: usize, time_window_duration: Duration) -> Self {
        Self {
            max_number_of_writes,
            time_window_duration,
            accumulated_number_of_writes: 0,
            time_window_start: Instant::now(),
        }
    }

    pub fn take(&mut self) -> bool {
        if self.time_window_start.elapsed() > self.time_window_duration {
            // To start a new window, reset counters
            self.accumulated_number_of_writes = 0;
            self.time_window_start = Instant::now();
        }
        self.accumulated_number_of_writes = self.accumulated_number_of_writes.saturating_add(1);
        self.accumulated_number_of_writes <= self.max_number_of_writes
    }
}

#[derive(Debug)]
pub(crate) struct CostCalculationMetrics {
    metrics_meter: MetricsMeter,
}

impl CostCalculationMetrics {
    pub fn new(max_number_of_writes: usize, time_window_duration: Duration) -> Self {
        Self {
            metrics_meter: MetricsMeter::new(max_number_of_writes, time_window_duration),
        }
    }

    pub fn report(
        &mut self,
        program_id: &Pubkey,
        program_timing: &ProgramTiming,
        calculated_units: u64,
    ) {
        if self.metrics_meter.take() {
            datapoint_info!(
                "large_change_in_cost_calculation",
                ("pubkey", program_id.to_string(), String),
                ("execute_us", program_timing.accumulated_us, i64),
                ("accumulated_units", program_timing.accumulated_units, i64),
                ("count", program_timing.count, i64),
                ("errored_units", program_timing.total_errored_units, i64),
                (
                    "errored_count",
                    program_timing.errored_txs_compute_consumed.len(),
                    i64
                ),
                ("calculated_units", calculated_units as i64, i64),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use {super::*, std::thread};

    #[test]
    fn test_metrics_meter() {
        let limit = 2;
        let duration = Duration::from_millis(250);

        // start first window, can take up to `limit` writes
        let mut metrics_meter = MetricsMeter::new(limit, duration);
        assert!(metrics_meter.take());
        assert!(metrics_meter.take());
        assert!(!metrics_meter.take());

        // start second window
        thread::sleep(duration);
        // can write again in second time_window
        assert!(metrics_meter.take());

        // start another new window, can take up to `limit` writes
        thread::sleep(duration);
        assert!(metrics_meter.take());
        assert!(metrics_meter.take());
        assert!(!metrics_meter.take());
    }
}

//! Stats for Accounts Background Services

use {
    solana_metrics::datapoint_info,
    solana_sdk::slot_history::Slot,
    std::time::{Duration, Instant},
};

const SUBMIT_INTERVAL: Duration = Duration::from_secs(60);

/// Manage the Accounts Background Service stats
///
/// Used to record the stats and submit the datapoints.
#[derive(Debug)]
pub(super) struct StatsManager {
    stats: Stats,
    previous_submit: Instant,
}

impl StatsManager {
    /// Make a new StatsManager
    #[must_use]
    pub(super) fn new() -> Self {
        Self {
            stats: Stats::default(),
            previous_submit: Instant::now(),
        }
    }

    /// Record stats from this iteration, and maybe submit the datapoints based on how long it has
    /// been since the previous submission.
    pub(super) fn record_and_maybe_submit(&mut self, runtime: Duration, slot: Slot) {
        self.stats.record(runtime);
        self.maybe_submit(slot);
    }

    /// Maybe submit the datapoints based on how long it has been since the previous submission.
    fn maybe_submit(&mut self, slot: Slot) {
        let duration_since_previous_submit = Instant::now() - self.previous_submit;
        if duration_since_previous_submit < SUBMIT_INTERVAL {
            return;
        }

        datapoint_info!(
            "accounts_background_service",
            (
                "duration_since_previous_submit_us",
                duration_since_previous_submit.as_micros(),
                i64
            ),
            ("num_iterations", self.stats.num_iterations, i64),
            (
                "cumulative_runtime_us",
                self.stats.cumulative_runtime.as_micros(),
                i64
            ),
            (
                "mean_runtime_us",
                self.stats.mean_runtime().as_micros(),
                i64
            ),
            ("min_runtime_us", self.stats.min_runtime.as_micros(), i64),
            ("max_runtime_us", self.stats.max_runtime.as_micros(), i64),
            ("slot", slot, i64),
        );

        // reset the stats back to default
        *self = Self::new();
    }
}

/// Stats for Accounts Background Services
///
/// Intended to record stats for each iteration of the ABS main loop.
#[derive(Debug)]
struct Stats {
    /// Number of iterations recorded
    num_iterations: usize,
    /// Total runtime of all iterations
    cumulative_runtime: Duration,
    /// Minimum runtime seen for one iteration
    min_runtime: Duration,
    /// Maximum runtime seen for one iteration
    max_runtime: Duration,
}

impl Stats {
    /// Record stats from this iteration
    fn record(&mut self, runtime: Duration) {
        self.num_iterations += 1;
        self.cumulative_runtime += runtime;
        self.min_runtime = self.min_runtime.min(runtime);
        self.max_runtime = self.max_runtime.max(runtime);
    }

    /// Calculate the mean runtime of all iterations
    ///
    /// Requires that the number of iterations recorded is in the range [0, u32::MAX].
    fn mean_runtime(&self) -> Duration {
        debug_assert!(self.num_iterations > 0);
        debug_assert!(self.num_iterations <= u32::MAX as usize);
        self.cumulative_runtime / self.num_iterations as u32
    }
}

impl Default for Stats {
    #[must_use]
    fn default() -> Self {
        Self {
            num_iterations: 0,
            cumulative_runtime: Duration::ZERO,
            min_runtime: Duration::MAX,
            max_runtime: Duration::ZERO,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stats_record() {
        let mut stats = Stats::default();

        // record first stat, will be both min and max
        let runtime1 = Duration::from_secs(44);
        stats.record(runtime1);
        assert_eq!(stats.num_iterations, 1);
        assert_eq!(stats.cumulative_runtime, runtime1);
        assert_eq!(stats.min_runtime, runtime1);
        assert_eq!(stats.max_runtime, runtime1);

        // record a new max
        let runtime2 = Duration::from_secs(99);
        stats.record(runtime2);
        assert_eq!(stats.num_iterations, 2);
        assert_eq!(stats.cumulative_runtime, runtime1 + runtime2);
        assert_eq!(stats.min_runtime, runtime1);
        assert_eq!(stats.max_runtime, runtime2);

        // record a new min
        let runtime3 = Duration::from_secs(11);
        stats.record(runtime3);
        assert_eq!(stats.num_iterations, 3);
        assert_eq!(stats.cumulative_runtime, runtime1 + runtime2 + runtime3);
        assert_eq!(stats.min_runtime, runtime3);
        assert_eq!(stats.max_runtime, runtime2);
    }

    #[test]
    fn test_stats_mean_runtime() {
        let mut stats = Stats::default();
        stats.record(Duration::from_secs(1));
        stats.record(Duration::from_secs(3));
        stats.record(Duration::from_secs(5));
        stats.record(Duration::from_secs(7));
        assert_eq!(stats.mean_runtime().as_secs(), (1 + 3 + 5 + 7) / 4);
    }

    #[test]
    #[should_panic]
    fn test_stats_mean_runtime_panic_zero_iterations() {
        let stats = Stats::default();
        let _ = stats.mean_runtime();
    }

    #[test]
    #[should_panic]
    fn test_stats_mean_runtime_panic_too_many_iterations() {
        let num_iterations = u32::MAX as usize + 1;
        let stats = Stats {
            num_iterations,
            ..Stats::default()
        };
        let _ = stats.mean_runtime();
    }
}

//! Reporting service that reports metrics on a regular interval or when a bank is completed.

use {
    solana_poh::poh_recorder::Slot,
    solana_runtime::leader_bank_status::LeaderBankStatus,
    std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        thread::JoinHandle,
        time::{Duration, Instant},
    },
};

/// Report metrics on a regular interval
pub trait IntervalMetricsReporter: Send + Sync {
    fn report(&self);
}

/// Report metrics at the end of each **leader** slot
pub trait SlotMetricsReporter: Send + Sync {
    fn report(&self, slot: Slot);
}

pub struct MetricsReporter {
    /// The interval at which interval metrics are reported
    report_interval: Duration,
    /// Per-Slot reported metrics
    slot_metrics_reporters: Vec<Box<dyn SlotMetricsReporter>>,
    /// Interval reported metrics
    interval_metrics_reporters: Vec<Box<dyn IntervalMetricsReporter>>,
}

impl MetricsReporter {
    pub fn new(interval: Duration) -> Self {
        Self {
            report_interval: interval,
            slot_metrics_reporters: vec![],
            interval_metrics_reporters: vec![],
        }
    }

    pub fn add_slot_metrics_reporter(&mut self, reporter: impl SlotMetricsReporter + 'static) {
        self.slot_metrics_reporters.push(Box::new(reporter));
    }

    pub fn add_interval_metrics_reporter(
        &mut self,
        reporter: impl IntervalMetricsReporter + 'static,
    ) {
        self.interval_metrics_reporters.push(Box::new(reporter));
    }

    pub fn run(
        self,
        leader_bank_status: Arc<LeaderBankStatus>,
        exit: Arc<AtomicBool>,
    ) -> JoinHandle<()> {
        let mut last_interval_report = Instant::now();
        std::thread::Builder::new()
            .name("solMetReporter".to_string())
            .spawn(move || {
                while !exit.load(Ordering::Relaxed) {
                    let timeout = self.report_interval - last_interval_report.elapsed();
                    if let Some(slot) = leader_bank_status.wait_for_next_completed_timeout(timeout)
                    {
                        for reporter in &self.slot_metrics_reporters {
                            reporter.report(slot);
                        }
                    } else {
                        for reporter in &self.interval_metrics_reporters {
                            reporter.report();
                        }
                        last_interval_report = Instant::now();
                    }
                }
            })
            .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use {super::*, std::sync::atomic::AtomicU64};

    #[derive(Default)]
    struct TestSlotMetricsReporter {
        field: AtomicU64,
    }
    impl SlotMetricsReporter for Arc<TestSlotMetricsReporter> {
        fn report(&self, slot: Slot) {
            info!("reporting slot: {slot}");
        }
    }

    #[derive(Default)]
    struct TestIntervalMetricsReporter {
        field: AtomicU64,
    }
    impl IntervalMetricsReporter for Arc<TestIntervalMetricsReporter> {
        fn report(&self) {
            info!("reporting interval");
        }
    }

    #[test]
    fn test_metrics_reporter_add() {
        let mut reporter = MetricsReporter::new(Duration::from_millis(100));
        let slot_metrics_reporter = Arc::new(TestSlotMetricsReporter::default());
        let interval_metrics_reporter = Arc::new(TestIntervalMetricsReporter::default());
        reporter.add_slot_metrics_reporter(slot_metrics_reporter.clone());
        reporter.add_interval_metrics_reporter(interval_metrics_reporter.clone());

        slot_metrics_reporter.field.fetch_add(1, Ordering::Relaxed);
        interval_metrics_reporter
            .field
            .fetch_add(1, Ordering::Relaxed);
    }
}

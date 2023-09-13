use {
    hdrhistogram::Histogram,
    std::time::{Duration, Instant},
};

type Sample = u64;

/// The data required to build a histogram of account load times and submit those metrics
#[derive(Debug)]
pub struct LoadAccountsHistogram {
    /// Samples, in nanoseconds.
    samples_ns: Vec<Sample>,
    /// The time of the previous submission.
    /// Used to decide when to do the next submission.
    previous_submit: Instant,
}

impl LoadAccountsHistogram {
    const SUBMIT_INTERVAL: Duration = Duration::from_secs(60);

    #[must_use]
    pub fn new() -> Self {
        Self {
            samples_ns: Vec::new(),
            previous_submit: Instant::now(),
        }
    }

    /// Records `sample`
    ///
    /// Will be included in the next submission
    pub fn record(&mut self, sample: Duration) {
        self.samples_ns.push(sample.as_nanos().try_into().unwrap());
    }

    /// Submits datapoint if enough time has passed since previous submission
    pub fn maybe_submit(&mut self) {
        if self.previous_submit.elapsed() >= Self::SUBMIT_INTERVAL {
            self.submit();
        }
    }

    /// Submits datapoint
    pub fn submit(&mut self) {
        let duration = self.previous_submit.elapsed(); // do this before building the histogram
        let samples_ns = std::mem::take(&mut self.samples_ns); // use `take` because we want to clear `samples_ns`
        let histogram = Self::build_histogram(&samples_ns);

        datapoint_info!(
            "load_accounts_histogram",
            ("num_samples", histogram.len(), i64),
            ("duration_ns", duration.as_nanos(), i64),
            ("load_time_ns-mean", histogram.mean(), i64),
            (
                "load_time_ns_p10",
                histogram.value_at_quantile(0.10000),
                i64
            ),
            (
                "load_time_ns_p25",
                histogram.value_at_quantile(0.25000),
                i64
            ),
            (
                "load_time_ns_p50",
                histogram.value_at_quantile(0.50000),
                i64
            ),
            (
                "load_time_ns_p75",
                histogram.value_at_quantile(0.75000),
                i64
            ),
            (
                "load_time_ns_p90",
                histogram.value_at_quantile(0.90000),
                i64
            ),
            (
                "load_time_ns_p95",
                histogram.value_at_quantile(0.95000),
                i64
            ),
            (
                "load_time_ns_p99",
                histogram.value_at_quantile(0.99000),
                i64
            ),
            (
                "load_time_ns_p99.9",
                histogram.value_at_quantile(0.99900),
                i64
            ),
            (
                "load_time_ns_p99.99",
                histogram.value_at_quantile(0.99990),
                i64
            ),
            (
                "load_time_ns_p99.999",
                histogram.value_at_quantile(0.99999),
                i64
            ),
            (
                "load_time_ns_p100",
                histogram.value_at_quantile(1.00000),
                i64
            ),
        );

        self.previous_submit = Instant::now();
    }

    fn build_histogram(samples: &[Sample]) -> Histogram<Sample> {
        let mut histogram = Histogram::new(3).expect("histogram: create");
        for sample in samples {
            histogram.record(*sample).expect("histogram: record sample")
        }
        histogram
    }
}

impl Default for LoadAccountsHistogram {
    fn default() -> Self {
        Self::new()
    }
}

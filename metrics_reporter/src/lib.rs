pub use {
    solana_metrics::datapoint_info,
    solana_metrics_reporter_derive::{MetricsReporter, MetricsReporterInterval},
};

pub trait MetricsReporter {
    /// Reports metrics using datapoint_info!
    fn report(&self);
}

pub trait MetricsReporterInterval {
    /// Reports metrics using datapoint_info! and resets fields
    fn report(&mut self);
}

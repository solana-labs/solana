pub use {
    solana_metrics::datapoint_info,
    solana_metrics_reporter_derive::MetricsReporter,
};

pub trait MetricsReporter {
    /// Reports metrics using datapoint_info!
    fn report(&self);
}

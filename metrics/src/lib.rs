pub mod counter;
pub mod datapoint;
mod metrics;
pub use crate::metrics::{flush, query, set_host_id, set_panic_hook, submit};

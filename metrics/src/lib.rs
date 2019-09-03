#[macro_use]
pub mod counter;

mod metrics;

pub use crate::metrics::{flush, query, set_host_id, set_panic_hook, submit};
pub use influx_db_client as influxdb;

#[macro_use]
pub mod counter;

mod metrics;

pub use crate::metrics::flush;
pub use crate::metrics::query;
pub use crate::metrics::set_panic_hook;
pub use crate::metrics::submit;
pub use influx_db_client as influxdb;

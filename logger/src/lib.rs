//! The `logger` module configures `env_logger`

use lazy_static::lazy_static;
use std::sync::{Arc, RwLock};

lazy_static! {
    static ref LOGGER: Arc<RwLock<env_logger::Logger>> =
        Arc::new(RwLock::new(env_logger::Logger::from_default_env()));
}

struct LoggerShim {}

impl log::Log for LoggerShim {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        LOGGER.read().unwrap().enabled(metadata)
    }

    fn log(&self, record: &log::Record) {
        LOGGER.read().unwrap().log(record);
    }

    fn flush(&self) {}
}

fn replace_logger(logger: env_logger::Logger) {
    let max_level = logger.filter();
    log::set_max_level(max_level);
    let mut rw = LOGGER.write().unwrap();
    std::mem::replace(&mut *rw, logger);
    let _ = log::set_boxed_logger(Box::new(LoggerShim {}));
}

// Configures logging with a specific filter overriding RUST_LOG.  _RUST_LOG is used instead
// so if set it takes precedence.
// May be called at any time to re-configure the log filter
pub fn setup_with(filter: &str) {
    let logger =
        env_logger::Builder::from_env(env_logger::Env::new().filter_or("_RUST_LOG", filter))
            .format_timestamp_nanos()
            .build();
    replace_logger(logger);
}

// Configures logging with a default filter if RUST_LOG is not set
pub fn setup_with_default(filter: &str) {
    let logger = env_logger::Builder::from_env(env_logger::Env::new().default_filter_or(filter))
        .format_timestamp_nanos()
        .build();
    replace_logger(logger);
}

// Configures logging with the default filter "error" if RUST_LOG is not set
pub fn setup() {
    setup_with_default("error");
}

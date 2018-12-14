//! The `logger` module provides a setup function for `env_logger`. Its only function,
//! `setup()` may be called multiple times.

use env_logger;
use std::sync::{Once, ONCE_INIT};

static INIT: Once = ONCE_INIT;

/// Setup function that is only run once, even if called multiple times.
pub fn setup() {
    INIT.call_once(|| {
        env_logger::Builder::from_default_env()
            .default_format_timestamp_nanos(true)
            .init();
    });
}

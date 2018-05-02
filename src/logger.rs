use std::sync::{Once, ONCE_INIT};
extern crate env_logger;

static INIT: Once = ONCE_INIT;

/// Setup function that is only run once, even if called multiple times.
pub fn setup() {
    INIT.call_once(|| {
        env_logger::init().unwrap();
    });
}




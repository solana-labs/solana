#![cfg(feature = "spinner")]
//! Spinner creator.
//! This module is wrapped by the `spinner` feature, which is on by default.
//! It can be disabled and the dependency on `indicatif` avoided by running
//! with `default-features = false`

use {
    indicatif::{ProgressBar, ProgressStyle},
    std::time::Duration,
};

pub fn new_progress_bar() -> ProgressBar {
    let progress_bar = ProgressBar::new(42);
    progress_bar.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {wide_msg}")
            .expect("ProgresStyle::template direct input to be correct"),
    );
    progress_bar.enable_steady_tick(Duration::from_millis(100));
    progress_bar
}

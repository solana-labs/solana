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
            .expect("ProgressStyle::template direct input to be correct"),
    );
    progress_bar.enable_steady_tick(Duration::from_millis(100));
    progress_bar
}

#[derive(Debug, Default)]
pub struct SendTransactionProgress {
    pub confirmed_transactions: usize,
    pub total_transactions: usize,
    pub block_height: u64,
    pub last_valid_block_height: u64,
}

impl SendTransactionProgress {
    pub fn set_message_for_confirmed_transactions(&self, progress_bar: &ProgressBar, status: &str) {
        progress_bar.set_message(format!(
            "{:>5.1}% | {:<40} [block height {}; re-sign in {} blocks]",
            self.confirmed_transactions as f64 * 100. / self.total_transactions as f64,
            status,
            self.block_height,
            self.last_valid_block_height
                .saturating_sub(self.block_height),
        ));
    }
}

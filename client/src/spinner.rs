//! Spinner creator

use indicatif::{ProgressBar, ProgressStyle};

pub(crate) fn new_progress_bar() -> ProgressBar {
    let progress_bar = ProgressBar::new(42);
    progress_bar
        .set_style(ProgressStyle::default_spinner().template("{spinner:.green} {wide_msg}"));
    progress_bar.enable_steady_tick(100);
    progress_bar
}

pub(crate) fn set_message_for_confirmed_transactions(
    progress_bar: &ProgressBar,
    confirmed_transactions: u32,
    total_transactions: usize,
    block_height: Option<u64>,
    last_valid_block_height: u64,
    status: &str,
) {
    progress_bar.set_message(format!(
        "{:>5.1}% | {:<40}{}",
        confirmed_transactions as f64 * 100. / total_transactions as f64,
        status,
        match block_height {
            Some(block_height) => format!(
                " [block height {}; re-sign in {} blocks]",
                block_height,
                last_valid_block_height.saturating_sub(block_height),
            ),
            None => String::new(),
        },
    ));
}

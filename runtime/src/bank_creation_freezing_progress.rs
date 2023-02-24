//! Keep track of how many banks have been created and how many have been frozen or dropped.
//! This is useful to track foreground progress to understand expected access to accounts db.
use {
    solana_sdk::timing::AtomicInterval,
    std::sync::atomic::{AtomicU32, Ordering},
};

#[derive(Debug, Default)]
/// Keeps track of when all banks that were started as of a known point in time have been frozen or otherwise destroyed.
/// When 'bank_freeze_or_destruction_count' exceeds a prior value of 'bank_creation_count',
/// this means that we can know all banks that began loading accounts have completed as of the prior value of 'bank_creation_count'.
pub(crate) struct BankCreationFreezingProgress {
    /// Incremented each time a bank is created.
    /// Starting now, this bank could be finding accounts in the index and loading them from accounts db.
    pub(crate) bank_creation_count: AtomicU32,
    /// Incremented each time a bank is frozen or destroyed.
    /// At this point, this bank has completed all account loading.
    pub(crate) bank_freeze_or_destruction_count: AtomicU32,

    last_report: AtomicInterval,
}

impl BankCreationFreezingProgress {
    pub(crate) fn report(&self) {
        if self.last_report.should_update(60_000) {
            datapoint_info!(
                "bank_progress",
                (
                    "difference",
                    self.bank_creation_count
                        .load(Ordering::Acquire)
                        .wrapping_sub(
                            self.bank_freeze_or_destruction_count
                                .load(Ordering::Acquire)
                        ),
                    i64
                )
            );
        }
    }
}

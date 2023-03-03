//! Keep track of how many banks have been created and how many have been frozen or dropped.
//! This is useful to track foreground progress to understand expected access to accounts db.
use {
    crate::waitable_condvar::WaitableCondvar,
    solana_sdk::{slot_history::Slot, timing::AtomicInterval},
    std::{
        collections::HashSet,
        sync::{Arc, RwLock},
    },
};

#[derive(Debug, Default)]
/// Keeps track of when all banks that were started as of a known point in time have been frozen or otherwise destroyed.
/// When 'bank_freeze_or_destruction_count' exceeds a prior value of 'bank_creation_count',
/// this means that we can know all banks that began loading accounts have completed as of the prior value of 'bank_creation_count'.
pub(crate) struct BankCreationFreezingProgress {
    /// current set of banks that have been created but not frozen or dropped yet
    current_banks: RwLock<HashSet<Slot>>,

    /// enable waiting for bank_freeze_or_destruction_count to increment
    bank_frozen_or_destroyed: Arc<WaitableCondvar>,

    last_report: AtomicInterval,

    #[allow(dead_code)]
    /// if we are running tests and foreground account processing will not occur.
    /// Otherwise, shrink tests could wait forever.
    pub(crate) tests_with_no_foreground_account_processing: bool,
}

impl BankCreationFreezingProgress {
    pub(crate) fn new(tests_with_no_foreground_account_processing: bool) -> Self {
        Self {
            tests_with_no_foreground_account_processing,
            ..Self::default()
        }
    }

    pub(crate) fn increment_bank_frozen_or_destroyed(&self, slot: Slot) {
        assert!(
            self.current_banks.write().unwrap().remove(&slot),
            "slot: {}, slots: {:?}",
            slot,
            self.current_banks.read().unwrap()
        );
        self.bank_frozen_or_destroyed.notify_all();
    }

    pub(crate) fn increment_bank_creation_count(&self, slot: Slot) {
        assert!(self.current_banks.write().unwrap().insert(slot));
    }

    /// note which banks are currently active.
    /// sleep and block until all these banks have been dropped or frozen
    pub(crate) fn wait_until_banks_complete(&self) {
        let mut wait_on_slots = self
            .current_banks
            .read()
            .unwrap()
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        let timeout = std::time::Duration::from_millis(100);

        while !wait_on_slots.is_empty() {
            self.bank_frozen_or_destroyed.wait_timeout(timeout);
            let mut to_remove = 0;
            let lock = self.current_banks.read().unwrap();
            for slot in wait_on_slots.iter().rev() {
                if lock.contains(slot) {
                    // this slot is still active, so stop looking
                    break;
                }
                to_remove += 1;
            }
            drop(lock);
            wait_on_slots.truncate(wait_on_slots.len().saturating_sub(to_remove));
        }
    }

    pub(crate) fn report(&self) {
        if self.last_report.should_update(60_000) {
            datapoint_info!(
                "bank_progress",
                ("num_active", self.current_banks.read().unwrap().len(), i64)
            );
        }
    }
}

#[cfg(test)]
pub mod tests {
    use {
        super::*,
        solana_sdk::timing::timestamp,
        std::{
            sync::atomic::{AtomicU32, Ordering},
            thread::Builder,
        },
    };

    #[test]
    fn test_count() {
        solana_logger::setup();
        let progress = BankCreationFreezingProgress::default();
        assert!(progress.current_banks.read().unwrap().is_empty());
        let slot1 = 1;
        progress.increment_bank_creation_count(slot1);
        assert_eq!(progress.current_banks.read().unwrap().len(), 1);
        assert!(progress.current_banks.read().unwrap().contains(&slot1));
        progress.increment_bank_frozen_or_destroyed(slot1);
        assert!(progress.current_banks.read().unwrap().is_empty());
    }

    #[test]
    fn test_wait() {
        solana_logger::setup();
        let progress = BankCreationFreezingProgress::default();
        let waiter = progress.bank_frozen_or_destroyed.clone();
        let duration = std::time::Duration::default();
        assert!(waiter.wait_timeout(duration));
        let tester = Arc::new(AtomicU32::default());
        let tester2 = tester.clone();

        let secs = 10;

        let thread = Builder::new()
            .name("test_wait".to_string())
            .spawn(move || {
                assert!(!waiter.wait_timeout(std::time::Duration::from_secs(secs)));
                tester2.store(1, Ordering::Release);
            })
            .unwrap();
        let start = timestamp();
        let mut i = 0;
        while tester.load(Ordering::Acquire) == 0 {
            // keep incrementing until the waiter thread has picked up the notification that we incremented
            progress.increment_bank_creation_count(i);
            progress.increment_bank_frozen_or_destroyed(i);
            i += 1;
            let now = timestamp();
            let elapsed = now.wrapping_sub(start);
            assert!(elapsed < secs * 1_000, "elapsed: {elapsed}");
        }
        thread.join().expect("failed");
    }
}

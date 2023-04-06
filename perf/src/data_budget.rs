use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

#[derive(Default)]
pub struct DataBudget {
    // Amount of bytes we have in the budget to send.
    bytes: AtomicUsize,
    // Last time that we upped the bytes count, used
    // to detect when to up the bytes budget again
    asof: AtomicU64,
}

impl DataBudget {
    /// Create a data budget with max bytes, used for tests
    pub fn restricted() -> Self {
        Self {
            bytes: AtomicUsize::default(),
            asof: AtomicU64::new(u64::MAX),
        }
    }

    // If there are enough bytes in the budget, consumes from
    // the budget and returns true. Otherwise returns false.
    #[must_use]
    pub fn take(&self, size: usize) -> bool {
        let mut bytes = self.bytes.load(Ordering::Acquire);
        loop {
            bytes = match self.bytes.compare_exchange_weak(
                bytes,
                match bytes.checked_sub(size) {
                    None => return false,
                    Some(bytes) => bytes,
                },
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(bytes) => bytes,
            }
        }
    }

    // Updates timestamp and returns true, if at least given milliseconds
    // has passed since last update. Otherwise returns false.
    fn can_update(&self, duration_millis: u64) -> bool {
        let now = solana_sdk::timing::timestamp();
        let mut asof = self.asof.load(Ordering::Acquire);
        while asof.saturating_add(duration_millis) <= now {
            asof = match self.asof.compare_exchange_weak(
                asof,
                now,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(asof) => asof,
            }
        }
        false
    }

    /// Updates the budget if at least given milliseconds has passed since last
    /// update. Updater function maps current value of bytes to the new one.
    /// Returns current data-budget after the update.
    pub fn update<F>(&self, duration_millis: u64, updater: F) -> usize
    where
        F: Fn(usize) -> usize,
    {
        if self.can_update(duration_millis) {
            let mut bytes = self.bytes.load(Ordering::Acquire);
            loop {
                bytes = match self.bytes.compare_exchange_weak(
                    bytes,
                    updater(bytes),
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ) {
                    Ok(_) => break,
                    Err(bytes) => bytes,
                }
            }
        }
        self.bytes.load(Ordering::Acquire)
    }

    #[must_use]
    pub fn check(&self, size: usize) -> bool {
        size <= self.bytes.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use {super::*, std::time::Duration};

    #[test]
    fn test_data_budget() {
        let budget = DataBudget::default();
        assert!(!budget.take(1)); // budget = 0.

        assert_eq!(budget.update(1000, |bytes| bytes + 5), 5); // budget updates to 5.
        assert!(budget.take(1));
        assert!(budget.take(2));
        assert!(!budget.take(3)); // budget = 2, out of budget.

        assert_eq!(budget.update(30, |_| 10), 2); // no update, budget = 2.
        assert!(!budget.take(3)); // budget = 2, out of budget.

        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(budget.update(30, |bytes| bytes * 2), 4); // budget updates to 4.

        assert!(budget.take(3));
        assert!(budget.take(1));
        assert!(!budget.take(1)); // budget = 0.
    }
}

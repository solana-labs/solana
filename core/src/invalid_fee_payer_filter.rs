//! Manages a set of recent invalid fee payers.
//! This is used to filter out transactions that are unlikely to be included
//! in any block, much earlier in the transaction pipeline.
//!

use {
    dashmap::DashSet,
    solana_sdk::{pubkey::Pubkey, timing::AtomicInterval},
    std::sync::atomic::{AtomicUsize, Ordering},
};

pub struct InvalidFeePayerFilter {
    capacity: usize,
    recent_invalid_fee_payers: DashSet<Pubkey>,
    stats: InvalidFeePayerFilterStats,
    interval: AtomicInterval,
}

impl Default for InvalidFeePayerFilter {
    fn default() -> Self {
        // Upper-bound on the number of invalid fee payers to track, to avoid
        // unbounded memory growth.
        // Number chosen so it is large enough to track a large number of
        // invalid fee payers, but small enough to not use too much memory.
        // Typically, unique invalid fee paying is rare so this is probably
        // overkill.
        const INVALID_FEE_PAYER_CAPACITY: usize = 16 * 1024;

        Self {
            capacity: INVALID_FEE_PAYER_CAPACITY,
            recent_invalid_fee_payers: DashSet::new(),
            stats: InvalidFeePayerFilterStats::default(),
            interval: AtomicInterval::default(),
        }
    }
}

impl InvalidFeePayerFilter {
    /// Reset the filter if enough time has passed since last reset.
    /// If reset, also report stats.
    pub fn reset_on_interval(&self) {
        const RESET_INTERVAL_MS: u64 = 2000;
        if self.interval.should_update(RESET_INTERVAL_MS) {
            self.reset();
            self.stats.report();
        }
    }

    fn reset(&self) {
        self.recent_invalid_fee_payers.clear();
    }

    /// Add a pubkey to the filter.
    pub fn add(&self, pubkey: Pubkey) {
        self.stats.num_added.fetch_add(1, Ordering::Relaxed);
        self.recent_invalid_fee_payers.insert(pubkey);

        if self.recent_invalid_fee_payers.len() > self.capacity {
            if let Some(key) = self
                .recent_invalid_fee_payers
                .iter()
                .next()
                .map(|k| *k.key())
            {
                self.recent_invalid_fee_payers.remove(&key);
            }
        }
    }

    /// Check if a pubkey is in the filter.
    pub fn should_reject(&self, pubkey: &Pubkey) -> bool {
        let should_reject = self.recent_invalid_fee_payers.contains(pubkey);
        if should_reject {
            self.stats.num_rejects.fetch_add(1, Ordering::Relaxed);
        }
        should_reject
    }
}

#[derive(Default)]
struct InvalidFeePayerFilterStats {
    num_added: AtomicUsize,
    num_rejects: AtomicUsize,
}

impl InvalidFeePayerFilterStats {
    /// Report metrics since last report, if any invalid fee-payers have been detected.
    fn report(&self) {
        let num_added = self.num_added.swap(0, Ordering::Relaxed);
        if num_added > 0 {
            let num_rejects = self.num_rejects.swap(0, Ordering::Relaxed);
            datapoint_info!(
                "invalid_fee_payer_filter_stats",
                ("added", num_added, i64),
                ("rejects", num_rejects, i64),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use {super::*, solana_sdk::pubkey::Pubkey, std::sync::Arc};

    #[test]
    fn test_invalid_fee_payer_filter() {
        let invalid_fee_payer_filter = Arc::new(InvalidFeePayerFilter::default());
        let pubkey1 = Pubkey::new_unique();
        let pubkey2 = Pubkey::new_unique();
        assert!(!invalid_fee_payer_filter.should_reject(&pubkey1));
        assert!(!invalid_fee_payer_filter.should_reject(&pubkey2));
        invalid_fee_payer_filter.add(pubkey1);
        assert!(invalid_fee_payer_filter.should_reject(&pubkey1));
        assert!(!invalid_fee_payer_filter.should_reject(&pubkey2));

        invalid_fee_payer_filter.reset();
        assert!(!invalid_fee_payer_filter.should_reject(&pubkey1));
        assert!(!invalid_fee_payer_filter.should_reject(&pubkey2));
    }
}

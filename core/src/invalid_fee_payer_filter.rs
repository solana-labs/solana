//! Manages a set of recent invalid fee payers.
//! This is used to filter out transactions that are unlikely to be included
//! in any block, much earlier in the transaction pipeline.
//!

use {
    dashmap::DashSet,
    solana_sdk::{pubkey::Pubkey, timing::AtomicInterval},
    std::sync::atomic::{AtomicUsize, Ordering},
};

#[derive(Default)]
pub struct InvalidFeePayerFilter {
    recent_invalid_fee_payers: DashSet<Pubkey>,
    stats: InvalidFeePayerFilterStats,
}

impl InvalidFeePayerFilter {
    /// Reset the filter if enough time has passed since last reset.
    pub fn reset_on_interval(&self) {
        if self.stats.try_report() {
            self.recent_invalid_fee_payers.clear();
        }
    }

    /// Add a pubkey to the filter.
    pub fn add(&self, pubkey: Pubkey) {
        self.stats.num_added.fetch_add(1, Ordering::Relaxed);
        self.recent_invalid_fee_payers.insert(pubkey);
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
    interval: AtomicInterval,
    num_added: AtomicUsize,
    num_rejects: AtomicUsize,
}

impl InvalidFeePayerFilterStats {
    fn try_report(&self) -> bool {
        const REPORT_INTERVAL_MS: u64 = 2000;

        if self.interval.should_update(REPORT_INTERVAL_MS) {
            let num_added = self.num_added.swap(0, Ordering::Relaxed);
            if num_added > 0 {
                let num_rejects = self.num_rejects.swap(0, Ordering::Relaxed);
                datapoint_info!(
                    "invalid_fee_payer_filter_stats",
                    ("added", num_added, i64),
                    ("rejects", num_rejects, i64),
                );
            }
            true
        } else {
            false
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

        invalid_fee_payer_filter.reset_on_interval();
        assert!(!invalid_fee_payer_filter.should_reject(&pubkey1));
        assert!(!invalid_fee_payer_filter.should_reject(&pubkey2));
    }
}

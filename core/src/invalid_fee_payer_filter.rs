//! Manages a set of recent invalid fee payers.
//! This is used to filter out transactions that are unlikely to be included
//! in any block, much earlier in the transaction pipeline.
//!

use {dashmap::DashSet, solana_sdk::pubkey::Pubkey};

#[derive(Default)]
pub struct InvalidFeePayerFilter {
    recent_invalid_fee_payers: DashSet<Pubkey>,
}

impl InvalidFeePayerFilter {
    /// Reset the filter - this should be called periodically.
    pub fn reset(&self) {
        self.recent_invalid_fee_payers.clear();
    }

    /// Add a pubkey to the filter.
    pub fn add(&self, pubkey: Pubkey) {
        self.recent_invalid_fee_payers.insert(pubkey);
    }

    /// Check if a pubkey is in the filter.
    pub fn should_reject(&self, pubkey: &Pubkey) -> bool {
        self.recent_invalid_fee_payers.contains(pubkey)
    }
}

use std::sync::{Arc, RwLock};

use solana_runtime::{bank::Bank, bank_forks::BankForks, commitment::BlockCommitmentCache};

use crate::utils::Metric;

pub struct BanksWithCommitments {
    pub finalized_bank: Arc<Bank>,
    pub confirmed_bank: Arc<Bank>,
    pub processed_bank: Arc<Bank>,
}

impl BanksWithCommitments {
    pub fn new(
        bank_forks: &Arc<RwLock<BankForks>>,
        block_commitment_cache: &Arc<RwLock<BlockCommitmentCache>>,
    ) -> Self {
        let block_commitment_cache = block_commitment_cache.read().unwrap();
        let finalized_slot = block_commitment_cache
            .slot_with_commitment(solana_sdk::commitment_config::CommitmentLevel::Finalized);
        let confirmed_slot = block_commitment_cache
            .slot_with_commitment(solana_sdk::commitment_config::CommitmentLevel::Confirmed);
        let processed_slot = block_commitment_cache
            .slot_with_commitment(solana_sdk::commitment_config::CommitmentLevel::Processed);

        let r_bank_forks = bank_forks.read().unwrap();

        let default_closure = || {
            // From rpc/src/rpc.rs
            // We log a warning instead of returning an error, because all known error cases
            // are due to known bugs that should be fixed instead.
            //
            // The slot may not be found as a result of a known bug in snapshot creation, where
            // the bank at the given slot was not included in the snapshot.
            // Also, it may occur after an old bank has been purged from BankForks and a new
            // BlockCommitmentCache has not yet arrived. To make this case impossible,
            // BlockCommitmentCache should hold an `Arc<Bank>` everywhere it currently holds
            // a slot.
            //
            // For more information, see https://github.com/solana-labs/solana/issues/11078
            r_bank_forks.root_bank()
        };
        let finalized_bank = r_bank_forks
            .get(finalized_slot)
            .unwrap_or_else(default_closure);
        let confirmed_bank = r_bank_forks
            .get(confirmed_slot)
            .unwrap_or_else(default_closure);
        let processed_bank = r_bank_forks
            .get(processed_slot)
            .unwrap_or_else(default_closure);
        BanksWithCommitments {
            finalized_bank,
            confirmed_bank,
            processed_bank,
        }
    }

    /// Call function callback for each commitment level, and returns a vector of metrics.
    pub fn for_each_commitment<F: Fn(&Bank) -> Metric>(&self, get: F) -> Vec<Metric> {
        vec![
            get(&self.finalized_bank).with_label("commitment_level", "finalized".to_owned()),
            get(&self.confirmed_bank).with_label("commitment_level", "confirmed".to_owned()),
            get(&self.processed_bank).with_label("commitment_level", "processed".to_owned()),
        ]
    }
}

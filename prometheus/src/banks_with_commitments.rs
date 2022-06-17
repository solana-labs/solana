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
        let (finalized_slot, confirmed_slot, processed_slot) = {
            let block_commitment_cache = block_commitment_cache.read().unwrap();
            (
                block_commitment_cache.slot_with_commitment(
                    solana_sdk::commitment_config::CommitmentLevel::Finalized,
                ),
                block_commitment_cache.slot_with_commitment(
                    solana_sdk::commitment_config::CommitmentLevel::Confirmed,
                ),
                block_commitment_cache.slot_with_commitment(
                    solana_sdk::commitment_config::CommitmentLevel::Processed,
                ),
            )
        };
        let r_bank_forks = bank_forks.read().unwrap();

        let default_closure = || {
            // From rpc/src/rpc.rs
            //
            // It may occur after an old bank has been purged from BankForks and a new
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

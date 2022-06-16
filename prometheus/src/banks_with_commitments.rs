use std::sync::Arc;

use solana_runtime::bank::Bank;

use crate::utils::Metric;

pub struct BanksWithCommitments {
    pub finalized_bank: Arc<Bank>,
    pub confirmed_bank: Arc<Bank>,
    pub processed_bank: Arc<Bank>,
}

impl BanksWithCommitments {
    pub fn new(
        finalized_bank: Arc<Bank>,
        confirmed_bank: Arc<Bank>,
        processed_bank: Arc<Bank>,
    ) -> Self {
        BanksWithCommitments {
            finalized_bank,
            confirmed_bank,
            processed_bank,
        }
    }

    pub fn for_each_commitment<F: Fn(&Bank) -> Metric>(&self, get: F) -> Vec<Metric> {
        vec![
            get(&self.finalized_bank).with_label("commitment_level", "finalized".to_owned()),
            get(&self.confirmed_bank).with_label("commitment_level", "confirmed".to_owned()),
            get(&self.processed_bank).with_label("commitment_level", "processed".to_owned()),
        ]
    }
}

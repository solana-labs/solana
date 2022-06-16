use std::sync::Arc;

use solana_runtime::bank::Bank;

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
}

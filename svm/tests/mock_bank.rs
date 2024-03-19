use {
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        feature_set::FeatureSet,
        hash::Hash,
        pubkey::Pubkey,
        rent_collector::RentCollector,
    },
    solana_svm::transaction_processor::TransactionProcessingCallback,
    std::{collections::HashMap, sync::Arc},
};

#[derive(Default)]
pub struct MockBankCallback {
    rent_collector: RentCollector,
    feature_set: Arc<FeatureSet>,
    pub account_shared_data: HashMap<Pubkey, AccountSharedData>,
}

impl TransactionProcessingCallback for MockBankCallback {
    fn account_matches_owners(&self, account: &Pubkey, owners: &[Pubkey]) -> Option<usize> {
        if let Some(data) = self.account_shared_data.get(account) {
            if data.lamports() == 0 {
                None
            } else {
                owners.iter().position(|entry| data.owner() == entry)
            }
        } else {
            None
        }
    }

    fn get_account_shared_data(&self, pubkey: &Pubkey) -> Option<AccountSharedData> {
        self.account_shared_data.get(pubkey).cloned()
    }

    fn get_last_blockhash_and_lamports_per_signature(&self) -> (Hash, u64) {
        // Mock a hash and a value
        (Hash::new_unique(), 2)
    }

    fn get_rent_collector(&self) -> &RentCollector {
        &self.rent_collector
    }

    fn get_feature_set(&self) -> Arc<FeatureSet> {
        self.feature_set.clone()
    }
}

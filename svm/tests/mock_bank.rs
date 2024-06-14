use {
    solana_program_runtime::loaded_programs::{BlockRelation, ForkGraph},
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        clock::Epoch,
        feature_set::FeatureSet,
        native_loader,
        pubkey::Pubkey,
        slot_hashes::Slot,
    },
    solana_svm::transaction_processing_callback::TransactionProcessingCallback,
    std::{cell::RefCell, cmp::Ordering, collections::HashMap, sync::Arc},
};

pub struct MockForkGraph {}

impl ForkGraph for MockForkGraph {
    fn relationship(&self, a: Slot, b: Slot) -> BlockRelation {
        match a.cmp(&b) {
            Ordering::Less => BlockRelation::Ancestor,
            Ordering::Equal => BlockRelation::Equal,
            Ordering::Greater => BlockRelation::Descendant,
        }
    }

    fn slot_epoch(&self, _slot: Slot) -> Option<Epoch> {
        Some(0)
    }
}

#[derive(Default)]
pub struct MockBankCallback {
    pub feature_set: Arc<FeatureSet>,
    pub account_shared_data: RefCell<HashMap<Pubkey, AccountSharedData>>,
}

impl TransactionProcessingCallback for MockBankCallback {
    fn account_matches_owners(&self, account: &Pubkey, owners: &[Pubkey]) -> Option<usize> {
        if let Some(data) = self.account_shared_data.borrow().get(account) {
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
        self.account_shared_data.borrow().get(pubkey).cloned()
    }

    fn add_builtin_account(&self, name: &str, program_id: &Pubkey) {
        let account_data = native_loader::create_loadable_account_with_fields(name, (5000, 0));

        self.account_shared_data
            .borrow_mut()
            .insert(*program_id, account_data);
    }
}

impl MockBankCallback {
    #[allow(dead_code)]
    pub fn override_feature_set(&mut self, new_set: FeatureSet) {
        self.feature_set = Arc::new(new_set)
    }
}

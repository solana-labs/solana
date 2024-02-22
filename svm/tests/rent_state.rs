#![cfg(test)]

use {
    solana_program_runtime::{
        compute_budget::ComputeBudget, compute_budget_processor,
        loaded_programs::LoadedProgramsForTxBatch,
    },
    solana_sdk::{
        account::{AccountSharedData, WritableAccount},
        feature_set::FeatureSet,
        fee::FeeStructure,
        hash::Hash,
        native_loader,
        native_token::sol_to_lamports,
        pubkey::Pubkey,
        rent::Rent,
        rent_collector::RentCollector,
        signature::{Keypair, Signer},
        system_transaction,
        transaction::SanitizedTransaction,
        transaction_context::TransactionContext,
    },
    solana_svm::{
        account_loader::load_accounts, transaction_account_state_info::TransactionAccountStateInfo,
        transaction_error_metrics::TransactionErrorMetrics,
        transaction_processor::TransactionProcessingCallback,
    },
    std::{collections::HashMap, sync::Arc},
};

#[derive(Default)]
struct MockBankCallback {
    rent_collector: RentCollector,
    feature_set: Arc<FeatureSet>,
    account_shared_data: HashMap<Pubkey, AccountSharedData>,
}

impl TransactionProcessingCallback for MockBankCallback {
    fn account_matches_owners(&self, _account: &Pubkey, _owners: &[Pubkey]) -> Option<usize> {
        todo!()
    }

    fn get_account_shared_data(&self, pubkey: &Pubkey) -> Option<AccountSharedData> {
        self.account_shared_data.get(pubkey).cloned()
    }

    fn get_last_blockhash_and_lamports_per_signature(&self) -> (Hash, u64) {
        todo!()
    }

    fn get_rent_collector(&self) -> &RentCollector {
        &self.rent_collector
    }

    fn get_feature_set(&self) -> Arc<FeatureSet> {
        self.feature_set.clone()
    }
}

#[test]
fn test_rent_state_list_len() {
    let mint_keypair = Keypair::new();
    let mut bank = MockBankCallback::default();
    let recipient = Pubkey::new_unique();
    let last_block_hash = Hash::new_unique();

    let mut system_data = AccountSharedData::default();
    system_data.set_executable(true);
    system_data.set_owner(native_loader::id());
    bank.account_shared_data
        .insert(Pubkey::new_from_array([0u8; 32]), system_data);

    let mut mint_data = AccountSharedData::default();
    mint_data.set_lamports(2);
    bank.account_shared_data
        .insert(mint_keypair.pubkey(), mint_data);

    bank.account_shared_data
        .insert(recipient, AccountSharedData::default());

    let tx = system_transaction::transfer(
        &mint_keypair,
        &recipient,
        sol_to_lamports(1.),
        last_block_hash,
    );
    let num_accounts = tx.message().account_keys.len();
    let sanitized_tx = SanitizedTransaction::try_from_legacy_transaction(tx).unwrap();
    let mut error_counters = TransactionErrorMetrics::default();
    let loaded_txs = load_accounts(
        &bank,
        &[sanitized_tx.clone()],
        &[(Ok(()), None, Some(0))],
        &mut error_counters,
        &FeeStructure::default(),
        None,
        &HashMap::new(),
        &LoadedProgramsForTxBatch::default(),
    );

    let compute_budget = ComputeBudget::new(u64::from(
        compute_budget_processor::DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT,
    ));
    let transaction_context = TransactionContext::new(
        loaded_txs[0].0.as_ref().unwrap().accounts.clone(),
        Rent::default(),
        compute_budget.max_invoke_stack_height,
        compute_budget.max_instruction_trace_length,
    );

    assert_eq!(
        TransactionAccountStateInfo::new(
            &Rent::default(),
            &transaction_context,
            sanitized_tx.message()
        )
        .len(),
        num_accounts,
    );
}

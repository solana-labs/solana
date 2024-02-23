#![cfg(test)]

use {
    solana_program_runtime::loaded_programs::{BlockRelation, ForkGraph},
    solana_sdk::{
        account::AccountSharedData,
        clock::Slot,
        hash::Hash,
        instruction::CompiledInstruction,
        pubkey::Pubkey,
        signature::Keypair,
        transaction::{SanitizedTransaction, Transaction, TransactionError},
    },
    solana_svm::transaction_processor::TransactionBatchProcessor,
};

mod mock_bank;

struct MockForkGraph {}

impl ForkGraph for MockForkGraph {
    fn relationship(&self, _a: Slot, _b: Slot) -> BlockRelation {
        todo!()
    }
}

#[test]
fn test_filter_executable_program_accounts() {
    let keypair1 = Keypair::new();
    let keypair2 = Keypair::new();

    let non_program_pubkey1 = Pubkey::new_unique();
    let non_program_pubkey2 = Pubkey::new_unique();
    let program1_pubkey = Pubkey::new_unique();
    let program2_pubkey = Pubkey::new_unique();
    let account1_pubkey = Pubkey::new_unique();
    let account2_pubkey = Pubkey::new_unique();
    let account3_pubkey = Pubkey::new_unique();
    let account4_pubkey = Pubkey::new_unique();

    let account5_pubkey = Pubkey::new_unique();

    let mut bank = mock_bank::MockBankCallback::default();
    bank.account_shared_data.insert(
        non_program_pubkey1,
        AccountSharedData::new(1, 10, &account5_pubkey),
    );
    bank.account_shared_data.insert(
        non_program_pubkey2,
        AccountSharedData::new(1, 10, &account5_pubkey),
    );
    bank.account_shared_data.insert(
        program1_pubkey,
        AccountSharedData::new(40, 1, &account5_pubkey),
    );
    bank.account_shared_data.insert(
        program2_pubkey,
        AccountSharedData::new(40, 1, &account5_pubkey),
    );
    bank.account_shared_data.insert(
        account1_pubkey,
        AccountSharedData::new(1, 10, &non_program_pubkey1),
    );
    bank.account_shared_data.insert(
        account2_pubkey,
        AccountSharedData::new(1, 10, &non_program_pubkey2),
    );
    bank.account_shared_data.insert(
        account3_pubkey,
        AccountSharedData::new(40, 1, &program1_pubkey),
    );
    bank.account_shared_data.insert(
        account4_pubkey,
        AccountSharedData::new(40, 1, &program2_pubkey),
    );

    let tx1 = Transaction::new_with_compiled_instructions(
        &[&keypair1],
        &[non_program_pubkey1],
        Hash::new_unique(),
        vec![account1_pubkey, account2_pubkey, account3_pubkey],
        vec![CompiledInstruction::new(1, &(), vec![0])],
    );
    let sanitized_tx1 = SanitizedTransaction::from_transaction_for_tests(tx1);

    let tx2 = Transaction::new_with_compiled_instructions(
        &[&keypair2],
        &[non_program_pubkey2],
        Hash::new_unique(),
        vec![account4_pubkey, account3_pubkey, account2_pubkey],
        vec![CompiledInstruction::new(1, &(), vec![0])],
    );
    let sanitized_tx2 = SanitizedTransaction::from_transaction_for_tests(tx2);

    let owners = &[program1_pubkey, program2_pubkey];
    let programs = TransactionBatchProcessor::<MockForkGraph>::filter_executable_program_accounts(
        &bank,
        &[sanitized_tx1, sanitized_tx2],
        &mut [(Ok(()), None, Some(0)), (Ok(()), None, Some(0))],
        owners,
    );

    // The result should contain only account3_pubkey, and account4_pubkey as the program accounts
    assert_eq!(programs.len(), 2);
    assert_eq!(
        programs
            .get(&account3_pubkey)
            .expect("failed to find the program account"),
        &(&program1_pubkey, 2)
    );
    assert_eq!(
        programs
            .get(&account4_pubkey)
            .expect("failed to find the program account"),
        &(&program2_pubkey, 1)
    );
}

#[test]
fn test_filter_executable_program_accounts_invalid_blockhash() {
    let keypair1 = Keypair::new();
    let keypair2 = Keypair::new();

    let non_program_pubkey1 = Pubkey::new_unique();
    let non_program_pubkey2 = Pubkey::new_unique();
    let program1_pubkey = Pubkey::new_unique();
    let program2_pubkey = Pubkey::new_unique();
    let account1_pubkey = Pubkey::new_unique();
    let account2_pubkey = Pubkey::new_unique();
    let account3_pubkey = Pubkey::new_unique();
    let account4_pubkey = Pubkey::new_unique();

    let account5_pubkey = Pubkey::new_unique();

    let mut bank = mock_bank::MockBankCallback::default();
    bank.account_shared_data.insert(
        non_program_pubkey1,
        AccountSharedData::new(1, 10, &account5_pubkey),
    );
    bank.account_shared_data.insert(
        non_program_pubkey2,
        AccountSharedData::new(1, 10, &account5_pubkey),
    );
    bank.account_shared_data.insert(
        program1_pubkey,
        AccountSharedData::new(40, 1, &account5_pubkey),
    );
    bank.account_shared_data.insert(
        program2_pubkey,
        AccountSharedData::new(40, 1, &account5_pubkey),
    );
    bank.account_shared_data.insert(
        account1_pubkey,
        AccountSharedData::new(1, 10, &non_program_pubkey1),
    );
    bank.account_shared_data.insert(
        account2_pubkey,
        AccountSharedData::new(1, 10, &non_program_pubkey2),
    );
    bank.account_shared_data.insert(
        account3_pubkey,
        AccountSharedData::new(40, 1, &program1_pubkey),
    );
    bank.account_shared_data.insert(
        account4_pubkey,
        AccountSharedData::new(40, 1, &program2_pubkey),
    );

    let tx1 = Transaction::new_with_compiled_instructions(
        &[&keypair1],
        &[non_program_pubkey1],
        Hash::new_unique(),
        vec![account1_pubkey, account2_pubkey, account3_pubkey],
        vec![CompiledInstruction::new(1, &(), vec![0])],
    );
    let sanitized_tx1 = SanitizedTransaction::from_transaction_for_tests(tx1);

    let tx2 = Transaction::new_with_compiled_instructions(
        &[&keypair2],
        &[non_program_pubkey2],
        Hash::new_unique(),
        vec![account4_pubkey, account3_pubkey, account2_pubkey],
        vec![CompiledInstruction::new(1, &(), vec![0])],
    );
    // Let's not register blockhash from tx2. This should cause the tx2 to fail
    let sanitized_tx2 = SanitizedTransaction::from_transaction_for_tests(tx2);

    let owners = &[program1_pubkey, program2_pubkey];
    let mut lock_results = vec![(Ok(()), None, Some(0)), (Ok(()), None, None)];
    let programs = TransactionBatchProcessor::<MockForkGraph>::filter_executable_program_accounts(
        &bank,
        &[sanitized_tx1, sanitized_tx2],
        &mut lock_results,
        owners,
    );

    // The result should contain only account3_pubkey as the program accounts
    assert_eq!(programs.len(), 1);
    assert_eq!(
        programs
            .get(&account3_pubkey)
            .expect("failed to find the program account"),
        &(&program1_pubkey, 1)
    );
    assert_eq!(lock_results[1].0, Err(TransactionError::BlockhashNotFound));
}

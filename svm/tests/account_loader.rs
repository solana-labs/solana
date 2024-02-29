use {
    crate::mock_bank::MockBankCallback,
    solana_program_runtime::loaded_programs::LoadedProgramsForTxBatch,
    solana_sdk::{
        account::{AccountSharedData, WritableAccount},
        fee::FeeStructure,
        hash::Hash,
        instruction::CompiledInstruction,
        message::{LegacyMessage, Message, MessageHeader, SanitizedMessage},
        native_loader,
        nonce_info::{NonceFull, NoncePartial},
        pubkey::Pubkey,
        rent_collector::RENT_EXEMPT_RENT_EPOCH,
        rent_debits::RentDebits,
        signature::{Keypair, Signature, Signer},
        transaction::{SanitizedTransaction, TransactionError},
    },
    solana_svm::{
        account_loader::{load_accounts, LoadedTransaction, TransactionCheckResult},
        transaction_error_metrics::TransactionErrorMetrics,
    },
    std::collections::HashMap,
};

mod mock_bank;

#[test]
fn test_load_accounts_success() {
    let key1 = Keypair::new();
    let key2 = Keypair::new();
    let key3 = Keypair::new();
    let key4 = Keypair::new();

    let message = Message {
        account_keys: vec![key2.pubkey(), key1.pubkey(), key4.pubkey()],
        header: MessageHeader::default(),
        instructions: vec![
            CompiledInstruction {
                program_id_index: 1,
                accounts: vec![0],
                data: vec![],
            },
            CompiledInstruction {
                program_id_index: 1,
                accounts: vec![2],
                data: vec![],
            },
        ],
        recent_blockhash: Hash::default(),
    };

    let legacy = LegacyMessage::new(message);
    let sanitized_message = SanitizedMessage::Legacy(legacy);
    let mut mock_bank = MockBankCallback::default();
    let mut account_data = AccountSharedData::default();
    account_data.set_executable(true);
    account_data.set_owner(key3.pubkey());
    mock_bank
        .account_shared_data
        .insert(key1.pubkey(), account_data);

    let mut account_data = AccountSharedData::default();
    account_data.set_lamports(200);
    mock_bank
        .account_shared_data
        .insert(key2.pubkey(), account_data);

    let mut account_data = AccountSharedData::default();
    account_data.set_executable(true);
    account_data.set_owner(native_loader::id());
    mock_bank
        .account_shared_data
        .insert(key3.pubkey(), account_data);

    let mut error_counter = TransactionErrorMetrics::default();
    let loaded_programs = LoadedProgramsForTxBatch::default();

    let sanitized_transaction = SanitizedTransaction::new_for_tests(
        sanitized_message,
        vec![Signature::new_unique()],
        false,
    );
    let lock_results =
        (Ok(()), Some(NoncePartial::default()), Some(20u64)) as TransactionCheckResult;

    let results = load_accounts(
        &mock_bank,
        &[sanitized_transaction],
        &[lock_results],
        &mut error_counter,
        &FeeStructure::default(),
        None,
        &HashMap::new(),
        &loaded_programs,
    );

    let mut account_data = AccountSharedData::default();
    account_data.set_rent_epoch(RENT_EXEMPT_RENT_EPOCH);

    assert_eq!(results.len(), 1);
    let (loaded_result, nonce) = results[0].clone();
    assert_eq!(
        loaded_result.unwrap(),
        LoadedTransaction {
            accounts: vec![
                (
                    key2.pubkey(),
                    mock_bank.account_shared_data[&key2.pubkey()].clone()
                ),
                (
                    key1.pubkey(),
                    mock_bank.account_shared_data[&key1.pubkey()].clone()
                ),
                (key4.pubkey(), account_data),
                (
                    key3.pubkey(),
                    mock_bank.account_shared_data[&key3.pubkey()].clone()
                ),
            ],
            program_indices: vec![vec![3, 1], vec![3, 1]],
            rent: 0,
            rent_debits: RentDebits::default()
        }
    );

    assert_eq!(
        nonce.unwrap(),
        NonceFull::new(
            Pubkey::from([0; 32]),
            AccountSharedData::default(),
            Some(mock_bank.account_shared_data[&key2.pubkey()].clone())
        )
    );
}

#[test]
fn test_load_accounts_error() {
    let mock_bank = MockBankCallback::default();
    let message = Message {
        account_keys: vec![Pubkey::new_from_array([0; 32])],
        header: MessageHeader::default(),
        instructions: vec![CompiledInstruction {
            program_id_index: 0,
            accounts: vec![],
            data: vec![],
        }],
        recent_blockhash: Hash::default(),
    };

    let legacy = LegacyMessage::new(message);
    let sanitized_message = SanitizedMessage::Legacy(legacy);
    let sanitized_transaction = SanitizedTransaction::new_for_tests(
        sanitized_message,
        vec![Signature::new_unique()],
        false,
    );

    let lock_results = (Ok(()), Some(NoncePartial::default()), None) as TransactionCheckResult;
    let fee_structure = FeeStructure::default();

    let result = load_accounts(
        &mock_bank,
        &[sanitized_transaction.clone()],
        &[lock_results],
        &mut TransactionErrorMetrics::default(),
        &fee_structure,
        None,
        &HashMap::new(),
        &LoadedProgramsForTxBatch::default(),
    );

    assert_eq!(
        result,
        vec![(Err(TransactionError::BlockhashNotFound), None)]
    );

    let lock_results =
        (Ok(()), Some(NoncePartial::default()), Some(20u64)) as TransactionCheckResult;

    let result = load_accounts(
        &mock_bank,
        &[sanitized_transaction.clone()],
        &[lock_results.clone()],
        &mut TransactionErrorMetrics::default(),
        &fee_structure,
        None,
        &HashMap::new(),
        &LoadedProgramsForTxBatch::default(),
    );

    assert_eq!(result, vec![(Err(TransactionError::AccountNotFound), None)]);

    let lock_results = (
        Err(TransactionError::InvalidWritableAccount),
        Some(NoncePartial::default()),
        Some(20u64),
    ) as TransactionCheckResult;

    let result = load_accounts(
        &mock_bank,
        &[sanitized_transaction.clone()],
        &[lock_results],
        &mut TransactionErrorMetrics::default(),
        &fee_structure,
        None,
        &HashMap::new(),
        &LoadedProgramsForTxBatch::default(),
    );

    assert_eq!(
        result,
        vec![(Err(TransactionError::InvalidWritableAccount), None)]
    );
}

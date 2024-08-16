#![cfg(test)]

use {
    crate::{
        mock_bank::{
            create_executable_environment, deploy_program, register_builtins, MockBankCallback,
            MockForkGraph,
        },
        transaction_builder::SanitizedTransactionBuilder,
    },
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount, WritableAccount},
        clock::Clock,
        hash::Hash,
        instruction::AccountMeta,
        pubkey::Pubkey,
        signature::Signature,
        sysvar::SysvarId,
        transaction::{SanitizedTransaction, TransactionError},
    },
    solana_svm::{
        account_loader::{CheckedTransactionDetails, TransactionCheckResult},
        transaction_processing_callback::TransactionProcessingCallback,
        transaction_processing_result::TransactionProcessingResultExtensions,
        transaction_processor::{
            ExecutionRecordingConfig, TransactionBatchProcessor, TransactionProcessingConfig,
            TransactionProcessingEnvironment,
        },
    },
    solana_type_overrides::sync::{Arc, RwLock},
    std::collections::{HashMap, HashSet},
};

// This module contains the implementation of TransactionProcessingCallback
mod mock_bank;
mod transaction_builder;

const DEPLOYMENT_SLOT: u64 = 0;
const EXECUTION_SLOT: u64 = 5; // The execution slot must be greater than the deployment slot
const EXECUTION_EPOCH: u64 = 2; // The execution epoch must be greater than the deployment epoch

fn prepare_transactions(
    mock_bank: &MockBankCallback,
) -> (Vec<SanitizedTransaction>, Vec<TransactionCheckResult>) {
    let mut transaction_builder = SanitizedTransactionBuilder::default();
    let mut all_transactions = Vec::new();
    let mut transaction_checks = Vec::new();

    // A transaction that works without any account
    let hello_program = deploy_program("hello-solana".to_string(), DEPLOYMENT_SLOT, mock_bank);
    let fee_payer = Pubkey::new_unique();
    transaction_builder.create_instruction(hello_program, Vec::new(), HashMap::new(), Vec::new());

    let sanitized_transaction =
        transaction_builder.build(Hash::default(), (fee_payer, Signature::new_unique()), false);

    all_transactions.push(sanitized_transaction.unwrap());
    transaction_checks.push(Ok(CheckedTransactionDetails {
        nonce: None,
        lamports_per_signature: 20,
    }));

    // The transaction fee payer must have enough funds
    let mut account_data = AccountSharedData::default();
    account_data.set_lamports(80000);
    mock_bank
        .account_shared_data
        .write()
        .unwrap()
        .insert(fee_payer, account_data);

    // A simple funds transfer between accounts
    let transfer_program_account =
        deploy_program("simple-transfer".to_string(), DEPLOYMENT_SLOT, mock_bank);
    let sender = Pubkey::new_unique();
    let recipient = Pubkey::new_unique();
    let fee_payer = Pubkey::new_unique();
    let system_account = Pubkey::from([0u8; 32]);

    transaction_builder.create_instruction(
        transfer_program_account,
        vec![
            AccountMeta {
                pubkey: sender,
                is_signer: true,
                is_writable: true,
            },
            AccountMeta {
                pubkey: recipient,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: system_account,
                is_signer: false,
                is_writable: false,
            },
        ],
        HashMap::from([(sender, Signature::new_unique())]),
        vec![0, 0, 0, 0, 0, 0, 0, 10],
    );

    let sanitized_transaction =
        transaction_builder.build(Hash::default(), (fee_payer, Signature::new_unique()), true);
    all_transactions.push(sanitized_transaction.unwrap());
    transaction_checks.push(Ok(CheckedTransactionDetails {
        nonce: None,
        lamports_per_signature: 20,
    }));

    // Setting up the accounts for the transfer

    // fee payer
    let mut account_data = AccountSharedData::default();
    account_data.set_lamports(80000);
    mock_bank
        .account_shared_data
        .write()
        .unwrap()
        .insert(fee_payer, account_data);

    // sender
    let mut account_data = AccountSharedData::default();
    account_data.set_lamports(900000);
    mock_bank
        .account_shared_data
        .write()
        .unwrap()
        .insert(sender, account_data);

    // recipient
    let mut account_data = AccountSharedData::default();
    account_data.set_lamports(900000);
    mock_bank
        .account_shared_data
        .write()
        .unwrap()
        .insert(recipient, account_data);

    // The system account is set in `create_executable_environment`

    // A program that utilizes a Sysvar
    let program_account = deploy_program("clock-sysvar".to_string(), DEPLOYMENT_SLOT, mock_bank);
    let fee_payer = Pubkey::new_unique();
    transaction_builder.create_instruction(program_account, Vec::new(), HashMap::new(), Vec::new());

    let sanitized_transaction =
        transaction_builder.build(Hash::default(), (fee_payer, Signature::new_unique()), false);

    all_transactions.push(sanitized_transaction.unwrap());
    transaction_checks.push(Ok(CheckedTransactionDetails {
        nonce: None,
        lamports_per_signature: 20,
    }));

    let mut account_data = AccountSharedData::default();
    account_data.set_lamports(80000);
    mock_bank
        .account_shared_data
        .write()
        .unwrap()
        .insert(fee_payer, account_data);

    // A transaction that fails
    let sender = Pubkey::new_unique();
    let recipient = Pubkey::new_unique();
    let fee_payer = Pubkey::new_unique();
    let system_account = Pubkey::new_from_array([0; 32]);
    let data = 900050u64.to_be_bytes().to_vec();
    transaction_builder.create_instruction(
        transfer_program_account,
        vec![
            AccountMeta {
                pubkey: sender,
                is_signer: true,
                is_writable: true,
            },
            AccountMeta {
                pubkey: recipient,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: system_account,
                is_signer: false,
                is_writable: false,
            },
        ],
        HashMap::from([(sender, Signature::new_unique())]),
        data,
    );

    let sanitized_transaction =
        transaction_builder.build(Hash::default(), (fee_payer, Signature::new_unique()), true);
    all_transactions.push(sanitized_transaction.clone().unwrap());
    transaction_checks.push(Ok(CheckedTransactionDetails {
        nonce: None,
        lamports_per_signature: 20,
    }));

    // fee payer
    let mut account_data = AccountSharedData::default();
    account_data.set_lamports(80000);
    mock_bank
        .account_shared_data
        .write()
        .unwrap()
        .insert(fee_payer, account_data);

    // Sender without enough funds
    let mut account_data = AccountSharedData::default();
    account_data.set_lamports(900000);
    mock_bank
        .account_shared_data
        .write()
        .unwrap()
        .insert(sender, account_data);

    // recipient
    let mut account_data = AccountSharedData::default();
    account_data.set_lamports(900000);
    mock_bank
        .account_shared_data
        .write()
        .unwrap()
        .insert(recipient, account_data);

    // A transaction whose verification has already failed
    all_transactions.push(sanitized_transaction.unwrap());
    transaction_checks.push(Err(TransactionError::BlockhashNotFound));

    (all_transactions, transaction_checks)
}

#[test]
fn svm_integration() {
    let mock_bank = MockBankCallback::default();
    let (transactions, check_results) = prepare_transactions(&mock_bank);
    let batch_processor = TransactionBatchProcessor::<MockForkGraph>::new(
        EXECUTION_SLOT,
        EXECUTION_EPOCH,
        HashSet::new(),
    );

    let fork_graph = Arc::new(RwLock::new(MockForkGraph {}));

    create_executable_environment(
        fork_graph.clone(),
        &mock_bank,
        &mut batch_processor.program_cache.write().unwrap(),
    );

    // The sysvars must be put in the cache
    batch_processor.fill_missing_sysvar_cache_entries(&mock_bank);
    register_builtins(&mock_bank, &batch_processor);

    let processing_config = TransactionProcessingConfig {
        recording_config: ExecutionRecordingConfig {
            enable_log_recording: true,
            enable_return_data_recording: true,
            enable_cpi_recording: false,
        },
        ..Default::default()
    };

    let result = batch_processor.load_and_execute_sanitized_transactions(
        &mock_bank,
        &transactions,
        check_results,
        &TransactionProcessingEnvironment::default(),
        &processing_config,
    );

    assert_eq!(result.processing_results.len(), 5);

    let executed_tx_0 = result.processing_results[0]
        .processed_transaction()
        .unwrap();
    assert!(executed_tx_0.was_successful());
    let logs = executed_tx_0
        .execution_details
        .log_messages
        .as_ref()
        .unwrap();
    assert!(logs.contains(&"Program log: Hello, Solana!".to_string()));

    let executed_tx_1 = result.processing_results[1]
        .processed_transaction()
        .unwrap();
    assert!(executed_tx_1.was_successful());

    // The SVM does not commit the account changes in MockBank
    let recipient_key = transactions[1].message().account_keys()[2];
    let recipient_data = executed_tx_1
        .loaded_transaction
        .accounts
        .iter()
        .find(|key| key.0 == recipient_key)
        .unwrap();
    assert_eq!(recipient_data.1.lamports(), 900010);

    let executed_tx_2 = result.processing_results[2]
        .processed_transaction()
        .unwrap();
    let return_data = executed_tx_2
        .execution_details
        .return_data
        .as_ref()
        .unwrap();
    let time = i64::from_be_bytes(return_data.data[0..8].try_into().unwrap());
    let clock_data = mock_bank.get_account_shared_data(&Clock::id()).unwrap();
    let clock_info: Clock = bincode::deserialize(clock_data.data()).unwrap();
    assert_eq!(clock_info.unix_timestamp, time);

    let executed_tx_3 = result.processing_results[3]
        .processed_transaction()
        .unwrap();
    assert!(executed_tx_3.execution_details.status.is_err());
    assert!(executed_tx_3
        .execution_details
        .log_messages
        .as_ref()
        .unwrap()
        .contains(&"Transfer: insufficient lamports 900000, need 900050".to_string()));

    assert!(matches!(
        result.processing_results[4],
        Err(TransactionError::BlockhashNotFound)
    ));
}

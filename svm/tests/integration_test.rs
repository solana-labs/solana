#![cfg(test)]

use {
    crate::mock_bank::{
        create_executable_environment, deploy_program, program_address, register_builtins,
        MockBankCallback, MockForkGraph, WALLCLOCK_TIME,
    },
    solana_sdk::{
        account::{AccountSharedData, WritableAccount},
        clock::Slot,
        hash::Hash,
        instruction::{AccountMeta, Instruction},
        native_token::LAMPORTS_PER_SOL,
        pubkey::Pubkey,
        signature::Signer,
        signer::keypair::Keypair,
        system_instruction, system_program, system_transaction,
        transaction::{SanitizedTransaction, Transaction, TransactionError},
        transaction_context::TransactionReturnData,
    },
    solana_svm::{
        account_loader::{CheckedTransactionDetails, TransactionCheckResult},
        transaction_execution_result::TransactionExecutionDetails,
        transaction_processing_result::ProcessedTransaction,
        transaction_processor::{
            ExecutionRecordingConfig, TransactionBatchProcessor, TransactionProcessingConfig,
            TransactionProcessingEnvironment,
        },
    },
    solana_type_overrides::sync::{Arc, RwLock},
    std::collections::{HashMap, HashSet},
    test_case::test_case,
};

// This module contains the implementation of TransactionProcessingCallback
mod mock_bank;

const DEPLOYMENT_SLOT: u64 = 0;
const EXECUTION_SLOT: u64 = 5; // The execution slot must be greater than the deployment slot
const EXECUTION_EPOCH: u64 = 2; // The execution epoch must be greater than the deployment epoch
const LAMPORTS_PER_SIGNATURE: u64 = 5000;

pub type AccountMap = HashMap<Pubkey, AccountSharedData>;

// container for a transaction batch and all data needed to run and verify it against svm
#[derive(Debug, Default)]
pub struct SvmTestEntry {
    // programs to deploy to the new svm before transaction execution
    pub initial_programs: Vec<(String, Slot)>,

    // accounts to deploy to the new svm before transaction execution
    pub initial_accounts: AccountMap,

    // transactions to execute and transaction-specific checks to perform on the results from svm
    pub transaction_batch: Vec<TransactionBatchItem>,

    // expected final account states, checked after transaction execution
    pub final_accounts: AccountMap,
}

impl SvmTestEntry {
    // add a new a rent-exempt account that exists before the batch
    // inserts it into both account maps, assuming it lives unchanged (except for svm fixing rent epoch)
    // rent-paying accounts must be added by hand because svm will not set rent epoch to u64::MAX
    pub fn add_initial_account(&mut self, pubkey: Pubkey, account: &AccountSharedData) {
        assert!(self
            .initial_accounts
            .insert(pubkey, account.clone())
            .is_none());

        self.create_expected_account(pubkey, account);
    }

    // add a new rent-exempt account that is created by the transaction
    // inserts it only into the post account map
    pub fn create_expected_account(&mut self, pubkey: Pubkey, account: &AccountSharedData) {
        let mut account = account.clone();
        account.set_rent_epoch(u64::MAX);

        assert!(self.final_accounts.insert(pubkey, account).is_none());
    }

    // edit an existing account to reflect changes you expect the transaction to make to it
    pub fn update_expected_account_data(&mut self, pubkey: Pubkey, account: &AccountSharedData) {
        let mut account = account.clone();
        account.set_rent_epoch(u64::MAX);

        assert!(self.final_accounts.insert(pubkey, account).is_some());
    }

    // add lamports to an existing expected final account state
    pub fn increase_expected_lamports(&mut self, pubkey: &Pubkey, lamports: u64) {
        self.final_accounts
            .get_mut(pubkey)
            .unwrap()
            .checked_add_lamports(lamports)
            .unwrap();
    }

    // subtract lamports from an existing expected final account state
    pub fn decrease_expected_lamports(&mut self, pubkey: &Pubkey, lamports: u64) {
        self.final_accounts
            .get_mut(pubkey)
            .unwrap()
            .checked_sub_lamports(lamports)
            .unwrap();
    }

    // convenience function that adds a transaction that is expected to succeed
    pub fn push_transaction(&mut self, transaction: Transaction) {
        self.transaction_batch.push(TransactionBatchItem {
            transaction,
            ..TransactionBatchItem::default()
        });
    }

    // convenience function that adds a transaction that is expected to execute but fail
    pub fn push_failed_transaction(&mut self, transaction: Transaction) {
        self.transaction_batch.push(TransactionBatchItem {
            transaction,
            asserts: TransactionBatchItemAsserts::failed(),
            ..TransactionBatchItem::default()
        });
    }

    // internal helper to gather SanitizedTransaction objects for execution
    fn prepare_transactions(&self) -> (Vec<SanitizedTransaction>, Vec<TransactionCheckResult>) {
        self.transaction_batch
            .iter()
            .cloned()
            .map(|item| {
                (
                    SanitizedTransaction::from_transaction_for_tests(item.transaction),
                    item.check_result,
                )
            })
            .unzip()
    }

    // internal helper to gather test items for post-execution checks
    fn asserts(&self) -> Vec<TransactionBatchItemAsserts> {
        self.transaction_batch
            .iter()
            .cloned()
            .map(|item| item.asserts)
            .collect()
    }
}

// one transaction in a batch plus check results for svm and asserts for tests
#[derive(Clone, Debug)]
pub struct TransactionBatchItem {
    pub transaction: Transaction,
    pub check_result: TransactionCheckResult,
    pub asserts: TransactionBatchItemAsserts,
}

impl Default for TransactionBatchItem {
    fn default() -> Self {
        Self {
            transaction: Transaction::default(),
            check_result: Ok(CheckedTransactionDetails {
                nonce: None,
                lamports_per_signature: LAMPORTS_PER_SIGNATURE,
            }),
            asserts: TransactionBatchItemAsserts::succeeded(),
        }
    }
}

// asserts for a given transaction in a batch
// we can automatically check whether it executed, whether it succeeded
// log items we expect to see (exect match only), and rodata
#[derive(Clone, Debug)]
pub struct TransactionBatchItemAsserts {
    pub executed: bool,
    pub succeeded: bool,
    pub logs: Vec<String>,
    pub return_data: ReturnDataAssert,
}

impl TransactionBatchItemAsserts {
    pub fn succeeded() -> Self {
        Self {
            executed: true,
            succeeded: true,
            logs: vec![],
            return_data: ReturnDataAssert::Skip,
        }
    }

    pub fn failed() -> Self {
        Self {
            executed: true,
            succeeded: false,
            logs: vec![],
            return_data: ReturnDataAssert::Skip,
        }
    }

    pub fn not_executed() -> Self {
        Self {
            executed: false,
            succeeded: false,
            logs: vec![],
            return_data: ReturnDataAssert::Skip,
        }
    }

    pub fn check_executed_transaction(&self, execution_details: &TransactionExecutionDetails) {
        assert!(self.executed);
        assert_eq!(self.succeeded, execution_details.status.is_ok());

        if !self.logs.is_empty() {
            let actual_logs = execution_details.log_messages.as_ref().unwrap();
            for expected_log in &self.logs {
                assert!(actual_logs.contains(expected_log));
            }
        }

        if self.return_data != ReturnDataAssert::Skip {
            assert_eq!(
                self.return_data,
                execution_details.return_data.clone().into()
            );
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum ReturnDataAssert {
    Some(TransactionReturnData),
    None,
    #[default]
    Skip,
}

impl From<Option<TransactionReturnData>> for ReturnDataAssert {
    fn from(option_ro_data: Option<TransactionReturnData>) -> Self {
        match option_ro_data {
            Some(ro_data) => Self::Some(ro_data),
            None => Self::None,
        }
    }
}

fn program_medley() -> Vec<SvmTestEntry> {
    let mut test_entry = SvmTestEntry::default();

    // 0: A transaction that works without any account
    {
        let program_name = "hello-solana".to_string();
        let program_id = program_address(&program_name);
        test_entry
            .initial_programs
            .push((program_name, DEPLOYMENT_SLOT));

        let fee_payer_keypair = Keypair::new();
        let fee_payer = fee_payer_keypair.pubkey();

        let mut fee_payer_data = AccountSharedData::default();
        fee_payer_data.set_lamports(LAMPORTS_PER_SOL);
        test_entry.add_initial_account(fee_payer, &fee_payer_data);

        let instruction = Instruction::new_with_bytes(program_id, &[], vec![]);
        test_entry.push_transaction(Transaction::new_signed_with_payer(
            &[instruction],
            Some(&fee_payer),
            &[&fee_payer_keypair],
            Hash::default(),
        ));

        test_entry.transaction_batch[0]
            .asserts
            .logs
            .push("Program log: Hello, Solana!".to_string());

        test_entry.decrease_expected_lamports(&fee_payer, LAMPORTS_PER_SIGNATURE);
    }

    // 1: A simple funds transfer between accounts
    {
        let program_name = "simple-transfer".to_string();
        let program_id = program_address(&program_name);
        test_entry
            .initial_programs
            .push((program_name, DEPLOYMENT_SLOT));

        let fee_payer_keypair = Keypair::new();
        let sender_keypair = Keypair::new();

        let fee_payer = fee_payer_keypair.pubkey();
        let sender = sender_keypair.pubkey();
        let recipient = Pubkey::new_unique();

        let transfer_amount = 10;

        let mut fee_payer_data = AccountSharedData::default();
        fee_payer_data.set_lamports(LAMPORTS_PER_SOL);
        test_entry.add_initial_account(fee_payer, &fee_payer_data);

        let mut sender_data = AccountSharedData::default();
        sender_data.set_lamports(LAMPORTS_PER_SOL);
        test_entry.add_initial_account(sender, &sender_data);

        let mut recipient_data = AccountSharedData::default();
        recipient_data.set_lamports(LAMPORTS_PER_SOL);
        test_entry.add_initial_account(recipient, &recipient_data);

        let instruction = Instruction::new_with_bytes(
            program_id,
            &u64::to_be_bytes(transfer_amount),
            vec![
                AccountMeta::new(sender, true),
                AccountMeta::new(recipient, false),
                AccountMeta::new_readonly(system_program::id(), false),
            ],
        );

        test_entry.push_transaction(Transaction::new_signed_with_payer(
            &[instruction],
            Some(&fee_payer),
            &[&fee_payer_keypair, &sender_keypair],
            Hash::default(),
        ));

        test_entry.increase_expected_lamports(&recipient, transfer_amount);
        test_entry.decrease_expected_lamports(&sender, transfer_amount);
        test_entry.decrease_expected_lamports(&fee_payer, LAMPORTS_PER_SIGNATURE * 2);
    }

    // 2: A program that utilizes a Sysvar
    {
        let program_name = "clock-sysvar".to_string();
        let program_id = program_address(&program_name);
        test_entry
            .initial_programs
            .push((program_name, DEPLOYMENT_SLOT));

        let fee_payer_keypair = Keypair::new();
        let fee_payer = fee_payer_keypair.pubkey();

        let mut fee_payer_data = AccountSharedData::default();
        fee_payer_data.set_lamports(LAMPORTS_PER_SOL);
        test_entry.add_initial_account(fee_payer, &fee_payer_data);

        let instruction = Instruction::new_with_bytes(program_id, &[], vec![]);
        test_entry.push_transaction(Transaction::new_signed_with_payer(
            &[instruction],
            Some(&fee_payer),
            &[&fee_payer_keypair],
            Hash::default(),
        ));

        let ro_data = TransactionReturnData {
            program_id,
            data: i64::to_be_bytes(WALLCLOCK_TIME).to_vec(),
        };
        test_entry.transaction_batch[2].asserts.return_data = ReturnDataAssert::Some(ro_data);

        test_entry.decrease_expected_lamports(&fee_payer, LAMPORTS_PER_SIGNATURE);
    }

    // 3: A transaction that fails
    {
        let program_id = program_address("simple-transfer");

        let fee_payer_keypair = Keypair::new();
        let sender_keypair = Keypair::new();

        let fee_payer = fee_payer_keypair.pubkey();
        let sender = sender_keypair.pubkey();
        let recipient = Pubkey::new_unique();

        let base_amount = 900_000;
        let transfer_amount = base_amount + 50;

        let mut fee_payer_data = AccountSharedData::default();
        fee_payer_data.set_lamports(LAMPORTS_PER_SOL);
        test_entry.add_initial_account(fee_payer, &fee_payer_data);

        let mut sender_data = AccountSharedData::default();
        sender_data.set_lamports(base_amount);
        test_entry.add_initial_account(sender, &sender_data);

        let mut recipient_data = AccountSharedData::default();
        recipient_data.set_lamports(base_amount);
        test_entry.add_initial_account(recipient, &recipient_data);

        let instruction = Instruction::new_with_bytes(
            program_id,
            &u64::to_be_bytes(transfer_amount),
            vec![
                AccountMeta::new(sender, true),
                AccountMeta::new(recipient, false),
                AccountMeta::new_readonly(system_program::id(), false),
            ],
        );

        test_entry.push_failed_transaction(Transaction::new_signed_with_payer(
            &[instruction],
            Some(&fee_payer),
            &[&fee_payer_keypair, &sender_keypair],
            Hash::default(),
        ));

        test_entry.transaction_batch[3]
            .asserts
            .logs
            .push("Transfer: insufficient lamports 900000, need 900050".to_string());

        test_entry.decrease_expected_lamports(&fee_payer, LAMPORTS_PER_SIGNATURE * 2);
    }

    // 4: A transaction whose verification has already failed
    {
        let fee_payer_keypair = Keypair::new();
        let fee_payer = fee_payer_keypair.pubkey();

        test_entry.transaction_batch.push(TransactionBatchItem {
            transaction: Transaction::new_signed_with_payer(
                &[],
                Some(&fee_payer),
                &[&fee_payer_keypair],
                Hash::default(),
            ),
            check_result: Err(TransactionError::BlockhashNotFound),
            asserts: TransactionBatchItemAsserts::not_executed(),
        });
    }

    vec![test_entry]
}

fn simple_transfer() -> Vec<SvmTestEntry> {
    let mut test_entry = SvmTestEntry::default();
    let transfer_amount = LAMPORTS_PER_SOL;

    // 0: a transfer that succeeds
    {
        let source_keypair = Keypair::new();
        let source = source_keypair.pubkey();
        let destination = Pubkey::new_unique();

        let mut source_data = AccountSharedData::default();
        let mut destination_data = AccountSharedData::default();

        source_data.set_lamports(LAMPORTS_PER_SOL * 10);
        test_entry.add_initial_account(source, &source_data);

        test_entry.push_transaction(system_transaction::transfer(
            &source_keypair,
            &destination,
            transfer_amount,
            Hash::default(),
        ));

        destination_data
            .checked_add_lamports(transfer_amount)
            .unwrap();
        test_entry.create_expected_account(destination, &destination_data);

        test_entry.decrease_expected_lamports(&source, transfer_amount + LAMPORTS_PER_SIGNATURE);
    }

    // 1: an executable transfer that fails
    {
        let source_keypair = Keypair::new();
        let source = source_keypair.pubkey();

        let mut source_data = AccountSharedData::default();

        source_data.set_lamports(transfer_amount - 1);
        test_entry.add_initial_account(source, &source_data);

        test_entry.push_failed_transaction(system_transaction::transfer(
            &source_keypair,
            &Pubkey::new_unique(),
            transfer_amount,
            Hash::default(),
        ));

        test_entry.decrease_expected_lamports(&source, LAMPORTS_PER_SIGNATURE);
    }

    // 2: a non-executable transfer that fails before loading
    {
        test_entry.transaction_batch.push(TransactionBatchItem {
            transaction: system_transaction::transfer(
                &Keypair::new(),
                &Pubkey::new_unique(),
                transfer_amount,
                Hash::default(),
            ),
            check_result: Err(TransactionError::BlockhashNotFound),
            asserts: TransactionBatchItemAsserts::not_executed(),
        });
    }

    // 3: a non-executable transfer that fails loading the fee-payer
    // NOTE when we support the processed/executed distinction, this is NOT processed
    {
        test_entry.transaction_batch.push(TransactionBatchItem {
            transaction: system_transaction::transfer(
                &Keypair::new(),
                &Pubkey::new_unique(),
                transfer_amount,
                Hash::default(),
            ),
            asserts: TransactionBatchItemAsserts::not_executed(),
            ..TransactionBatchItem::default()
        });
    }

    // 4: a non-executable transfer that fails loading the program
    // NOTE when we support the processed/executed distinction, this IS processed
    // thus this test case will fail with the feature enabled
    {
        let source_keypair = Keypair::new();
        let source = source_keypair.pubkey();

        let mut source_data = AccountSharedData::default();

        source_data.set_lamports(transfer_amount * 10);
        test_entry
            .initial_accounts
            .insert(source, source_data.clone());
        test_entry.final_accounts.insert(source, source_data);

        let mut instruction =
            system_instruction::transfer(&source, &Pubkey::new_unique(), transfer_amount);
        instruction.program_id = Pubkey::new_unique();

        test_entry.transaction_batch.push(TransactionBatchItem {
            transaction: Transaction::new_signed_with_payer(
                &[instruction],
                Some(&source),
                &[&source_keypair],
                Hash::default(),
            ),
            asserts: TransactionBatchItemAsserts::not_executed(),
            ..TransactionBatchItem::default()
        });
    }

    vec![test_entry]
}

#[test_case(program_medley())]
#[test_case(simple_transfer())]
fn svm_integration(test_entries: Vec<SvmTestEntry>) {
    for test_entry in test_entries {
        execute_test_entry(test_entry);
    }
}

fn execute_test_entry(test_entry: SvmTestEntry) {
    let mock_bank = MockBankCallback::default();

    for (name, slot) in &test_entry.initial_programs {
        deploy_program(name.to_string(), *slot, &mock_bank);
    }

    for (pubkey, account) in &test_entry.initial_accounts {
        mock_bank
            .account_shared_data
            .write()
            .unwrap()
            .insert(*pubkey, account.clone());
    }

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

    // execute transaction batch
    let (transactions, check_results) = test_entry.prepare_transactions();
    let batch_output = batch_processor.load_and_execute_sanitized_transactions(
        &mock_bank,
        &transactions,
        check_results,
        &TransactionProcessingEnvironment::default(),
        &processing_config,
    );

    // build a hashmap of final account states incrementally, starting with all initial states, updating to all final states
    // NOTE with SIMD-83 an account may appear multiple times in the same batch
    let mut final_accounts_actual = test_entry.initial_accounts.clone();
    for processed_transaction in batch_output
        .processing_results
        .iter()
        .filter_map(|r| r.as_ref().ok())
    {
        match processed_transaction {
            ProcessedTransaction::Executed(executed_transaction) => {
                for (pubkey, account_data) in
                    executed_transaction.loaded_transaction.accounts.clone()
                {
                    final_accounts_actual.insert(pubkey, account_data);
                }
            }
            // NOTE this is a possible state with `feature_set::enable_transaction_loading_failure_fees` enabled
            // by using `TransactionProcessingEnvironment::default()` we have all features disabled
            // in other words, this will be unreachable until we are ready to test fee-only transactions
            // (or the feature is activated on mainnet and removed... but we should do it before then!)
            ProcessedTransaction::FeesOnly(_) => unreachable!(),
        }
    }

    // check that all the account states we care about are present and correct
    for (pubkey, expected_account_data) in test_entry.final_accounts.iter() {
        let actual_account_data = final_accounts_actual.get(pubkey);
        assert_eq!(
            Some(expected_account_data),
            actual_account_data,
            "mismatch on account {}",
            pubkey
        );
    }

    // now run our transaction-by-transaction checks
    for (processing_result, test_item_asserts) in batch_output
        .processing_results
        .iter()
        .zip(test_entry.asserts())
    {
        match processing_result {
            Ok(ProcessedTransaction::Executed(executed_transaction)) => test_item_asserts
                .check_executed_transaction(&executed_transaction.execution_details),
            Ok(ProcessedTransaction::FeesOnly(_)) => unreachable!(),
            Err(_) => assert!(!test_item_asserts.executed),
        }
    }
}

#[test]
fn svm_inspect_account() {
    let mock_bank = MockBankCallback::default();
    let mut expected_inspected_accounts: HashMap<_, Vec<_>> = HashMap::new();

    let transfer_program =
        deploy_program("simple-transfer".to_string(), DEPLOYMENT_SLOT, &mock_bank);

    let fee_payer_keypair = Keypair::new();
    let sender_keypair = Keypair::new();

    let fee_payer = fee_payer_keypair.pubkey();
    let sender = sender_keypair.pubkey();
    let recipient = Pubkey::new_unique();
    let system = system_program::id();

    // Setting up the accounts for the transfer

    // fee payer
    let mut fee_payer_account = AccountSharedData::default();
    fee_payer_account.set_lamports(80_020);
    mock_bank
        .account_shared_data
        .write()
        .unwrap()
        .insert(fee_payer, fee_payer_account.clone());
    expected_inspected_accounts
        .entry(fee_payer)
        .or_default()
        .push((Some(fee_payer_account.clone()), true));

    // sender
    let mut sender_account = AccountSharedData::default();
    sender_account.set_lamports(11_000_000);
    mock_bank
        .account_shared_data
        .write()
        .unwrap()
        .insert(sender, sender_account.clone());
    expected_inspected_accounts
        .entry(sender)
        .or_default()
        .push((Some(sender_account.clone()), true));

    // recipient -- initially dead
    expected_inspected_accounts
        .entry(recipient)
        .or_default()
        .push((None, true));

    let instruction = Instruction::new_with_bytes(
        transfer_program,
        &u64::to_be_bytes(1_000_000),
        vec![
            AccountMeta::new(sender, true),
            AccountMeta::new(recipient, false),
            AccountMeta::new_readonly(system, false),
        ],
    );
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&fee_payer),
        &[&fee_payer_keypair, &sender_keypair],
        Hash::default(),
    );
    let sanitized_transaction = SanitizedTransaction::from_transaction_for_tests(transaction);
    let transaction_check = Ok(CheckedTransactionDetails {
        nonce: None,
        lamports_per_signature: 20,
    });

    // Load and execute the transaction

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

    let _result = batch_processor.load_and_execute_sanitized_transactions(
        &mock_bank,
        &[sanitized_transaction],
        vec![transaction_check],
        &TransactionProcessingEnvironment::default(),
        &TransactionProcessingConfig::default(),
    );

    // the system account is modified during transaction processing,
    // so set the expected inspected account afterwards.
    let system_account = mock_bank
        .account_shared_data
        .read()
        .unwrap()
        .get(&system)
        .cloned();
    expected_inspected_accounts
        .entry(system)
        .or_default()
        .push((system_account, false));

    // do another transfer; recipient should be alive now

    // fee payer
    let mut fee_payer_account = AccountSharedData::default();
    fee_payer_account.set_lamports(80_000);
    mock_bank
        .account_shared_data
        .write()
        .unwrap()
        .insert(fee_payer, fee_payer_account.clone());
    expected_inspected_accounts
        .entry(fee_payer)
        .or_default()
        .push((Some(fee_payer_account.clone()), true));

    // sender
    let mut sender_account = AccountSharedData::default();
    sender_account.set_lamports(10_000_000);
    mock_bank
        .account_shared_data
        .write()
        .unwrap()
        .insert(sender, sender_account.clone());
    expected_inspected_accounts
        .entry(sender)
        .or_default()
        .push((Some(sender_account.clone()), true));

    // recipient -- now alive
    let mut recipient_account = AccountSharedData::default();
    recipient_account.set_lamports(1_000_000);
    mock_bank
        .account_shared_data
        .write()
        .unwrap()
        .insert(recipient, recipient_account.clone());
    expected_inspected_accounts
        .entry(recipient)
        .or_default()
        .push((Some(recipient_account.clone()), true));

    let instruction = Instruction::new_with_bytes(
        transfer_program,
        &u64::to_be_bytes(456),
        vec![
            AccountMeta::new(sender, true),
            AccountMeta::new(recipient, false),
            AccountMeta::new_readonly(system, false),
        ],
    );
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&fee_payer),
        &[&fee_payer_keypair, &sender_keypair],
        Hash::default(),
    );
    let sanitized_transaction = SanitizedTransaction::from_transaction_for_tests(transaction);
    let transaction_check = Ok(CheckedTransactionDetails {
        nonce: None,
        lamports_per_signature: 20,
    });

    // Load and execute the second transaction
    let _result = batch_processor.load_and_execute_sanitized_transactions(
        &mock_bank,
        &[sanitized_transaction],
        vec![transaction_check],
        &TransactionProcessingEnvironment::default(),
        &TransactionProcessingConfig::default(),
    );

    // the system account is modified during transaction processing,
    // so set the expected inspected account afterwards.
    let system_account = mock_bank
        .account_shared_data
        .read()
        .unwrap()
        .get(&system)
        .cloned();
    expected_inspected_accounts
        .entry(system)
        .or_default()
        .push((system_account, false));

    // Ensure all the expected inspected accounts were inspected
    let actual_inspected_accounts = mock_bank.inspected_accounts.read().unwrap().clone();
    for (expected_pubkey, expected_account) in &expected_inspected_accounts {
        let actual_account = actual_inspected_accounts.get(expected_pubkey).unwrap();
        assert_eq!(
            expected_account, actual_account,
            "pubkey: {expected_pubkey}",
        );
    }

    // The transfer program account is also loaded during transaction processing, however the
    // account state passed to `inspect_account()` is *not* the same as what is held by
    // MockBankCallback::account_shared_data.  So we check the transfer program differently.
    //
    // First ensure we have the correct number of inspected accounts, correctly counting the
    // transfer program.
    let num_expected_inspected_accounts: usize =
        expected_inspected_accounts.values().map(Vec::len).sum();
    let num_actual_inspected_accounts: usize =
        actual_inspected_accounts.values().map(Vec::len).sum();
    assert_eq!(
        num_expected_inspected_accounts + 2,
        num_actual_inspected_accounts,
    );

    // And second, ensure the inspected transfer program accounts are alive and not writable.
    let actual_transfer_program_accounts =
        actual_inspected_accounts.get(&transfer_program).unwrap();
    for actual_transfer_program_account in actual_transfer_program_accounts {
        assert!(actual_transfer_program_account.0.is_some());
        assert!(!actual_transfer_program_account.1);
    }
}

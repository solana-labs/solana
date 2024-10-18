#![cfg(test)]
#![allow(clippy::arithmetic_side_effects)]

use {
    crate::mock_bank::{
        create_executable_environment, deploy_program_with_upgrade_authority, program_address,
        register_builtins, MockBankCallback, MockForkGraph, EXECUTION_EPOCH, EXECUTION_SLOT,
        WALLCLOCK_TIME,
    },
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount, WritableAccount},
        clock::Slot,
        compute_budget::ComputeBudgetInstruction,
        feature_set::{self, FeatureSet},
        hash::Hash,
        instruction::{AccountMeta, Instruction},
        native_token::LAMPORTS_PER_SOL,
        nonce::{self, state::DurableNonce},
        pubkey::Pubkey,
        signature::Signer,
        signer::keypair::Keypair,
        system_instruction, system_program, system_transaction,
        sysvar::rent::Rent,
        transaction::{SanitizedTransaction, Transaction, TransactionError},
        transaction_context::TransactionReturnData,
    },
    solana_svm::{
        account_loader::{CheckedTransactionDetails, TransactionCheckResult},
        nonce_info::NonceInfo,
        rollback_accounts::RollbackAccounts,
        transaction_execution_result::TransactionExecutionDetails,
        transaction_processing_result::{ProcessedTransaction, TransactionProcessingResult},
        transaction_processor::{
            ExecutionRecordingConfig, TransactionBatchProcessor, TransactionProcessingConfig,
            TransactionProcessingEnvironment,
        },
    },
    solana_svm_transaction::svm_message::SVMMessage,
    solana_type_overrides::sync::{Arc, RwLock},
    std::collections::HashMap,
    test_case::test_case,
};

// This module contains the implementation of TransactionProcessingCallback
mod mock_bank;

const DEPLOYMENT_SLOT: u64 = 0;
const LAMPORTS_PER_SIGNATURE: u64 = 5000;
const LAST_BLOCKHASH: Hash = Hash::new_from_array([7; 32]); // Arbitrary constant hash for advancing nonces

pub type AccountsMap = HashMap<Pubkey, AccountSharedData>;

// container for everything needed to execute a test entry
// care should be taken if reused, because we update bank account states, but otherwise leave it as-is
// the environment is made available for tests that check it after processing
pub struct SvmTestEnvironment<'a> {
    pub mock_bank: MockBankCallback,
    pub fork_graph: Arc<RwLock<MockForkGraph>>,
    pub batch_processor: TransactionBatchProcessor<MockForkGraph>,
    pub processing_config: TransactionProcessingConfig<'a>,
    pub processing_environment: TransactionProcessingEnvironment<'a>,
    pub test_entry: SvmTestEntry,
}

impl SvmTestEnvironment<'_> {
    pub fn create(test_entry: SvmTestEntry) -> Self {
        let mock_bank = MockBankCallback::default();

        for (name, slot, authority) in &test_entry.initial_programs {
            deploy_program_with_upgrade_authority(name.to_string(), *slot, &mock_bank, *authority);
        }

        for (pubkey, account) in &test_entry.initial_accounts {
            mock_bank
                .account_shared_data
                .write()
                .unwrap()
                .insert(*pubkey, account.clone());
        }

        let batch_processor = TransactionBatchProcessor::<MockForkGraph>::new_uninitialized(
            EXECUTION_SLOT,
            EXECUTION_EPOCH,
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

        let mut feature_set = FeatureSet::default();
        for feature_id in &test_entry.enabled_features {
            feature_set.activate(feature_id, 0);
        }

        let processing_environment = TransactionProcessingEnvironment {
            blockhash: LAST_BLOCKHASH,
            feature_set: feature_set.into(),
            lamports_per_signature: LAMPORTS_PER_SIGNATURE,
            ..TransactionProcessingEnvironment::default()
        };

        Self {
            mock_bank,
            fork_graph,
            batch_processor,
            processing_config,
            processing_environment,
            test_entry,
        }
    }

    pub fn execute(&self) {
        let (transactions, check_results) = self.test_entry.prepare_transactions();
        let batch_output = self
            .batch_processor
            .load_and_execute_sanitized_transactions(
                &self.mock_bank,
                &transactions,
                check_results,
                &self.processing_environment,
                &self.processing_config,
            );

        // build a hashmap of final account states incrementally
        // starting with all initial states, updating to all final states
        // with SIMD83, an account might change multiple times in the same batch
        // but it might not exist on all transactions
        let mut final_accounts_actual = self.test_entry.initial_accounts.clone();

        for (index, processed_transaction) in batch_output.processing_results.iter().enumerate() {
            match processed_transaction {
                Ok(ProcessedTransaction::Executed(executed_transaction)) => {
                    for (pubkey, account_data) in
                        executed_transaction.loaded_transaction.accounts.clone()
                    {
                        final_accounts_actual.insert(pubkey, account_data);
                    }
                }
                Ok(ProcessedTransaction::FeesOnly(fees_only_transaction)) => {
                    let fee_payer = transactions[index].fee_payer();

                    match fees_only_transaction.rollback_accounts.clone() {
                        RollbackAccounts::FeePayerOnly { fee_payer_account } => {
                            final_accounts_actual.insert(*fee_payer, fee_payer_account);
                        }
                        RollbackAccounts::SameNonceAndFeePayer { nonce } => {
                            final_accounts_actual.insert(*nonce.address(), nonce.account().clone());
                        }
                        RollbackAccounts::SeparateNonceAndFeePayer {
                            nonce,
                            fee_payer_account,
                        } => {
                            final_accounts_actual.insert(*fee_payer, fee_payer_account);
                            final_accounts_actual.insert(*nonce.address(), nonce.account().clone());
                        }
                    }
                }
                Err(_) => {}
            }
        }

        // first assert all transaction states together, it makes test-driven development much less of a headache
        let (expected_statuses, actual_statuses): (Vec<_>, Vec<_>) = batch_output
            .processing_results
            .iter()
            .zip(self.test_entry.asserts())
            .map(|(processing_result, test_item_assert)| {
                (
                    ExecutionStatus::from(processing_result),
                    test_item_assert.status,
                )
            })
            .unzip();
        assert_eq!(expected_statuses, actual_statuses);

        // check that all the account states we care about are present and correct
        for (pubkey, expected_account_data) in self.test_entry.final_accounts.iter() {
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
            .zip(self.test_entry.asserts())
        {
            match processing_result {
                Ok(ProcessedTransaction::Executed(executed_transaction)) => test_item_asserts
                    .check_executed_transaction(&executed_transaction.execution_details),
                Ok(ProcessedTransaction::FeesOnly(_)) => {
                    assert!(test_item_asserts.processed());
                    assert!(!test_item_asserts.executed());
                }
                Err(_) => assert!(test_item_asserts.discarded()),
            }
        }

        let mut mock_bank_accounts = self.mock_bank.account_shared_data.write().unwrap();
        *mock_bank_accounts = final_accounts_actual;
    }
}

// container for a transaction batch and all data needed to run and verify it against svm
#[derive(Clone, Debug, Default)]
pub struct SvmTestEntry {
    // features are disabled by default; these will be enabled
    pub enabled_features: Vec<Pubkey>,

    // programs to deploy to the new svm
    pub initial_programs: Vec<(String, Slot, Option<Pubkey>)>,

    // accounts to deploy to the new svm before transaction execution
    pub initial_accounts: AccountsMap,

    // transactions to execute and transaction-specific checks to perform on the results from svm
    pub transaction_batch: Vec<TransactionBatchItem>,

    // expected final account states, checked after transaction execution
    pub final_accounts: AccountsMap,
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

    // add an immutable program that will have been deployed before the slot we execute transactions in
    pub fn add_initial_program(&mut self, program_name: &str) {
        self.initial_programs
            .push((program_name.to_string(), DEPLOYMENT_SLOT, None));
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
        self.push_transaction_with_status(transaction, ExecutionStatus::Succeeded)
    }

    // convenience function that adds a transaction with an expected execution status
    pub fn push_transaction_with_status(
        &mut self,
        transaction: Transaction,
        status: ExecutionStatus,
    ) {
        self.transaction_batch.push(TransactionBatchItem {
            transaction,
            asserts: TransactionBatchItemAsserts {
                status,
                ..TransactionBatchItemAsserts::default()
            },
            ..TransactionBatchItem::default()
        });
    }

    // convenience function that adds a nonce transaction that is expected to succeed
    // we accept the prior nonce state and advance it for the check status, since this happens before svm
    pub fn push_nonce_transaction(&mut self, transaction: Transaction, nonce_info: NonceInfo) {
        self.push_nonce_transaction_with_status(transaction, nonce_info, ExecutionStatus::Succeeded)
    }

    // convenience function that adds a nonce transaction with an expected execution status
    // we accept the prior nonce state and advance it for the check status, since this happens before svm
    pub fn push_nonce_transaction_with_status(
        &mut self,
        transaction: Transaction,
        mut nonce_info: NonceInfo,
        status: ExecutionStatus,
    ) {
        if status != ExecutionStatus::Discarded {
            nonce_info
                .try_advance_nonce(
                    DurableNonce::from_blockhash(&LAST_BLOCKHASH),
                    LAMPORTS_PER_SIGNATURE,
                )
                .unwrap();
        }

        self.transaction_batch.push(TransactionBatchItem {
            transaction,
            asserts: TransactionBatchItemAsserts {
                status,
                ..TransactionBatchItemAsserts::default()
            },
            ..TransactionBatchItem::with_nonce(nonce_info)
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

impl TransactionBatchItem {
    fn with_nonce(nonce_info: NonceInfo) -> Self {
        Self {
            check_result: Ok(CheckedTransactionDetails {
                nonce: Some(nonce_info),
                lamports_per_signature: LAMPORTS_PER_SIGNATURE,
            }),
            ..Self::default()
        }
    }
}

impl Default for TransactionBatchItem {
    fn default() -> Self {
        Self {
            transaction: Transaction::default(),
            check_result: Ok(CheckedTransactionDetails {
                nonce: None,
                lamports_per_signature: LAMPORTS_PER_SIGNATURE,
            }),
            asserts: TransactionBatchItemAsserts::default(),
        }
    }
}

// asserts for a given transaction in a batch
// we can automatically check whether it executed, whether it succeeded
// log items we expect to see (exect match only), and rodata
#[derive(Clone, Debug, Default)]
pub struct TransactionBatchItemAsserts {
    pub status: ExecutionStatus,
    pub logs: Vec<String>,
    pub return_data: ReturnDataAssert,
}

impl TransactionBatchItemAsserts {
    pub fn succeeded(&self) -> bool {
        self.status.succeeded()
    }

    pub fn executed(&self) -> bool {
        self.status.executed()
    }

    pub fn processed(&self) -> bool {
        self.status.processed()
    }

    pub fn discarded(&self) -> bool {
        self.status.discarded()
    }

    pub fn check_executed_transaction(&self, execution_details: &TransactionExecutionDetails) {
        assert!(self.executed());
        assert_eq!(self.succeeded(), execution_details.status.is_ok());

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

impl From<ExecutionStatus> for TransactionBatchItemAsserts {
    fn from(status: ExecutionStatus) -> Self {
        Self {
            status,
            ..Self::default()
        }
    }
}

// states a transaction can end in after a trip through the batch processor:
// * discarded: no-op. not even processed. a flawed transaction excluded from the entry
// * processed-failed: aka fee (and nonce) only. charged and added to an entry but not executed, would have failed invariably
// * executed-failed: failed during execution. as above, fees charged and nonce advanced
// * succeeded: what we all aspire to be in our transaction processing lifecycles
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExecutionStatus {
    Discarded,
    ProcessedFailed,
    ExecutedFailed,
    #[default]
    Succeeded,
}

// note we avoid the word "failed" because it is confusing
// the batch processor uses it to mean "executed and not succeeded"
// but intuitively (and from the point of a user) it could just as likely mean "any state other than succeeded"
impl ExecutionStatus {
    pub fn succeeded(self) -> bool {
        self == Self::Succeeded
    }

    pub fn executed(self) -> bool {
        self > Self::ProcessedFailed
    }

    pub fn processed(self) -> bool {
        self != Self::Discarded
    }

    pub fn discarded(self) -> bool {
        self == Self::Discarded
    }
}

impl From<&TransactionProcessingResult> for ExecutionStatus {
    fn from(processing_result: &TransactionProcessingResult) -> Self {
        match processing_result {
            Ok(ProcessedTransaction::Executed(executed_transaction)) => {
                if executed_transaction.execution_details.status.is_ok() {
                    ExecutionStatus::Succeeded
                } else {
                    ExecutionStatus::ExecutedFailed
                }
            }
            Ok(ProcessedTransaction::FeesOnly(_)) => ExecutionStatus::ProcessedFailed,
            Err(_) => ExecutionStatus::Discarded,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
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
        let program_name = "hello-solana";
        let program_id = program_address(program_name);
        test_entry.add_initial_program(program_name);

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
        let program_name = "simple-transfer";
        let program_id = program_address(program_name);
        test_entry.add_initial_program(program_name);

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
        let program_name = "clock-sysvar";
        let program_id = program_address(program_name);
        test_entry.add_initial_program(program_name);

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

        test_entry.push_transaction_with_status(
            Transaction::new_signed_with_payer(
                &[instruction],
                Some(&fee_payer),
                &[&fee_payer_keypair, &sender_keypair],
                Hash::default(),
            ),
            ExecutionStatus::ExecutedFailed,
        );

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
            asserts: ExecutionStatus::Discarded.into(),
        });
    }

    vec![test_entry]
}

fn simple_transfer(enable_fee_only_transactions: bool) -> Vec<SvmTestEntry> {
    let mut test_entry = SvmTestEntry::default();
    let transfer_amount = LAMPORTS_PER_SOL;
    if enable_fee_only_transactions {
        test_entry
            .enabled_features
            .push(feature_set::enable_transaction_loading_failure_fees::id());
    }

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

        test_entry.push_transaction_with_status(
            system_transaction::transfer(
                &source_keypair,
                &Pubkey::new_unique(),
                transfer_amount,
                Hash::default(),
            ),
            ExecutionStatus::ExecutedFailed,
        );

        test_entry.decrease_expected_lamports(&source, LAMPORTS_PER_SIGNATURE);
    }

    // 2: a non-processable transfer that fails before loading
    {
        test_entry.transaction_batch.push(TransactionBatchItem {
            transaction: system_transaction::transfer(
                &Keypair::new(),
                &Pubkey::new_unique(),
                transfer_amount,
                Hash::default(),
            ),
            check_result: Err(TransactionError::BlockhashNotFound),
            asserts: ExecutionStatus::Discarded.into(),
        });
    }

    // 3: a non-processable transfer that fails loading the fee-payer
    {
        test_entry.push_transaction_with_status(
            system_transaction::transfer(
                &Keypair::new(),
                &Pubkey::new_unique(),
                transfer_amount,
                Hash::default(),
            ),
            ExecutionStatus::Discarded,
        );
    }

    // 4: a processable non-executable transfer that fails loading the program
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

        let expected_status = if enable_fee_only_transactions {
            test_entry.decrease_expected_lamports(&source, LAMPORTS_PER_SIGNATURE);
            ExecutionStatus::ProcessedFailed
        } else {
            ExecutionStatus::Discarded
        };

        test_entry.push_transaction_with_status(
            Transaction::new_signed_with_payer(
                &[instruction],
                Some(&source),
                &[&source_keypair],
                Hash::default(),
            ),
            expected_status,
        );
    }

    vec![test_entry]
}

fn simple_nonce(enable_fee_only_transactions: bool, fee_paying_nonce: bool) -> Vec<SvmTestEntry> {
    let mut test_entry = SvmTestEntry::default();
    if enable_fee_only_transactions {
        test_entry
            .enabled_features
            .push(feature_set::enable_transaction_loading_failure_fees::id());
    }

    let program_name = "hello-solana";
    let real_program_id = program_address(program_name);
    test_entry.add_initial_program(program_name);

    // create and return a transaction, fee payer, and nonce info
    // sets up initial account states but not final ones
    // there are four cases of fee_paying_nonce and fake_fee_payer:
    // * false/false: normal nonce account with rent minimum, normal fee payer account with 1sol
    // * true/false: normal nonce account used to pay fees with rent minimum plus 1sol
    // * false/true: normal nonce account with rent minimum, fee payer doesnt exist
    // * true/true: same account for both which does not exist
    // we also provide a side door to bring a fee-paying nonce account below rent-exemption
    let mk_nonce_transaction = |test_entry: &mut SvmTestEntry,
                                program_id,
                                fake_fee_payer: bool,
                                rent_paying_nonce: bool| {
        let fee_payer_keypair = Keypair::new();
        let fee_payer = fee_payer_keypair.pubkey();
        let nonce_pubkey = if fee_paying_nonce {
            fee_payer
        } else {
            Pubkey::new_unique()
        };

        let nonce_size = nonce::State::size();
        let mut nonce_balance = Rent::default().minimum_balance(nonce_size);

        if !fake_fee_payer && !fee_paying_nonce {
            let mut fee_payer_data = AccountSharedData::default();
            fee_payer_data.set_lamports(LAMPORTS_PER_SOL);
            test_entry.add_initial_account(fee_payer, &fee_payer_data);
        } else if rent_paying_nonce {
            assert!(fee_paying_nonce);
            nonce_balance += LAMPORTS_PER_SIGNATURE;
            nonce_balance -= 1;
        } else if fee_paying_nonce {
            nonce_balance += LAMPORTS_PER_SOL;
        }

        let nonce_initial_hash = DurableNonce::from_blockhash(&Hash::new_unique());
        let nonce_data =
            nonce::state::Data::new(fee_payer, nonce_initial_hash, LAMPORTS_PER_SIGNATURE);
        let nonce_account = AccountSharedData::new_data(
            nonce_balance,
            &nonce::state::Versions::new(nonce::State::Initialized(nonce_data.clone())),
            &system_program::id(),
        )
        .unwrap();
        let nonce_info = NonceInfo::new(nonce_pubkey, nonce_account.clone());

        if !(fake_fee_payer && fee_paying_nonce) {
            test_entry.add_initial_account(nonce_pubkey, &nonce_account);
        }

        let instructions = vec![
            system_instruction::advance_nonce_account(&nonce_pubkey, &fee_payer),
            Instruction::new_with_bytes(program_id, &[], vec![]),
        ];

        let transaction = Transaction::new_signed_with_payer(
            &instructions,
            Some(&fee_payer),
            &[&fee_payer_keypair],
            nonce_data.blockhash(),
        );

        (transaction, fee_payer, nonce_info)
    };

    // 0: successful nonce transaction, regardless of features
    {
        let (transaction, fee_payer, mut nonce_info) =
            mk_nonce_transaction(&mut test_entry, real_program_id, false, false);

        test_entry.push_nonce_transaction(transaction, nonce_info.clone());

        test_entry.decrease_expected_lamports(&fee_payer, LAMPORTS_PER_SIGNATURE);

        nonce_info
            .try_advance_nonce(
                DurableNonce::from_blockhash(&LAST_BLOCKHASH),
                LAMPORTS_PER_SIGNATURE,
            )
            .unwrap();

        test_entry
            .final_accounts
            .get_mut(nonce_info.address())
            .unwrap()
            .data_as_mut_slice()
            .copy_from_slice(nonce_info.account().data());
    }

    // 1: non-executing nonce transaction (fee payer doesnt exist) regardless of features
    {
        let (transaction, _fee_payer, nonce_info) =
            mk_nonce_transaction(&mut test_entry, real_program_id, true, false);

        test_entry
            .final_accounts
            .entry(*nonce_info.address())
            .and_modify(|account| account.set_rent_epoch(0));

        test_entry.push_nonce_transaction_with_status(
            transaction,
            nonce_info,
            ExecutionStatus::Discarded,
        );
    }

    // 2: failing nonce transaction (bad system instruction) regardless of features
    {
        let (transaction, fee_payer, mut nonce_info) =
            mk_nonce_transaction(&mut test_entry, system_program::id(), false, false);

        test_entry.push_nonce_transaction_with_status(
            transaction,
            nonce_info.clone(),
            ExecutionStatus::ExecutedFailed,
        );

        test_entry.decrease_expected_lamports(&fee_payer, LAMPORTS_PER_SIGNATURE);

        nonce_info
            .try_advance_nonce(
                DurableNonce::from_blockhash(&LAST_BLOCKHASH),
                LAMPORTS_PER_SIGNATURE,
            )
            .unwrap();

        test_entry
            .final_accounts
            .get_mut(nonce_info.address())
            .unwrap()
            .data_as_mut_slice()
            .copy_from_slice(nonce_info.account().data());
    }

    // 3: processable non-executable nonce transaction with fee-only enabled, otherwise discarded
    {
        let (transaction, fee_payer, mut nonce_info) =
            mk_nonce_transaction(&mut test_entry, Pubkey::new_unique(), false, false);

        if enable_fee_only_transactions {
            test_entry.push_nonce_transaction_with_status(
                transaction,
                nonce_info.clone(),
                ExecutionStatus::ProcessedFailed,
            );

            test_entry.decrease_expected_lamports(&fee_payer, LAMPORTS_PER_SIGNATURE);

            nonce_info
                .try_advance_nonce(
                    DurableNonce::from_blockhash(&LAST_BLOCKHASH),
                    LAMPORTS_PER_SIGNATURE,
                )
                .unwrap();

            test_entry
                .final_accounts
                .get_mut(nonce_info.address())
                .unwrap()
                .data_as_mut_slice()
                .copy_from_slice(nonce_info.account().data());

            // if the nonce account pays fees, it keeps its new rent epoch, otherwise it resets
            if !fee_paying_nonce {
                test_entry
                    .final_accounts
                    .get_mut(nonce_info.address())
                    .unwrap()
                    .set_rent_epoch(0);
            }
        } else {
            test_entry
                .final_accounts
                .get_mut(&fee_payer)
                .unwrap()
                .set_rent_epoch(0);

            test_entry
                .final_accounts
                .get_mut(nonce_info.address())
                .unwrap()
                .set_rent_epoch(0);

            test_entry.push_nonce_transaction_with_status(
                transaction,
                nonce_info,
                ExecutionStatus::Discarded,
            );
        }
    }

    // 4: safety check that nonce fee-payers are required to be rent-exempt (blockhash fee-payers may be below rent-exemption)
    // if this situation is ever allowed in the future, the nonce account MUST be hidden for fee-only transactions
    // as an aside, nonce accounts closed by WithdrawNonceAccount are safe because they are ordinary executed transactions
    // we also dont care whether a non-fee nonce (or any account) pays rent because rent is charged on executed transactions
    if fee_paying_nonce {
        let (transaction, _, nonce_info) =
            mk_nonce_transaction(&mut test_entry, real_program_id, false, true);

        test_entry
            .final_accounts
            .get_mut(nonce_info.address())
            .unwrap()
            .set_rent_epoch(0);

        test_entry.push_nonce_transaction_with_status(
            transaction,
            nonce_info.clone(),
            ExecutionStatus::Discarded,
        );
    }

    // 5: rent-paying nonce fee-payers are also not charged for fee-only transactions
    if enable_fee_only_transactions && fee_paying_nonce {
        let (transaction, _, nonce_info) =
            mk_nonce_transaction(&mut test_entry, Pubkey::new_unique(), false, true);

        test_entry
            .final_accounts
            .get_mut(nonce_info.address())
            .unwrap()
            .set_rent_epoch(0);

        test_entry.push_nonce_transaction_with_status(
            transaction,
            nonce_info.clone(),
            ExecutionStatus::Discarded,
        );
    }

    vec![test_entry]
}

#[allow(unused)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WriteProgramInstruction {
    Print,
    Set,
    Dealloc,
    Realloc(usize),
}
impl WriteProgramInstruction {
    fn _create_transaction(
        self,
        program_id: Pubkey,
        fee_payer: &Keypair,
        target: Pubkey,
        clamp_data_size: Option<u32>,
    ) -> Transaction {
        let (instruction_data, account_metas) = match self {
            Self::Print => (vec![0], vec![AccountMeta::new_readonly(target, false)]),
            Self::Set => (vec![1], vec![AccountMeta::new(target, false)]),
            Self::Dealloc => (
                vec![2],
                vec![
                    AccountMeta::new(target, false),
                    AccountMeta::new(solana_sdk::incinerator::id(), false),
                ],
            ),
            Self::Realloc(new_size) => {
                let mut instruction_data = vec![3];
                instruction_data.extend_from_slice(&new_size.to_le_bytes());
                (instruction_data, vec![AccountMeta::new(target, false)])
            }
        };

        let mut instructions = vec![];

        if let Some(size) = clamp_data_size {
            instructions.push(ComputeBudgetInstruction::set_loaded_accounts_data_size_limit(size));
        }

        instructions.push(Instruction::new_with_bytes(
            program_id,
            &instruction_data,
            account_metas,
        ));

        Transaction::new_signed_with_payer(
            &instructions,
            Some(&fee_payer.pubkey()),
            &[fee_payer],
            Hash::default(),
        )
    }
}

#[test_case(program_medley())]
#[test_case(simple_transfer(false))]
#[test_case(simple_transfer(true))]
#[test_case(simple_nonce(false, false))]
#[test_case(simple_nonce(true, false))]
#[test_case(simple_nonce(false, true))]
#[test_case(simple_nonce(true, true))]
fn svm_integration(test_entries: Vec<SvmTestEntry>) {
    for test_entry in test_entries {
        let env = SvmTestEnvironment::create(test_entry);
        env.execute();
    }
}

#[test]
fn svm_inspect_account() {
    let mut initial_test_entry = SvmTestEntry::default();
    let mut expected_inspected_accounts: HashMap<_, Vec<_>> = HashMap::new();

    let fee_payer_keypair = Keypair::new();
    let sender_keypair = Keypair::new();

    let fee_payer = fee_payer_keypair.pubkey();
    let sender = sender_keypair.pubkey();
    let recipient = Pubkey::new_unique();

    // Setting up the accounts for the transfer

    // fee payer
    let mut fee_payer_account = AccountSharedData::default();
    fee_payer_account.set_lamports(85_000);
    fee_payer_account.set_rent_epoch(u64::MAX);
    initial_test_entry.add_initial_account(fee_payer, &fee_payer_account);
    expected_inspected_accounts
        .entry(fee_payer)
        .or_default()
        .push((Some(fee_payer_account.clone()), true));

    // sender
    let mut sender_account = AccountSharedData::default();
    sender_account.set_lamports(11_000_000);
    sender_account.set_rent_epoch(u64::MAX);
    initial_test_entry.add_initial_account(sender, &sender_account);
    expected_inspected_accounts
        .entry(sender)
        .or_default()
        .push((Some(sender_account.clone()), true));

    // recipient -- initially dead
    expected_inspected_accounts
        .entry(recipient)
        .or_default()
        .push((None, true));

    let transfer_amount = 1_000_000;
    let transaction = Transaction::new_signed_with_payer(
        &[system_instruction::transfer(
            &sender,
            &recipient,
            transfer_amount,
        )],
        Some(&fee_payer),
        &[&fee_payer_keypair, &sender_keypair],
        Hash::default(),
    );

    initial_test_entry.push_transaction(transaction);

    let mut recipient_account = AccountSharedData::default();
    recipient_account.set_lamports(transfer_amount);

    initial_test_entry.decrease_expected_lamports(&fee_payer, LAMPORTS_PER_SIGNATURE * 2);
    initial_test_entry.decrease_expected_lamports(&sender, transfer_amount);
    initial_test_entry.create_expected_account(recipient, &recipient_account);

    let initial_test_entry = initial_test_entry;

    // Load and execute the transaction
    let mut env = SvmTestEnvironment::create(initial_test_entry.clone());
    env.execute();

    // do another transfer; recipient should be alive now

    // fee payer
    let intermediate_fee_payer_account = initial_test_entry.final_accounts.get(&fee_payer).cloned();
    assert!(intermediate_fee_payer_account.is_some());

    expected_inspected_accounts
        .entry(fee_payer)
        .or_default()
        .push((intermediate_fee_payer_account, true));

    // sender
    let intermediate_sender_account = initial_test_entry.final_accounts.get(&sender).cloned();
    assert!(intermediate_sender_account.is_some());

    expected_inspected_accounts
        .entry(sender)
        .or_default()
        .push((intermediate_sender_account, true));

    // recipient -- now alive
    let intermediate_recipient_account = initial_test_entry.final_accounts.get(&recipient).cloned();
    assert!(intermediate_recipient_account.is_some());

    expected_inspected_accounts
        .entry(recipient)
        .or_default()
        .push((intermediate_recipient_account, true));

    let mut final_test_entry = SvmTestEntry {
        initial_accounts: initial_test_entry.final_accounts.clone(),
        final_accounts: initial_test_entry.final_accounts.clone(),
        ..SvmTestEntry::default()
    };

    let transfer_amount = 456;
    let transaction = Transaction::new_signed_with_payer(
        &[system_instruction::transfer(
            &sender,
            &recipient,
            transfer_amount,
        )],
        Some(&fee_payer),
        &[&fee_payer_keypair, &sender_keypair],
        Hash::default(),
    );

    final_test_entry.push_transaction(transaction);

    final_test_entry.decrease_expected_lamports(&fee_payer, LAMPORTS_PER_SIGNATURE * 2);
    final_test_entry.decrease_expected_lamports(&sender, transfer_amount);
    final_test_entry.increase_expected_lamports(&recipient, transfer_amount);

    // Load and execute the second transaction
    env.test_entry = final_test_entry;
    env.execute();

    // Ensure all the expected inspected accounts were inspected
    let actual_inspected_accounts = env.mock_bank.inspected_accounts.read().unwrap().clone();
    for (expected_pubkey, expected_account) in &expected_inspected_accounts {
        let actual_account = actual_inspected_accounts.get(expected_pubkey).unwrap();
        assert_eq!(
            expected_account, actual_account,
            "pubkey: {expected_pubkey}",
        );
    }

    // The system program is retreived from the program cache, which does not
    // inspect accounts, because they are necessarily read-only. Verify it has not made
    // its way into the inspected accounts list.
    let num_expected_inspected_accounts: usize =
        expected_inspected_accounts.values().map(Vec::len).sum();
    let num_actual_inspected_accounts: usize =
        actual_inspected_accounts.values().map(Vec::len).sum();

    assert_eq!(
        num_expected_inspected_accounts,
        num_actual_inspected_accounts,
    );
}

use {
    crate::{
        rollback_accounts::RollbackAccounts,
        transaction_processing_result::{
            TransactionProcessingResult, TransactionProcessingResultExtensions,
        },
    },
    solana_sdk::{
        account::AccountSharedData, nonce::state::DurableNonce, pubkey::Pubkey,
        transaction_context::TransactionAccount,
    },
    solana_svm_transaction::svm_message::SVMMessage,
};

// Used to approximate how many accounts will be calculated for storage so that
// vectors are allocated with an appropriate capacity. Doesn't account for some
// optimization edge cases where some write locked accounts have skip storage.
fn max_number_of_accounts_to_collect(
    txs: &[impl SVMMessage],
    processing_results: &[TransactionProcessingResult],
) -> usize {
    processing_results
        .iter()
        .zip(txs)
        .filter_map(|(processing_result, tx)| {
            processing_result
                .processed_transaction()
                .map(|processed_tx| (processed_tx, tx))
        })
        .map(
            |(processed_tx, tx)| match processed_tx.execution_details.status {
                Ok(_) => tx.num_write_locks() as usize,
                Err(_) => processed_tx.loaded_transaction.rollback_accounts.count(),
            },
        )
        .sum()
}

pub fn collect_accounts_to_store<'a, T: SVMMessage>(
    txs: &'a [T],
    processing_results: &'a mut [TransactionProcessingResult],
    durable_nonce: &DurableNonce,
    lamports_per_signature: u64,
) -> (Vec<(&'a Pubkey, &'a AccountSharedData)>, Vec<Option<&'a T>>) {
    let collect_capacity = max_number_of_accounts_to_collect(txs, processing_results);
    let mut accounts = Vec::with_capacity(collect_capacity);
    let mut transactions = Vec::with_capacity(collect_capacity);
    for (processing_result, transaction) in processing_results.iter_mut().zip(txs) {
        let Some(processed_tx) = processing_result.processed_transaction_mut() else {
            // Don't store any accounts if tx wasn't executed
            continue;
        };

        if processed_tx.execution_details.status.is_ok() {
            collect_accounts_for_successful_tx(
                &mut accounts,
                &mut transactions,
                transaction,
                &processed_tx.loaded_transaction.accounts,
            );
        } else {
            collect_accounts_for_failed_tx(
                &mut accounts,
                &mut transactions,
                transaction,
                &mut processed_tx.loaded_transaction.rollback_accounts,
                durable_nonce,
                lamports_per_signature,
            );
        }
    }
    (accounts, transactions)
}

fn collect_accounts_for_successful_tx<'a, T: SVMMessage>(
    collected_accounts: &mut Vec<(&'a Pubkey, &'a AccountSharedData)>,
    collected_account_transactions: &mut Vec<Option<&'a T>>,
    transaction: &'a T,
    transaction_accounts: &'a [TransactionAccount],
) {
    for (_, (address, account)) in (0..transaction.account_keys().len())
        .zip(transaction_accounts)
        .filter(|(i, _)| {
            transaction.is_writable(*i) && {
                // Accounts that are invoked and also not passed as an instruction
                // account to a program don't need to be stored because it's assumed
                // to be impossible for a committable transaction to modify an
                // invoked account if said account isn't passed to some program.
                !transaction.is_invoked(*i) || transaction.is_instruction_account(*i)
            }
        })
    {
        collected_accounts.push((address, account));
        collected_account_transactions.push(Some(transaction));
    }
}

fn collect_accounts_for_failed_tx<'a, T: SVMMessage>(
    collected_accounts: &mut Vec<(&'a Pubkey, &'a AccountSharedData)>,
    collected_account_transactions: &mut Vec<Option<&'a T>>,
    transaction: &'a T,
    rollback_accounts: &'a mut RollbackAccounts,
    durable_nonce: &DurableNonce,
    lamports_per_signature: u64,
) {
    let fee_payer_address = transaction.fee_payer();
    match rollback_accounts {
        RollbackAccounts::FeePayerOnly { fee_payer_account } => {
            collected_accounts.push((fee_payer_address, &*fee_payer_account));
            collected_account_transactions.push(Some(transaction));
        }
        RollbackAccounts::SameNonceAndFeePayer { nonce } => {
            // Since we know we are dealing with a valid nonce account,
            // unwrap is safe here
            nonce
                .try_advance_nonce(*durable_nonce, lamports_per_signature)
                .unwrap();
            collected_accounts.push((nonce.address(), nonce.account()));
            collected_account_transactions.push(Some(transaction));
        }
        RollbackAccounts::SeparateNonceAndFeePayer {
            nonce,
            fee_payer_account,
        } => {
            collected_accounts.push((fee_payer_address, &*fee_payer_account));
            collected_account_transactions.push(Some(transaction));

            // Since we know we are dealing with a valid nonce account,
            // unwrap is safe here
            nonce
                .try_advance_nonce(*durable_nonce, lamports_per_signature)
                .unwrap();
            collected_accounts.push((nonce.address(), nonce.account()));
            collected_account_transactions.push(Some(transaction));
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            account_loader::LoadedTransaction,
            nonce_info::NonceInfo,
            transaction_execution_result::{ExecutedTransaction, TransactionExecutionDetails},
        },
        solana_compute_budget::compute_budget_limits::ComputeBudgetLimits,
        solana_sdk::{
            account::{AccountSharedData, ReadableAccount},
            fee::FeeDetails,
            hash::Hash,
            instruction::{CompiledInstruction, InstructionError},
            message::Message,
            native_loader,
            nonce::{
                state::{Data as NonceData, Versions as NonceVersions},
                State as NonceState,
            },
            nonce_account,
            rent_debits::RentDebits,
            signature::{keypair_from_seed, signers::Signers, Keypair, Signer},
            system_instruction, system_program,
            transaction::{Result, SanitizedTransaction, Transaction, TransactionError},
        },
        std::collections::HashMap,
    };

    fn new_sanitized_tx<T: Signers>(
        from_keypairs: &T,
        message: Message,
        recent_blockhash: Hash,
    ) -> SanitizedTransaction {
        SanitizedTransaction::from_transaction_for_tests(Transaction::new(
            from_keypairs,
            message,
            recent_blockhash,
        ))
    }

    fn new_processing_result(
        status: Result<()>,
        loaded_transaction: LoadedTransaction,
    ) -> TransactionProcessingResult {
        Ok(ExecutedTransaction {
            execution_details: TransactionExecutionDetails {
                status,
                log_messages: None,
                inner_instructions: None,
                return_data: None,
                executed_units: 0,
                accounts_data_len_delta: 0,
            },
            loaded_transaction,
            programs_modified_by_tx: HashMap::new(),
        })
    }

    #[test]
    fn test_collect_accounts_to_store() {
        let keypair0 = Keypair::new();
        let keypair1 = Keypair::new();
        let pubkey = solana_sdk::pubkey::new_rand();
        let account0 = AccountSharedData::new(1, 0, &Pubkey::default());
        let account1 = AccountSharedData::new(2, 0, &Pubkey::default());
        let account2 = AccountSharedData::new(3, 0, &Pubkey::default());

        let instructions = vec![CompiledInstruction::new(2, &(), vec![0, 1])];
        let message = Message::new_with_compiled_instructions(
            1,
            0,
            2,
            vec![keypair0.pubkey(), pubkey, native_loader::id()],
            Hash::default(),
            instructions,
        );
        let transaction_accounts0 = vec![
            (message.account_keys[0], account0),
            (message.account_keys[1], account2.clone()),
        ];
        let tx0 = new_sanitized_tx(&[&keypair0], message, Hash::default());

        let instructions = vec![CompiledInstruction::new(2, &(), vec![0, 1])];
        let message = Message::new_with_compiled_instructions(
            1,
            0,
            2,
            vec![keypair1.pubkey(), pubkey, native_loader::id()],
            Hash::default(),
            instructions,
        );
        let transaction_accounts1 = vec![
            (message.account_keys[0], account1),
            (message.account_keys[1], account2),
        ];
        let tx1 = new_sanitized_tx(&[&keypair1], message, Hash::default());

        let loaded0 = LoadedTransaction {
            accounts: transaction_accounts0,
            program_indices: vec![],
            fee_details: FeeDetails::default(),
            rollback_accounts: RollbackAccounts::default(),
            compute_budget_limits: ComputeBudgetLimits::default(),
            rent: 0,
            rent_debits: RentDebits::default(),
            loaded_accounts_data_size: 0,
        };

        let loaded1 = LoadedTransaction {
            accounts: transaction_accounts1,
            program_indices: vec![],
            fee_details: FeeDetails::default(),
            rollback_accounts: RollbackAccounts::default(),
            compute_budget_limits: ComputeBudgetLimits::default(),
            rent: 0,
            rent_debits: RentDebits::default(),
            loaded_accounts_data_size: 0,
        };

        let txs = vec![tx0.clone(), tx1.clone()];
        let mut processing_results = vec![
            new_processing_result(Ok(()), loaded0),
            new_processing_result(Ok(()), loaded1),
        ];
        let max_collected_accounts = max_number_of_accounts_to_collect(&txs, &processing_results);
        assert_eq!(max_collected_accounts, 2);
        let (collected_accounts, transactions) =
            collect_accounts_to_store(&txs, &mut processing_results, &DurableNonce::default(), 0);
        assert_eq!(collected_accounts.len(), 2);
        assert!(collected_accounts
            .iter()
            .any(|(pubkey, _account)| *pubkey == &keypair0.pubkey()));
        assert!(collected_accounts
            .iter()
            .any(|(pubkey, _account)| *pubkey == &keypair1.pubkey()));

        assert_eq!(transactions.len(), 2);
        assert!(transactions.iter().any(|txn| txn.unwrap().eq(&tx0)));
        assert!(transactions.iter().any(|txn| txn.unwrap().eq(&tx1)));
    }

    #[test]
    fn test_nonced_failure_accounts_rollback_fee_payer_only() {
        let from = keypair_from_seed(&[1; 32]).unwrap();
        let from_address = from.pubkey();
        let to_address = Pubkey::new_unique();
        let from_account_post = AccountSharedData::new(4199, 0, &Pubkey::default());
        let to_account = AccountSharedData::new(2, 0, &Pubkey::default());

        let instructions = vec![system_instruction::transfer(&from_address, &to_address, 42)];
        let message = Message::new(&instructions, Some(&from_address));
        let blockhash = Hash::new_unique();
        let transaction_accounts = vec![
            (message.account_keys[0], from_account_post),
            (message.account_keys[1], to_account),
        ];
        let tx = new_sanitized_tx(&[&from], message, blockhash);

        let from_account_pre = AccountSharedData::new(4242, 0, &Pubkey::default());

        let loaded = LoadedTransaction {
            accounts: transaction_accounts,
            program_indices: vec![],
            fee_details: FeeDetails::default(),
            rollback_accounts: RollbackAccounts::FeePayerOnly {
                fee_payer_account: from_account_pre.clone(),
            },
            compute_budget_limits: ComputeBudgetLimits::default(),
            rent: 0,
            rent_debits: RentDebits::default(),
            loaded_accounts_data_size: 0,
        };

        let txs = vec![tx];
        let mut processing_results = vec![new_processing_result(
            Err(TransactionError::InstructionError(
                1,
                InstructionError::InvalidArgument,
            )),
            loaded,
        )];
        let max_collected_accounts = max_number_of_accounts_to_collect(&txs, &processing_results);
        assert_eq!(max_collected_accounts, 1);
        let durable_nonce = DurableNonce::from_blockhash(&Hash::new_unique());
        let (collected_accounts, _) =
            collect_accounts_to_store(&txs, &mut processing_results, &durable_nonce, 0);
        assert_eq!(collected_accounts.len(), 1);
        assert_eq!(
            collected_accounts
                .iter()
                .find(|(pubkey, _account)| *pubkey == &from_address)
                .map(|(_pubkey, account)| *account)
                .cloned()
                .unwrap(),
            from_account_pre,
        );
    }

    #[test]
    fn test_nonced_failure_accounts_rollback_separate_nonce_and_fee_payer() {
        let nonce_address = Pubkey::new_unique();
        let nonce_authority = keypair_from_seed(&[0; 32]).unwrap();
        let from = keypair_from_seed(&[1; 32]).unwrap();
        let from_address = from.pubkey();
        let to_address = Pubkey::new_unique();
        let durable_nonce = DurableNonce::from_blockhash(&Hash::new_unique());
        let nonce_state = NonceVersions::new(NonceState::Initialized(NonceData::new(
            nonce_authority.pubkey(),
            durable_nonce,
            0,
        )));
        let nonce_account_post =
            AccountSharedData::new_data(43, &nonce_state, &system_program::id()).unwrap();
        let from_account_post = AccountSharedData::new(4199, 0, &Pubkey::default());
        let to_account = AccountSharedData::new(2, 0, &Pubkey::default());
        let nonce_authority_account = AccountSharedData::new(3, 0, &Pubkey::default());
        let recent_blockhashes_sysvar_account = AccountSharedData::new(4, 0, &Pubkey::default());

        let instructions = vec![
            system_instruction::advance_nonce_account(&nonce_address, &nonce_authority.pubkey()),
            system_instruction::transfer(&from_address, &to_address, 42),
        ];
        let message = Message::new(&instructions, Some(&from_address));
        let blockhash = Hash::new_unique();
        let transaction_accounts = vec![
            (message.account_keys[0], from_account_post),
            (message.account_keys[1], nonce_authority_account),
            (message.account_keys[2], nonce_account_post),
            (message.account_keys[3], to_account),
            (message.account_keys[4], recent_blockhashes_sysvar_account),
        ];
        let tx = new_sanitized_tx(&[&nonce_authority, &from], message, blockhash);

        let durable_nonce = DurableNonce::from_blockhash(&Hash::new_unique());
        let nonce_state = NonceVersions::new(NonceState::Initialized(NonceData::new(
            nonce_authority.pubkey(),
            durable_nonce,
            0,
        )));
        let nonce_account_pre =
            AccountSharedData::new_data(42, &nonce_state, &system_program::id()).unwrap();
        let from_account_pre = AccountSharedData::new(4242, 0, &Pubkey::default());

        let nonce = NonceInfo::new(nonce_address, nonce_account_pre.clone());
        let loaded = LoadedTransaction {
            accounts: transaction_accounts,
            program_indices: vec![],
            fee_details: FeeDetails::default(),
            rollback_accounts: RollbackAccounts::SeparateNonceAndFeePayer {
                nonce: nonce.clone(),
                fee_payer_account: from_account_pre.clone(),
            },
            compute_budget_limits: ComputeBudgetLimits::default(),
            rent: 0,
            rent_debits: RentDebits::default(),
            loaded_accounts_data_size: 0,
        };

        let durable_nonce = DurableNonce::from_blockhash(&Hash::new_unique());
        let txs = vec![tx];
        let mut processing_results = vec![new_processing_result(
            Err(TransactionError::InstructionError(
                1,
                InstructionError::InvalidArgument,
            )),
            loaded,
        )];
        let max_collected_accounts = max_number_of_accounts_to_collect(&txs, &processing_results);
        assert_eq!(max_collected_accounts, 2);
        let (collected_accounts, _) =
            collect_accounts_to_store(&txs, &mut processing_results, &durable_nonce, 0);
        assert_eq!(collected_accounts.len(), 2);
        assert_eq!(
            collected_accounts
                .iter()
                .find(|(pubkey, _account)| *pubkey == &from_address)
                .map(|(_pubkey, account)| *account)
                .cloned()
                .unwrap(),
            from_account_pre,
        );
        let collected_nonce_account = collected_accounts
            .iter()
            .find(|(pubkey, _account)| *pubkey == &nonce_address)
            .map(|(_pubkey, account)| *account)
            .cloned()
            .unwrap();
        assert_eq!(
            collected_nonce_account.lamports(),
            nonce_account_pre.lamports(),
        );
        assert!(nonce_account::verify_nonce_account(
            &collected_nonce_account,
            durable_nonce.as_hash()
        )
        .is_some());
    }

    #[test]
    fn test_nonced_failure_accounts_rollback_same_nonce_and_fee_payer() {
        let nonce_authority = keypair_from_seed(&[0; 32]).unwrap();
        let nonce_address = nonce_authority.pubkey();
        let from = keypair_from_seed(&[1; 32]).unwrap();
        let from_address = from.pubkey();
        let to_address = Pubkey::new_unique();
        let durable_nonce = DurableNonce::from_blockhash(&Hash::new_unique());
        let nonce_state = NonceVersions::new(NonceState::Initialized(NonceData::new(
            nonce_authority.pubkey(),
            durable_nonce,
            0,
        )));
        let nonce_account_post =
            AccountSharedData::new_data(43, &nonce_state, &system_program::id()).unwrap();
        let from_account_post = AccountSharedData::new(4200, 0, &Pubkey::default());
        let to_account = AccountSharedData::new(2, 0, &Pubkey::default());
        let nonce_authority_account = AccountSharedData::new(3, 0, &Pubkey::default());
        let recent_blockhashes_sysvar_account = AccountSharedData::new(4, 0, &Pubkey::default());

        let instructions = vec![
            system_instruction::advance_nonce_account(&nonce_address, &nonce_authority.pubkey()),
            system_instruction::transfer(&from_address, &to_address, 42),
        ];
        let message = Message::new(&instructions, Some(&nonce_address));
        let blockhash = Hash::new_unique();
        let transaction_accounts = vec![
            (message.account_keys[0], from_account_post),
            (message.account_keys[1], nonce_authority_account),
            (message.account_keys[2], nonce_account_post),
            (message.account_keys[3], to_account),
            (message.account_keys[4], recent_blockhashes_sysvar_account),
        ];
        let tx = new_sanitized_tx(&[&nonce_authority, &from], message, blockhash);

        let durable_nonce = DurableNonce::from_blockhash(&Hash::new_unique());
        let nonce_state = NonceVersions::new(NonceState::Initialized(NonceData::new(
            nonce_authority.pubkey(),
            durable_nonce,
            0,
        )));
        let nonce_account_pre =
            AccountSharedData::new_data(42, &nonce_state, &system_program::id()).unwrap();

        let nonce = NonceInfo::new(nonce_address, nonce_account_pre.clone());
        let loaded = LoadedTransaction {
            accounts: transaction_accounts,
            program_indices: vec![],
            fee_details: FeeDetails::default(),
            rollback_accounts: RollbackAccounts::SameNonceAndFeePayer {
                nonce: nonce.clone(),
            },
            compute_budget_limits: ComputeBudgetLimits::default(),
            rent: 0,
            rent_debits: RentDebits::default(),
            loaded_accounts_data_size: 0,
        };

        let durable_nonce = DurableNonce::from_blockhash(&Hash::new_unique());
        let txs = vec![tx];
        let mut processing_results = vec![new_processing_result(
            Err(TransactionError::InstructionError(
                1,
                InstructionError::InvalidArgument,
            )),
            loaded,
        )];
        let max_collected_accounts = max_number_of_accounts_to_collect(&txs, &processing_results);
        assert_eq!(max_collected_accounts, 1);
        let (collected_accounts, _) =
            collect_accounts_to_store(&txs, &mut processing_results, &durable_nonce, 0);
        assert_eq!(collected_accounts.len(), 1);
        let collected_nonce_account = collected_accounts
            .iter()
            .find(|(pubkey, _account)| *pubkey == &nonce_address)
            .map(|(_pubkey, account)| *account)
            .cloned()
            .unwrap();
        assert_eq!(
            collected_nonce_account.lamports(),
            nonce_account_pre.lamports()
        );
        assert!(nonce_account::verify_nonce_account(
            &collected_nonce_account,
            durable_nonce.as_hash()
        )
        .is_some());
    }
}

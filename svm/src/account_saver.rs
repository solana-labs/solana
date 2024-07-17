use {
    crate::{
        account_loader::TransactionLoadResult, nonce_info::NonceInfo,
        rollback_accounts::RollbackAccounts, transaction_results::TransactionExecutionResult,
    },
    solana_sdk::{
        account::AccountSharedData,
        account_utils::StateMut,
        message::SanitizedMessage,
        nonce::{
            state::{DurableNonce, Versions as NonceVersions},
            State as NonceState,
        },
        pubkey::Pubkey,
        transaction::SanitizedTransaction,
    },
};

pub fn collect_accounts_to_store<'a>(
    txs: &'a [SanitizedTransaction],
    execution_results: &'a [TransactionExecutionResult],
    load_results: &'a mut [TransactionLoadResult],
    durable_nonce: &DurableNonce,
    lamports_per_signature: u64,
) -> (
    Vec<(&'a Pubkey, &'a AccountSharedData)>,
    Vec<Option<&'a SanitizedTransaction>>,
) {
    let mut accounts = Vec::with_capacity(load_results.len());
    let mut transactions = Vec::with_capacity(load_results.len());
    for (i, (tx_load_result, tx)) in load_results.iter_mut().zip(txs).enumerate() {
        let Ok(loaded_transaction) = tx_load_result else {
            // Don't store any accounts if tx failed to load
            continue;
        };

        let execution_status = match &execution_results[i] {
            TransactionExecutionResult::Executed { details, .. } => &details.status,
            // Don't store any accounts if tx wasn't executed
            TransactionExecutionResult::NotExecuted(_) => continue,
        };

        // Accounts that are invoked and also not passed as an instruction
        // account to a program don't need to be stored because it's assumed
        // to be impossible for a committable transaction to modify an
        // invoked account if said account isn't passed to some program.
        //
        // Note that this assumption might not hold in the future after
        // SIMD-0082 is implemented because we may decide to commit
        // transactions that incorrectly attempt to invoke a fee payer or
        // durable nonce account. If that happens, we would NOT want to use
        // this filter because we always NEED to store those accounts even
        // if they aren't passed to any programs (because they are mutated
        // outside of the VM).
        let is_storable_account = |message: &SanitizedMessage, key_index: usize| -> bool {
            !message.is_invoked(key_index) || message.is_instruction_account(key_index)
        };

        let message = tx.message();
        let rollback_accounts = &loaded_transaction.rollback_accounts;
        let maybe_nonce_address = rollback_accounts.nonce().map(|account| account.address());

        for (i, (address, account)) in (0..message.account_keys().len())
            .zip(loaded_transaction.accounts.iter_mut())
            .filter(|(i, _)| is_storable_account(message, *i))
        {
            if message.is_writable(i) {
                let should_collect_account = match execution_status {
                    Ok(()) => true,
                    Err(_) => {
                        let is_fee_payer = i == 0;
                        let is_nonce_account = Some(&*address) == maybe_nonce_address;
                        post_process_failed_tx(
                            account,
                            is_fee_payer,
                            is_nonce_account,
                            rollback_accounts,
                            durable_nonce,
                            lamports_per_signature,
                        );

                        is_fee_payer || is_nonce_account
                    }
                };

                if should_collect_account {
                    // Add to the accounts to store
                    accounts.push((&*address, &*account));
                    transactions.push(Some(tx));
                }
            }
        }
    }
    (accounts, transactions)
}

fn post_process_failed_tx(
    account: &mut AccountSharedData,
    is_fee_payer: bool,
    is_nonce_account: bool,
    rollback_accounts: &RollbackAccounts,
    &durable_nonce: &DurableNonce,
    lamports_per_signature: u64,
) {
    // For the case of RollbackAccounts::SameNonceAndFeePayer, it's crucial
    // for `is_nonce_account` to be checked earlier than `is_fee_payer`.
    if is_nonce_account {
        if let Some(nonce) = rollback_accounts.nonce() {
            // The transaction failed which would normally drop the account
            // processing changes, since this account is now being included
            // in the accounts written back to the db, roll it back to
            // pre-processing state.
            *account = nonce.account().clone();

            // Advance the stored blockhash to prevent fee theft by someone
            // replaying nonce transactions that have failed with an
            // `InstructionError`.
            //
            // Since we know we are dealing with a valid nonce account,
            // unwrap is safe here
            let nonce_versions = StateMut::<NonceVersions>::state(account).unwrap();
            if let NonceState::Initialized(ref data) = nonce_versions.state() {
                let nonce_state = NonceState::new_initialized(
                    &data.authority,
                    durable_nonce,
                    lamports_per_signature,
                );
                let nonce_versions = NonceVersions::new(nonce_state);
                account.set_state(&nonce_versions).unwrap();
            }
        }
    } else if is_fee_payer {
        *account = rollback_accounts.fee_payer_account().clone();
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            account_loader::LoadedTransaction, nonce_info::NoncePartial,
            transaction_results::TransactionExecutionDetails,
        },
        solana_compute_budget::compute_budget_processor::ComputeBudgetLimits,
        solana_sdk::{
            account::{AccountSharedData, ReadableAccount, WritableAccount},
            fee::FeeDetails,
            hash::Hash,
            instruction::{CompiledInstruction, InstructionError},
            message::Message,
            native_loader,
            nonce::state::Data as NonceData,
            nonce_account,
            rent_debits::RentDebits,
            signature::{keypair_from_seed, signers::Signers, Keypair, Signer},
            system_instruction, system_program,
            transaction::{Result, Transaction, TransactionError},
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

    fn new_execution_result(status: Result<()>) -> TransactionExecutionResult {
        TransactionExecutionResult::Executed {
            details: TransactionExecutionDetails {
                status,
                log_messages: None,
                inner_instructions: None,
                fee_details: FeeDetails::default(),
                return_data: None,
                executed_units: 0,
                accounts_data_len_delta: 0,
            },
            programs_modified_by_tx: HashMap::new(),
        }
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

        let loaded0 = Ok(LoadedTransaction {
            accounts: transaction_accounts0,
            program_indices: vec![],
            fee_details: FeeDetails::default(),
            rollback_accounts: RollbackAccounts::default(),
            compute_budget_limits: ComputeBudgetLimits::default(),
            rent: 0,
            rent_debits: RentDebits::default(),
            loaded_accounts_data_size: 0,
        });

        let loaded1 = Ok(LoadedTransaction {
            accounts: transaction_accounts1,
            program_indices: vec![],
            fee_details: FeeDetails::default(),
            rollback_accounts: RollbackAccounts::default(),
            compute_budget_limits: ComputeBudgetLimits::default(),
            rent: 0,
            rent_debits: RentDebits::default(),
            loaded_accounts_data_size: 0,
        });

        let mut loaded = vec![loaded0, loaded1];

        let txs = vec![tx0.clone(), tx1.clone()];
        let execution_results = vec![new_execution_result(Ok(())); 2];
        let (collected_accounts, transactions) = collect_accounts_to_store(
            &txs,
            &execution_results,
            loaded.as_mut_slice(),
            &DurableNonce::default(),
            0,
        );
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

    fn create_accounts_post_process_failed_tx() -> (
        Pubkey,
        AccountSharedData,
        AccountSharedData,
        DurableNonce,
        u64,
    ) {
        let data = NonceVersions::new(NonceState::Initialized(NonceData::default()));
        let account = AccountSharedData::new_data(42, &data, &system_program::id()).unwrap();
        let mut pre_account = account.clone();
        pre_account.set_lamports(43);
        let durable_nonce = DurableNonce::from_blockhash(&Hash::new(&[1u8; 32]));
        (Pubkey::default(), pre_account, account, durable_nonce, 1234)
    }

    fn run_post_process_failed_tx_test(
        account: &mut AccountSharedData,
        is_fee_payer: bool,
        is_nonce_account: bool,
        rollback_accounts: &RollbackAccounts,
        durable_nonce: &DurableNonce,
        lamports_per_signature: u64,
        expect_account: &AccountSharedData,
    ) -> bool {
        // Verify expect_account's relationship
        if !is_fee_payer {
            if is_nonce_account {
                assert_ne!(expect_account, rollback_accounts.nonce().unwrap().account());
            } else {
                assert_eq!(expect_account, account);
            }
        }

        post_process_failed_tx(
            account,
            is_fee_payer,
            is_nonce_account,
            rollback_accounts,
            durable_nonce,
            lamports_per_signature,
        );
        assert_eq!(expect_account, account);
        expect_account == account
    }

    #[test]
    fn test_post_process_failed_tx_expected() {
        let (pre_account_address, pre_account, mut post_account, blockhash, lamports_per_signature) =
            create_accounts_post_process_failed_tx();
        let rollback_accounts = RollbackAccounts::SameNonceAndFeePayer {
            nonce: NoncePartial::new(pre_account_address, pre_account.clone()),
        };

        let mut expect_account = pre_account;
        expect_account
            .set_state(&NonceVersions::new(NonceState::Initialized(
                NonceData::new(Pubkey::default(), blockhash, lamports_per_signature),
            )))
            .unwrap();

        assert!(run_post_process_failed_tx_test(
            &mut post_account,
            false, // is_fee_payer
            true,  // is_nonce_account
            &rollback_accounts,
            &blockhash,
            lamports_per_signature,
            &expect_account,
        ));
    }

    #[test]
    fn test_post_process_failed_tx_not_nonce_address() {
        let (pre_account_address, pre_account, mut post_account, blockhash, lamports_per_signature) =
            create_accounts_post_process_failed_tx();

        let rollback_accounts = RollbackAccounts::SameNonceAndFeePayer {
            nonce: NoncePartial::new(pre_account_address, pre_account.clone()),
        };

        let expect_account = post_account.clone();
        assert!(run_post_process_failed_tx_test(
            &mut post_account,
            false, // is_fee_payer
            false, // is_nonce_account
            &rollback_accounts,
            &blockhash,
            lamports_per_signature,
            &expect_account,
        ));
    }

    #[test]
    fn test_rollback_nonce_fee_payer() {
        let nonce_account = AccountSharedData::new_data(1, &(), &system_program::id()).unwrap();
        let pre_fee_payer_account =
            AccountSharedData::new_data(42, &(), &system_program::id()).unwrap();
        let post_fee_payer_account =
            AccountSharedData::new_data(84, &[1, 2, 3, 4], &system_program::id()).unwrap();
        let rollback_accounts = RollbackAccounts::SeparateNonceAndFeePayer {
            nonce: NoncePartial::new(Pubkey::new_unique(), nonce_account),
            fee_payer_account: pre_fee_payer_account.clone(),
        };

        assert!(run_post_process_failed_tx_test(
            &mut post_fee_payer_account.clone(),
            false, // is_fee_payer
            false, // is_nonce_account
            &rollback_accounts,
            &DurableNonce::default(),
            1,
            &post_fee_payer_account,
        ));

        assert!(run_post_process_failed_tx_test(
            &mut post_fee_payer_account.clone(),
            true,  // is_fee_payer
            false, // is_nonce_account
            &rollback_accounts,
            &DurableNonce::default(),
            1,
            &pre_fee_payer_account,
        ));
    }

    #[test]
    fn test_nonced_failure_accounts_rollback_from_pays() {
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

        let nonce = NoncePartial::new(nonce_address, nonce_account_pre.clone());
        let loaded = Ok(LoadedTransaction {
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
        });

        let mut loaded = vec![loaded];

        let durable_nonce = DurableNonce::from_blockhash(&Hash::new_unique());
        let txs = vec![tx];
        let execution_results = vec![new_execution_result(Err(
            TransactionError::InstructionError(1, InstructionError::InvalidArgument),
        ))];
        let (collected_accounts, _) = collect_accounts_to_store(
            &txs,
            &execution_results,
            loaded.as_mut_slice(),
            &durable_nonce,
            0,
        );
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
    fn test_nonced_failure_accounts_rollback_nonce_pays() {
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

        let nonce = NoncePartial::new(nonce_address, nonce_account_pre.clone());
        let loaded = Ok(LoadedTransaction {
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
        });

        let mut loaded = vec![loaded];

        let durable_nonce = DurableNonce::from_blockhash(&Hash::new_unique());
        let txs = vec![tx];
        let execution_results = vec![new_execution_result(Err(
            TransactionError::InstructionError(1, InstructionError::InvalidArgument),
        ))];
        let (collected_accounts, _) = collect_accounts_to_store(
            &txs,
            &execution_results,
            loaded.as_mut_slice(),
            &durable_nonce,
            0,
        );
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

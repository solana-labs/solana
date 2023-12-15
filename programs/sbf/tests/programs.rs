#![cfg(any(feature = "sbf_c", feature = "sbf_rust"))]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::needless_range_loop)]
#![allow(clippy::redundant_clone)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::cmp_owned)]
#![allow(clippy::needless_collect)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::unnecessary_cast)]
#![allow(clippy::uninlined_format_args)]

#[cfg(feature = "sbf_rust")]
use {
    itertools::izip,
    solana_account_decoder::parse_bpf_loader::{
        parse_bpf_upgradeable_loader, BpfUpgradeableLoaderAccountType,
    },
    solana_accounts_db::transaction_results::{
        DurableNonceFee, InnerInstruction, TransactionExecutionDetails, TransactionExecutionResult,
        TransactionResults,
    },
    solana_ledger::token_balances::collect_token_balances,
    solana_program_runtime::{compute_budget::ComputeBudget, timings::ExecuteTimings},
    solana_rbpf::vm::ContextObject,
    solana_runtime::{
        bank::TransactionBalancesSet,
        loader_utils::{
            create_program, load_and_finalize_program, load_program, load_program_from_file,
            load_upgradeable_buffer, load_upgradeable_program, set_upgrade_authority,
            upgrade_program,
        },
    },
    solana_sbf_rust_invoke::instructions::*,
    solana_sbf_rust_realloc::instructions::*,
    solana_sbf_rust_realloc_invoke::instructions::*,
    solana_sdk::{
        account::{ReadableAccount, WritableAccount},
        account_utils::StateMut,
        bpf_loader_upgradeable,
        clock::MAX_PROCESSING_AGE,
        compute_budget::ComputeBudgetInstruction,
        entrypoint::MAX_PERMITTED_DATA_INCREASE,
        feature_set::{self, remove_deprecated_request_unit_ix, FeatureSet},
        fee::FeeStructure,
        loader_instruction,
        message::{v0::LoadedAddresses, SanitizedMessage},
        signature::keypair_from_seed,
        stake,
        system_instruction::{self, MAX_PERMITTED_DATA_LENGTH},
        sysvar::{self, clock, rent},
        transaction::VersionedTransaction,
    },
    solana_transaction_status::{
        ConfirmedTransactionWithStatusMeta, InnerInstructions, TransactionStatusMeta,
        TransactionWithStatusMeta, VersionedTransactionWithStatusMeta,
    },
    std::collections::HashMap,
};
use {
    solana_program_runtime::invoke_context::mock_process_instruction,
    solana_runtime::{
        bank::Bank,
        bank_client::BankClient,
        genesis_utils::{
            bootstrap_validator_stake_lamports, create_genesis_config,
            create_genesis_config_with_leader_ex, GenesisConfigInfo,
        },
    },
    solana_sdk::{
        account::AccountSharedData,
        bpf_loader, bpf_loader_deprecated,
        client::SyncClient,
        clock::UnixTimestamp,
        fee_calculator::FeeRateGovernor,
        genesis_config::ClusterType,
        hash::Hash,
        instruction::{AccountMeta, Instruction, InstructionError},
        message::Message,
        pubkey::Pubkey,
        rent::Rent,
        signature::{Keypair, Signer},
        system_program,
        transaction::{SanitizedTransaction, Transaction, TransactionError},
    },
    std::{cell::RefCell, str::FromStr, sync::Arc, time::Duration},
};

#[cfg(feature = "sbf_rust")]
fn process_transaction_and_record_inner(
    bank: &Bank,
    tx: Transaction,
) -> (
    Result<(), TransactionError>,
    Vec<Vec<InnerInstruction>>,
    Vec<String>,
) {
    let signature = tx.signatures.get(0).unwrap().clone();
    let txs = vec![tx];
    let tx_batch = bank.prepare_batch_for_tests(txs);
    let mut results = bank
        .load_execute_and_commit_transactions(
            &tx_batch,
            MAX_PROCESSING_AGE,
            false,
            true,
            true,
            false,
            &mut ExecuteTimings::default(),
            None,
        )
        .0;
    let result = results
        .fee_collection_results
        .swap_remove(0)
        .and_then(|_| bank.get_signature_status(&signature).unwrap());
    let execution_details = results
        .execution_results
        .swap_remove(0)
        .details()
        .expect("tx should be executed")
        .clone();
    let inner_instructions = execution_details
        .inner_instructions
        .expect("cpi recording should be enabled");
    let log_messages = execution_details
        .log_messages
        .expect("log recording should be enabled");
    (result, inner_instructions, log_messages)
}

#[cfg(feature = "sbf_rust")]
fn execute_transactions(
    bank: &Bank,
    txs: Vec<Transaction>,
) -> Vec<Result<ConfirmedTransactionWithStatusMeta, TransactionError>> {
    let batch = bank.prepare_batch_for_tests(txs.clone());
    let mut timings = ExecuteTimings::default();
    let mut mint_decimals = HashMap::new();
    let tx_pre_token_balances = collect_token_balances(&bank, &batch, &mut mint_decimals);
    let (
        TransactionResults {
            execution_results, ..
        },
        TransactionBalancesSet {
            pre_balances,
            post_balances,
            ..
        },
    ) = bank.load_execute_and_commit_transactions(
        &batch,
        std::usize::MAX,
        true,
        true,
        true,
        true,
        &mut timings,
        None,
    );
    let tx_post_token_balances = collect_token_balances(&bank, &batch, &mut mint_decimals);

    izip!(
        txs.iter(),
        execution_results.into_iter(),
        pre_balances.into_iter(),
        post_balances.into_iter(),
        tx_pre_token_balances.into_iter(),
        tx_post_token_balances.into_iter(),
    )
    .map(
        |(
            tx,
            execution_result,
            pre_balances,
            post_balances,
            pre_token_balances,
            post_token_balances,
        )| {
            match execution_result {
                TransactionExecutionResult::Executed { details, .. } => {
                    let TransactionExecutionDetails {
                        status,
                        log_messages,
                        inner_instructions,
                        durable_nonce_fee,
                        return_data,
                        executed_units,
                        ..
                    } = details;

                    let lamports_per_signature = match durable_nonce_fee {
                        Some(DurableNonceFee::Valid(lamports_per_signature)) => {
                            Some(lamports_per_signature)
                        }
                        Some(DurableNonceFee::Invalid) => None,
                        None => bank.get_lamports_per_signature_for_blockhash(
                            &tx.message().recent_blockhash,
                        ),
                    }
                    .expect("lamports_per_signature must be available");
                    let fee = bank.get_fee_for_message_with_lamports_per_signature(
                        &SanitizedMessage::try_from(tx.message().clone()).unwrap(),
                        lamports_per_signature,
                    );

                    let inner_instructions = inner_instructions.map(|inner_instructions| {
                        inner_instructions
                            .into_iter()
                            .enumerate()
                            .map(|(index, instructions)| InnerInstructions {
                                index: index as u8,
                                instructions: instructions
                                    .into_iter()
                                    .map(|ix| solana_transaction_status::InnerInstruction {
                                        instruction: ix.instruction,
                                        stack_height: Some(u32::from(ix.stack_height)),
                                    })
                                    .collect(),
                            })
                            .filter(|i| !i.instructions.is_empty())
                            .collect()
                    });

                    let tx_status_meta = TransactionStatusMeta {
                        status,
                        fee,
                        pre_balances,
                        post_balances,
                        pre_token_balances: Some(pre_token_balances),
                        post_token_balances: Some(post_token_balances),
                        inner_instructions,
                        log_messages,
                        rewards: None,
                        loaded_addresses: LoadedAddresses::default(),
                        return_data,
                        compute_units_consumed: Some(executed_units),
                    };

                    Ok(ConfirmedTransactionWithStatusMeta {
                        slot: bank.slot(),
                        tx_with_meta: TransactionWithStatusMeta::Complete(
                            VersionedTransactionWithStatusMeta {
                                transaction: VersionedTransaction::from(tx.clone()),
                                meta: tx_status_meta,
                            },
                        ),
                        block_time: None,
                    })
                }
                TransactionExecutionResult::NotExecuted(err) => Err(err.clone()),
            }
        },
    )
    .collect()
}

fn load_program_and_advance_slot(
    bank_client: &mut BankClient,
    loader_id: &Pubkey,
    payer_keypair: &Keypair,
    name: &str,
) -> (Arc<Bank>, Pubkey) {
    let pubkey = load_program(bank_client, loader_id, payer_keypair, name);
    (
        bank_client
            .advance_slot(1, &Pubkey::default())
            .expect("Failed to advance the slot"),
        pubkey,
    )
}

#[test]
#[cfg(any(feature = "sbf_c", feature = "sbf_rust"))]
fn test_program_sbf_sanity() {
    solana_logger::setup();

    let mut programs = Vec::new();
    #[cfg(feature = "sbf_c")]
    {
        programs.extend_from_slice(&[
            ("alloc", true),
            ("alt_bn128", true),
            ("alt_bn128_compression", true),
            ("sbf_to_sbf", true),
            ("float", true),
            ("multiple_static", true),
            ("noop", true),
            ("noop++", true),
            ("panic", false),
            ("poseidon", true),
            ("relative_call", true),
            ("return_data", true),
            ("sanity", true),
            ("sanity++", true),
            ("secp256k1_recover", true),
            ("sha", true),
            ("stdlib", true),
            ("struct_pass", true),
            ("struct_ret", true),
        ]);
    }
    #[cfg(feature = "sbf_rust")]
    {
        programs.extend_from_slice(&[
            ("solana_sbf_rust_128bit", true),
            ("solana_sbf_rust_alloc", true),
            ("solana_sbf_rust_alt_bn128", true),
            ("solana_sbf_rust_alt_bn128_compression", true),
            ("solana_sbf_rust_curve25519", true),
            ("solana_sbf_rust_custom_heap", true),
            ("solana_sbf_rust_dep_crate", true),
            ("solana_sbf_rust_external_spend", false),
            ("solana_sbf_rust_iter", true),
            ("solana_sbf_rust_many_args", true),
            ("solana_sbf_rust_membuiltins", true),
            ("solana_sbf_rust_noop", true),
            ("solana_sbf_rust_panic", false),
            ("solana_sbf_rust_param_passing", true),
            ("solana_sbf_rust_poseidon", true),
            ("solana_sbf_rust_rand", true),
            ("solana_sbf_rust_sanity", true),
            ("solana_sbf_rust_secp256k1_recover", true),
            ("solana_sbf_rust_sha", true),
        ]);
    }

    for program in programs.iter() {
        println!("Test program: {:?}", program.0);

        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(50);

        let bank = Bank::new_for_tests(&genesis_config);
        let mut bank_client = BankClient::new(bank);

        // Call user program
        let (_, program_id) = load_program_and_advance_slot(
            &mut bank_client,
            &bpf_loader::id(),
            &mint_keypair,
            program.0,
        );
        let account_metas = vec![
            AccountMeta::new(mint_keypair.pubkey(), true),
            AccountMeta::new(Keypair::new().pubkey(), false),
        ];
        let instruction = Instruction::new_with_bytes(program_id, &[1], account_metas);
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        if program.1 {
            assert!(result.is_ok(), "{result:?}");
        } else {
            assert!(result.is_err(), "{result:?}");
        }
    }
}

#[test]
#[cfg(any(feature = "sbf_c", feature = "sbf_rust"))]
fn test_program_sbf_loader_deprecated() {
    solana_logger::setup();

    let mut programs = Vec::new();
    #[cfg(feature = "sbf_c")]
    {
        programs.extend_from_slice(&[("deprecated_loader")]);
    }
    #[cfg(feature = "sbf_rust")]
    {
        programs.extend_from_slice(&[("solana_sbf_rust_deprecated_loader")]);
    }

    for program in programs.iter() {
        println!("Test program: {:?}", program);

        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(50);
        genesis_config
            .accounts
            .remove(&solana_sdk::feature_set::disable_deploy_of_alloc_free_syscall::id())
            .unwrap();
        let bank = Bank::new_for_tests(&genesis_config);
        let program_id = create_program(&bank, &bpf_loader_deprecated::id(), program);

        let mut bank_client = BankClient::new(bank);
        bank_client
            .advance_slot(1, &Pubkey::default())
            .expect("Failed to advance the slot");
        let account_metas = vec![AccountMeta::new(mint_keypair.pubkey(), true)];
        let instruction = Instruction::new_with_bytes(program_id, &[255], account_metas);
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        assert!(result.is_ok());
    }
}

#[test]
#[cfg(feature = "sbf_rust")]
fn test_sol_alloc_free_no_longer_deployable() {
    solana_logger::setup();

    let program_keypair = Keypair::new();
    let program_address = program_keypair.pubkey();

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50);
    let mut bank = Bank::new_for_tests(&genesis_config);

    // Populate loader account with elf that depends on _sol_alloc_free syscall
    let elf = load_program_from_file("solana_sbf_rust_deprecated_loader");
    let mut program_account = AccountSharedData::new(1, elf.len(), &bpf_loader::id());
    program_account
        .data_as_mut_slice()
        .get_mut(..)
        .unwrap()
        .copy_from_slice(&elf);
    bank.store_account(&program_address, &program_account);

    let finalize_tx = Transaction::new(
        &[&mint_keypair, &program_keypair],
        Message::new(
            &[loader_instruction::finalize(
                &program_keypair.pubkey(),
                &bpf_loader::id(),
            )],
            Some(&mint_keypair.pubkey()),
        ),
        bank.last_blockhash(),
    );

    let invoke_tx = Transaction::new(
        &[&mint_keypair],
        Message::new(
            &[Instruction::new_with_bytes(
                program_address,
                &[255],
                vec![AccountMeta::new(mint_keypair.pubkey(), true)],
            )],
            Some(&mint_keypair.pubkey()),
        ),
        bank.last_blockhash(),
    );

    // Try and deploy a program that depends on _sol_alloc_free
    assert_eq!(
        bank.process_transaction(&finalize_tx).unwrap_err(),
        TransactionError::InstructionError(0, InstructionError::InvalidAccountData)
    );

    // Enable _sol_alloc_free syscall
    bank.deactivate_feature(&solana_sdk::feature_set::disable_deploy_of_alloc_free_syscall::id());
    bank.clear_signatures();
    bank.clear_program_cache();

    // Try and finalize the program now that sol_alloc_free is re-enabled
    assert!(bank.process_transaction(&finalize_tx).is_ok());
    let new_slot = bank.slot() + 1;
    let mut bank = Bank::new_from_parent(Arc::new(bank), &Pubkey::default(), new_slot);

    // invoke the program
    assert!(bank.process_transaction(&invoke_tx).is_ok());

    // disable _sol_alloc_free
    bank.activate_feature(&solana_sdk::feature_set::disable_deploy_of_alloc_free_syscall::id());
    bank.clear_signatures();

    // invoke should still succeed because cached
    assert!(bank.process_transaction(&invoke_tx).is_ok());

    bank.clear_signatures();
    bank.clear_program_cache();

    // invoke should still succeed on execute because the program is already deployed
    assert!(bank.process_transaction(&invoke_tx).is_ok());
}

#[test]
#[cfg(feature = "sbf_rust")]
fn test_program_sbf_duplicate_accounts() {
    solana_logger::setup();

    let mut programs = Vec::new();
    #[cfg(feature = "sbf_c")]
    {
        programs.extend_from_slice(&[("dup_accounts")]);
    }
    #[cfg(feature = "sbf_rust")]
    {
        programs.extend_from_slice(&[("solana_sbf_rust_dup_accounts")]);
    }

    for program in programs.iter() {
        println!("Test program: {:?}", program);

        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(50);
        let bank = Bank::new_for_tests(&genesis_config);
        let bank = Arc::new(bank);
        let mut bank_client = BankClient::new_shared(bank.clone());
        let (bank, program_id) = load_program_and_advance_slot(
            &mut bank_client,
            &bpf_loader::id(),
            &mint_keypair,
            program,
        );
        let payee_account = AccountSharedData::new(10, 1, &program_id);
        let payee_pubkey = Pubkey::new_unique();
        bank.store_account(&payee_pubkey, &payee_account);
        let account = AccountSharedData::new(10, 1, &program_id);

        let pubkey = Pubkey::new_unique();
        let account_metas = vec![
            AccountMeta::new(mint_keypair.pubkey(), true),
            AccountMeta::new(payee_pubkey, false),
            AccountMeta::new(pubkey, false),
            AccountMeta::new(pubkey, false),
        ];

        bank.store_account(&pubkey, &account);
        let instruction = Instruction::new_with_bytes(program_id, &[1], account_metas.clone());
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        let data = bank_client.get_account_data(&pubkey).unwrap().unwrap();
        assert!(result.is_ok());
        assert_eq!(data[0], 1);

        bank.store_account(&pubkey, &account);
        let instruction = Instruction::new_with_bytes(program_id, &[2], account_metas.clone());
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        let data = bank_client.get_account_data(&pubkey).unwrap().unwrap();
        assert!(result.is_ok());
        assert_eq!(data[0], 2);

        bank.store_account(&pubkey, &account);
        let instruction = Instruction::new_with_bytes(program_id, &[3], account_metas.clone());
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        let data = bank_client.get_account_data(&pubkey).unwrap().unwrap();
        assert!(result.is_ok());
        assert_eq!(data[0], 3);

        bank.store_account(&pubkey, &account);
        let instruction = Instruction::new_with_bytes(program_id, &[4], account_metas.clone());
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        let lamports = bank_client.get_balance(&pubkey).unwrap();
        assert!(result.is_ok());
        assert_eq!(lamports, 11);

        bank.store_account(&pubkey, &account);
        let instruction = Instruction::new_with_bytes(program_id, &[5], account_metas.clone());
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        let lamports = bank_client.get_balance(&pubkey).unwrap();
        assert!(result.is_ok());
        assert_eq!(lamports, 12);

        bank.store_account(&pubkey, &account);
        let instruction = Instruction::new_with_bytes(program_id, &[6], account_metas.clone());
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        let lamports = bank_client.get_balance(&pubkey).unwrap();
        assert!(result.is_ok());
        assert_eq!(lamports, 13);

        let keypair = Keypair::new();
        let pubkey = keypair.pubkey();
        let account_metas = vec![
            AccountMeta::new(mint_keypair.pubkey(), true),
            AccountMeta::new(payee_pubkey, false),
            AccountMeta::new(pubkey, false),
            AccountMeta::new_readonly(pubkey, true),
            AccountMeta::new_readonly(program_id, false),
        ];
        bank.store_account(&pubkey, &account);
        let instruction = Instruction::new_with_bytes(program_id, &[7], account_metas.clone());
        let message = Message::new(&[instruction], Some(&mint_keypair.pubkey()));
        let result = bank_client.send_and_confirm_message(&[&mint_keypair, &keypair], message);
        assert!(result.is_ok());
    }
}

#[test]
#[cfg(feature = "sbf_rust")]
fn test_program_sbf_error_handling() {
    solana_logger::setup();

    let mut programs = Vec::new();
    #[cfg(feature = "sbf_c")]
    {
        programs.extend_from_slice(&[("error_handling")]);
    }
    #[cfg(feature = "sbf_rust")]
    {
        programs.extend_from_slice(&[("solana_sbf_rust_error_handling")]);
    }

    for program in programs.iter() {
        println!("Test program: {:?}", program);

        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(50);
        let bank = Bank::new_for_tests(&genesis_config);
        let mut bank_client = BankClient::new(bank);
        let (_, program_id) = load_program_and_advance_slot(
            &mut bank_client,
            &bpf_loader::id(),
            &mint_keypair,
            program,
        );
        let account_metas = vec![AccountMeta::new(mint_keypair.pubkey(), true)];

        let instruction = Instruction::new_with_bytes(program_id, &[1], account_metas.clone());
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        assert!(result.is_ok());

        let instruction = Instruction::new_with_bytes(program_id, &[2], account_metas.clone());
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        assert_eq!(
            result.unwrap_err().unwrap(),
            TransactionError::InstructionError(0, InstructionError::InvalidAccountData)
        );

        let instruction = Instruction::new_with_bytes(program_id, &[3], account_metas.clone());
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        assert_eq!(
            result.unwrap_err().unwrap(),
            TransactionError::InstructionError(0, InstructionError::Custom(0))
        );

        let instruction = Instruction::new_with_bytes(program_id, &[4], account_metas.clone());
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        assert_eq!(
            result.unwrap_err().unwrap(),
            TransactionError::InstructionError(0, InstructionError::Custom(42))
        );

        let instruction = Instruction::new_with_bytes(program_id, &[5], account_metas.clone());
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        let result = result.unwrap_err().unwrap();
        if TransactionError::InstructionError(0, InstructionError::InvalidInstructionData) != result
        {
            assert_eq!(
                result,
                TransactionError::InstructionError(0, InstructionError::InvalidError)
            );
        }

        let instruction = Instruction::new_with_bytes(program_id, &[6], account_metas.clone());
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        let result = result.unwrap_err().unwrap();
        if TransactionError::InstructionError(0, InstructionError::InvalidInstructionData) != result
        {
            assert_eq!(
                result,
                TransactionError::InstructionError(0, InstructionError::InvalidError)
            );
        }

        let instruction = Instruction::new_with_bytes(program_id, &[7], account_metas.clone());
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        let result = result.unwrap_err().unwrap();
        if TransactionError::InstructionError(0, InstructionError::InvalidInstructionData) != result
        {
            assert_eq!(
                result,
                TransactionError::InstructionError(0, InstructionError::AccountBorrowFailed)
            );
        }

        let instruction = Instruction::new_with_bytes(program_id, &[8], account_metas.clone());
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        assert_eq!(
            result.unwrap_err().unwrap(),
            TransactionError::InstructionError(0, InstructionError::InvalidInstructionData)
        );

        let instruction = Instruction::new_with_bytes(program_id, &[9], account_metas.clone());
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        assert_eq!(
            result.unwrap_err().unwrap(),
            TransactionError::InstructionError(0, InstructionError::MaxSeedLengthExceeded)
        );
    }
}

#[test]
#[cfg(any(feature = "sbf_c", feature = "sbf_rust"))]
fn test_return_data_and_log_data_syscall() {
    solana_logger::setup();

    let mut programs = Vec::new();
    #[cfg(feature = "sbf_c")]
    {
        programs.extend_from_slice(&[("log_data")]);
    }
    #[cfg(feature = "sbf_rust")]
    {
        programs.extend_from_slice(&[("solana_sbf_rust_log_data")]);
    }

    for program in programs.iter() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(50);
        let bank = Bank::new_for_tests(&genesis_config);
        let bank = Arc::new(bank);
        let mut bank_client = BankClient::new_shared(bank.clone());

        let (bank, program_id) = load_program_and_advance_slot(
            &mut bank_client,
            &bpf_loader::id(),
            &mint_keypair,
            program,
        );

        bank.freeze();

        let account_metas = vec![AccountMeta::new(mint_keypair.pubkey(), true)];
        let instruction =
            Instruction::new_with_bytes(program_id, &[1, 2, 3, 0, 4, 5, 6], account_metas);

        let blockhash = bank.last_blockhash();
        let message = Message::new(&[instruction], Some(&mint_keypair.pubkey()));
        let transaction = Transaction::new(&[&mint_keypair], message, blockhash);
        let sanitized_tx = SanitizedTransaction::from_transaction_for_tests(transaction);

        let result = bank.simulate_transaction(sanitized_tx);

        assert!(result.result.is_ok());

        assert_eq!(result.logs[1], "Program data: AQID BAUG");

        assert_eq!(
            result.logs[3],
            format!("Program return: {} CAFE", program_id)
        );
    }
}

#[test]
#[cfg(feature = "sbf_rust")]
fn test_program_sbf_invoke_sanity() {
    solana_logger::setup();

    #[allow(dead_code)]
    #[derive(Debug)]
    enum Languages {
        C,
        Rust,
    }
    let mut programs = Vec::new();
    #[cfg(feature = "sbf_c")]
    {
        programs.push((Languages::C, "invoke", "invoked", "noop"));
    }
    #[cfg(feature = "sbf_rust")]
    {
        programs.push((
            Languages::Rust,
            "solana_sbf_rust_invoke",
            "solana_sbf_rust_invoked",
            "solana_sbf_rust_noop",
        ));
    }
    for program in programs.iter() {
        println!("Test program: {:?}", program);

        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(50);
        let bank = Bank::new_for_tests(&genesis_config);
        let bank = Arc::new(bank);
        let mut bank_client = BankClient::new_shared(bank.clone());

        let invoke_program_id =
            load_program(&bank_client, &bpf_loader::id(), &mint_keypair, program.1);
        let invoked_program_id =
            load_program(&bank_client, &bpf_loader::id(), &mint_keypair, program.2);
        let (bank, noop_program_id) = load_program_and_advance_slot(
            &mut bank_client,
            &bpf_loader::id(),
            &mint_keypair,
            program.3,
        );

        let argument_keypair = Keypair::new();
        let account = AccountSharedData::new(42, 100, &invoke_program_id);
        bank.store_account(&argument_keypair.pubkey(), &account);

        let invoked_argument_keypair = Keypair::new();
        let account = AccountSharedData::new(10, 10, &invoked_program_id);
        bank.store_account(&invoked_argument_keypair.pubkey(), &account);

        let from_keypair = Keypair::new();
        let account = AccountSharedData::new(84, 0, &system_program::id());
        bank.store_account(&from_keypair.pubkey(), &account);

        let (derived_key1, bump_seed1) =
            Pubkey::find_program_address(&[b"You pass butter"], &invoke_program_id);
        let (derived_key2, bump_seed2) =
            Pubkey::find_program_address(&[b"Lil'", b"Bits"], &invoked_program_id);
        let (derived_key3, bump_seed3) =
            Pubkey::find_program_address(&[derived_key2.as_ref()], &invoked_program_id);

        let mint_pubkey = mint_keypair.pubkey();
        let account_metas = vec![
            AccountMeta::new(mint_pubkey, true),
            AccountMeta::new(argument_keypair.pubkey(), true),
            AccountMeta::new_readonly(invoked_program_id, false),
            AccountMeta::new(invoked_argument_keypair.pubkey(), true),
            AccountMeta::new_readonly(invoked_program_id, false),
            AccountMeta::new(argument_keypair.pubkey(), true),
            AccountMeta::new(derived_key1, false),
            AccountMeta::new(derived_key2, false),
            AccountMeta::new_readonly(derived_key3, false),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new(from_keypair.pubkey(), true),
            AccountMeta::new_readonly(solana_sdk::ed25519_program::id(), false),
            AccountMeta::new_readonly(invoke_program_id, false),
        ];

        // success cases

        let instruction = Instruction::new_with_bytes(
            invoke_program_id,
            &[TEST_SUCCESS, bump_seed1, bump_seed2, bump_seed3],
            account_metas.clone(),
        );
        let noop_instruction = Instruction::new_with_bytes(noop_program_id, &[], vec![]);
        let message = Message::new(&[instruction, noop_instruction], Some(&mint_pubkey));
        let tx = Transaction::new(
            &[
                &mint_keypair,
                &argument_keypair,
                &invoked_argument_keypair,
                &from_keypair,
            ],
            message.clone(),
            bank.last_blockhash(),
        );
        let (result, inner_instructions, _log_messages) =
            process_transaction_and_record_inner(&bank, tx);
        assert_eq!(result, Ok(()));

        let invoked_programs: Vec<Pubkey> = inner_instructions[0]
            .iter()
            .map(|ix| &message.account_keys[ix.instruction.program_id_index as usize])
            .cloned()
            .collect();
        let expected_invoked_programs = match program.0 {
            Languages::C => vec![
                system_program::id(),
                system_program::id(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
            ],
            Languages::Rust => vec![
                system_program::id(),
                system_program::id(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
                system_program::id(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
            ],
        };
        assert_eq!(invoked_programs.len(), expected_invoked_programs.len());
        assert_eq!(invoked_programs, expected_invoked_programs);
        let no_invoked_programs: Vec<Pubkey> = inner_instructions[1]
            .iter()
            .map(|ix| &message.account_keys[ix.instruction.program_id_index as usize])
            .cloned()
            .collect();
        assert_eq!(no_invoked_programs.len(), 0);

        // failure cases

        let do_invoke_failure_test_local =
            |test: u8,
             expected_error: TransactionError,
             expected_invoked_programs: &[Pubkey],
             expected_log_messages: Option<Vec<String>>| {
                println!("Running failure test #{:?}", test);
                let instruction_data = &[test, bump_seed1, bump_seed2, bump_seed3];
                let signers = vec![
                    &mint_keypair,
                    &argument_keypair,
                    &invoked_argument_keypair,
                    &from_keypair,
                ];
                let instruction = Instruction::new_with_bytes(
                    invoke_program_id,
                    instruction_data,
                    account_metas.clone(),
                );
                let message = Message::new(&[instruction], Some(&mint_pubkey));
                let tx = Transaction::new(&signers, message.clone(), bank.last_blockhash());
                let (result, inner_instructions, log_messages) =
                    process_transaction_and_record_inner(&bank, tx);
                let invoked_programs: Vec<Pubkey> = inner_instructions[0]
                    .iter()
                    .map(|ix| &message.account_keys[ix.instruction.program_id_index as usize])
                    .cloned()
                    .collect();
                assert_eq!(result, Err(expected_error));
                assert_eq!(invoked_programs, expected_invoked_programs);
                if let Some(expected_log_messages) = expected_log_messages {
                    assert_eq!(log_messages.len(), expected_log_messages.len());
                    expected_log_messages
                        .into_iter()
                        .zip(log_messages)
                        .for_each(|(expected_log_message, log_message)| {
                            if expected_log_message != String::from("skip") {
                                assert_eq!(log_message, expected_log_message);
                            }
                        });
                }
            };

        let program_lang = match program.0 {
            Languages::Rust => "Rust",
            Languages::C => "C",
        };

        do_invoke_failure_test_local(
            TEST_PRIVILEGE_ESCALATION_SIGNER,
            TransactionError::InstructionError(0, InstructionError::PrivilegeEscalation),
            &[invoked_program_id.clone()],
            None,
        );

        do_invoke_failure_test_local(
            TEST_PRIVILEGE_ESCALATION_WRITABLE,
            TransactionError::InstructionError(0, InstructionError::PrivilegeEscalation),
            &[invoked_program_id.clone()],
            None,
        );

        do_invoke_failure_test_local(
            TEST_PPROGRAM_NOT_EXECUTABLE,
            TransactionError::InstructionError(0, InstructionError::AccountNotExecutable),
            &[],
            None,
        );

        do_invoke_failure_test_local(
            TEST_EMPTY_ACCOUNTS_SLICE,
            TransactionError::InstructionError(0, InstructionError::MissingAccount),
            &[],
            None,
        );

        do_invoke_failure_test_local(
            TEST_CAP_SEEDS,
            TransactionError::InstructionError(0, InstructionError::MaxSeedLengthExceeded),
            &[],
            None,
        );

        do_invoke_failure_test_local(
            TEST_CAP_SIGNERS,
            TransactionError::InstructionError(0, InstructionError::ProgramFailedToComplete),
            &[],
            None,
        );

        do_invoke_failure_test_local(
            TEST_MAX_INSTRUCTION_DATA_LEN_EXCEEDED,
            TransactionError::InstructionError(0, InstructionError::ProgramFailedToComplete),
            &[],
            Some(vec![
                format!("Program {invoke_program_id} invoke [1]"),
                format!("Program log: invoke {program_lang} program"),
                "Program log: Test max instruction data len exceeded".into(),
                "skip".into(), // don't compare compute consumption logs
                format!("Program {invoke_program_id} failed: Invoked an instruction with data that is too large (10241 > 10240)"),
            ]),
        );

        do_invoke_failure_test_local(
            TEST_MAX_INSTRUCTION_ACCOUNTS_EXCEEDED,
            TransactionError::InstructionError(0, InstructionError::ProgramFailedToComplete),
            &[],
            Some(vec![
                format!("Program {invoke_program_id} invoke [1]"),
                format!("Program log: invoke {program_lang} program"),
                "Program log: Test max instruction accounts exceeded".into(),
                "skip".into(), // don't compare compute consumption logs
                format!("Program {invoke_program_id} failed: Invoked an instruction with too many accounts (256 > 255)"),
            ]),
        );

        do_invoke_failure_test_local(
            TEST_MAX_ACCOUNT_INFOS_EXCEEDED,
            TransactionError::InstructionError(0, InstructionError::ProgramFailedToComplete),
            &[],
            Some(vec![
                format!("Program {invoke_program_id} invoke [1]"),
                format!("Program log: invoke {program_lang} program"),
                "Program log: Test max account infos exceeded".into(),
                "skip".into(), // don't compare compute consumption logs
                format!("Program {invoke_program_id} failed: Invoked an instruction with too many account info's (129 > 128)"),
            ]),
        );

        do_invoke_failure_test_local(
            TEST_RETURN_ERROR,
            TransactionError::InstructionError(0, InstructionError::Custom(42)),
            &[invoked_program_id.clone()],
            None,
        );

        do_invoke_failure_test_local(
            TEST_PRIVILEGE_DEESCALATION_ESCALATION_SIGNER,
            TransactionError::InstructionError(0, InstructionError::PrivilegeEscalation),
            &[invoked_program_id.clone()],
            None,
        );

        do_invoke_failure_test_local(
            TEST_PRIVILEGE_DEESCALATION_ESCALATION_WRITABLE,
            TransactionError::InstructionError(0, InstructionError::PrivilegeEscalation),
            &[invoked_program_id.clone()],
            None,
        );

        do_invoke_failure_test_local(
            TEST_WRITABLE_DEESCALATION_WRITABLE,
            TransactionError::InstructionError(0, InstructionError::ReadonlyDataModified),
            &[invoked_program_id.clone()],
            None,
        );

        do_invoke_failure_test_local(
            TEST_NESTED_INVOKE_TOO_DEEP,
            TransactionError::InstructionError(0, InstructionError::CallDepth),
            &[
                invoked_program_id.clone(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
                invoked_program_id.clone(),
            ],
            None,
        );

        do_invoke_failure_test_local(
            TEST_CALL_PRECOMPILE,
            TransactionError::InstructionError(0, InstructionError::ProgramFailedToComplete),
            &[],
            None,
        );

        do_invoke_failure_test_local(
            TEST_RETURN_DATA_TOO_LARGE,
            TransactionError::InstructionError(0, InstructionError::ProgramFailedToComplete),
            &[],
            None,
        );

        do_invoke_failure_test_local(
            TEST_DUPLICATE_PRIVILEGE_ESCALATION_SIGNER,
            TransactionError::InstructionError(0, InstructionError::PrivilegeEscalation),
            &[invoked_program_id.clone()],
            None,
        );

        do_invoke_failure_test_local(
            TEST_DUPLICATE_PRIVILEGE_ESCALATION_WRITABLE,
            TransactionError::InstructionError(0, InstructionError::PrivilegeEscalation),
            &[invoked_program_id.clone()],
            None,
        );

        // Check resulting state

        assert_eq!(43, bank.get_balance(&derived_key1));
        let account = bank.get_account(&derived_key1).unwrap();
        assert_eq!(&invoke_program_id, account.owner());
        assert_eq!(
            MAX_PERMITTED_DATA_INCREASE,
            bank.get_account(&derived_key1).unwrap().data().len()
        );
        for i in 0..20 {
            assert_eq!(i as u8, account.data()[i]);
        }

        // Attempt to realloc into unauthorized address space
        let account = AccountSharedData::new(84, 0, &system_program::id());
        bank.store_account(&from_keypair.pubkey(), &account);
        bank.store_account(&derived_key1, &AccountSharedData::default());
        let instruction = Instruction::new_with_bytes(
            invoke_program_id,
            &[
                TEST_ALLOC_ACCESS_VIOLATION,
                bump_seed1,
                bump_seed2,
                bump_seed3,
            ],
            account_metas.clone(),
        );
        let message = Message::new(&[instruction], Some(&mint_pubkey));
        let tx = Transaction::new(
            &[
                &mint_keypair,
                &argument_keypair,
                &invoked_argument_keypair,
                &from_keypair,
            ],
            message.clone(),
            bank.last_blockhash(),
        );
        let (result, inner_instructions, _log_messages) =
            process_transaction_and_record_inner(&bank, tx);
        let invoked_programs: Vec<Pubkey> = inner_instructions[0]
            .iter()
            .map(|ix| &message.account_keys[ix.instruction.program_id_index as usize])
            .cloned()
            .collect();
        assert_eq!(invoked_programs, vec![]);
        assert_eq!(
            result.unwrap_err(),
            TransactionError::InstructionError(0, InstructionError::ProgramFailedToComplete)
        );
    }
}

#[test]
#[cfg(feature = "sbf_rust")]
fn test_program_sbf_program_id_spoofing() {
    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50);
    let bank = Bank::new_for_tests(&genesis_config);
    let bank = Arc::new(bank);
    let mut bank_client = BankClient::new_shared(bank.clone());

    let malicious_swap_pubkey = load_program(
        &bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_spoof1",
    );
    let (bank, malicious_system_pubkey) = load_program_and_advance_slot(
        &mut bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_spoof1_system",
    );

    let from_pubkey = Pubkey::new_unique();
    let account = AccountSharedData::new(10, 0, &system_program::id());
    bank.store_account(&from_pubkey, &account);

    let to_pubkey = Pubkey::new_unique();
    let account = AccountSharedData::new(0, 0, &system_program::id());
    bank.store_account(&to_pubkey, &account);

    let account_metas = vec![
        AccountMeta::new_readonly(system_program::id(), false),
        AccountMeta::new_readonly(malicious_system_pubkey, false),
        AccountMeta::new(from_pubkey, false),
        AccountMeta::new(to_pubkey, false),
    ];

    let instruction =
        Instruction::new_with_bytes(malicious_swap_pubkey, &[], account_metas.clone());
    let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
    assert_eq!(
        result.unwrap_err().unwrap(),
        TransactionError::InstructionError(0, InstructionError::MissingRequiredSignature)
    );
    assert_eq!(10, bank.get_balance(&from_pubkey));
    assert_eq!(0, bank.get_balance(&to_pubkey));
}

#[test]
#[cfg(feature = "sbf_rust")]
fn test_program_sbf_caller_has_access_to_cpi_program() {
    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50);
    let bank = Bank::new_for_tests(&genesis_config);
    let bank = Arc::new(bank);
    let mut bank_client = BankClient::new_shared(bank.clone());

    let caller_pubkey = load_program(
        &bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_caller_access",
    );
    let (_, caller2_pubkey) = load_program_and_advance_slot(
        &mut bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_caller_access",
    );
    let account_metas = vec![
        AccountMeta::new_readonly(caller_pubkey, false),
        AccountMeta::new_readonly(caller2_pubkey, false),
    ];
    let instruction = Instruction::new_with_bytes(caller_pubkey, &[1], account_metas.clone());
    let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
    assert_eq!(
        result.unwrap_err().unwrap(),
        TransactionError::InstructionError(0, InstructionError::MissingAccount)
    );
}

#[test]
#[cfg(feature = "sbf_rust")]
fn test_program_sbf_ro_modify() {
    solana_logger::setup();

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50);
    let bank = Bank::new_for_tests(&genesis_config);
    let bank = Arc::new(bank);
    let mut bank_client = BankClient::new_shared(bank.clone());

    let (bank, program_pubkey) = load_program_and_advance_slot(
        &mut bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_ro_modify",
    );

    let test_keypair = Keypair::new();
    let account = AccountSharedData::new(10, 0, &system_program::id());
    bank.store_account(&test_keypair.pubkey(), &account);

    let account_metas = vec![
        AccountMeta::new_readonly(system_program::id(), false),
        AccountMeta::new(test_keypair.pubkey(), true),
    ];

    let instruction = Instruction::new_with_bytes(program_pubkey, &[1], account_metas.clone());
    let message = Message::new(&[instruction], Some(&mint_keypair.pubkey()));
    let result = bank_client.send_and_confirm_message(&[&mint_keypair, &test_keypair], message);
    assert_eq!(
        result.unwrap_err().unwrap(),
        TransactionError::InstructionError(0, InstructionError::ProgramFailedToComplete)
    );

    let instruction = Instruction::new_with_bytes(program_pubkey, &[3], account_metas.clone());
    let message = Message::new(&[instruction], Some(&mint_keypair.pubkey()));
    let result = bank_client.send_and_confirm_message(&[&mint_keypair, &test_keypair], message);
    assert_eq!(
        result.unwrap_err().unwrap(),
        TransactionError::InstructionError(0, InstructionError::ProgramFailedToComplete)
    );

    let instruction = Instruction::new_with_bytes(program_pubkey, &[4], account_metas.clone());
    let message = Message::new(&[instruction], Some(&mint_keypair.pubkey()));
    let result = bank_client.send_and_confirm_message(&[&mint_keypair, &test_keypair], message);
    assert_eq!(
        result.unwrap_err().unwrap(),
        TransactionError::InstructionError(0, InstructionError::ProgramFailedToComplete)
    );
}

#[test]
#[cfg(feature = "sbf_rust")]
fn test_program_sbf_call_depth() {
    solana_logger::setup();

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50);
    let bank = Bank::new_for_tests(&genesis_config);
    let mut bank_client = BankClient::new(bank);
    let (_, program_id) = load_program_and_advance_slot(
        &mut bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_call_depth",
    );

    let instruction = Instruction::new_with_bincode(
        program_id,
        &(ComputeBudget::default().max_call_depth - 1),
        vec![],
    );
    let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
    assert!(result.is_ok());

    let instruction =
        Instruction::new_with_bincode(program_id, &ComputeBudget::default().max_call_depth, vec![]);
    let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
    assert!(result.is_err());
}

#[test]
#[cfg(feature = "sbf_rust")]
fn test_program_sbf_compute_budget() {
    solana_logger::setup();

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50);
    let bank = Bank::new_for_tests(&genesis_config);
    let mut bank_client = BankClient::new(bank);
    let (_, program_id) = load_program_and_advance_slot(
        &mut bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_noop",
    );
    let message = Message::new(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(150),
            Instruction::new_with_bincode(program_id, &0, vec![]),
        ],
        Some(&mint_keypair.pubkey()),
    );
    let result = bank_client.send_and_confirm_message(&[&mint_keypair], message);
    assert_eq!(
        result.unwrap_err().unwrap(),
        TransactionError::InstructionError(1, InstructionError::ProgramFailedToComplete),
    );
}

#[test]
fn assert_instruction_count() {
    solana_logger::setup();

    let mut programs = Vec::new();
    #[cfg(feature = "sbf_c")]
    {
        programs.extend_from_slice(&[
            ("alloc", 11502),
            ("sbf_to_sbf", 313),
            ("multiple_static", 208),
            ("noop", 5),
            ("noop++", 5),
            ("relative_call", 210),
            ("return_data", 980),
            ("sanity", 2377),
            ("sanity++", 2277),
            ("secp256k1_recover", 25383),
            ("sha", 1355),
            ("struct_pass", 108),
            ("struct_ret", 122),
        ]);
    }
    #[cfg(feature = "sbf_rust")]
    {
        programs.extend_from_slice(&[
            ("solana_sbf_rust_128bit", 1218),
            ("solana_sbf_rust_alloc", 5067),
            ("solana_sbf_rust_custom_heap", 398),
            ("solana_sbf_rust_dep_crate", 2),
            ("solana_sbf_rust_iter", 1013),
            ("solana_sbf_rust_many_args", 1289),
            ("solana_sbf_rust_mem", 2067),
            ("solana_sbf_rust_membuiltins", 1539),
            ("solana_sbf_rust_noop", 275),
            ("solana_sbf_rust_param_passing", 146),
            ("solana_sbf_rust_rand", 378),
            ("solana_sbf_rust_sanity", 51931),
            ("solana_sbf_rust_secp256k1_recover", 91185),
            ("solana_sbf_rust_sha", 24059),
        ]);
    }

    println!("\n  {:36} expected actual  diff", "SBF program");
    for (program_name, expected_consumption) in programs.iter() {
        let loader_id = bpf_loader::id();
        let program_key = Pubkey::new_unique();
        let mut transaction_accounts = vec![
            (program_key, AccountSharedData::new(0, 0, &loader_id)),
            (
                Pubkey::new_unique(),
                AccountSharedData::new(0, 0, &program_key),
            ),
        ];
        let instruction_accounts = vec![AccountMeta {
            pubkey: transaction_accounts[1].0,
            is_signer: false,
            is_writable: false,
        }];
        transaction_accounts[0]
            .1
            .set_data_from_slice(&load_program_from_file(program_name));
        transaction_accounts[0].1.set_executable(true);

        let prev_compute_meter = RefCell::new(0);
        print!("  {:36} {:8}", program_name, *expected_consumption);
        mock_process_instruction(
            &loader_id,
            vec![0],
            &[],
            transaction_accounts,
            instruction_accounts,
            Ok(()),
            solana_bpf_loader_program::Entrypoint::vm,
            |invoke_context| {
                *prev_compute_meter.borrow_mut() = invoke_context.get_remaining();
                solana_bpf_loader_program::test_utils::load_all_invoked_programs(invoke_context);
            },
            |invoke_context| {
                let consumption = prev_compute_meter
                    .borrow()
                    .saturating_sub(invoke_context.get_remaining());
                let diff: i64 = consumption as i64 - *expected_consumption as i64;
                println!(
                    "{:6} {:+5} ({:+3.0}%)",
                    consumption,
                    diff,
                    100.0_f64 * consumption as f64 / *expected_consumption as f64 - 100.0_f64,
                );
                assert_eq!(consumption, *expected_consumption);
            },
        );
    }
}

#[test]
#[cfg(feature = "sbf_rust")]
fn test_program_sbf_instruction_introspection() {
    solana_logger::setup();

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50_000);
    let bank = Bank::new_for_tests(&genesis_config);
    let bank = Arc::new(bank);
    let mut bank_client = BankClient::new_shared(bank.clone());

    let (bank, program_id) = load_program_and_advance_slot(
        &mut bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_instruction_introspection",
    );

    // Passing transaction
    let account_metas = vec![
        AccountMeta::new_readonly(program_id, false),
        AccountMeta::new_readonly(sysvar::instructions::id(), false),
    ];
    let instruction0 = Instruction::new_with_bytes(program_id, &[0u8, 0u8], account_metas.clone());
    let instruction1 = Instruction::new_with_bytes(program_id, &[0u8, 1u8], account_metas.clone());
    let instruction2 = Instruction::new_with_bytes(program_id, &[0u8, 2u8], account_metas);
    let message = Message::new(
        &[instruction0, instruction1, instruction2],
        Some(&mint_keypair.pubkey()),
    );
    let result = bank_client.send_and_confirm_message(&[&mint_keypair], message);
    assert!(result.is_ok());

    // writable special instructions11111 key, should not be allowed
    let account_metas = vec![AccountMeta::new(sysvar::instructions::id(), false)];
    let instruction = Instruction::new_with_bytes(program_id, &[0], account_metas);
    let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
    assert_eq!(
        result.unwrap_err().unwrap(),
        // sysvar write locks are demoted to read only. So this will no longer
        // cause InvalidAccountIndex error.
        TransactionError::InstructionError(0, InstructionError::ProgramFailedToComplete),
    );

    // No accounts, should error
    let instruction = Instruction::new_with_bytes(program_id, &[0], vec![]);
    let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().unwrap(),
        TransactionError::InstructionError(0, InstructionError::NotEnoughAccountKeys)
    );
    assert!(bank.get_account(&sysvar::instructions::id()).is_none());
}

#[test]
#[cfg(feature = "sbf_rust")]
fn test_program_sbf_test_use_latest_executor() {
    solana_logger::setup();

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50);
    let bank = Bank::new_for_tests(&genesis_config);
    let mut bank_client = BankClient::new(bank);
    let panic_id = load_program(
        &bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_panic",
    );

    // Write the panic program into the program account
    let (program_keypair, instruction) = load_and_finalize_program(
        &bank_client,
        &bpf_loader::id(),
        None,
        &mint_keypair,
        "solana_sbf_rust_panic",
    );

    // Finalize the panic program, but fail the tx
    let message = Message::new(
        &[
            instruction,
            Instruction::new_with_bytes(panic_id, &[0], vec![]),
        ],
        Some(&mint_keypair.pubkey()),
    );

    bank_client
        .advance_slot(1, &Pubkey::default())
        .expect("Failed to advance the slot");
    assert!(bank_client
        .send_and_confirm_message(&[&mint_keypair, &program_keypair], message)
        .is_err());

    // Write the noop program into the same program account
    let (program_keypair, instruction) = load_and_finalize_program(
        &bank_client,
        &bpf_loader::id(),
        Some(program_keypair),
        &mint_keypair,
        "solana_sbf_rust_noop",
    );
    bank_client
        .advance_slot(1, &Pubkey::default())
        .expect("Failed to advance the slot");
    let message = Message::new(&[instruction], Some(&mint_keypair.pubkey()));
    bank_client
        .send_and_confirm_message(&[&mint_keypair, &program_keypair], message)
        .unwrap();

    // Call the noop program, should get noop not panic
    let message = Message::new(
        &[Instruction::new_with_bytes(
            program_keypair.pubkey(),
            &[0],
            vec![],
        )],
        Some(&mint_keypair.pubkey()),
    );
    bank_client
        .advance_slot(1, &Pubkey::default())
        .expect("Failed to advance the slot");
    assert!(bank_client
        .send_and_confirm_message(&[&mint_keypair], message)
        .is_ok());
}

#[test]
#[cfg(feature = "sbf_rust")]
fn test_program_sbf_upgrade() {
    solana_logger::setup();

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50);
    let bank = Bank::new_for_tests(&genesis_config);
    let mut bank_client = BankClient::new(bank);

    // Deploy upgrade program
    let buffer_keypair = Keypair::new();
    let program_keypair = Keypair::new();
    let program_id = program_keypair.pubkey();
    let authority_keypair = Keypair::new();
    load_upgradeable_program(
        &bank_client,
        &mint_keypair,
        &buffer_keypair,
        &program_keypair,
        &authority_keypair,
        "solana_sbf_rust_upgradeable",
    );
    bank_client
        .advance_slot(1, &Pubkey::default())
        .expect("Failed to advance the slot");

    let mut instruction =
        Instruction::new_with_bytes(program_id, &[0], vec![AccountMeta::new(clock::id(), false)]);

    // Call upgrade program
    let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction.clone());
    assert_eq!(
        result.unwrap_err().unwrap(),
        TransactionError::InstructionError(0, InstructionError::Custom(42))
    );

    // Upgrade program
    let buffer_keypair = Keypair::new();
    upgrade_program(
        &bank_client,
        &mint_keypair,
        &buffer_keypair,
        &program_id,
        &authority_keypair,
        "solana_sbf_rust_upgraded",
    );
    bank_client.set_sysvar_for_tests(&clock::Clock {
        slot: 2,
        ..clock::Clock::default()
    });
    bank_client
        .advance_slot(1, &Pubkey::default())
        .expect("Failed to advance the slot");

    // Call upgraded program
    instruction.data[0] += 1;
    let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction.clone());
    assert_eq!(
        result.unwrap_err().unwrap(),
        TransactionError::InstructionError(0, InstructionError::Custom(43))
    );

    // Set a new authority
    let new_authority_keypair = Keypair::new();
    set_upgrade_authority(
        &bank_client,
        &mint_keypair,
        &program_id,
        &authority_keypair,
        Some(&new_authority_keypair.pubkey()),
    );

    // Upgrade back to the original program
    let buffer_keypair = Keypair::new();
    upgrade_program(
        &bank_client,
        &mint_keypair,
        &buffer_keypair,
        &program_id,
        &new_authority_keypair,
        "solana_sbf_rust_upgradeable",
    );

    bank_client
        .advance_slot(1, &Pubkey::default())
        .expect("Failed to advance the slot");

    // Call original program
    instruction.data[0] += 1;
    let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
    assert_eq!(
        result.unwrap_err().unwrap(),
        TransactionError::InstructionError(0, InstructionError::Custom(42))
    );
}

fn get_stable_genesis_config() -> GenesisConfigInfo {
    let validator_pubkey =
        Pubkey::from_str("GLh546CXmtZdvpEzL8sxzqhhUf7KPvmGaRpFHB5W1sjV").unwrap();
    let mint_keypair = Keypair::from_base58_string(
        "4YTH9JSRgZocmK9ezMZeJCCV2LVeR2NatTBA8AFXkg2x83fqrt8Vwyk91961E7ns4vee9yUBzuDfztb8i9iwTLFd",
    );
    let voting_keypair = Keypair::from_base58_string(
        "4EPWEn72zdNY1JSKkzyZ2vTZcKdPW3jM5WjAgUadnoz83FR5cDFApbo7s5mwBcYXn8afVe2syReJaqBi4fkhG3mH",
    );
    let stake_pubkey = Pubkey::from_str("HGq9JF77xFXRgWRJy8VQuhdbdugrT856RvQDzr1KJo6E").unwrap();

    let mut genesis_config = create_genesis_config_with_leader_ex(
        123,
        &mint_keypair.pubkey(),
        &validator_pubkey,
        &voting_keypair.pubkey(),
        &stake_pubkey,
        bootstrap_validator_stake_lamports(),
        42,
        FeeRateGovernor::new(0, 0), // most tests can't handle transaction fees
        Rent::free(),               // most tests don't expect rent
        ClusterType::Development,
        vec![],
    );
    genesis_config.creation_time = Duration::ZERO.as_secs() as UnixTimestamp;

    GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        voting_keypair,
        validator_pubkey,
    }
}

#[test]
#[ignore]
#[cfg(feature = "sbf_rust")]
fn test_program_sbf_invoke_stable_genesis_and_bank() {
    // The purpose of this test is to exercise various code branches of runtime/VM and
    // assert that the resulting bank hash matches with the expected value.
    // The assert check is commented out by default. Please refer to the last few lines
    // of the test to enable the assertion.
    solana_logger::setup();

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = get_stable_genesis_config();
    let bank = Bank::new_for_tests(&genesis_config);
    let bank = Arc::new(bank);
    let bank_client = BankClient::new_shared(bank.clone());

    // Deploy upgradeable program
    let buffer_keypair = Keypair::from_base58_string(
        "4q4UvWxh2oMifTGbChDeWCbdN8eJEUQ1E6cuNnmymJ6AN5CMUT2VW5A1RKnG9dy7ypLczB9inMUAafh5TkpXrtxg",
    );
    let program_keypair = Keypair::from_base58_string(
        "3LQpBxgpaFNJPit5a8t51pJKMkUmNUn5PhSTcuuhuuBxe43cTeqVPhMtKkFNr5VpFzCExf4ihibvuZgGxmjy6t8n",
    );
    let program_id = program_keypair.pubkey();
    let authority_keypair = Keypair::from_base58_string(
        "285XFW2NTWd6CMvtHzvYYS1kWzmzcGBnyEXbH1v8hq6YJqJsLMTYMPkbEQqeE7m7UqhoMeK5V3HMJLf9DdxwU2Gy",
    );

    let instruction =
        Instruction::new_with_bytes(program_id, &[0], vec![AccountMeta::new(clock::id(), false)]);

    // Call program before its deployed
    let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction.clone());
    assert_eq!(
        result.unwrap_err().unwrap(),
        TransactionError::ProgramAccountNotFound
    );

    load_upgradeable_program(
        &bank_client,
        &mint_keypair,
        &buffer_keypair,
        &program_keypair,
        &authority_keypair,
        "solana_sbf_rust_noop",
    );

    // Deploy indirect invocation program
    let indirect_program_keypair = Keypair::from_base58_string(
        "2BgE4gD5wUCwiAVPYbmWd2xzXSsD9W2fWgNjwmVkm8WL7i51vK9XAXNnX1VB6oKQZmjaUPRd5RzE6RggB9DeKbZC",
    );
    load_upgradeable_program(
        &bank_client,
        &mint_keypair,
        &buffer_keypair,
        &indirect_program_keypair,
        &authority_keypair,
        "solana_sbf_rust_invoke_and_return",
    );

    let invoke_instruction =
        Instruction::new_with_bytes(program_id, &[0], vec![AccountMeta::new(clock::id(), false)]);
    let indirect_invoke_instruction = Instruction::new_with_bytes(
        indirect_program_keypair.pubkey(),
        &[0],
        vec![
            AccountMeta::new_readonly(program_id, false),
            AccountMeta::new_readonly(clock::id(), false),
        ],
    );

    // Prepare redeployment
    let buffer_keypair = Keypair::from_base58_string(
        "5T5L31FiUphXh4N6mxiWhEKPrdLhvMJSbaHo1Ne7zZYkw6YT1fVkqsWdA6pHMtqATiMTc4sfx5yTV9M9AnWDoBkW",
    );
    load_upgradeable_buffer(
        &bank_client,
        &mint_keypair,
        &buffer_keypair,
        &authority_keypair,
        "solana_sbf_rust_panic",
    );
    let redeployment_instruction = bpf_loader_upgradeable::upgrade(
        &program_id,
        &buffer_keypair.pubkey(),
        &authority_keypair.pubkey(),
        &mint_keypair.pubkey(),
    );

    // Redeployment causes programs to be unavailable to both top-level-instructions and CPI instructions
    for invoke_instruction in [invoke_instruction, indirect_invoke_instruction] {
        // Call upgradeable program
        let result =
            bank_client.send_and_confirm_instruction(&mint_keypair, invoke_instruction.clone());
        assert!(result.is_ok());

        // Upgrade the program and invoke in same tx
        let message = Message::new(
            &[redeployment_instruction.clone(), invoke_instruction],
            Some(&mint_keypair.pubkey()),
        );
        let tx = Transaction::new(
            &[&mint_keypair, &authority_keypair],
            message.clone(),
            bank.last_blockhash(),
        );
        let (result, _, _) = process_transaction_and_record_inner(&bank, tx);
        assert_eq!(
            result.unwrap_err(),
            TransactionError::InstructionError(1, InstructionError::InvalidAccountData),
        );
    }

    // Prepare undeployment
    let (programdata_address, _) = Pubkey::find_program_address(
        &[program_keypair.pubkey().as_ref()],
        &bpf_loader_upgradeable::id(),
    );
    let undeployment_instruction = bpf_loader_upgradeable::close_any(
        &programdata_address,
        &mint_keypair.pubkey(),
        Some(&authority_keypair.pubkey()),
        Some(&program_id),
    );

    let invoke_instruction =
        Instruction::new_with_bytes(program_id, &[1], vec![AccountMeta::new(clock::id(), false)]);
    let indirect_invoke_instruction = Instruction::new_with_bytes(
        indirect_program_keypair.pubkey(),
        &[1],
        vec![
            AccountMeta::new_readonly(program_id, false),
            AccountMeta::new_readonly(clock::id(), false),
        ],
    );

    // Undeployment is visible to both top-level-instructions and CPI instructions
    for invoke_instruction in [invoke_instruction, indirect_invoke_instruction] {
        // Call upgradeable program
        let result =
            bank_client.send_and_confirm_instruction(&mint_keypair, invoke_instruction.clone());
        assert!(result.is_ok());

        // Undeploy the program and invoke in same tx
        let message = Message::new(
            &[undeployment_instruction.clone(), invoke_instruction],
            Some(&mint_keypair.pubkey()),
        );
        let tx = Transaction::new(
            &[&mint_keypair, &authority_keypair],
            message.clone(),
            bank.last_blockhash(),
        );
        let (result, _, _) = process_transaction_and_record_inner(&bank, tx);
        assert_eq!(
            result.unwrap_err(),
            TransactionError::InstructionError(1, InstructionError::InvalidAccountData),
        );
    }

    bank.freeze();
    let expected_hash = Hash::from_str("2A2vqbUKExRbnaAzSnDFXdsBZRZSpCjGZCAA3mFZG2sV")
        .expect("Failed to generate hash");
    println!("Stable test produced bank hash: {}", bank.hash());
    println!("Expected hash: {}", expected_hash);

    // Enable the following code to match the bank hash with the expected bank hash.
    // Follow these steps.
    // 1. Run this test on the baseline/master commit, and get the expected bank hash.
    // 2. Update the `expected_hash` to match the expected bank hash.
    // 3. Run the test in the PR branch that's being tested.
    // If the hash doesn't match, the PR likely has runtime changes that can lead to
    // consensus failure.
    //  assert_eq!(bank.hash(), expected_hash);
}

#[test]
#[cfg(feature = "sbf_rust")]
fn test_program_sbf_invoke_in_same_tx_as_deployment() {
    solana_logger::setup();

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50);
    let bank = Bank::new_for_tests(&genesis_config);
    let bank = Arc::new(bank);
    let mut bank_client = BankClient::new_shared(bank.clone());

    // Deploy upgradeable program
    let buffer_keypair = Keypair::new();
    let program_keypair = Keypair::new();
    let program_id = program_keypair.pubkey();
    let authority_keypair = Keypair::new();

    // Deploy indirect invocation program
    let indirect_program_keypair = Keypair::new();
    load_upgradeable_program(
        &bank_client,
        &mint_keypair,
        &buffer_keypair,
        &indirect_program_keypair,
        &authority_keypair,
        "solana_sbf_rust_invoke_and_return",
    );

    let invoke_instruction =
        Instruction::new_with_bytes(program_id, &[0], vec![AccountMeta::new(clock::id(), false)]);
    let indirect_invoke_instruction = Instruction::new_with_bytes(
        indirect_program_keypair.pubkey(),
        &[0],
        vec![
            AccountMeta::new_readonly(program_id, false),
            AccountMeta::new_readonly(clock::id(), false),
        ],
    );

    // Prepare deployment
    let program = load_upgradeable_buffer(
        &bank_client,
        &mint_keypair,
        &buffer_keypair,
        &authority_keypair,
        "solana_sbf_rust_noop",
    );
    let deployment_instructions = bpf_loader_upgradeable::deploy_with_max_program_len(
        &mint_keypair.pubkey(),
        &program_keypair.pubkey(),
        &buffer_keypair.pubkey(),
        &authority_keypair.pubkey(),
        1.max(
            bank_client
                .get_minimum_balance_for_rent_exemption(
                    bpf_loader_upgradeable::UpgradeableLoaderState::size_of_program(),
                )
                .unwrap(),
        ),
        program.len() * 2,
    )
    .unwrap();

    let bank = bank_client
        .advance_slot(1, &Pubkey::default())
        .expect("Failed to advance slot");

    // Deployment is invisible to both top-level-instructions and CPI instructions
    for (index, invoke_instruction) in [invoke_instruction, indirect_invoke_instruction]
        .into_iter()
        .enumerate()
    {
        let mut instructions = deployment_instructions.clone();
        instructions.push(invoke_instruction);
        let tx = Transaction::new(
            &[&mint_keypair, &program_keypair, &authority_keypair],
            Message::new(&instructions, Some(&mint_keypair.pubkey())),
            bank.last_blockhash(),
        );
        if index == 0 {
            let results = execute_transactions(&bank, vec![tx]);
            assert_eq!(
                results[0].as_ref().unwrap_err(),
                &TransactionError::ProgramAccountNotFound,
            );
        } else {
            let (result, _, _) = process_transaction_and_record_inner(&bank, tx);
            assert_eq!(
                result.unwrap_err(),
                TransactionError::InstructionError(2, InstructionError::InvalidAccountData),
            );
        }
    }
}

#[test]
#[cfg(feature = "sbf_rust")]
fn test_program_sbf_invoke_in_same_tx_as_redeployment() {
    solana_logger::setup();

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50);
    let bank = Bank::new_for_tests(&genesis_config);
    let bank = Arc::new(bank);
    let mut bank_client = BankClient::new_shared(bank.clone());

    // Deploy upgradeable program
    let buffer_keypair = Keypair::new();
    let program_keypair = Keypair::new();
    let program_id = program_keypair.pubkey();
    let authority_keypair = Keypair::new();
    load_upgradeable_program(
        &bank_client,
        &mint_keypair,
        &buffer_keypair,
        &program_keypair,
        &authority_keypair,
        "solana_sbf_rust_noop",
    );

    // Deploy indirect invocation program
    let indirect_program_keypair = Keypair::new();
    load_upgradeable_program(
        &bank_client,
        &mint_keypair,
        &buffer_keypair,
        &indirect_program_keypair,
        &authority_keypair,
        "solana_sbf_rust_invoke_and_return",
    );

    // Deploy panic program
    let panic_program_keypair = Keypair::new();
    load_upgradeable_program(
        &bank_client,
        &mint_keypair,
        &buffer_keypair,
        &panic_program_keypair,
        &authority_keypair,
        "solana_sbf_rust_panic",
    );

    let invoke_instruction =
        Instruction::new_with_bytes(program_id, &[0], vec![AccountMeta::new(clock::id(), false)]);
    let indirect_invoke_instruction = Instruction::new_with_bytes(
        indirect_program_keypair.pubkey(),
        &[0],
        vec![
            AccountMeta::new_readonly(program_id, false),
            AccountMeta::new_readonly(clock::id(), false),
        ],
    );

    // load_upgradeable_program sets clock sysvar to 1, which causes the program to be effective
    // after 2 slots. They need to be called individually to create the correct fork graph in between.
    bank_client.advance_slot(1, &Pubkey::default()).unwrap();
    let bank = bank_client.advance_slot(1, &Pubkey::default()).unwrap();

    // Prepare redeployment
    let buffer_keypair = Keypair::new();
    load_upgradeable_buffer(
        &bank_client,
        &mint_keypair,
        &buffer_keypair,
        &authority_keypair,
        "solana_sbf_rust_panic",
    );
    let redeployment_instruction = bpf_loader_upgradeable::upgrade(
        &program_id,
        &buffer_keypair.pubkey(),
        &authority_keypair.pubkey(),
        &mint_keypair.pubkey(),
    );

    // Redeployment causes programs to be unavailable to both top-level-instructions and CPI instructions
    for invoke_instruction in [invoke_instruction, indirect_invoke_instruction] {
        // Call upgradeable program
        let result =
            bank_client.send_and_confirm_instruction(&mint_keypair, invoke_instruction.clone());
        assert!(result.is_ok());

        // Upgrade the program and invoke in same tx
        let message = Message::new(
            &[redeployment_instruction.clone(), invoke_instruction],
            Some(&mint_keypair.pubkey()),
        );
        let tx = Transaction::new(
            &[&mint_keypair, &authority_keypair],
            message.clone(),
            bank.last_blockhash(),
        );
        let (result, _, _) = process_transaction_and_record_inner(&bank, tx);
        assert_eq!(
            result.unwrap_err(),
            TransactionError::InstructionError(1, InstructionError::InvalidAccountData),
        );
    }
}

#[test]
#[cfg(feature = "sbf_rust")]
fn test_program_sbf_invoke_in_same_tx_as_undeployment() {
    solana_logger::setup();

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50);
    let bank = Bank::new_for_tests(&genesis_config);
    let bank = Arc::new(bank);
    let mut bank_client = BankClient::new_shared(bank.clone());

    // Deploy upgradeable program
    let buffer_keypair = Keypair::new();
    let program_keypair = Keypair::new();
    let program_id = program_keypair.pubkey();
    let authority_keypair = Keypair::new();
    load_upgradeable_program(
        &bank_client,
        &mint_keypair,
        &buffer_keypair,
        &program_keypair,
        &authority_keypair,
        "solana_sbf_rust_noop",
    );

    // Deploy indirect invocation program
    let indirect_program_keypair = Keypair::new();
    load_upgradeable_program(
        &bank_client,
        &mint_keypair,
        &buffer_keypair,
        &indirect_program_keypair,
        &authority_keypair,
        "solana_sbf_rust_invoke_and_return",
    );

    let invoke_instruction =
        Instruction::new_with_bytes(program_id, &[0], vec![AccountMeta::new(clock::id(), false)]);
    let indirect_invoke_instruction = Instruction::new_with_bytes(
        indirect_program_keypair.pubkey(),
        &[0],
        vec![
            AccountMeta::new_readonly(program_id, false),
            AccountMeta::new_readonly(clock::id(), false),
        ],
    );

    // load_upgradeable_program sets clock sysvar to 1, which causes the program to be effective
    // after 2 slots. They need to be called individually to create the correct fork graph in between.
    bank_client.advance_slot(1, &Pubkey::default()).unwrap();
    let bank = bank_client.advance_slot(1, &Pubkey::default()).unwrap();

    // Prepare undeployment
    let (programdata_address, _) = Pubkey::find_program_address(
        &[program_keypair.pubkey().as_ref()],
        &bpf_loader_upgradeable::id(),
    );
    let undeployment_instruction = bpf_loader_upgradeable::close_any(
        &programdata_address,
        &mint_keypair.pubkey(),
        Some(&authority_keypair.pubkey()),
        Some(&program_id),
    );

    // Undeployment is visible to both top-level-instructions and CPI instructions
    for invoke_instruction in [invoke_instruction, indirect_invoke_instruction] {
        // Call upgradeable program
        let result =
            bank_client.send_and_confirm_instruction(&mint_keypair, invoke_instruction.clone());
        assert!(result.is_ok());

        // Upgrade the program and invoke in same tx
        let message = Message::new(
            &[undeployment_instruction.clone(), invoke_instruction],
            Some(&mint_keypair.pubkey()),
        );
        let tx = Transaction::new(
            &[&mint_keypair, &authority_keypair],
            message.clone(),
            bank.last_blockhash(),
        );
        let (result, _, _) = process_transaction_and_record_inner(&bank, tx);
        assert_eq!(
            result.unwrap_err(),
            TransactionError::InstructionError(1, InstructionError::InvalidAccountData),
        );
    }
}

#[test]
#[cfg(feature = "sbf_rust")]
fn test_program_sbf_invoke_upgradeable_via_cpi() {
    solana_logger::setup();

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50);
    let bank = Bank::new_for_tests(&genesis_config);
    let mut bank_client = BankClient::new(bank);
    let invoke_and_return = load_program(
        &bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_invoke_and_return",
    );

    // Deploy upgradeable program
    let buffer_keypair = Keypair::new();
    let program_keypair = Keypair::new();
    let program_id = program_keypair.pubkey();
    let authority_keypair = Keypair::new();
    load_upgradeable_program(
        &bank_client,
        &mint_keypair,
        &buffer_keypair,
        &program_keypair,
        &authority_keypair,
        "solana_sbf_rust_upgradeable",
    );

    bank_client
        .advance_slot(1, &Pubkey::default())
        .expect("Failed to advance slot");

    let mut instruction = Instruction::new_with_bytes(
        invoke_and_return,
        &[0],
        vec![
            AccountMeta::new_readonly(program_id, false),
            AccountMeta::new_readonly(clock::id(), false),
        ],
    );

    // Call invoker program to invoke the upgradeable program
    instruction.data[0] += 1;
    let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction.clone());
    assert_eq!(
        result.unwrap_err().unwrap(),
        TransactionError::InstructionError(0, InstructionError::Custom(42))
    );

    // Upgrade program
    let buffer_keypair = Keypair::new();
    upgrade_program(
        &bank_client,
        &mint_keypair,
        &buffer_keypair,
        &program_id,
        &authority_keypair,
        "solana_sbf_rust_upgraded",
    );
    bank_client.set_sysvar_for_tests(&clock::Clock {
        slot: 2,
        ..clock::Clock::default()
    });
    bank_client
        .advance_slot(1, &Pubkey::default())
        .expect("Failed to advance slot");

    // Call the upgraded program
    instruction.data[0] += 1;
    let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction.clone());
    assert_eq!(
        result.unwrap_err().unwrap(),
        TransactionError::InstructionError(0, InstructionError::Custom(43))
    );

    // Set a new authority
    let new_authority_keypair = Keypair::new();
    set_upgrade_authority(
        &bank_client,
        &mint_keypair,
        &program_id,
        &authority_keypair,
        Some(&new_authority_keypair.pubkey()),
    );

    // Upgrade back to the original program
    let buffer_keypair = Keypair::new();
    upgrade_program(
        &bank_client,
        &mint_keypair,
        &buffer_keypair,
        &program_id,
        &new_authority_keypair,
        "solana_sbf_rust_upgradeable",
    );

    bank_client
        .advance_slot(1, &Pubkey::default())
        .expect("Failed to advance slot");

    // Call original program
    instruction.data[0] += 1;
    let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction.clone());
    assert_eq!(
        result.unwrap_err().unwrap(),
        TransactionError::InstructionError(0, InstructionError::Custom(42))
    );
}

#[test]
#[cfg(any(feature = "sbf_c", feature = "sbf_rust"))]
fn test_program_sbf_disguised_as_sbf_loader() {
    solana_logger::setup();

    let mut programs = Vec::new();
    #[cfg(feature = "sbf_c")]
    {
        programs.extend_from_slice(&[("noop")]);
    }
    #[cfg(feature = "sbf_rust")]
    {
        programs.extend_from_slice(&[("solana_sbf_rust_noop")]);
    }

    for program in programs.iter() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(50);

        let mut bank = Bank::new_for_tests(&genesis_config);
        // disable native_programs_consume_cu feature to allow test program
        // not consume units.
        let mut feature_set = FeatureSet::all_enabled();
        feature_set.deactivate(&solana_sdk::feature_set::native_programs_consume_cu::id());
        bank.feature_set = Arc::new(feature_set);
        bank.deactivate_feature(
            &solana_sdk::feature_set::remove_bpf_loader_incorrect_program_id::id(),
        );
        let bank_client = BankClient::new(bank);

        let program_id = load_program(&bank_client, &bpf_loader::id(), &mint_keypair, program);
        let account_metas = vec![AccountMeta::new_readonly(program_id, false)];
        let instruction = Instruction::new_with_bytes(bpf_loader::id(), &[1], account_metas);
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        assert_eq!(
            result.unwrap_err().unwrap(),
            TransactionError::InstructionError(0, InstructionError::IncorrectProgramId)
        );
    }
}

#[test]
#[cfg(feature = "sbf_c")]
fn test_program_reads_from_program_account() {
    solana_logger::setup();

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50);
    let bank = Bank::new_for_tests(&genesis_config);
    let mut bank_client = BankClient::new(bank);

    let (_, program_id) = load_program_and_advance_slot(
        &mut bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "read_program",
    );
    let account_metas = vec![AccountMeta::new_readonly(program_id, false)];
    let instruction = Instruction::new_with_bytes(program_id, &[], account_metas);
    bank_client
        .send_and_confirm_instruction(&mint_keypair, instruction)
        .unwrap();
}

#[test]
#[cfg(feature = "sbf_c")]
fn test_program_sbf_c_dup() {
    solana_logger::setup();

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50);
    let bank = Bank::new_for_tests(&genesis_config);

    let account_address = Pubkey::new_unique();
    let account = AccountSharedData::new_data(42, &[1_u8, 2, 3], &system_program::id()).unwrap();
    bank.store_account(&account_address, &account);

    let mut bank_client = BankClient::new(bank);

    let (_, program_id) =
        load_program_and_advance_slot(&mut bank_client, &bpf_loader::id(), &mint_keypair, "ser");
    let account_metas = vec![
        AccountMeta::new_readonly(account_address, false),
        AccountMeta::new_readonly(account_address, false),
    ];
    let instruction = Instruction::new_with_bytes(program_id, &[4, 5, 6, 7], account_metas);
    bank_client
        .send_and_confirm_instruction(&mint_keypair, instruction)
        .unwrap();
}

#[test]
#[cfg(feature = "sbf_rust")]
fn test_program_sbf_upgrade_via_cpi() {
    solana_logger::setup();

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50);
    let bank = Bank::new_for_tests(&genesis_config);
    let mut bank_client = BankClient::new(bank);
    let invoke_and_return = load_program(
        &bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_invoke_and_return",
    );

    // Deploy upgradeable program
    let buffer_keypair = Keypair::new();
    let program_keypair = Keypair::new();
    let program_id = program_keypair.pubkey();
    let authority_keypair = Keypair::new();
    load_upgradeable_program(
        &bank_client,
        &mint_keypair,
        &buffer_keypair,
        &program_keypair,
        &authority_keypair,
        "solana_sbf_rust_upgradeable",
    );
    bank_client
        .advance_slot(1, &Pubkey::default())
        .expect("Failed to advance the slot");
    let program_account = bank_client.get_account(&program_id).unwrap().unwrap();
    let Ok(bpf_loader_upgradeable::UpgradeableLoaderState::Program {
        programdata_address,
    }) = program_account.state()
    else {
        unreachable!()
    };
    let original_programdata = bank_client
        .get_account_data(&programdata_address)
        .unwrap()
        .unwrap();

    let mut instruction = Instruction::new_with_bytes(
        invoke_and_return,
        &[0],
        vec![
            AccountMeta::new_readonly(program_id, false),
            AccountMeta::new_readonly(clock::id(), false),
        ],
    );

    // Call the upgradable program
    instruction.data[0] += 1;
    let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction.clone());
    assert_eq!(
        result.unwrap_err().unwrap(),
        TransactionError::InstructionError(0, InstructionError::Custom(42))
    );

    // Load the buffer account
    let buffer_keypair = Keypair::new();
    load_upgradeable_buffer(
        &bank_client,
        &mint_keypair,
        &buffer_keypair,
        &authority_keypair,
        "solana_sbf_rust_upgraded",
    );

    // Upgrade program via CPI
    let mut upgrade_instruction = bpf_loader_upgradeable::upgrade(
        &program_id,
        &buffer_keypair.pubkey(),
        &authority_keypair.pubkey(),
        &mint_keypair.pubkey(),
    );
    upgrade_instruction.program_id = invoke_and_return;
    upgrade_instruction
        .accounts
        .insert(0, AccountMeta::new(bpf_loader_upgradeable::id(), false));
    let message = Message::new(&[upgrade_instruction], Some(&mint_keypair.pubkey()));
    bank_client
        .send_and_confirm_message(&[&mint_keypair, &authority_keypair], message)
        .unwrap();

    bank_client
        .advance_slot(1, &Pubkey::default())
        .expect("Failed to advance the slot");

    // Call the upgraded program
    instruction.data[0] += 1;
    let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction.clone());
    assert_eq!(
        result.unwrap_err().unwrap(),
        TransactionError::InstructionError(0, InstructionError::Custom(43))
    );

    // Validate that the programdata was actually overwritten
    let programdata = bank_client
        .get_account_data(&programdata_address)
        .unwrap()
        .unwrap();
    assert_ne!(programdata, original_programdata);
}

#[test]
#[cfg(feature = "sbf_rust")]
fn test_program_sbf_set_upgrade_authority_via_cpi() {
    solana_logger::setup();

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50);
    let bank = Bank::new_for_tests(&genesis_config);
    let mut bank_client = BankClient::new(bank);

    // Deploy CPI invoker program
    let invoke_and_return = load_program(
        &bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_invoke_and_return",
    );

    // Deploy upgradeable program
    let buffer_keypair = Keypair::new();
    let program_keypair = Keypair::new();
    let program_id = program_keypair.pubkey();
    let authority_keypair = Keypair::new();
    load_upgradeable_program(
        &bank_client,
        &mint_keypair,
        &buffer_keypair,
        &program_keypair,
        &authority_keypair,
        "solana_sbf_rust_upgradeable",
    );

    bank_client
        .advance_slot(1, &Pubkey::default())
        .expect("Failed to advance the slot");

    // Set program upgrade authority instruction to invoke via CPI
    let new_upgrade_authority_key = Keypair::new().pubkey();
    let mut set_upgrade_authority_instruction = bpf_loader_upgradeable::set_upgrade_authority(
        &program_id,
        &authority_keypair.pubkey(),
        Some(&new_upgrade_authority_key),
    );

    // Invoke set_upgrade_authority via CPI invoker program
    set_upgrade_authority_instruction.program_id = invoke_and_return;
    set_upgrade_authority_instruction
        .accounts
        .insert(0, AccountMeta::new(bpf_loader_upgradeable::id(), false));

    let message = Message::new(
        &[set_upgrade_authority_instruction],
        Some(&mint_keypair.pubkey()),
    );
    bank_client
        .send_and_confirm_message(&[&mint_keypair, &authority_keypair], message)
        .unwrap();

    // Assert upgrade authority was changed
    let program_account_data = bank_client.get_account_data(&program_id).unwrap().unwrap();
    let program_account = parse_bpf_upgradeable_loader(&program_account_data).unwrap();

    let upgrade_authority_key = match program_account {
        BpfUpgradeableLoaderAccountType::Program(ui_program) => {
            let program_data_account_key = Pubkey::from_str(&ui_program.program_data).unwrap();
            let program_data_account_data = bank_client
                .get_account_data(&program_data_account_key)
                .unwrap()
                .unwrap();
            let program_data_account =
                parse_bpf_upgradeable_loader(&program_data_account_data).unwrap();

            match program_data_account {
                BpfUpgradeableLoaderAccountType::ProgramData(ui_program_data) => ui_program_data
                    .authority
                    .map(|a| Pubkey::from_str(&a).unwrap()),
                _ => None,
            }
        }
        _ => None,
    };

    assert_eq!(Some(new_upgrade_authority_key), upgrade_authority_key);
}

#[test]
#[cfg(feature = "sbf_rust")]
fn test_program_upgradeable_locks() {
    fn setup_program_upgradeable_locks(
        payer_keypair: &Keypair,
        buffer_keypair: &Keypair,
        program_keypair: &Keypair,
    ) -> (Arc<Bank>, Transaction, Transaction) {
        solana_logger::setup();

        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(2_000_000_000);
        let bank = Bank::new_for_tests(&genesis_config);
        let bank = Arc::new(bank);
        let mut bank_client = BankClient::new_shared(bank.clone());

        load_upgradeable_program(
            &bank_client,
            &mint_keypair,
            buffer_keypair,
            program_keypair,
            payer_keypair,
            "solana_sbf_rust_panic",
        );

        // Load the buffer account
        load_upgradeable_buffer(
            &bank_client,
            &mint_keypair,
            buffer_keypair,
            &payer_keypair,
            "solana_sbf_rust_noop",
        );

        let bank = bank_client
            .advance_slot(1, &Pubkey::default())
            .expect("Failed to advance the slot");

        bank_client
            .send_and_confirm_instruction(
                &mint_keypair,
                system_instruction::transfer(
                    &mint_keypair.pubkey(),
                    &payer_keypair.pubkey(),
                    1_000_000_000,
                ),
            )
            .unwrap();

        let invoke_tx = Transaction::new(
            &[payer_keypair],
            Message::new(
                &[Instruction::new_with_bytes(
                    program_keypair.pubkey(),
                    &[0; 0],
                    vec![],
                )],
                Some(&payer_keypair.pubkey()),
            ),
            bank.last_blockhash(),
        );
        let upgrade_tx = Transaction::new(
            &[payer_keypair],
            Message::new(
                &[bpf_loader_upgradeable::upgrade(
                    &program_keypair.pubkey(),
                    &buffer_keypair.pubkey(),
                    &payer_keypair.pubkey(),
                    &payer_keypair.pubkey(),
                )],
                Some(&payer_keypair.pubkey()),
            ),
            bank.last_blockhash(),
        );

        (bank, invoke_tx, upgrade_tx)
    }

    let payer_keypair = keypair_from_seed(&[56u8; 32]).unwrap();
    let buffer_keypair = keypair_from_seed(&[11; 32]).unwrap();
    let program_keypair = keypair_from_seed(&[77u8; 32]).unwrap();

    let results1 = {
        let (bank, invoke_tx, upgrade_tx) =
            setup_program_upgradeable_locks(&payer_keypair, &buffer_keypair, &program_keypair);
        execute_transactions(&bank, vec![upgrade_tx, invoke_tx])
    };

    let results2 = {
        let (bank, invoke_tx, upgrade_tx) =
            setup_program_upgradeable_locks(&payer_keypair, &buffer_keypair, &program_keypair);
        execute_transactions(&bank, vec![invoke_tx, upgrade_tx])
    };

    assert!(matches!(
        results1[0],
        Ok(ConfirmedTransactionWithStatusMeta {
            tx_with_meta: TransactionWithStatusMeta::Complete(VersionedTransactionWithStatusMeta {
                meta: TransactionStatusMeta { status: Ok(()), .. },
                ..
            }),
            ..
        })
    ));
    assert_eq!(results1[1], Err(TransactionError::AccountInUse));

    assert!(matches!(
        results2[0],
        Ok(ConfirmedTransactionWithStatusMeta {
            tx_with_meta: TransactionWithStatusMeta::Complete(VersionedTransactionWithStatusMeta {
                meta: TransactionStatusMeta {
                    status: Err(TransactionError::InstructionError(
                        0,
                        InstructionError::ProgramFailedToComplete
                    )),
                    ..
                },
                ..
            }),
            ..
        })
    ));
    assert_eq!(results2[1], Err(TransactionError::AccountInUse));
}

#[test]
#[cfg(feature = "sbf_rust")]
fn test_program_sbf_finalize() {
    solana_logger::setup();

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50);
    let bank = Bank::new_for_tests(&genesis_config);
    let bank = Arc::new(bank);
    let mut bank_client = BankClient::new_shared(bank.clone());

    let (_, program_pubkey) = load_program_and_advance_slot(
        &mut bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_finalize",
    );

    // Write the noop program into the same program account
    let (program_keypair, _instruction) = load_and_finalize_program(
        &bank_client,
        &bpf_loader::id(),
        None,
        &mint_keypair,
        "solana_sbf_rust_noop",
    );

    bank_client
        .advance_slot(1, &Pubkey::default())
        .expect("Failed to advance the slot");

    let account_metas = vec![
        AccountMeta::new(program_keypair.pubkey(), true),
        AccountMeta::new_readonly(bpf_loader::id(), false),
        AccountMeta::new(rent::id(), false),
    ];
    let instruction = Instruction::new_with_bytes(program_pubkey, &[], account_metas.clone());
    let message = Message::new(&[instruction], Some(&mint_keypair.pubkey()));
    let result = bank_client.send_and_confirm_message(&[&mint_keypair, &program_keypair], message);
    assert_eq!(
        result.unwrap_err().unwrap(),
        TransactionError::InstructionError(0, InstructionError::ProgramFailedToComplete)
    );
}

#[test]
#[cfg(feature = "sbf_rust")]
fn test_program_sbf_ro_account_modify() {
    solana_logger::setup();

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50);
    let bank = Bank::new_for_tests(&genesis_config);
    let bank = Arc::new(bank);
    let mut bank_client = BankClient::new_shared(bank.clone());

    let (bank, program_id) = load_program_and_advance_slot(
        &mut bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_ro_account_modify",
    );

    let argument_keypair = Keypair::new();
    let account = AccountSharedData::new(42, 100, &program_id);
    bank.store_account(&argument_keypair.pubkey(), &account);

    let from_keypair = Keypair::new();
    let account = AccountSharedData::new(84, 0, &system_program::id());
    bank.store_account(&from_keypair.pubkey(), &account);

    let mint_pubkey = mint_keypair.pubkey();
    let account_metas = vec![
        AccountMeta::new_readonly(argument_keypair.pubkey(), false),
        AccountMeta::new_readonly(program_id, false),
    ];

    let instruction = Instruction::new_with_bytes(program_id, &[0], account_metas.clone());
    let message = Message::new(&[instruction], Some(&mint_pubkey));
    let result = bank_client.send_and_confirm_message(&[&mint_keypair], message);
    assert_eq!(
        result.unwrap_err().unwrap(),
        TransactionError::InstructionError(0, InstructionError::ReadonlyDataModified)
    );

    let instruction = Instruction::new_with_bytes(program_id, &[1], account_metas.clone());
    let message = Message::new(&[instruction], Some(&mint_pubkey));
    let result = bank_client.send_and_confirm_message(&[&mint_keypair], message);
    assert_eq!(
        result.unwrap_err().unwrap(),
        TransactionError::InstructionError(0, InstructionError::ReadonlyDataModified)
    );

    let instruction = Instruction::new_with_bytes(program_id, &[2], account_metas.clone());
    let message = Message::new(&[instruction], Some(&mint_pubkey));
    let result = bank_client.send_and_confirm_message(&[&mint_keypair], message);
    assert_eq!(
        result.unwrap_err().unwrap(),
        TransactionError::InstructionError(0, InstructionError::ReadonlyDataModified)
    );
}

#[test]
#[cfg(feature = "sbf_rust")]
fn test_program_sbf_realloc() {
    solana_logger::setup();

    const START_BALANCE: u64 = 100_000_000_000;

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(1_000_000_000_000);
    let mint_pubkey = mint_keypair.pubkey();
    let signer = &[&mint_keypair];
    for direct_mapping in [false, true] {
        let mut bank = Bank::new_for_tests(&genesis_config);
        let feature_set = Arc::make_mut(&mut bank.feature_set);
        // by default test banks have all features enabled, so we only need to
        // disable when needed
        if !direct_mapping {
            feature_set.deactivate(&feature_set::bpf_account_data_direct_mapping::id());
        }
        let bank = Arc::new(bank);
        let mut bank_client = BankClient::new_shared(bank.clone());

        let (bank, program_id) = load_program_and_advance_slot(
            &mut bank_client,
            &bpf_loader::id(),
            &mint_keypair,
            "solana_sbf_rust_realloc",
        );

        let mut bump = 0;
        let keypair = Keypair::new();
        let pubkey = keypair.pubkey();
        let account = AccountSharedData::new(START_BALANCE, 5, &program_id);
        bank.store_account(&pubkey, &account);

        // Realloc RO account
        let mut instruction = realloc(&program_id, &pubkey, 0, &mut bump);
        instruction.accounts[0].is_writable = false;
        assert_eq!(
            bank_client
                .send_and_confirm_message(signer, Message::new(&[instruction], Some(&mint_pubkey),),)
                .unwrap_err()
                .unwrap(),
            TransactionError::InstructionError(0, InstructionError::ReadonlyDataModified)
        );

        // Realloc account to overflow
        assert_eq!(
            bank_client
                .send_and_confirm_message(
                    signer,
                    Message::new(
                        &[realloc(&program_id, &pubkey, usize::MAX, &mut bump)],
                        Some(&mint_pubkey),
                    ),
                )
                .unwrap_err()
                .unwrap(),
            TransactionError::InstructionError(0, InstructionError::InvalidRealloc)
        );

        // Realloc account to 0
        bank_client
            .send_and_confirm_message(
                signer,
                Message::new(
                    &[realloc(&program_id, &pubkey, 0, &mut bump)],
                    Some(&mint_pubkey),
                ),
            )
            .unwrap();
        let data = bank_client.get_account_data(&pubkey).unwrap().unwrap();
        assert_eq!(0, data.len());

        // Realloc account to max then undo
        bank_client
            .send_and_confirm_message(
                signer,
                Message::new(
                    &[realloc_extend_and_undo(
                        &program_id,
                        &pubkey,
                        MAX_PERMITTED_DATA_INCREASE,
                        &mut bump,
                    )],
                    Some(&mint_pubkey),
                ),
            )
            .unwrap();
        let data = bank_client.get_account_data(&pubkey).unwrap().unwrap();
        assert_eq!(0, data.len());

        // Realloc account to max + 1 then undo
        assert_eq!(
            bank_client
                .send_and_confirm_message(
                    signer,
                    Message::new(
                        &[realloc_extend_and_undo(
                            &program_id,
                            &pubkey,
                            MAX_PERMITTED_DATA_INCREASE + 1,
                            &mut bump,
                        )],
                        Some(&mint_pubkey),
                    ),
                )
                .unwrap_err()
                .unwrap(),
            TransactionError::InstructionError(0, InstructionError::InvalidRealloc)
        );

        // Realloc to max + 1
        assert_eq!(
            bank_client
                .send_and_confirm_message(
                    signer,
                    Message::new(
                        &[realloc(
                            &program_id,
                            &pubkey,
                            MAX_PERMITTED_DATA_INCREASE + 1,
                            &mut bump
                        )],
                        Some(&mint_pubkey),
                    ),
                )
                .unwrap_err()
                .unwrap(),
            TransactionError::InstructionError(0, InstructionError::InvalidRealloc)
        );

        // Realloc to max length in max increase increments
        for i in 0..MAX_PERMITTED_DATA_LENGTH as usize / MAX_PERMITTED_DATA_INCREASE {
            let mut bump = i as u64;
            bank_client
                .send_and_confirm_message(
                    signer,
                    Message::new(
                        &[realloc_extend_and_fill(
                            &program_id,
                            &pubkey,
                            MAX_PERMITTED_DATA_INCREASE,
                            1,
                            &mut bump,
                        )],
                        Some(&mint_pubkey),
                    ),
                )
                .unwrap();
            let data = bank_client.get_account_data(&pubkey).unwrap().unwrap();
            assert_eq!((i + 1) * MAX_PERMITTED_DATA_INCREASE, data.len());
        }
        for i in 0..data.len() {
            assert_eq!(data[i], 1);
        }

        // and one more time should fail
        assert_eq!(
            bank_client
                .send_and_confirm_message(
                    signer,
                    Message::new(
                        &[realloc_extend(
                            &program_id,
                            &pubkey,
                            MAX_PERMITTED_DATA_INCREASE,
                            &mut bump
                        )],
                        Some(&mint_pubkey),
                    )
                )
                .unwrap_err()
                .unwrap(),
            TransactionError::InstructionError(0, InstructionError::InvalidRealloc)
        );

        // Realloc to 6 bytes
        bank_client
            .send_and_confirm_message(
                signer,
                Message::new(
                    &[realloc(&program_id, &pubkey, 6, &mut bump)],
                    Some(&mint_pubkey),
                ),
            )
            .unwrap();
        let data = bank_client.get_account_data(&pubkey).unwrap().unwrap();
        assert_eq!(6, data.len());

        // Extend by 2 bytes and write a u64. This ensures that we can do writes that span the original
        // account length (6 bytes) and the realloc data (2 bytes).
        bank_client
            .send_and_confirm_message(
                signer,
                Message::new(
                    &[extend_and_write_u64(
                        &program_id,
                        &pubkey,
                        0x1122334455667788,
                    )],
                    Some(&mint_pubkey),
                ),
            )
            .unwrap();
        let data = bank_client.get_account_data(&pubkey).unwrap().unwrap();
        assert_eq!(8, data.len());
        assert_eq!(0x1122334455667788, unsafe { *data.as_ptr().cast::<u64>() });

        // Realloc to 0
        bank_client
            .send_and_confirm_message(
                signer,
                Message::new(
                    &[realloc(&program_id, &pubkey, 0, &mut bump)],
                    Some(&mint_pubkey),
                ),
            )
            .unwrap();
        let data = bank_client.get_account_data(&pubkey).unwrap().unwrap();
        assert_eq!(0, data.len());

        // Realloc and assign
        bank_client
            .send_and_confirm_message(
                signer,
                Message::new(
                    &[Instruction::new_with_bytes(
                        program_id,
                        &[REALLOC_AND_ASSIGN],
                        vec![AccountMeta::new(pubkey, false)],
                    )],
                    Some(&mint_pubkey),
                ),
            )
            .unwrap();
        let account = bank.get_account(&pubkey).unwrap();
        assert_eq!(&solana_sdk::system_program::id(), account.owner());
        let data = bank_client.get_account_data(&pubkey).unwrap().unwrap();
        assert_eq!(MAX_PERMITTED_DATA_INCREASE, data.len());

        // Realloc to 0 with wrong owner
        assert_eq!(
            bank_client
                .send_and_confirm_message(
                    signer,
                    Message::new(
                        &[realloc(&program_id, &pubkey, 0, &mut bump)],
                        Some(&mint_pubkey),
                    ),
                )
                .unwrap_err()
                .unwrap(),
            TransactionError::InstructionError(0, InstructionError::AccountDataSizeChanged)
        );

        // realloc and assign to self via cpi
        assert_eq!(
            bank_client
                .send_and_confirm_message(
                    &[&mint_keypair, &keypair],
                    Message::new(
                        &[Instruction::new_with_bytes(
                            program_id,
                            &[REALLOC_AND_ASSIGN_TO_SELF_VIA_SYSTEM_PROGRAM],
                            vec![
                                AccountMeta::new(pubkey, true),
                                AccountMeta::new(solana_sdk::system_program::id(), false),
                            ],
                        )],
                        Some(&mint_pubkey),
                    )
                )
                .unwrap_err()
                .unwrap(),
            TransactionError::InstructionError(0, InstructionError::AccountDataSizeChanged)
        );

        // Assign to self and realloc via cpi
        bank_client
            .send_and_confirm_message(
                &[&mint_keypair, &keypair],
                Message::new(
                    &[Instruction::new_with_bytes(
                        program_id,
                        &[ASSIGN_TO_SELF_VIA_SYSTEM_PROGRAM_AND_REALLOC],
                        vec![
                            AccountMeta::new(pubkey, true),
                            AccountMeta::new(solana_sdk::system_program::id(), false),
                        ],
                    )],
                    Some(&mint_pubkey),
                ),
            )
            .unwrap();
        let account = bank.get_account(&pubkey).unwrap();
        assert_eq!(&program_id, account.owner());
        let data = bank_client.get_account_data(&pubkey).unwrap().unwrap();
        assert_eq!(2 * MAX_PERMITTED_DATA_INCREASE, data.len());

        // Realloc to 0
        bank_client
            .send_and_confirm_message(
                signer,
                Message::new(
                    &[realloc(&program_id, &pubkey, 0, &mut bump)],
                    Some(&mint_pubkey),
                ),
            )
            .unwrap();
        let data = bank_client.get_account_data(&pubkey).unwrap().unwrap();
        assert_eq!(0, data.len());

        // zero-init
        bank_client
            .send_and_confirm_message(
                &[&mint_keypair, &keypair],
                Message::new(
                    &[Instruction::new_with_bytes(
                        program_id,
                        &[ZERO_INIT],
                        vec![AccountMeta::new(pubkey, true)],
                    )],
                    Some(&mint_pubkey),
                ),
            )
            .unwrap();
    }
}

#[test]
#[cfg(feature = "sbf_rust")]
fn test_program_sbf_realloc_invoke() {
    solana_logger::setup();

    const START_BALANCE: u64 = 100_000_000_000;

    let GenesisConfigInfo {
        mut genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(1_000_000_000_000);
    genesis_config.rent = Rent::default();
    let mint_pubkey = mint_keypair.pubkey();
    let signer = &[&mint_keypair];

    let bank = Bank::new_for_tests(&genesis_config);
    let bank = Arc::new(bank);
    let mut bank_client = BankClient::new_shared(bank.clone());

    let realloc_program_id = load_program(
        &bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_realloc",
    );

    let (bank, realloc_invoke_program_id) = load_program_and_advance_slot(
        &mut bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_realloc_invoke",
    );

    let mut bump = 0;
    let keypair = Keypair::new();
    let pubkey = keypair.pubkey().clone();
    let account = AccountSharedData::new(START_BALANCE, 5, &realloc_program_id);
    bank.store_account(&pubkey, &account);
    let invoke_keypair = Keypair::new();
    let invoke_pubkey = invoke_keypair.pubkey().clone();

    // Realloc RO account
    assert_eq!(
        bank_client
            .send_and_confirm_message(
                signer,
                Message::new(
                    &[Instruction::new_with_bytes(
                        realloc_invoke_program_id,
                        &[INVOKE_REALLOC_ZERO_RO],
                        vec![
                            AccountMeta::new_readonly(pubkey, false),
                            AccountMeta::new_readonly(realloc_program_id, false),
                        ],
                    )],
                    Some(&mint_pubkey),
                )
            )
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(0, InstructionError::ReadonlyDataModified)
    );
    let account = bank.get_account(&pubkey).unwrap();
    assert_eq!(account.lamports(), START_BALANCE);

    // Realloc account to 0
    bank_client
        .send_and_confirm_message(
            signer,
            Message::new(
                &[realloc(&realloc_program_id, &pubkey, 0, &mut bump)],
                Some(&mint_pubkey),
            ),
        )
        .unwrap();
    let account = bank.get_account(&pubkey).unwrap();
    assert_eq!(account.lamports(), START_BALANCE);
    let data = bank_client.get_account_data(&pubkey).unwrap().unwrap();
    assert_eq!(0, data.len());

    // Realloc to max + 1
    assert_eq!(
        bank_client
            .send_and_confirm_message(
                signer,
                Message::new(
                    &[Instruction::new_with_bytes(
                        realloc_invoke_program_id,
                        &[INVOKE_REALLOC_MAX_PLUS_ONE],
                        vec![
                            AccountMeta::new(pubkey, false),
                            AccountMeta::new_readonly(realloc_program_id, false),
                        ],
                    )],
                    Some(&mint_pubkey),
                )
            )
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(0, InstructionError::InvalidRealloc)
    );

    // Realloc to max twice
    assert_eq!(
        bank_client
            .send_and_confirm_message(
                signer,
                Message::new(
                    &[Instruction::new_with_bytes(
                        realloc_invoke_program_id,
                        &[INVOKE_REALLOC_MAX_TWICE],
                        vec![
                            AccountMeta::new(pubkey, false),
                            AccountMeta::new_readonly(realloc_program_id, false),
                        ],
                    )],
                    Some(&mint_pubkey),
                )
            )
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(0, InstructionError::InvalidRealloc)
    );

    // Realloc account to 0
    bank_client
        .send_and_confirm_message(
            signer,
            Message::new(
                &[realloc(&realloc_program_id, &pubkey, 0, &mut bump)],
                Some(&mint_pubkey),
            ),
        )
        .unwrap();
    let account = bank.get_account(&pubkey).unwrap();
    assert_eq!(account.lamports(), START_BALANCE);
    let data = bank_client.get_account_data(&pubkey).unwrap().unwrap();
    assert_eq!(0, data.len());

    // Realloc and assign
    bank_client
        .send_and_confirm_message(
            signer,
            Message::new(
                &[Instruction::new_with_bytes(
                    realloc_invoke_program_id,
                    &[INVOKE_REALLOC_AND_ASSIGN],
                    vec![
                        AccountMeta::new(pubkey, false),
                        AccountMeta::new_readonly(realloc_program_id, false),
                    ],
                )],
                Some(&mint_pubkey),
            ),
        )
        .unwrap();
    let account = bank.get_account(&pubkey).unwrap();
    assert_eq!(&solana_sdk::system_program::id(), account.owner());
    let data = bank_client.get_account_data(&pubkey).unwrap().unwrap();
    assert_eq!(MAX_PERMITTED_DATA_INCREASE, data.len());

    // Realloc to 0 with wrong owner
    assert_eq!(
        bank_client
            .send_and_confirm_message(
                signer,
                Message::new(
                    &[realloc(&realloc_program_id, &pubkey, 0, &mut bump)],
                    Some(&mint_pubkey),
                )
            )
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(0, InstructionError::AccountDataSizeChanged)
    );

    // realloc and assign to self via system program
    assert_eq!(
        bank_client
            .send_and_confirm_message(
                &[&mint_keypair, &keypair],
                Message::new(
                    &[Instruction::new_with_bytes(
                        realloc_invoke_program_id,
                        &[INVOKE_REALLOC_AND_ASSIGN_TO_SELF_VIA_SYSTEM_PROGRAM],
                        vec![
                            AccountMeta::new(pubkey, true),
                            AccountMeta::new_readonly(realloc_program_id, false),
                            AccountMeta::new_readonly(solana_sdk::system_program::id(), false),
                        ],
                    )],
                    Some(&mint_pubkey),
                )
            )
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(0, InstructionError::AccountDataSizeChanged)
    );

    // Assign to self and realloc via system program
    bank_client
        .send_and_confirm_message(
            &[&mint_keypair, &keypair],
            Message::new(
                &[Instruction::new_with_bytes(
                    realloc_invoke_program_id,
                    &[INVOKE_ASSIGN_TO_SELF_VIA_SYSTEM_PROGRAM_AND_REALLOC],
                    vec![
                        AccountMeta::new(pubkey, true),
                        AccountMeta::new_readonly(realloc_program_id, false),
                        AccountMeta::new_readonly(solana_sdk::system_program::id(), false),
                    ],
                )],
                Some(&mint_pubkey),
            ),
        )
        .unwrap();
    let account = bank.get_account(&pubkey).unwrap();
    assert_eq!(&realloc_program_id, account.owner());
    let data = bank_client.get_account_data(&pubkey).unwrap().unwrap();
    assert_eq!(2 * MAX_PERMITTED_DATA_INCREASE, data.len());

    // Realloc to 0
    bank_client
        .send_and_confirm_message(
            signer,
            Message::new(
                &[realloc(&realloc_program_id, &pubkey, 0, &mut bump)],
                Some(&mint_pubkey),
            ),
        )
        .unwrap();
    let data = bank_client.get_account_data(&pubkey).unwrap().unwrap();
    assert_eq!(0, data.len());

    // Realloc to 100 and check via CPI
    let invoke_account = AccountSharedData::new(START_BALANCE, 5, &realloc_invoke_program_id);
    bank.store_account(&invoke_pubkey, &invoke_account);
    bank_client
        .send_and_confirm_message(
            signer,
            Message::new(
                &[Instruction::new_with_bytes(
                    realloc_invoke_program_id,
                    &[INVOKE_REALLOC_INVOKE_CHECK],
                    vec![
                        AccountMeta::new(invoke_pubkey, false),
                        AccountMeta::new_readonly(realloc_program_id, false),
                    ],
                )],
                Some(&mint_pubkey),
            ),
        )
        .unwrap();
    let data = bank_client
        .get_account_data(&invoke_pubkey)
        .unwrap()
        .unwrap();
    assert_eq!(100, data.len());
    for i in 0..5 {
        assert_eq!(data[i], 0);
    }
    for i in 5..data.len() {
        assert_eq!(data[i], 2);
    }

    // Create account, realloc, check
    let new_keypair = Keypair::new();
    let new_pubkey = new_keypair.pubkey().clone();
    let mut instruction_data = vec![];
    instruction_data.extend_from_slice(&[INVOKE_CREATE_ACCOUNT_REALLOC_CHECK, 1]);
    instruction_data.extend_from_slice(&100_usize.to_le_bytes());
    bank_client
        .send_and_confirm_message(
            &[&mint_keypair, &new_keypair],
            Message::new(
                &[Instruction::new_with_bytes(
                    realloc_invoke_program_id,
                    &instruction_data,
                    vec![
                        AccountMeta::new(mint_pubkey, true),
                        AccountMeta::new(new_pubkey, true),
                        AccountMeta::new(solana_sdk::system_program::id(), false),
                        AccountMeta::new_readonly(realloc_invoke_program_id, false),
                    ],
                )],
                Some(&mint_pubkey),
            ),
        )
        .unwrap();
    let data = bank_client.get_account_data(&new_pubkey).unwrap().unwrap();
    assert_eq!(200, data.len());
    let account = bank.get_account(&new_pubkey).unwrap();
    assert_eq!(&realloc_invoke_program_id, account.owner());

    // Invoke, dealloc, and assign
    let pre_len = 100;
    let new_len = pre_len * 2;
    let mut invoke_account = AccountSharedData::new(START_BALANCE, pre_len, &realloc_program_id);
    invoke_account.set_data_from_slice(&vec![1; pre_len]);
    bank.store_account(&invoke_pubkey, &invoke_account);
    let mut instruction_data = vec![];
    instruction_data.extend_from_slice(&[INVOKE_DEALLOC_AND_ASSIGN, 1]);
    instruction_data.extend_from_slice(&pre_len.to_le_bytes());
    bank_client
        .send_and_confirm_message(
            signer,
            Message::new(
                &[Instruction::new_with_bytes(
                    realloc_invoke_program_id,
                    &instruction_data,
                    vec![
                        AccountMeta::new(invoke_pubkey, false),
                        AccountMeta::new_readonly(realloc_invoke_program_id, false),
                        AccountMeta::new_readonly(realloc_program_id, false),
                    ],
                )],
                Some(&mint_pubkey),
            ),
        )
        .unwrap();
    let data = bank_client
        .get_account_data(&invoke_pubkey)
        .unwrap()
        .unwrap();
    assert_eq!(new_len, data.len());
    for i in 0..new_len {
        assert_eq!(data[i], 0);
    }

    // Realloc to max invoke max
    let invoke_account = AccountSharedData::new(42, 0, &realloc_invoke_program_id);
    bank.store_account(&invoke_pubkey, &invoke_account);
    assert_eq!(
        bank_client
            .send_and_confirm_message(
                signer,
                Message::new(
                    &[Instruction::new_with_bytes(
                        realloc_invoke_program_id,
                        &[INVOKE_REALLOC_MAX_INVOKE_MAX],
                        vec![
                            AccountMeta::new(invoke_pubkey, false),
                            AccountMeta::new_readonly(realloc_program_id, false),
                        ],
                    )],
                    Some(&mint_pubkey),
                )
            )
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(0, InstructionError::InvalidRealloc)
    );

    // CPI realloc extend then local realloc extend
    for (cpi_extend_bytes, local_extend_bytes, should_succeed) in [
        (0, 0, true),
        (MAX_PERMITTED_DATA_INCREASE, 0, true),
        (0, MAX_PERMITTED_DATA_INCREASE, true),
        (MAX_PERMITTED_DATA_INCREASE, 1, false),
        (1, MAX_PERMITTED_DATA_INCREASE, false),
    ] {
        let invoke_account = AccountSharedData::new(100_000_000, 0, &realloc_invoke_program_id);
        bank.store_account(&invoke_pubkey, &invoke_account);
        let mut instruction_data = vec![];
        instruction_data.extend_from_slice(&[INVOKE_REALLOC_TO_THEN_LOCAL_REALLOC_EXTEND, 1]);
        instruction_data.extend_from_slice(&cpi_extend_bytes.to_le_bytes());
        instruction_data.extend_from_slice(&local_extend_bytes.to_le_bytes());

        let result = bank_client.send_and_confirm_message(
            signer,
            Message::new(
                &[Instruction::new_with_bytes(
                    realloc_invoke_program_id,
                    &instruction_data,
                    vec![
                        AccountMeta::new(invoke_pubkey, false),
                        AccountMeta::new_readonly(realloc_invoke_program_id, false),
                    ],
                )],
                Some(&mint_pubkey),
            ),
        );

        if should_succeed {
            assert!(
                result.is_ok(),
                "cpi: {cpi_extend_bytes} local: {local_extend_bytes}, err: {:?}",
                result.err()
            );
        } else {
            assert_eq!(
                result.unwrap_err().unwrap(),
                TransactionError::InstructionError(0, InstructionError::InvalidRealloc),
                "cpi: {cpi_extend_bytes} local: {local_extend_bytes}",
            );
        }
    }

    // Realloc invoke max twice
    let invoke_account = AccountSharedData::new(42, 0, &realloc_program_id);
    bank.store_account(&invoke_pubkey, &invoke_account);
    assert_eq!(
        bank_client
            .send_and_confirm_message(
                signer,
                Message::new(
                    &[Instruction::new_with_bytes(
                        realloc_invoke_program_id,
                        &[INVOKE_INVOKE_MAX_TWICE],
                        vec![
                            AccountMeta::new(invoke_pubkey, false),
                            AccountMeta::new_readonly(realloc_invoke_program_id, false),
                            AccountMeta::new_readonly(realloc_program_id, false),
                        ],
                    )],
                    Some(&mint_pubkey),
                )
            )
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(0, InstructionError::InvalidRealloc)
    );

    // Realloc to 0
    bank_client
        .send_and_confirm_message(
            signer,
            Message::new(
                &[realloc(&realloc_program_id, &pubkey, 0, &mut bump)],
                Some(&mint_pubkey),
            ),
        )
        .unwrap();
    let data = bank_client.get_account_data(&pubkey).unwrap().unwrap();
    assert_eq!(0, data.len());

    // Realloc to max length in max increase increments
    for i in 0..MAX_PERMITTED_DATA_LENGTH as usize / MAX_PERMITTED_DATA_INCREASE {
        bank_client
            .send_and_confirm_message(
                signer,
                Message::new(
                    &[Instruction::new_with_bytes(
                        realloc_invoke_program_id,
                        &[INVOKE_REALLOC_EXTEND_MAX, 1, i as u8, (i / 255) as u8],
                        vec![
                            AccountMeta::new(pubkey, false),
                            AccountMeta::new_readonly(realloc_program_id, false),
                        ],
                    )],
                    Some(&mint_pubkey),
                ),
            )
            .unwrap();
        let data = bank_client.get_account_data(&pubkey).unwrap().unwrap();
        assert_eq!((i + 1) * MAX_PERMITTED_DATA_INCREASE, data.len());
    }
    for i in 0..data.len() {
        assert_eq!(data[i], 1);
    }

    // and one more time should fail
    assert_eq!(
        bank_client
            .send_and_confirm_message(
                signer,
                Message::new(
                    &[Instruction::new_with_bytes(
                        realloc_invoke_program_id,
                        &[INVOKE_REALLOC_EXTEND_MAX, 2, 1, 1],
                        vec![
                            AccountMeta::new(pubkey, false),
                            AccountMeta::new_readonly(realloc_program_id, false),
                        ],
                    )],
                    Some(&mint_pubkey),
                )
            )
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(0, InstructionError::InvalidRealloc)
    );

    // Realloc rescursively and fill data
    let invoke_keypair = Keypair::new();
    let invoke_pubkey = invoke_keypair.pubkey().clone();
    let invoke_account = AccountSharedData::new(START_BALANCE, 0, &realloc_invoke_program_id);
    bank.store_account(&invoke_pubkey, &invoke_account);
    let mut instruction_data = vec![];
    instruction_data.extend_from_slice(&[INVOKE_REALLOC_RECURSIVE, 1]);
    instruction_data.extend_from_slice(&100_usize.to_le_bytes());
    bank_client
        .send_and_confirm_message(
            signer,
            Message::new(
                &[Instruction::new_with_bytes(
                    realloc_invoke_program_id,
                    &instruction_data,
                    vec![
                        AccountMeta::new(invoke_pubkey, false),
                        AccountMeta::new_readonly(realloc_invoke_program_id, false),
                    ],
                )],
                Some(&mint_pubkey),
            ),
        )
        .unwrap();
    let data = bank_client
        .get_account_data(&invoke_pubkey)
        .unwrap()
        .unwrap();
    assert_eq!(200, data.len());
    for i in 0..100 {
        assert_eq!(data[i], 1);
    }
    for i in 100..200 {
        assert_eq!(data[i], 2);
    }
}

#[test]
#[cfg(feature = "sbf_rust")]
fn test_program_sbf_processed_inner_instruction() {
    solana_logger::setup();

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50);
    let bank = Bank::new_for_tests(&genesis_config);
    let bank = Arc::new(bank);
    let mut bank_client = BankClient::new_shared(bank.clone());

    let sibling_program_id = load_program(
        &bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_sibling_instructions",
    );
    let sibling_inner_program_id = load_program(
        &bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_sibling_inner_instructions",
    );
    let noop_program_id = load_program(
        &bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_noop",
    );
    let (_, invoke_and_return_program_id) = load_program_and_advance_slot(
        &mut bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_invoke_and_return",
    );

    let instruction2 = Instruction::new_with_bytes(
        noop_program_id,
        &[43],
        vec![
            AccountMeta::new_readonly(noop_program_id, false),
            AccountMeta::new(mint_keypair.pubkey(), true),
        ],
    );
    let instruction1 = Instruction::new_with_bytes(
        noop_program_id,
        &[42],
        vec![
            AccountMeta::new(mint_keypair.pubkey(), true),
            AccountMeta::new_readonly(noop_program_id, false),
        ],
    );
    let instruction0 = Instruction::new_with_bytes(
        sibling_program_id,
        &[1, 2, 3, 0, 4, 5, 6],
        vec![
            AccountMeta::new(mint_keypair.pubkey(), true),
            AccountMeta::new_readonly(noop_program_id, false),
            AccountMeta::new_readonly(invoke_and_return_program_id, false),
            AccountMeta::new_readonly(sibling_inner_program_id, false),
        ],
    );
    let message = Message::new(
        &[instruction2, instruction1, instruction0],
        Some(&mint_keypair.pubkey()),
    );
    assert!(bank_client
        .send_and_confirm_message(&[&mint_keypair], message)
        .is_ok());
}

#[test]
#[cfg(feature = "sbf_rust")]
fn test_program_fees() {
    solana_logger::setup();

    let congestion_multiplier = 1;

    let GenesisConfigInfo {
        mut genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(500_000_000);
    genesis_config.fee_rate_governor = FeeRateGovernor::new(congestion_multiplier, 0);
    let mut bank = Bank::new_for_tests(&genesis_config);
    let fee_structure =
        FeeStructure::new(0.000005, 0.0, vec![(200, 0.0000005), (1400000, 0.000005)]);
    bank.fee_structure = fee_structure.clone();
    bank.feature_set = Arc::new(FeatureSet::all_enabled());
    let mut bank_client = BankClient::new(bank);

    let (_, program_id) = load_program_and_advance_slot(
        &mut bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_noop",
    );

    let pre_balance = bank_client.get_balance(&mint_keypair.pubkey()).unwrap();
    let message = Message::new(
        &[Instruction::new_with_bytes(program_id, &[], vec![])],
        Some(&mint_keypair.pubkey()),
    );

    let mut feature_set = FeatureSet::all_enabled();
    feature_set.deactivate(&remove_deprecated_request_unit_ix::id());

    let sanitized_message = SanitizedMessage::try_from(message.clone()).unwrap();
    let expected_normal_fee = fee_structure.calculate_fee(
        &sanitized_message,
        congestion_multiplier,
        &ComputeBudget::fee_budget_limits(
            sanitized_message.program_instructions_iter(),
            &feature_set,
        ),
        true,
        false,
    );
    bank_client
        .send_and_confirm_message(&[&mint_keypair], message)
        .unwrap();
    let post_balance = bank_client.get_balance(&mint_keypair.pubkey()).unwrap();
    assert_eq!(pre_balance - post_balance, expected_normal_fee);

    let pre_balance = bank_client.get_balance(&mint_keypair.pubkey()).unwrap();
    let message = Message::new(
        &[
            ComputeBudgetInstruction::set_compute_unit_price(1),
            Instruction::new_with_bytes(program_id, &[], vec![]),
        ],
        Some(&mint_keypair.pubkey()),
    );
    let sanitized_message = SanitizedMessage::try_from(message.clone()).unwrap();
    let mut feature_set = FeatureSet::all_enabled();
    feature_set.deactivate(&remove_deprecated_request_unit_ix::id());
    let expected_prioritized_fee = fee_structure.calculate_fee(
        &sanitized_message,
        congestion_multiplier,
        &ComputeBudget::fee_budget_limits(
            sanitized_message.program_instructions_iter(),
            &feature_set,
        ),
        true,
        false,
    );
    assert!(expected_normal_fee < expected_prioritized_fee);

    bank_client
        .send_and_confirm_message(&[&mint_keypair], message)
        .unwrap();
    let post_balance = bank_client.get_balance(&mint_keypair.pubkey()).unwrap();
    assert_eq!(pre_balance - post_balance, expected_prioritized_fee);
}

#[test]
#[cfg(feature = "sbf_rust")]
fn test_get_minimum_delegation() {
    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(100_123_456_789);
    let mut bank = Bank::new_for_tests(&genesis_config);
    bank.feature_set = Arc::new(FeatureSet::all_enabled());
    let bank = Arc::new(bank);
    let mut bank_client = BankClient::new_shared(bank.clone());

    let (_, program_id) = load_program_and_advance_slot(
        &mut bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_get_minimum_delegation",
    );

    let account_metas = vec![AccountMeta::new_readonly(stake::program::id(), false)];
    let instruction = Instruction::new_with_bytes(program_id, &[], account_metas);
    let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
    assert!(result.is_ok());
}

#[test]
#[cfg(feature = "sbf_rust")]
fn test_program_sbf_inner_instruction_alignment_checks() {
    solana_logger::setup();

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50);
    let bank = Bank::new_for_tests(&genesis_config);
    let noop = create_program(&bank, &bpf_loader_deprecated::id(), "solana_sbf_rust_noop");
    let inner_instruction_alignment_check = create_program(
        &bank,
        &bpf_loader_deprecated::id(),
        "solana_sbf_rust_inner_instruction_alignment_check",
    );

    // invoke unaligned program, which will call aligned program twice,
    // unaligned should be allowed once invoke completes
    let mut bank_client = BankClient::new(bank);
    bank_client
        .advance_slot(1, &Pubkey::default())
        .expect("Failed to advance the slot");
    let mut instruction = Instruction::new_with_bytes(
        inner_instruction_alignment_check,
        &[0],
        vec![
            AccountMeta::new_readonly(noop, false),
            AccountMeta::new_readonly(mint_keypair.pubkey(), false),
        ],
    );

    instruction.data[0] += 1;
    let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction.clone());
    assert!(result.is_ok(), "{result:?}");
}

#[test]
#[cfg(feature = "sbf_rust")]
fn test_cpi_account_ownership_writability() {
    solana_logger::setup();

    for direct_mapping in [false, true] {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(100_123_456_789);
        let mut bank = Bank::new_for_tests(&genesis_config);
        let mut feature_set = FeatureSet::all_enabled();
        if !direct_mapping {
            feature_set.deactivate(&feature_set::bpf_account_data_direct_mapping::id());
        }
        bank.feature_set = Arc::new(feature_set);
        let bank = Arc::new(bank);
        let mut bank_client = BankClient::new_shared(bank);

        let invoke_program_id = load_program(
            &bank_client,
            &bpf_loader::id(),
            &mint_keypair,
            "solana_sbf_rust_invoke",
        );

        let invoked_program_id = load_program(
            &bank_client,
            &bpf_loader::id(),
            &mint_keypair,
            "solana_sbf_rust_invoked",
        );

        let (bank, realloc_program_id) = load_program_and_advance_slot(
            &mut bank_client,
            &bpf_loader::id(),
            &mint_keypair,
            "solana_sbf_rust_realloc",
        );

        let account_keypair = Keypair::new();

        let mint_pubkey = mint_keypair.pubkey();
        let account_metas = vec![
            AccountMeta::new(mint_pubkey, true),
            AccountMeta::new(account_keypair.pubkey(), false),
            AccountMeta::new_readonly(invoked_program_id, false),
            AccountMeta::new_readonly(invoke_program_id, false),
            AccountMeta::new_readonly(realloc_program_id, false),
        ];

        for (account_size, byte_index) in [
            (0, 0),                                     // first realloc byte
            (0, MAX_PERMITTED_DATA_INCREASE as u8),     // last realloc byte
            (2, 0),                                     // first data byte
            (2, 1),                                     // last data byte
            (2, 3),                                     // first realloc byte
            (2, 2 + MAX_PERMITTED_DATA_INCREASE as u8), // last realloc byte
        ] {
            for instruction_id in [
                TEST_FORBID_WRITE_AFTER_OWNERSHIP_CHANGE_IN_CALLEE,
                TEST_FORBID_WRITE_AFTER_OWNERSHIP_CHANGE_IN_CALLER,
            ] {
                bank.register_recent_blockhash(&Hash::new_unique());
                let account = AccountSharedData::new(42, account_size, &invoke_program_id);
                bank.store_account(&account_keypair.pubkey(), &account);

                let instruction = Instruction::new_with_bytes(
                    invoke_program_id,
                    &[instruction_id, byte_index, 42, 42],
                    account_metas.clone(),
                );

                let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);

                if (byte_index as usize) < account_size || direct_mapping {
                    assert_eq!(
                        result.unwrap_err().unwrap(),
                        TransactionError::InstructionError(
                            0,
                            InstructionError::ExternalAccountDataModified
                        )
                    );
                } else {
                    // without direct mapping, changes to the realloc padding
                    // outside the account length are ignored
                    assert!(result.is_ok(), "{result:?}");
                }
            }
        }
        // Test that the CPI code that updates `ref_to_len_in_vm` fails if we
        // make it write to an invalid location. This is the first variant which
        // correctly triggers ExternalAccountDataModified when direct mapping is
        // disabled. When direct mapping is enabled this tests fails early
        // because we move the account data pointer.
        // TEST_FORBID_LEN_UPDATE_AFTER_OWNERSHIP_CHANGE is able to make more
        // progress when direct mapping is on.
        let account = AccountSharedData::new(42, 0, &invoke_program_id);
        bank.store_account(&account_keypair.pubkey(), &account);
        let instruction_data = vec![
            TEST_FORBID_LEN_UPDATE_AFTER_OWNERSHIP_CHANGE_MOVING_DATA_POINTER,
            42,
            42,
            42,
        ];
        let instruction = Instruction::new_with_bytes(
            invoke_program_id,
            &instruction_data,
            account_metas.clone(),
        );
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        assert_eq!(
            result.unwrap_err().unwrap(),
            if direct_mapping {
                // We move the data pointer, direct mapping doesn't allow it
                // anymore so it errors out earlier. See
                // test_cpi_invalid_account_info_pointers.
                TransactionError::InstructionError(0, InstructionError::ProgramFailedToComplete)
            } else {
                // We managed to make CPI write into the account data, but the
                // usual checks still apply and we get an error.
                TransactionError::InstructionError(0, InstructionError::ExternalAccountDataModified)
            }
        );

        // We're going to try and make CPI write ref_to_len_in_vm into a 2nd
        // account, so we add an extra one here.
        let account2_keypair = Keypair::new();
        let mut account_metas = account_metas.clone();
        account_metas.push(AccountMeta::new(account2_keypair.pubkey(), false));

        for target_account in [1, account_metas.len() as u8 - 1] {
            // Similar to the test above where we try to make CPI write into account
            // data. This variant is for when direct mapping is enabled.
            let account = AccountSharedData::new(42, 0, &invoke_program_id);
            bank.store_account(&account_keypair.pubkey(), &account);
            let account = AccountSharedData::new(42, 0, &invoke_program_id);
            bank.store_account(&account2_keypair.pubkey(), &account);
            let instruction_data = vec![
                TEST_FORBID_LEN_UPDATE_AFTER_OWNERSHIP_CHANGE,
                target_account,
                42,
                42,
            ];
            let instruction = Instruction::new_with_bytes(
                invoke_program_id,
                &instruction_data,
                account_metas.clone(),
            );
            let message = Message::new(&[instruction], Some(&mint_pubkey));
            let tx = Transaction::new(&[&mint_keypair], message.clone(), bank.last_blockhash());
            let (result, _, logs) = process_transaction_and_record_inner(&bank, tx);
            if direct_mapping {
                assert_eq!(
                    result.unwrap_err(),
                    TransactionError::InstructionError(
                        0,
                        InstructionError::ProgramFailedToComplete
                    )
                );
                // We haven't moved the data pointer, but ref_to_len_vm _is_ in
                // the account data vm range and that's not allowed either.
                assert!(
                    logs.iter().any(|log| log.contains("Invalid pointer")),
                    "{logs:?}"
                );
            } else {
                // we expect this to succeed as after updating `ref_to_len_in_vm`,
                // CPI will sync the actual account data between the callee and the
                // caller, _always_ writing over the location pointed by
                // `ref_to_len_in_vm`. To verify this, we check that the account
                // data is in fact all zeroes like it is in the callee.
                result.unwrap();
                let account = bank.get_account(&account_keypair.pubkey()).unwrap();
                assert_eq!(account.data(), vec![0; 40]);
            }
        }
    }
}

#[test]
#[cfg(feature = "sbf_rust")]
fn test_cpi_account_data_updates() {
    solana_logger::setup();

    for direct_mapping in [false, true] {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(100_123_456_789);
        let mut bank = Bank::new_for_tests(&genesis_config);
        let mut feature_set = FeatureSet::all_enabled();
        if !direct_mapping {
            feature_set.deactivate(&feature_set::bpf_account_data_direct_mapping::id());
        }
        bank.feature_set = Arc::new(feature_set);
        let bank = Arc::new(bank);
        let mut bank_client = BankClient::new_shared(bank);

        let invoke_program_id = load_program(
            &bank_client,
            &bpf_loader::id(),
            &mint_keypair,
            "solana_sbf_rust_invoke",
        );

        let (bank, realloc_program_id) = load_program_and_advance_slot(
            &mut bank_client,
            &bpf_loader::id(),
            &mint_keypair,
            "solana_sbf_rust_realloc",
        );

        let account_keypair = Keypair::new();
        let mint_pubkey = mint_keypair.pubkey();
        let account_metas = vec![
            AccountMeta::new(mint_pubkey, true),
            AccountMeta::new(account_keypair.pubkey(), false),
            AccountMeta::new_readonly(realloc_program_id, false),
            AccountMeta::new_readonly(invoke_program_id, false),
        ];

        // This tests the case where a caller extends an account beyond the original
        // data length. The callee should see the extended data (asserted in the
        // callee program, not here).
        let mut account = AccountSharedData::new(42, 0, &invoke_program_id);
        account.set_data(b"foo".to_vec());
        bank.store_account(&account_keypair.pubkey(), &account);
        let mut instruction_data = vec![TEST_CPI_ACCOUNT_UPDATE_CALLER_GROWS];
        instruction_data.extend_from_slice(b"bar");
        let instruction = Instruction::new_with_bytes(
            invoke_program_id,
            &instruction_data,
            account_metas.clone(),
        );
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        assert!(result.is_ok(), "{result:?}");
        let account = bank.get_account(&account_keypair.pubkey()).unwrap();
        // "bar" here was copied from the realloc region
        assert_eq!(account.data(), b"foobar");

        // This tests the case where a callee extends an account beyond the original
        // data length. The caller should see the extended data where the realloc
        // region contains the new data. In this test the callee owns the account,
        // the caller can't write but the CPI glue still updates correctly.
        let mut account = AccountSharedData::new(42, 0, &realloc_program_id);
        account.set_data(b"foo".to_vec());
        bank.store_account(&account_keypair.pubkey(), &account);
        let mut instruction_data = vec![TEST_CPI_ACCOUNT_UPDATE_CALLEE_GROWS];
        instruction_data.extend_from_slice(b"bar");
        let instruction = Instruction::new_with_bytes(
            invoke_program_id,
            &instruction_data,
            account_metas.clone(),
        );
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        result.unwrap();
        let account = bank.get_account(&account_keypair.pubkey()).unwrap();
        // "bar" here was copied from the realloc region
        assert_eq!(account.data(), b"foobar");

        // This tests the case where a callee shrinks an account, the caller data
        // slice must be truncated accordingly and post_len..original_data_len must
        // be zeroed (zeroing is checked in the invoked program not here). Same as
        // above, the callee owns the account but the changes are still reflected in
        // the caller even if things are readonly from the caller's POV.
        let mut account = AccountSharedData::new(42, 0, &realloc_program_id);
        account.set_data(b"foobar".to_vec());
        bank.store_account(&account_keypair.pubkey(), &account);
        let mut instruction_data =
            vec![TEST_CPI_ACCOUNT_UPDATE_CALLEE_SHRINKS_SMALLER_THAN_ORIGINAL_LEN];
        instruction_data.extend_from_slice(4usize.to_le_bytes().as_ref());
        let instruction = Instruction::new_with_bytes(
            invoke_program_id,
            &instruction_data,
            account_metas.clone(),
        );
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        assert!(result.is_ok(), "{result:?}");
        let account = bank.get_account(&account_keypair.pubkey()).unwrap();
        assert_eq!(account.data(), b"foob");

        // This tests the case where the program extends an account, then calls
        // itself and in the inner call it shrinks the account to a size that is
        // still larger than the original size. The account data must be set to the
        // correct value in the caller frame, and the realloc region must be zeroed
        // (again tested in the invoked program).
        let mut account = AccountSharedData::new(42, 0, &invoke_program_id);
        account.set_data(b"foo".to_vec());
        bank.store_account(&account_keypair.pubkey(), &account);
        let mut instruction_data = vec![TEST_CPI_ACCOUNT_UPDATE_CALLER_GROWS_CALLEE_SHRINKS];
        // realloc to "foobazbad" then shrink to "foobazb"
        instruction_data.extend_from_slice(7usize.to_le_bytes().as_ref());
        instruction_data.extend_from_slice(b"bazbad");
        let instruction = Instruction::new_with_bytes(
            invoke_program_id,
            &instruction_data,
            account_metas.clone(),
        );
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        assert!(result.is_ok(), "{result:?}");
        let account = bank.get_account(&account_keypair.pubkey()).unwrap();
        assert_eq!(account.data(), b"foobazb");

        // Similar to the test above, but this time the nested invocation shrinks to
        // _below_ the original data length. Both the spare capacity in the account
        // data _end_ the realloc region must be zeroed.
        let mut account = AccountSharedData::new(42, 0, &invoke_program_id);
        account.set_data(b"foo".to_vec());
        bank.store_account(&account_keypair.pubkey(), &account);
        let mut instruction_data = vec![TEST_CPI_ACCOUNT_UPDATE_CALLER_GROWS_CALLEE_SHRINKS];
        // realloc to "foobazbad" then shrink to "f"
        instruction_data.extend_from_slice(1usize.to_le_bytes().as_ref());
        instruction_data.extend_from_slice(b"bazbad");
        let instruction = Instruction::new_with_bytes(
            invoke_program_id,
            &instruction_data,
            account_metas.clone(),
        );
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        assert!(result.is_ok(), "{result:?}");
        let account = bank.get_account(&account_keypair.pubkey()).unwrap();
        assert_eq!(account.data(), b"f");
    }
}

#[test]
#[cfg(feature = "sbf_rust")]
fn test_cpi_deprecated_loader_realloc() {
    solana_logger::setup();

    for direct_mapping in [false, true] {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(100_123_456_789);
        let mut bank = Bank::new_for_tests(&genesis_config);
        let mut feature_set = FeatureSet::all_enabled();
        if !direct_mapping {
            feature_set.deactivate(&feature_set::bpf_account_data_direct_mapping::id());
        }
        bank.feature_set = Arc::new(feature_set);
        let bank = Arc::new(bank);

        let deprecated_program_id = create_program(
            &bank,
            &bpf_loader_deprecated::id(),
            "solana_sbf_rust_deprecated_loader",
        );

        let mut bank_client = BankClient::new_shared(bank);

        let (bank, invoke_program_id) = load_program_and_advance_slot(
            &mut bank_client,
            &bpf_loader::id(),
            &mint_keypair,
            "solana_sbf_rust_invoke",
        );

        let account_keypair = Keypair::new();

        let mint_pubkey = mint_keypair.pubkey();
        let account_metas = vec![
            AccountMeta::new(mint_pubkey, true),
            AccountMeta::new(account_keypair.pubkey(), false),
            AccountMeta::new_readonly(deprecated_program_id, false),
            AccountMeta::new_readonly(deprecated_program_id, false),
            AccountMeta::new_readonly(invoke_program_id, false),
        ];

        // If a bpf_loader_deprecated program extends an account, the callee
        // accidentally sees the extended data when direct mapping is off, but
        // direct mapping fixes the issue
        let mut account = AccountSharedData::new(42, 0, &deprecated_program_id);
        account.set_data(b"foo".to_vec());
        bank.store_account(&account_keypair.pubkey(), &account);
        let mut instruction_data = vec![TEST_CPI_ACCOUNT_UPDATE_CALLER_GROWS];
        instruction_data.extend_from_slice(b"bar");
        let instruction = Instruction::new_with_bytes(
            deprecated_program_id,
            &instruction_data,
            account_metas.clone(),
        );
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        // when direct mapping is off, the realloc will accidentally clobber
        // whatever comes after the data slice (owner, executable, rent epoch
        // etc). When direct mapping is on, you get an InvalidRealloc error.
        if direct_mapping {
            assert_eq!(
                result.unwrap_err().unwrap(),
                TransactionError::InstructionError(0, InstructionError::InvalidRealloc)
            );
        } else {
            assert_eq!(
                result.unwrap_err().unwrap(),
                TransactionError::InstructionError(0, InstructionError::ModifiedProgramId)
            );
        }

        // check that if a bpf_loader_deprecated program extends an account, the
        // extended data is ignored
        let mut account = AccountSharedData::new(42, 0, &deprecated_program_id);
        account.set_data(b"foo".to_vec());
        bank.store_account(&account_keypair.pubkey(), &account);
        let mut instruction_data = vec![TEST_CPI_ACCOUNT_UPDATE_CALLEE_GROWS];
        instruction_data.extend_from_slice(b"bar");
        let instruction = Instruction::new_with_bytes(
            invoke_program_id,
            &instruction_data,
            account_metas.clone(),
        );
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        assert!(result.is_ok(), "{result:?}");
        let account = bank.get_account(&account_keypair.pubkey()).unwrap();
        assert_eq!(account.data(), b"foo");

        // check that if a bpf_loader_deprecated program truncates an account,
        // the caller doesn't see the truncation
        let mut account = AccountSharedData::new(42, 0, &deprecated_program_id);
        account.set_data(b"foobar".to_vec());
        bank.store_account(&account_keypair.pubkey(), &account);
        let mut instruction_data =
            vec![TEST_CPI_ACCOUNT_UPDATE_CALLEE_SHRINKS_SMALLER_THAN_ORIGINAL_LEN];
        instruction_data.extend_from_slice(4usize.to_le_bytes().as_ref());
        let instruction = Instruction::new_with_bytes(
            invoke_program_id,
            &instruction_data,
            account_metas.clone(),
        );
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        assert!(result.is_ok(), "{result:?}");
        let account = bank.get_account(&account_keypair.pubkey()).unwrap();
        assert_eq!(account.data(), b"foobar");
    }
}

#[test]
#[cfg(feature = "sbf_rust")]
fn test_cpi_change_account_data_memory_allocation() {
    use solana_program_runtime::{declare_process_instruction, loaded_programs::LoadedProgram};

    solana_logger::setup();

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(100_123_456_789);
    let mut bank = Bank::new_for_tests(&genesis_config);
    let feature_set = FeatureSet::all_enabled();
    bank.feature_set = Arc::new(feature_set);

    declare_process_instruction!(MockBuiltin, 42, |invoke_context| {
        let transaction_context = &invoke_context.transaction_context;
        let instruction_context = transaction_context.get_current_instruction_context()?;
        let instruction_data = instruction_context.get_instruction_data();

        let index_in_transaction =
            instruction_context.get_index_of_instruction_account_in_transaction(0)?;

        let mut account = transaction_context
            .accounts()
            .get(index_in_transaction)
            .unwrap()
            .borrow_mut();

        // Test changing the account data both in place and by changing the
        // underlying vector. CPI will have to detect the vector change and
        // update the corresponding memory region. In all cases CPI will have
        // to zero the spare bytes correctly.
        match instruction_data[0] {
            0xFE => account.set_data(instruction_data.to_vec()),
            0xFD => account.set_data_from_slice(instruction_data),
            0xFC => {
                // Exercise the update_caller_account capacity check where account len != capacity.
                let mut data = instruction_data.to_vec();
                data.reserve_exact(1);
                account.set_data(data)
            }
            _ => panic!(),
        }

        Ok(())
    });

    let builtin_program_id = Pubkey::new_unique();
    bank.add_builtin(
        builtin_program_id,
        "test_cpi_change_account_data_memory_allocation_builtin".to_string(),
        LoadedProgram::new_builtin(0, 42, MockBuiltin::vm),
    );

    let bank = Arc::new(bank);
    let mut bank_client = BankClient::new_shared(bank);

    let (bank, invoke_program_id) = load_program_and_advance_slot(
        &mut bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_invoke",
    );

    let account_keypair = Keypair::new();
    let mint_pubkey = mint_keypair.pubkey();
    let account_metas = vec![
        AccountMeta::new(mint_pubkey, true),
        AccountMeta::new(account_keypair.pubkey(), false),
        AccountMeta::new_readonly(builtin_program_id, false),
        AccountMeta::new_readonly(invoke_program_id, false),
    ];

    let mut account = AccountSharedData::new(42, 20, &builtin_program_id);
    account.set_data(vec![0xFF; 20]);
    bank.store_account(&account_keypair.pubkey(), &account);
    let mut instruction_data = vec![TEST_CPI_CHANGE_ACCOUNT_DATA_MEMORY_ALLOCATION];
    instruction_data.extend_from_slice(builtin_program_id.as_ref());
    let instruction =
        Instruction::new_with_bytes(invoke_program_id, &instruction_data, account_metas.clone());

    let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
#[cfg(feature = "sbf_rust")]
fn test_cpi_invalid_account_info_pointers() {
    solana_logger::setup();

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(100_123_456_789);
    let mut bank = Bank::new_for_tests(&genesis_config);
    let feature_set = FeatureSet::all_enabled();
    bank.feature_set = Arc::new(feature_set);
    let bank = Arc::new(bank);
    let mut bank_client = BankClient::new_shared(bank);

    let c_invoke_program_id =
        load_program(&bank_client, &bpf_loader::id(), &mint_keypair, "invoke");

    let (bank, invoke_program_id) = load_program_and_advance_slot(
        &mut bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_invoke",
    );

    let account_keypair = Keypair::new();
    let mint_pubkey = mint_keypair.pubkey();
    let account_metas = vec![
        AccountMeta::new(mint_pubkey, true),
        AccountMeta::new(account_keypair.pubkey(), false),
        AccountMeta::new_readonly(invoke_program_id, false),
        AccountMeta::new_readonly(c_invoke_program_id, false),
    ];

    for invoke_program_id in [invoke_program_id, c_invoke_program_id] {
        for ix in [
            TEST_CPI_INVALID_KEY_POINTER,
            TEST_CPI_INVALID_LAMPORTS_POINTER,
            TEST_CPI_INVALID_OWNER_POINTER,
            TEST_CPI_INVALID_DATA_POINTER,
        ] {
            let account = AccountSharedData::new(42, 5, &invoke_program_id);
            bank.store_account(&account_keypair.pubkey(), &account);
            let instruction = Instruction::new_with_bytes(
                invoke_program_id,
                &[ix, 42, 42, 42],
                account_metas.clone(),
            );

            let message = Message::new(&[instruction], Some(&mint_pubkey));
            let tx = Transaction::new(&[&mint_keypair], message.clone(), bank.last_blockhash());
            let (result, _, logs) = process_transaction_and_record_inner(&bank, tx);
            assert!(result.is_err(), "{result:?}");
            assert!(
                logs.iter().any(|log| log.contains("Invalid pointer")),
                "{logs:?}"
            );
        }
    }
}

#[test]
#[cfg(feature = "sbf_rust")]
fn test_deny_executable_write() {
    solana_logger::setup();

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(100_123_456_789);

    for direct_mapping in [false, true] {
        let mut bank = Bank::new_for_tests(&genesis_config);
        let feature_set = Arc::make_mut(&mut bank.feature_set);
        // by default test banks have all features enabled, so we only need to
        // disable when needed
        if !direct_mapping {
            feature_set.deactivate(&feature_set::bpf_account_data_direct_mapping::id());
        }
        let bank = Arc::new(bank);
        let mut bank_client = BankClient::new_shared(bank);

        let (_bank, invoke_program_id) = load_program_and_advance_slot(
            &mut bank_client,
            &bpf_loader::id(),
            &mint_keypair,
            "solana_sbf_rust_invoke",
        );

        let account_keypair = Keypair::new();
        let mint_pubkey = mint_keypair.pubkey();
        let account_metas = vec![
            AccountMeta::new(mint_pubkey, true),
            AccountMeta::new(account_keypair.pubkey(), false),
            AccountMeta::new_readonly(invoke_program_id, false),
        ];

        let mut instruction_data = vec![TEST_WRITE_ACCOUNT, 2];
        instruction_data.extend_from_slice(4usize.to_le_bytes().as_ref());
        instruction_data.push(42);
        let instruction = Instruction::new_with_bytes(
            invoke_program_id,
            &instruction_data,
            account_metas.clone(),
        );
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        assert_eq!(
            result.unwrap_err().unwrap(),
            TransactionError::InstructionError(0, InstructionError::ExecutableDataModified)
        );
    }
}

#![cfg(any(feature = "sbf_c", feature = "sbf_rust"))]

#[macro_use]
extern crate solana_bpf_loader_program;

#[cfg(feature = "sbf_rust")]
use {
    itertools::izip,
    solana_account_decoder::parse_bpf_loader::{
        parse_bpf_upgradeable_loader, BpfUpgradeableLoaderAccountType,
    },
    solana_ledger::token_balances::collect_token_balances,
    solana_program_runtime::{
        compute_budget::{self, ComputeBudget},
        invoke_context::InvokeContext,
        timings::ExecuteTimings,
    },
    solana_runtime::{
        bank::{
            DurableNonceFee, InnerInstruction, TransactionBalancesSet, TransactionExecutionDetails,
            TransactionExecutionResult, TransactionResults,
        },
        loader_utils::{
            load_buffer_account, load_upgradeable_program, set_upgrade_authority, upgrade_program,
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
        feature_set::FeatureSet,
        fee::FeeStructure,
        fee_calculator::FeeRateGovernor,
        loader_instruction,
        message::{v0::LoadedAddresses, SanitizedMessage},
        rent::Rent,
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
    std::{collections::HashMap, str::FromStr},
};
use {
    solana_bpf_loader_program::{
        create_vm,
        serialization::{deserialize_parameters, serialize_parameters},
        syscalls::create_loader,
    },
    solana_program_runtime::invoke_context::with_mock_invoke_context,
    solana_rbpf::{elf::Executable, verifier::RequisiteVerifier, vm::VerifiedExecutable},
    solana_runtime::{
        bank::Bank,
        bank_client::BankClient,
        genesis_utils::{create_genesis_config, GenesisConfigInfo},
        loader_utils::load_program,
    },
    solana_sdk::{
        account::AccountSharedData,
        bpf_loader, bpf_loader_deprecated,
        client::SyncClient,
        entrypoint::SUCCESS,
        instruction::{AccountMeta, Instruction, InstructionError},
        message::Message,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        system_program,
        transaction::{SanitizedTransaction, Transaction, TransactionError},
    },
    std::{env, fs::File, io::Read, path::PathBuf, sync::Arc},
};

/// SBF program file extension
const PLATFORM_FILE_EXTENSION_SBF: &str = "so";

/// Create a SBF program file name
fn create_sbf_path(name: &str) -> PathBuf {
    let mut pathbuf = {
        let current_exe = env::current_exe().unwrap();
        PathBuf::from(current_exe.parent().unwrap().parent().unwrap())
    };
    pathbuf.push("sbf/");
    pathbuf.push(name);
    pathbuf.set_extension(PLATFORM_FILE_EXTENSION_SBF);
    pathbuf
}

fn load_sbf_program(
    bank_client: &BankClient,
    loader_id: &Pubkey,
    payer_keypair: &Keypair,
    name: &str,
) -> Pubkey {
    let elf = read_sbf_program(name);
    load_program(bank_client, payer_keypair, loader_id, elf)
}

fn read_sbf_program(name: &str) -> Vec<u8> {
    let path = create_sbf_path(name);
    let mut file = File::open(&path).unwrap_or_else(|err| {
        panic!("Failed to open {}: {}", path.display(), err);
    });
    let mut elf = Vec::new();
    file.read_to_end(&mut elf).unwrap();

    elf
}

#[cfg(feature = "sbf_rust")]
fn write_sbf_program(
    bank_client: &BankClient,
    loader_id: &Pubkey,
    payer_keypair: &Keypair,
    program_keypair: &Keypair,
    elf: &[u8],
) {
    let chunk_size = 512; // Size of chunk just needs to fit into tx
    let mut offset = 0;
    for chunk in elf.chunks(chunk_size) {
        let instruction =
            loader_instruction::write(&program_keypair.pubkey(), loader_id, offset, chunk.to_vec());
        let message = Message::new(&[instruction], Some(&payer_keypair.pubkey()));

        bank_client
            .send_and_confirm_message(&[payer_keypair, &program_keypair], message)
            .unwrap();

        offset += chunk_size as u32;
    }
}

#[cfg(feature = "sbf_rust")]
fn load_upgradeable_sbf_program(
    bank_client: &BankClient,
    payer_keypair: &Keypair,
    buffer_keypair: &Keypair,
    executable_keypair: &Keypair,
    authority_keypair: &Keypair,
    name: &str,
) {
    let path = create_sbf_path(name);
    let mut file = File::open(&path).unwrap_or_else(|err| {
        panic!("Failed to open {}: {}", path.display(), err);
    });
    let mut elf = Vec::new();
    file.read_to_end(&mut elf).unwrap();
    load_upgradeable_program(
        bank_client,
        payer_keypair,
        buffer_keypair,
        executable_keypair,
        authority_keypair,
        elf,
    );
    bank_client.set_sysvar_for_tests(&clock::Clock {
        slot: 1,
        ..clock::Clock::default()
    });
}

#[cfg(feature = "sbf_rust")]
fn load_upgradeable_buffer(
    bank_client: &BankClient,
    payer_keypair: &Keypair,
    buffer_keypair: &Keypair,
    buffer_authority_keypair: &Keypair,
    name: &str,
) {
    let path = create_sbf_path(name);
    let mut file = File::open(&path).unwrap_or_else(|err| {
        panic!("Failed to open {}: {}", path.display(), err);
    });
    let mut elf = Vec::new();
    file.read_to_end(&mut elf).unwrap();
    load_buffer_account(
        bank_client,
        payer_keypair,
        &buffer_keypair,
        buffer_authority_keypair,
        &elf,
    );
}

#[cfg(feature = "sbf_rust")]
fn upgrade_sbf_program(
    bank_client: &BankClient,
    payer_keypair: &Keypair,
    buffer_keypair: &Keypair,
    executable_pubkey: &Pubkey,
    authority_keypair: &Keypair,
    name: &str,
) {
    load_upgradeable_buffer(
        bank_client,
        payer_keypair,
        buffer_keypair,
        authority_keypair,
        name,
    );
    upgrade_program(
        bank_client,
        payer_keypair,
        executable_pubkey,
        &buffer_keypair.pubkey(),
        &authority_keypair,
        &payer_keypair.pubkey(),
    );
}

fn run_program(name: &str) -> u64 {
    let mut file = File::open(create_sbf_path(name)).unwrap();
    let mut data = vec![];
    file.read_to_end(&mut data).unwrap();
    let loader_id = bpf_loader::id();
    with_mock_invoke_context(loader_id, 0, false, |invoke_context| {
        let loader = create_loader(
            &invoke_context.feature_set,
            &ComputeBudget::default(),
            true,
            true,
            true,
        )
        .unwrap();
        let executable = Executable::<InvokeContext>::from_elf(&data, loader).unwrap();

        #[allow(unused_mut)]
        let mut verified_executable =
            VerifiedExecutable::<RequisiteVerifier, InvokeContext>::from_executable(executable)
                .unwrap();

        let run_program_iterations = {
            #[cfg(target_arch = "x86_64")]
            {
                verified_executable.jit_compile().unwrap();
                2
            }
            #[cfg(not(target_arch = "x86_64"))]
            1
        };

        let mut instruction_count = 0;
        let mut trace_log = None;
        for i in 0..run_program_iterations {
            let transaction_context = &mut invoke_context.transaction_context;
            let instruction_context = transaction_context
                .get_current_instruction_context()
                .unwrap();
            let caller = *instruction_context
                .get_last_program_key(transaction_context)
                .unwrap();
            transaction_context
                .set_return_data(caller, Vec::new())
                .unwrap();

            let (parameter_bytes, regions, account_lengths) = serialize_parameters(
                invoke_context.transaction_context,
                invoke_context
                    .transaction_context
                    .get_current_instruction_context()
                    .unwrap(),
                true, // should_cap_ix_accounts
            )
            .unwrap();

            {
                let mut vm = create_vm(
                    &verified_executable,
                    regions,
                    account_lengths.clone(),
                    invoke_context,
                )
                .unwrap();
                let (compute_units_consumed, result) = vm.execute_program(i == 0);
                assert_eq!(SUCCESS, result.unwrap());
                if i == 1 {
                    assert_eq!(instruction_count, compute_units_consumed);
                }
                instruction_count = compute_units_consumed;
                if i == 0 {
                    trace_log = Some(vm.env.context_object_pointer.trace_log.clone());
                } else {
                    let interpreter = trace_log.as_ref().unwrap().as_slice();
                    let mut jit = vm.env.context_object_pointer.trace_log.as_slice();
                    if jit.len() > interpreter.len() {
                        jit = &jit[0..interpreter.len()];
                    }
                    assert_eq!(interpreter, jit);
                    trace_log = None;
                }
            }
            assert!(match deserialize_parameters(
                invoke_context.transaction_context,
                invoke_context
                    .transaction_context
                    .get_current_instruction_context()
                    .unwrap(),
                parameter_bytes.as_slice(),
                &account_lengths,
            ) {
                Ok(()) => true,
                Err(InstructionError::ModifiedProgramId) => true,
                Err(InstructionError::ExternalAccountLamportSpend) => true,
                Err(InstructionError::ReadonlyLamportChange) => true,
                Err(InstructionError::ExecutableLamportChange) => true,
                Err(InstructionError::ExecutableAccountNotRentExempt) => true,
                Err(InstructionError::ExecutableModified) => true,
                Err(InstructionError::AccountDataSizeChanged) => true,
                Err(InstructionError::InvalidRealloc) => true,
                Err(InstructionError::ExecutableDataModified) => true,
                Err(InstructionError::ReadonlyDataModified) => true,
                Err(InstructionError::ExternalAccountDataModified) => true,
                _ => false,
            });
        }
        instruction_count
    })
}

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

#[test]
#[cfg(any(feature = "sbf_c", feature = "sbf_rust"))]
fn test_program_sbf_sanity() {
    solana_logger::setup();

    let mut programs = Vec::new();
    #[cfg(feature = "sbf_c")]
    {
        programs.extend_from_slice(&[
            ("alloc", true),
            ("sbf_to_sbf", true),
            ("float", true),
            ("multiple_static", true),
            ("noop", true),
            ("noop++", true),
            ("panic", false),
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

        let mut bank = Bank::new_for_tests(&genesis_config);
        let (name, id, entrypoint) = solana_bpf_loader_program!();
        bank.add_builtin(&name, &id, entrypoint);
        let bank_client = BankClient::new(bank);

        // Call user program
        let program_id =
            load_sbf_program(&bank_client, &bpf_loader::id(), &mint_keypair, program.0);
        let account_metas = vec![
            AccountMeta::new(mint_keypair.pubkey(), true),
            AccountMeta::new(Keypair::new().pubkey(), false),
        ];
        let instruction = Instruction::new_with_bytes(program_id, &[1], account_metas);
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        if program.1 {
            assert!(result.is_ok());
        } else {
            assert!(result.is_err());
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
            .remove(&solana_sdk::feature_set::disable_deprecated_loader::id())
            .unwrap();
        genesis_config
            .accounts
            .remove(&solana_sdk::feature_set::disable_deploy_of_alloc_free_syscall::id())
            .unwrap();
        let mut bank = Bank::new_for_tests(&genesis_config);
        let (name, id, entrypoint) = solana_bpf_loader_deprecated_program!();
        bank.add_builtin(&name, &id, entrypoint);
        let bank_client = BankClient::new(bank);

        let program_id = load_sbf_program(
            &bank_client,
            &bpf_loader_deprecated::id(),
            &mint_keypair,
            program,
        );
        let account_metas = vec![AccountMeta::new(mint_keypair.pubkey(), true)];
        let instruction = Instruction::new_with_bytes(program_id, &[1], account_metas);
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
    let loader_address = bpf_loader_deprecated::id();

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50);
    let mut bank = Bank::new_for_tests(&genesis_config);

    bank.deactivate_feature(&solana_sdk::feature_set::disable_deprecated_loader::id());
    let (name, id, entrypoint) = solana_bpf_loader_deprecated_program!();
    bank.add_builtin(&name, &id, entrypoint);

    // Populate loader account with elf that depends on _sol_alloc_free syscall
    let elf = read_sbf_program("solana_sbf_rust_deprecated_loader");
    let mut program_account = AccountSharedData::new(1, elf.len(), &loader_address);
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
                &loader_address,
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
                &[1],
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
    bank.clear_executors();

    // Try and finalize the program now that sol_alloc_free is re-enabled
    assert!(bank.process_transaction(&finalize_tx).is_ok());

    // invoke the program
    assert!(bank.process_transaction(&invoke_tx).is_ok());

    // disable _sol_alloc_free
    bank.activate_feature(&solana_sdk::feature_set::disable_deploy_of_alloc_free_syscall::id());
    bank.clear_signatures();

    // invoke should still succeed because cached
    assert!(bank.process_transaction(&invoke_tx).is_ok());

    bank.clear_signatures();
    bank.clear_executors();

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
        let mut bank = Bank::new_for_tests(&genesis_config);
        let (name, id, entrypoint) = solana_bpf_loader_program!();
        bank.add_builtin(&name, &id, entrypoint);
        let bank = Arc::new(bank);
        let bank_client = BankClient::new_shared(&bank);
        let program_id = load_sbf_program(&bank_client, &bpf_loader::id(), &mint_keypair, program);
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
        let mut bank = Bank::new_for_tests(&genesis_config);
        let (name, id, entrypoint) = solana_bpf_loader_program!();
        bank.add_builtin(&name, &id, entrypoint);
        let bank_client = BankClient::new(bank);
        let program_id = load_sbf_program(&bank_client, &bpf_loader::id(), &mint_keypair, program);
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
        let mut bank = Bank::new_for_tests(&genesis_config);
        let (name, id, entrypoint) = solana_bpf_loader_program!();
        bank.add_builtin(&name, &id, entrypoint);
        let bank = Arc::new(bank);
        let bank_client = BankClient::new_shared(&bank);

        let program_id = load_sbf_program(&bank_client, &bpf_loader::id(), &mint_keypair, program);

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
        let mut bank = Bank::new_for_tests(&genesis_config);
        let (name, id, entrypoint) = solana_bpf_loader_program!();
        bank.add_builtin(&name, &id, entrypoint);
        let bank = Arc::new(bank);
        let bank_client = BankClient::new_shared(&bank);

        let invoke_program_id =
            load_sbf_program(&bank_client, &bpf_loader::id(), &mint_keypair, program.1);
        let invoked_program_id =
            load_sbf_program(&bank_client, &bpf_loader::id(), &mint_keypair, program.2);
        let noop_program_id =
            load_sbf_program(&bank_client, &bpf_loader::id(), &mint_keypair, program.3);

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
                "Program failed to complete: Invoked an instruction with data that is too large (10241 > 10240)".into(),
                format!("Program {invoke_program_id} failed: Program failed to complete"),
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
                "Program failed to complete: Invoked an instruction with too many accounts (256 > 255)".into(),
                format!("Program {invoke_program_id} failed: Program failed to complete"),
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
                "Program failed to complete: Invoked an instruction with too many account info's (129 > 128)".into(),
                format!("Program {invoke_program_id} failed: Program failed to complete"),
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
        assert_eq!(invoked_programs, vec![system_program::id()]);
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
    let mut bank = Bank::new_for_tests(&genesis_config);
    let (name, id, entrypoint) = solana_bpf_loader_program!();
    bank.add_builtin(&name, &id, entrypoint);
    let bank = Arc::new(bank);
    let bank_client = BankClient::new_shared(&bank);

    let malicious_swap_pubkey = load_sbf_program(
        &bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_spoof1",
    );
    let malicious_system_pubkey = load_sbf_program(
        &bank_client,
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
    let mut bank = Bank::new_for_tests(&genesis_config);
    let (name, id, entrypoint) = solana_bpf_loader_program!();
    bank.add_builtin(&name, &id, entrypoint);
    let bank = Arc::new(bank);
    let bank_client = BankClient::new_shared(&bank);

    let caller_pubkey = load_sbf_program(
        &bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_caller_access",
    );
    let caller2_pubkey = load_sbf_program(
        &bank_client,
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
    let mut bank = Bank::new_for_tests(&genesis_config);
    let (name, id, entrypoint) = solana_bpf_loader_program!();
    bank.add_builtin(&name, &id, entrypoint);
    let bank = Arc::new(bank);
    let bank_client = BankClient::new_shared(&bank);

    let program_pubkey = load_sbf_program(
        &bank_client,
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
    let mut bank = Bank::new_for_tests(&genesis_config);
    let (name, id, entrypoint) = solana_bpf_loader_program!();
    bank.add_builtin(&name, &id, entrypoint);
    let bank_client = BankClient::new(bank);
    let program_id = load_sbf_program(
        &bank_client,
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
    let mut bank = Bank::new_for_tests(&genesis_config);
    let (name, id, entrypoint) = solana_bpf_loader_program!();
    bank.add_builtin(&name, &id, entrypoint);
    let bank_client = BankClient::new(bank);
    let program_id = load_sbf_program(
        &bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_noop",
    );
    let message = Message::new(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(1),
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
            ("solana_sbf_rust_custom_heap", 422),
            ("solana_sbf_rust_dep_crate", 2),
            ("solana_sbf_rust_external_spend", 288),
            ("solana_sbf_rust_iter", 1013),
            ("solana_sbf_rust_many_args", 1289),
            ("solana_sbf_rust_mem", 2067),
            ("solana_sbf_rust_membuiltins", 1539),
            ("solana_sbf_rust_noop", 275),
            ("solana_sbf_rust_param_passing", 146),
            ("solana_sbf_rust_rand", 378),
            ("solana_sbf_rust_sanity", 51814),
            ("solana_sbf_rust_secp256k1_recover", 91185),
            ("solana_sbf_rust_sha", 24075),
        ]);
    }

    let mut passed = true;
    println!("\n  {:36} expected actual  diff", "SBF program");
    for program in programs.iter() {
        let count = run_program(program.0);
        let diff: i64 = count as i64 - program.1 as i64;
        println!(
            "  {:36} {:8} {:6} {:+5} ({:+3.0}%)",
            program.0,
            program.1,
            count,
            diff,
            100.0_f64 * count as f64 / program.1 as f64 - 100.0_f64,
        );
        if count > program.1 {
            passed = false;
        }
    }
    assert!(passed);
}

#[test]
#[cfg(any(feature = "sbf_rust"))]
fn test_program_sbf_instruction_introspection() {
    solana_logger::setup();

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50_000);
    let mut bank = Bank::new_for_tests(&genesis_config);

    let (name, id, entrypoint) = solana_bpf_loader_program!();
    bank.add_builtin(&name, &id, entrypoint);
    let bank = Arc::new(bank);
    let bank_client = BankClient::new_shared(&bank);

    let program_id = load_sbf_program(
        &bank_client,
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
    let mut bank = Bank::new_for_tests(&genesis_config);
    let (name, id, entrypoint) = solana_bpf_loader_program!();
    bank.add_builtin(&name, &id, entrypoint);
    let bank_client = BankClient::new(bank);
    let panic_id = load_sbf_program(
        &bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_panic",
    );

    let program_keypair = Keypair::new();

    // Write the panic program into the program account
    let elf = read_sbf_program("solana_sbf_rust_panic");
    let message = Message::new(
        &[system_instruction::create_account(
            &mint_keypair.pubkey(),
            &program_keypair.pubkey(),
            1,
            elf.len() as u64 * 2,
            &bpf_loader::id(),
        )],
        Some(&mint_keypair.pubkey()),
    );
    assert!(bank_client
        .send_and_confirm_message(&[&mint_keypair, &program_keypair], message)
        .is_ok());
    write_sbf_program(
        &bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        &program_keypair,
        &elf,
    );

    // Finalize the panic program, but fail the tx
    let message = Message::new(
        &[
            loader_instruction::finalize(&program_keypair.pubkey(), &bpf_loader::id()),
            Instruction::new_with_bytes(panic_id, &[0], vec![]),
        ],
        Some(&mint_keypair.pubkey()),
    );
    assert!(bank_client
        .send_and_confirm_message(&[&mint_keypair, &program_keypair], message)
        .is_err());

    // Write the noop program into the same program account
    let elf = read_sbf_program("solana_sbf_rust_noop");
    write_sbf_program(
        &bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        &program_keypair,
        &elf,
    );

    // Finalize the noop program
    let message = Message::new(
        &[loader_instruction::finalize(
            &program_keypair.pubkey(),
            &bpf_loader::id(),
        )],
        Some(&mint_keypair.pubkey()),
    );
    assert!(bank_client
        .send_and_confirm_message(&[&mint_keypair, &program_keypair], message)
        .is_ok());

    // Call the noop program, should get noop not panic
    let message = Message::new(
        &[Instruction::new_with_bytes(
            program_keypair.pubkey(),
            &[0],
            vec![],
        )],
        Some(&mint_keypair.pubkey()),
    );
    assert!(bank_client
        .send_and_confirm_message(&[&mint_keypair], message)
        .is_ok());
}

#[ignore] // Invoking SBF loaders from CPI not allowed
#[test]
#[cfg(feature = "sbf_rust")]
fn test_program_sbf_test_use_latest_executor2() {
    solana_logger::setup();

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50);
    let mut bank = Bank::new_for_tests(&genesis_config);
    let (name, id, entrypoint) = solana_bpf_loader_program!();
    bank.add_builtin(&name, &id, entrypoint);
    let bank_client = BankClient::new(bank);
    let invoke_and_error = load_sbf_program(
        &bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_invoke_and_error",
    );
    let invoke_and_ok = load_sbf_program(
        &bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_invoke_and_ok",
    );

    let program_keypair = Keypair::new();

    // Write the panic program into the program account
    let elf = read_sbf_program("solana_sbf_rust_panic");
    let message = Message::new(
        &[system_instruction::create_account(
            &mint_keypair.pubkey(),
            &program_keypair.pubkey(),
            1,
            elf.len() as u64 * 2,
            &bpf_loader::id(),
        )],
        Some(&mint_keypair.pubkey()),
    );
    assert!(bank_client
        .send_and_confirm_message(&[&mint_keypair, &program_keypair], message)
        .is_ok());
    write_sbf_program(
        &bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        &program_keypair,
        &elf,
    );

    // - invoke finalize and return error, swallow error
    let mut instruction =
        loader_instruction::finalize(&program_keypair.pubkey(), &bpf_loader::id());
    instruction.accounts.insert(
        0,
        AccountMeta {
            is_signer: false,
            is_writable: false,
            pubkey: instruction.program_id,
        },
    );
    instruction.program_id = invoke_and_ok;
    instruction.accounts.insert(
        0,
        AccountMeta {
            is_signer: false,
            is_writable: false,
            pubkey: invoke_and_error,
        },
    );
    let message = Message::new(&[instruction], Some(&mint_keypair.pubkey()));
    assert!(bank_client
        .send_and_confirm_message(&[&mint_keypair, &program_keypair], message)
        .is_ok());

    // invoke program, verify not found
    let message = Message::new(
        &[Instruction::new_with_bytes(
            program_keypair.pubkey(),
            &[0],
            vec![],
        )],
        Some(&mint_keypair.pubkey()),
    );
    assert_eq!(
        bank_client
            .send_and_confirm_message(&[&mint_keypair], message)
            .unwrap_err()
            .unwrap(),
        TransactionError::InvalidProgramForExecution
    );

    // Write the noop program into the same program account
    let elf = read_sbf_program("solana_sbf_rust_noop");
    write_sbf_program(
        &bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        &program_keypair,
        &elf,
    );

    // Finalize the noop program
    let message = Message::new(
        &[loader_instruction::finalize(
            &program_keypair.pubkey(),
            &bpf_loader::id(),
        )],
        Some(&mint_keypair.pubkey()),
    );
    assert!(bank_client
        .send_and_confirm_message(&[&mint_keypair, &program_keypair], message)
        .is_ok());

    // Call the program, should get noop, not panic
    let message = Message::new(
        &[Instruction::new_with_bytes(
            program_keypair.pubkey(),
            &[0],
            vec![],
        )],
        Some(&mint_keypair.pubkey()),
    );
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
    let mut bank = Bank::new_for_tests(&genesis_config);
    let (name, id, entrypoint) = solana_bpf_loader_upgradeable_program!();
    bank.add_builtin(&name, &id, entrypoint);
    let bank_client = BankClient::new(bank);

    // Deploy upgrade program
    let buffer_keypair = Keypair::new();
    let program_keypair = Keypair::new();
    let program_id = program_keypair.pubkey();
    let authority_keypair = Keypair::new();
    load_upgradeable_sbf_program(
        &bank_client,
        &mint_keypair,
        &buffer_keypair,
        &program_keypair,
        &authority_keypair,
        "solana_sbf_rust_upgradeable",
    );

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
    upgrade_sbf_program(
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
    upgrade_sbf_program(
        &bank_client,
        &mint_keypair,
        &buffer_keypair,
        &program_id,
        &new_authority_keypair,
        "solana_sbf_rust_upgradeable",
    );

    // Call original program
    instruction.data[0] += 1;
    let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
    assert_eq!(
        result.unwrap_err().unwrap(),
        TransactionError::InstructionError(0, InstructionError::Custom(42))
    );
}

#[test]
#[cfg(feature = "sbf_rust")]
fn test_program_sbf_upgrade_and_invoke_in_same_tx() {
    solana_logger::setup();

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50);
    let mut bank = Bank::new_for_tests(&genesis_config);
    let (name, id, entrypoint) = solana_bpf_loader_upgradeable_program!();
    bank.add_builtin(&name, &id, entrypoint);
    let bank = Arc::new(bank);
    let bank_client = BankClient::new_shared(&bank);

    // Deploy upgrade program
    let buffer_keypair = Keypair::new();
    let program_keypair = Keypair::new();
    let program_id = program_keypair.pubkey();
    let authority_keypair = Keypair::new();
    load_upgradeable_sbf_program(
        &bank_client,
        &mint_keypair,
        &buffer_keypair,
        &program_keypair,
        &authority_keypair,
        "solana_sbf_rust_noop",
    );

    let invoke_instruction =
        Instruction::new_with_bytes(program_id, &[0], vec![AccountMeta::new(clock::id(), false)]);

    // Call upgradeable program
    let result =
        bank_client.send_and_confirm_instruction(&mint_keypair, invoke_instruction.clone());
    assert!(result.is_ok());

    // Prepare for upgrade
    let buffer_keypair = Keypair::new();
    load_upgradeable_buffer(
        &bank_client,
        &mint_keypair,
        &buffer_keypair,
        &authority_keypair,
        "solana_sbf_rust_panic",
    );

    // Invoke, then upgrade the program, and then invoke again in same tx
    let message = Message::new(
        &[
            invoke_instruction.clone(),
            bpf_loader_upgradeable::upgrade(
                &program_id,
                &buffer_keypair.pubkey(),
                &authority_keypair.pubkey(),
                &mint_keypair.pubkey(),
            ),
            invoke_instruction,
        ],
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
        TransactionError::InstructionError(2, InstructionError::ProgramFailedToComplete)
    );
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
    let mut bank = Bank::new_for_tests(&genesis_config);
    let (name, id, entrypoint) = solana_bpf_loader_program!();
    bank.add_builtin(&name, &id, entrypoint);
    let (name, id, entrypoint) = solana_bpf_loader_upgradeable_program!();
    bank.add_builtin(&name, &id, entrypoint);
    let bank_client = BankClient::new(bank);
    let invoke_and_return = load_sbf_program(
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
    load_upgradeable_sbf_program(
        &bank_client,
        &mint_keypair,
        &buffer_keypair,
        &program_keypair,
        &authority_keypair,
        "solana_sbf_rust_upgradeable",
    );

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
    upgrade_sbf_program(
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
    upgrade_sbf_program(
        &bank_client,
        &mint_keypair,
        &buffer_keypair,
        &program_id,
        &new_authority_keypair,
        "solana_sbf_rust_upgradeable",
    );

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
        let (name, id, entrypoint) = solana_bpf_loader_program!();
        bank.add_builtin(&name, &id, entrypoint);
        let bank_client = BankClient::new(bank);

        let program_id = load_sbf_program(&bank_client, &bpf_loader::id(), &mint_keypair, program);
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
fn test_program_sbf_c_dup() {
    solana_logger::setup();

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50);
    let mut bank = Bank::new_for_tests(&genesis_config);
    let (name, id, entrypoint) = solana_bpf_loader_program!();
    bank.add_builtin(&name, &id, entrypoint);

    let account_address = Pubkey::new_unique();
    let account = AccountSharedData::new_data(42, &[1_u8, 2, 3], &system_program::id()).unwrap();
    bank.store_account(&account_address, &account);

    let bank_client = BankClient::new(bank);

    let program_id = load_sbf_program(&bank_client, &bpf_loader::id(), &mint_keypair, "ser");
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
    let mut bank = Bank::new_for_tests(&genesis_config);
    let (name, id, entrypoint) = solana_bpf_loader_program!();
    bank.add_builtin(&name, &id, entrypoint);
    let (name, id, entrypoint) = solana_bpf_loader_upgradeable_program!();
    bank.add_builtin(&name, &id, entrypoint);
    let bank_client = BankClient::new(bank);
    let invoke_and_return = load_sbf_program(
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
    load_upgradeable_sbf_program(
        &bank_client,
        &mint_keypair,
        &buffer_keypair,
        &program_keypair,
        &authority_keypair,
        "solana_sbf_rust_upgradeable",
    );
    let program_account = bank_client.get_account(&program_id).unwrap().unwrap();
    let programdata_address = match program_account.state() {
        Ok(bpf_loader_upgradeable::UpgradeableLoaderState::Program {
            programdata_address,
        }) => programdata_address,
        _ => unreachable!(),
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
    let path = create_sbf_path("solana_sbf_rust_upgraded");
    let mut file = File::open(&path).unwrap_or_else(|err| {
        panic!("Failed to open {}: {}", path.display(), err);
    });
    let mut elf = Vec::new();
    file.read_to_end(&mut elf).unwrap();
    let buffer_keypair = Keypair::new();
    load_buffer_account(
        &bank_client,
        &mint_keypair,
        &buffer_keypair,
        &authority_keypair,
        &elf,
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
fn test_program_sbf_upgrade_self_via_cpi() {
    solana_logger::setup();

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50);
    let mut bank = Bank::new_for_tests(&genesis_config);
    let (name, id, entrypoint) = solana_bpf_loader_program!();
    bank.add_builtin(&name, &id, entrypoint);
    let (name, id, entrypoint) = solana_bpf_loader_upgradeable_program!();
    bank.add_builtin(&name, &id, entrypoint);
    let bank = Arc::new(bank);
    let bank_client = BankClient::new_shared(&bank);
    let noop_program_id = load_sbf_program(
        &bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_noop",
    );

    // Deploy upgradeable program
    let buffer_keypair = Keypair::new();
    let program_keypair = Keypair::new();
    let program_id = program_keypair.pubkey();
    let authority_keypair = Keypair::new();
    load_upgradeable_sbf_program(
        &bank_client,
        &mint_keypair,
        &buffer_keypair,
        &program_keypair,
        &authority_keypair,
        "solana_sbf_rust_invoke_and_return",
    );

    let mut invoke_instruction = Instruction::new_with_bytes(
        program_id,
        &[0],
        vec![
            AccountMeta::new_readonly(noop_program_id, false),
            AccountMeta::new_readonly(clock::id(), false),
        ],
    );

    // Call the upgraded program
    invoke_instruction.data[0] += 1;
    let result =
        bank_client.send_and_confirm_instruction(&mint_keypair, invoke_instruction.clone());
    assert!(result.is_ok());

    // Prepare for upgrade
    let buffer_keypair = Keypair::new();
    load_upgradeable_buffer(
        &bank_client,
        &mint_keypair,
        &buffer_keypair,
        &authority_keypair,
        "solana_sbf_rust_panic",
    );

    // Invoke, then upgrade the program, and then invoke again in same tx
    let message = Message::new(
        &[
            invoke_instruction.clone(),
            bpf_loader_upgradeable::upgrade(
                &program_id,
                &buffer_keypair.pubkey(),
                &authority_keypair.pubkey(),
                &mint_keypair.pubkey(),
            ),
            invoke_instruction,
        ],
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
        TransactionError::InstructionError(2, InstructionError::ProgramFailedToComplete)
    );
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
    let mut bank = Bank::new_for_tests(&genesis_config);
    let (name, id, entrypoint) = solana_bpf_loader_program!();
    bank.add_builtin(&name, &id, entrypoint);
    let (name, id, entrypoint) = solana_bpf_loader_upgradeable_program!();
    bank.add_builtin(&name, &id, entrypoint);
    let bank_client = BankClient::new(bank);

    // Deploy CPI invoker program
    let invoke_and_return = load_sbf_program(
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
    load_upgradeable_sbf_program(
        &bank_client,
        &mint_keypair,
        &buffer_keypair,
        &program_keypair,
        &authority_keypair,
        "solana_sbf_rust_upgradeable",
    );

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
        let mut bank = Bank::new_for_tests(&genesis_config);
        let (name, id, entrypoint) = solana_bpf_loader_upgradeable_program!();
        bank.add_builtin(&name, &id, entrypoint);
        let bank = Arc::new(bank);
        let bank_client = BankClient::new_shared(&bank);

        load_upgradeable_sbf_program(
            &bank_client,
            &mint_keypair,
            buffer_keypair,
            program_keypair,
            payer_keypair,
            "solana_sbf_rust_panic",
        );

        // Load the buffer account
        let path = create_sbf_path("solana_sbf_rust_noop");
        let mut file = File::open(&path).unwrap_or_else(|err| {
            panic!("Failed to open {}: {}", path.display(), err);
        });
        let mut elf = Vec::new();
        file.read_to_end(&mut elf).unwrap();
        load_buffer_account(
            &bank_client,
            &mint_keypair,
            buffer_keypair,
            &payer_keypair,
            &elf,
        );

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
    let mut bank = Bank::new_for_tests(&genesis_config);
    let (name, id, entrypoint) = solana_bpf_loader_program!();
    bank.add_builtin(&name, &id, entrypoint);
    let bank = Arc::new(bank);
    let bank_client = BankClient::new_shared(&bank);

    let program_pubkey = load_sbf_program(
        &bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_finalize",
    );

    let noop_keypair = Keypair::new();

    // Write the noop program into the same program account
    let elf = read_sbf_program("solana_sbf_rust_noop");
    let message = Message::new(
        &[system_instruction::create_account(
            &mint_keypair.pubkey(),
            &noop_keypair.pubkey(),
            1,
            elf.len() as u64 * 2,
            &bpf_loader::id(),
        )],
        Some(&mint_keypair.pubkey()),
    );
    assert!(bank_client
        .send_and_confirm_message(&[&mint_keypair, &noop_keypair], message)
        .is_ok());
    write_sbf_program(
        &bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        &noop_keypair,
        &elf,
    );

    let account_metas = vec![
        AccountMeta::new(noop_keypair.pubkey(), true),
        AccountMeta::new_readonly(bpf_loader::id(), false),
        AccountMeta::new(rent::id(), false),
    ];
    let instruction = Instruction::new_with_bytes(program_pubkey, &[], account_metas.clone());
    let message = Message::new(&[instruction], Some(&mint_keypair.pubkey()));
    let result = bank_client.send_and_confirm_message(&[&mint_keypair, &noop_keypair], message);
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
    let mut bank = Bank::new_for_tests(&genesis_config);
    let (name, id, entrypoint) = solana_bpf_loader_program!();
    bank.add_builtin(&name, &id, entrypoint);
    let bank = Arc::new(bank);
    let bank_client = BankClient::new_shared(&bank);

    let program_id = load_sbf_program(
        &bank_client,
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

    let mut bank = Bank::new_for_tests(&genesis_config);
    let (name, id, entrypoint) = solana_bpf_loader_program!();
    bank.add_builtin(&name, &id, entrypoint);
    let bank = Arc::new(bank);
    let bank_client = BankClient::new_shared(&bank);

    let program_id = load_sbf_program(
        &bank_client,
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
                    &[
                        realloc_extend_and_fill(
                            &program_id,
                            &pubkey,
                            MAX_PERMITTED_DATA_INCREASE,
                            1,
                            &mut bump,
                        ),
                        // Request max transaction accounts data size to allow large instruction
                        ComputeBudgetInstruction::set_accounts_data_size_limit(u32::MAX),
                    ],
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
                    &[
                        realloc_extend(
                            &program_id,
                            &pubkey,
                            MAX_PERMITTED_DATA_INCREASE,
                            &mut bump
                        ),
                        // Request max transaction accounts data size to allow large instruction
                        ComputeBudgetInstruction::set_accounts_data_size_limit(u32::MAX),
                    ],
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
                &[
                    realloc(&program_id, &pubkey, 0, &mut bump),
                    ComputeBudgetInstruction::set_accounts_data_size_limit(u32::MAX),
                ],
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

    let mut bank = Bank::new_for_tests(&genesis_config);
    let (name, id, entrypoint) = solana_bpf_loader_program!();
    bank.add_builtin(&name, &id, entrypoint);
    let bank = Arc::new(bank);
    let bank_client = BankClient::new_shared(&bank);

    let realloc_program_id = load_sbf_program(
        &bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_realloc",
    );

    let realloc_invoke_program_id = load_sbf_program(
        &bank_client,
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
                    &[
                        Instruction::new_with_bytes(
                            realloc_invoke_program_id,
                            &[INVOKE_REALLOC_MAX_PLUS_ONE],
                            vec![
                                AccountMeta::new(pubkey, false),
                                AccountMeta::new_readonly(realloc_program_id, false),
                            ],
                        ),
                        // Request max transaction accounts data size to allow large instruction
                        ComputeBudgetInstruction::set_accounts_data_size_limit(u32::MAX),
                    ],
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
                    &[
                        Instruction::new_with_bytes(
                            realloc_invoke_program_id,
                            &[INVOKE_REALLOC_MAX_TWICE],
                            vec![
                                AccountMeta::new(pubkey, false),
                                AccountMeta::new_readonly(realloc_program_id, false),
                            ],
                        ),
                        // Request max transaction accounts data size to allow large instruction
                        ComputeBudgetInstruction::set_accounts_data_size_limit(u32::MAX),
                    ],
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
                    &[
                        Instruction::new_with_bytes(
                            realloc_invoke_program_id,
                            &[INVOKE_REALLOC_MAX_INVOKE_MAX],
                            vec![
                                AccountMeta::new(invoke_pubkey, false),
                                AccountMeta::new_readonly(realloc_program_id, false),
                            ],
                        ),
                        // Request max transaction accounts data size to allow large instruction
                        ComputeBudgetInstruction::set_accounts_data_size_limit(u32::MAX),
                    ],
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
                    &[
                        Instruction::new_with_bytes(
                            realloc_invoke_program_id,
                            &[INVOKE_INVOKE_MAX_TWICE],
                            vec![
                                AccountMeta::new(invoke_pubkey, false),
                                AccountMeta::new_readonly(realloc_invoke_program_id, false),
                                AccountMeta::new_readonly(realloc_program_id, false),
                            ],
                        ),
                        // Request max transaction accounts data size to allow large instruction
                        ComputeBudgetInstruction::set_accounts_data_size_limit(u32::MAX),
                    ],
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
                    &[
                        Instruction::new_with_bytes(
                            realloc_invoke_program_id,
                            &[INVOKE_REALLOC_EXTEND_MAX, 1, i as u8, (i / 255) as u8],
                            vec![
                                AccountMeta::new(pubkey, false),
                                AccountMeta::new_readonly(realloc_program_id, false),
                            ],
                        ),
                        // Request max transaction accounts data size to allow large instruction
                        ComputeBudgetInstruction::set_accounts_data_size_limit(u32::MAX),
                    ],
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
                    &[
                        Instruction::new_with_bytes(
                            realloc_invoke_program_id,
                            &[INVOKE_REALLOC_EXTEND_MAX, 2, 1, 1],
                            vec![
                                AccountMeta::new(pubkey, false),
                                AccountMeta::new_readonly(realloc_program_id, false),
                            ],
                        ),
                        // Request max transaction accounts data size to allow large instruction
                        ComputeBudgetInstruction::set_accounts_data_size_limit(u32::MAX),
                    ],
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
#[cfg(any(feature = "sbf_rust"))]
fn test_program_sbf_processed_inner_instruction() {
    solana_logger::setup();

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50);
    let mut bank = Bank::new_for_tests(&genesis_config);
    let (name, id, entrypoint) = solana_bpf_loader_program!();
    bank.add_builtin(&name, &id, entrypoint);
    let bank = Arc::new(bank);
    let bank_client = BankClient::new_shared(&bank);

    let sibling_program_id = load_sbf_program(
        &bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_sibling_instructions",
    );
    let sibling_inner_program_id = load_sbf_program(
        &bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_sibling_inner_instructions",
    );
    let noop_program_id = load_sbf_program(
        &bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_noop",
    );
    let invoke_and_return_program_id = load_sbf_program(
        &bank_client,
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

    let (name, id, entrypoint) = solana_bpf_loader_program!();
    bank.add_builtin(&name, &id, entrypoint);
    let bank_client = BankClient::new(bank);

    let program_id = load_sbf_program(
        &bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_noop",
    );

    let pre_balance = bank_client.get_balance(&mint_keypair.pubkey()).unwrap();
    let message = Message::new(
        &[Instruction::new_with_bytes(program_id, &[], vec![])],
        Some(&mint_keypair.pubkey()),
    );

    let sanitized_message = SanitizedMessage::try_from(message.clone()).unwrap();
    let expected_normal_fee = Bank::calculate_fee(
        &sanitized_message,
        congestion_multiplier,
        &fee_structure,
        true,
        false,
        false,
        compute_budget::LoadedAccountsDataLimitType::V0,
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
    let expected_prioritized_fee = Bank::calculate_fee(
        &sanitized_message,
        congestion_multiplier,
        &fee_structure,
        true,
        false,
        false,
        compute_budget::LoadedAccountsDataLimitType::V0,
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

    let (name, id, entrypoint) = solana_bpf_loader_program!();
    bank.add_builtin(&name, &id, entrypoint);
    let bank = Arc::new(bank);
    let bank_client = BankClient::new_shared(&bank);

    let program_id = load_sbf_program(
        &bank_client,
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
        mut genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50);
    genesis_config
        .accounts
        .remove(&solana_sdk::feature_set::disable_deprecated_loader::id())
        .unwrap();
    let mut bank = Bank::new_for_tests(&genesis_config);
    let (name, id, entrypoint) = solana_bpf_loader_program!();
    bank.add_builtin(&name, &id, entrypoint);
    let (name, id, entrypoint) = solana_bpf_loader_deprecated_program!();
    bank.add_builtin(&name, &id, entrypoint);
    let bank_client = BankClient::new(bank);

    // load aligned program
    let noop = load_sbf_program(
        &bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_noop",
    );

    // Load unaligned program
    let inner_instruction_alignment_check = load_sbf_program(
        &bank_client,
        &bpf_loader_deprecated::id(),
        &mint_keypair,
        "solana_sbf_rust_inner_instruction_alignment_check",
    );

    // invoke unaligned program, which will call aligned program twice,
    // unaligned should be allowed once invoke completes
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
    assert!(result.is_ok());
}

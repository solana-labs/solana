#![cfg(any(feature = "bpf_c", feature = "bpf_rust"))]

#[macro_use]
extern crate solana_bpf_loader_program;

use solana_bpf_loader_program::{
    create_vm,
    serialization::{deserialize_parameters, serialize_parameters},
};
use solana_rbpf::vm::{EbpfVm, InstructionMeter};
use solana_runtime::{
    bank::Bank,
    bank_client::BankClient,
    genesis_utils::{create_genesis_config, GenesisConfigInfo},
    loader_utils::load_program,
    process_instruction::{
        ComputeBudget, ComputeMeter, Executor, InvokeContext, Logger, ProcessInstruction,
    },
};
use solana_sdk::{
    account::Account,
    bpf_loader, bpf_loader_deprecated,
    client::SyncClient,
    clock::{DEFAULT_SLOTS_PER_EPOCH, MAX_PROCESSING_AGE},
    entrypoint::{MAX_PERMITTED_DATA_INCREASE, SUCCESS},
    instruction::{AccountMeta, CompiledInstruction, Instruction, InstructionError},
    keyed_account::KeyedAccount,
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    sysvar::{clock, fees, rent, slot_hashes, stake_history},
    transaction::{Transaction, TransactionError},
};
use std::{cell::RefCell, env, fs::File, io::Read, path::PathBuf, rc::Rc, sync::Arc};

/// BPF program file extension
const PLATFORM_FILE_EXTENSION_BPF: &str = "so";

/// Create a BPF program file name
fn create_bpf_path(name: &str) -> PathBuf {
    let mut pathbuf = {
        let current_exe = env::current_exe().unwrap();
        PathBuf::from(current_exe.parent().unwrap().parent().unwrap())
    };
    pathbuf.push("bpf/");
    pathbuf.push(name);
    pathbuf.set_extension(PLATFORM_FILE_EXTENSION_BPF);
    pathbuf
}

fn load_bpf_program(
    bank_client: &BankClient,
    loader_id: &Pubkey,
    payer_keypair: &Keypair,
    name: &str,
) -> Pubkey {
    let path = create_bpf_path(name);
    let mut file = File::open(&path).unwrap_or_else(|err| {
        panic!("Failed to open {}: {}", path.display(), err);
    });
    let mut elf = Vec::new();
    file.read_to_end(&mut elf).unwrap();
    load_program(bank_client, payer_keypair, loader_id, elf)
}

fn run_program(
    name: &str,
    program_id: &Pubkey,
    parameter_accounts: &[KeyedAccount],
    instruction_data: &[u8],
) -> Result<u64, InstructionError> {
    let path = create_bpf_path(name);
    let mut file = File::open(path).unwrap();

    let mut data = vec![];
    file.read_to_end(&mut data).unwrap();
    let loader_id = bpf_loader::id();
    let mut invoke_context = MockInvokeContext::default();

    let executable = EbpfVm::create_executable_from_elf(&data, None).unwrap();
    let (mut vm, heap_region) = create_vm(
        &loader_id,
        executable.as_ref(),
        parameter_accounts,
        &mut invoke_context,
    )
    .unwrap();
    let mut parameter_bytes = serialize_parameters(
        &bpf_loader::id(),
        program_id,
        parameter_accounts,
        &instruction_data,
    )
    .unwrap();
    assert_eq!(
        SUCCESS,
        vm.execute_program(parameter_bytes.as_mut_slice(), &[], &[heap_region.clone()])
            .unwrap()
    );
    deserialize_parameters(&bpf_loader::id(), parameter_accounts, &parameter_bytes).unwrap();
    Ok(vm.get_total_instruction_count())
}

fn process_transaction_and_record_inner(
    bank: &Bank,
    tx: Transaction,
) -> (Result<(), TransactionError>, Vec<Vec<CompiledInstruction>>) {
    let signature = tx.signatures.get(0).unwrap().clone();
    let txs = vec![tx];
    let tx_batch = bank.prepare_batch(&txs, None);
    let (mut results, _, mut inner, _transaction_logs) = bank.load_execute_and_commit_transactions(
        &tx_batch,
        MAX_PROCESSING_AGE,
        false,
        true,
        false,
    );
    let inner_instructions = inner.swap_remove(0);
    let result = results
        .fee_collection_results
        .swap_remove(0)
        .and_then(|_| bank.get_signature_status(&signature).unwrap());
    (
        result,
        inner_instructions.expect("cpi recording should be enabled"),
    )
}

#[test]
#[cfg(any(feature = "bpf_c", feature = "bpf_rust"))]
fn test_program_bpf_sanity() {
    solana_logger::setup();

    let mut programs = Vec::new();
    #[cfg(feature = "bpf_c")]
    {
        programs.extend_from_slice(&[
            ("alloc", true),
            ("bpf_to_bpf", true),
            ("multiple_static", true),
            ("noop", true),
            ("noop++", true),
            ("panic", false),
            ("relative_call", true),
            ("sanity", true),
            ("sanity++", true),
            ("sha256", true),
            ("struct_pass", true),
            ("struct_ret", true),
        ]);
    }
    #[cfg(feature = "bpf_rust")]
    {
        programs.extend_from_slice(&[
            ("solana_bpf_rust_128bit", true),
            ("solana_bpf_rust_alloc", true),
            ("solana_bpf_rust_custom_heap", true),
            ("solana_bpf_rust_dep_crate", true),
            ("solana_bpf_rust_external_spend", false),
            ("solana_bpf_rust_iter", true),
            ("solana_bpf_rust_many_args", true),
            ("solana_bpf_rust_noop", true),
            ("solana_bpf_rust_panic", false),
            ("solana_bpf_rust_param_passing", true),
            ("solana_bpf_rust_rand", true),
            ("solana_bpf_rust_ristretto", true),
            ("solana_bpf_rust_sanity", true),
            ("solana_bpf_rust_sha256", true),
            ("solana_bpf_rust_sysval", true),
        ]);
    }

    for program in programs.iter() {
        println!("Test program: {:?}", program.0);

        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(50);
        let mut bank = Bank::new(&genesis_config);
        let (name, id, entrypoint) = solana_bpf_loader_program!();
        bank.add_builtin_loader(&name, id, entrypoint);
        let bank = Arc::new(bank);

        // Create bank with a specific slot, used by solana_bpf_rust_sysvar test
        let bank = Bank::new_from_parent(&bank, &Pubkey::default(), DEFAULT_SLOTS_PER_EPOCH + 1);
        let bank_client = BankClient::new(bank);

        // Call user program
        let program_id =
            load_bpf_program(&bank_client, &bpf_loader::id(), &mint_keypair, program.0);
        let account_metas = vec![
            AccountMeta::new(mint_keypair.pubkey(), true),
            AccountMeta::new(Keypair::new().pubkey(), false),
            AccountMeta::new(clock::id(), false),
            AccountMeta::new(fees::id(), false),
            AccountMeta::new(slot_hashes::id(), false),
            AccountMeta::new(stake_history::id(), false),
            AccountMeta::new(rent::id(), false),
        ];
        let instruction = Instruction::new(program_id, &1u8, account_metas);
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        if program.1 {
            assert!(result.is_ok());
        } else {
            assert!(result.is_err());
        }
    }
}

#[test]
#[cfg(any(feature = "bpf_c", feature = "bpf_rust"))]
fn test_program_bpf_loader_deprecated() {
    solana_logger::setup();

    let mut programs = Vec::new();
    #[cfg(feature = "bpf_c")]
    {
        programs.extend_from_slice(&[("deprecated_loader")]);
    }
    #[cfg(feature = "bpf_rust")]
    {
        programs.extend_from_slice(&[("solana_bpf_rust_deprecated_loader")]);
    }

    for program in programs.iter() {
        println!("Test program: {:?}", program);

        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(50);
        let mut bank = Bank::new(&genesis_config);
        let (name, id, entrypoint) = solana_bpf_loader_deprecated_program!();
        bank.add_builtin_loader(&name, id, entrypoint);
        let bank_client = BankClient::new(bank);

        let program_id = load_bpf_program(
            &bank_client,
            &bpf_loader_deprecated::id(),
            &mint_keypair,
            program,
        );
        let account_metas = vec![AccountMeta::new(mint_keypair.pubkey(), true)];
        let instruction = Instruction::new(program_id, &1u8, account_metas);
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        assert!(result.is_ok());
    }
}

#[test]
fn test_program_bpf_duplicate_accounts() {
    solana_logger::setup();

    let mut programs = Vec::new();
    #[cfg(feature = "bpf_c")]
    {
        programs.extend_from_slice(&[("dup_accounts")]);
    }
    #[cfg(feature = "bpf_rust")]
    {
        programs.extend_from_slice(&[("solana_bpf_rust_dup_accounts")]);
    }

    for program in programs.iter() {
        println!("Test program: {:?}", program);

        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(50);
        let mut bank = Bank::new(&genesis_config);
        let (name, id, entrypoint) = solana_bpf_loader_program!();
        bank.add_builtin_loader(&name, id, entrypoint);
        let bank = Arc::new(bank);
        let bank_client = BankClient::new_shared(&bank);
        let program_id = load_bpf_program(&bank_client, &bpf_loader::id(), &mint_keypair, program);
        let payee_account = Account::new(10, 1, &program_id);
        let payee_pubkey = solana_sdk::pubkey::new_rand();
        bank.store_account(&payee_pubkey, &payee_account);

        let account = Account::new(10, 1, &program_id);
        let pubkey = solana_sdk::pubkey::new_rand();
        let account_metas = vec![
            AccountMeta::new(mint_keypair.pubkey(), true),
            AccountMeta::new(payee_pubkey, false),
            AccountMeta::new(pubkey, false),
            AccountMeta::new(pubkey, false),
        ];

        bank.store_account(&pubkey, &account);
        let instruction = Instruction::new(program_id, &1u8, account_metas.clone());
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        let data = bank_client.get_account_data(&pubkey).unwrap().unwrap();
        assert!(result.is_ok());
        assert_eq!(data[0], 1);

        bank.store_account(&pubkey, &account);
        let instruction = Instruction::new(program_id, &2u8, account_metas.clone());
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        let data = bank_client.get_account_data(&pubkey).unwrap().unwrap();
        assert!(result.is_ok());
        assert_eq!(data[0], 2);

        bank.store_account(&pubkey, &account);
        let instruction = Instruction::new(program_id, &3u8, account_metas.clone());
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        let data = bank_client.get_account_data(&pubkey).unwrap().unwrap();
        assert!(result.is_ok());
        assert_eq!(data[0], 3);

        bank.store_account(&pubkey, &account);
        let instruction = Instruction::new(program_id, &4u8, account_metas.clone());
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        let lamports = bank_client.get_balance(&pubkey).unwrap();
        assert!(result.is_ok());
        assert_eq!(lamports, 11);

        bank.store_account(&pubkey, &account);
        let instruction = Instruction::new(program_id, &5u8, account_metas.clone());
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        let lamports = bank_client.get_balance(&pubkey).unwrap();
        assert!(result.is_ok());
        assert_eq!(lamports, 12);

        bank.store_account(&pubkey, &account);
        let instruction = Instruction::new(program_id, &6u8, account_metas.clone());
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        let lamports = bank_client.get_balance(&pubkey).unwrap();
        assert!(result.is_ok());
        assert_eq!(lamports, 13);
    }
}

#[test]
fn test_program_bpf_error_handling() {
    solana_logger::setup();

    let mut programs = Vec::new();
    #[cfg(feature = "bpf_c")]
    {
        programs.extend_from_slice(&[("error_handling")]);
    }
    #[cfg(feature = "bpf_rust")]
    {
        programs.extend_from_slice(&[("solana_bpf_rust_error_handling")]);
    }

    for program in programs.iter() {
        println!("Test program: {:?}", program);

        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(50);
        let mut bank = Bank::new(&genesis_config);
        let (name, id, entrypoint) = solana_bpf_loader_program!();
        bank.add_builtin_loader(&name, id, entrypoint);
        let bank_client = BankClient::new(bank);
        let program_id = load_bpf_program(&bank_client, &bpf_loader::id(), &mint_keypair, program);
        let account_metas = vec![AccountMeta::new(mint_keypair.pubkey(), true)];

        let instruction = Instruction::new(program_id, &1u8, account_metas.clone());
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        assert!(result.is_ok());

        let instruction = Instruction::new(program_id, &2u8, account_metas.clone());
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        assert_eq!(
            result.unwrap_err().unwrap(),
            TransactionError::InstructionError(0, InstructionError::InvalidAccountData)
        );

        let instruction = Instruction::new(program_id, &3u8, account_metas.clone());
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        assert_eq!(
            result.unwrap_err().unwrap(),
            TransactionError::InstructionError(0, InstructionError::Custom(0))
        );

        let instruction = Instruction::new(program_id, &4u8, account_metas.clone());
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        assert_eq!(
            result.unwrap_err().unwrap(),
            TransactionError::InstructionError(0, InstructionError::Custom(42))
        );

        let instruction = Instruction::new(program_id, &5u8, account_metas.clone());
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        let result = result.unwrap_err().unwrap();
        if TransactionError::InstructionError(0, InstructionError::InvalidInstructionData) != result
        {
            assert_eq!(
                result,
                TransactionError::InstructionError(0, InstructionError::InvalidError)
            );
        }

        let instruction = Instruction::new(program_id, &6u8, account_metas.clone());
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        let result = result.unwrap_err().unwrap();
        if TransactionError::InstructionError(0, InstructionError::InvalidInstructionData) != result
        {
            assert_eq!(
                result,
                TransactionError::InstructionError(0, InstructionError::InvalidError)
            );
        }

        let instruction = Instruction::new(program_id, &7u8, account_metas.clone());
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        let result = result.unwrap_err().unwrap();
        if TransactionError::InstructionError(0, InstructionError::InvalidInstructionData) != result
        {
            assert_eq!(
                result,
                TransactionError::InstructionError(0, InstructionError::AccountBorrowFailed)
            );
        }

        let instruction = Instruction::new(program_id, &8u8, account_metas.clone());
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        assert_eq!(
            result.unwrap_err().unwrap(),
            TransactionError::InstructionError(0, InstructionError::InvalidInstructionData)
        );

        let instruction = Instruction::new(program_id, &9u8, account_metas.clone());
        let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
        assert_eq!(
            result.unwrap_err().unwrap(),
            TransactionError::InstructionError(0, InstructionError::MaxSeedLengthExceeded)
        );
    }
}

#[test]
fn test_program_bpf_invoke() {
    solana_logger::setup();

    const TEST_SUCCESS: u8 = 1;
    const TEST_PRIVILEGE_ESCALATION_SIGNER: u8 = 2;
    const TEST_PRIVILEGE_ESCALATION_WRITABLE: u8 = 3;
    const TEST_PPROGRAM_NOT_EXECUTABLE: u8 = 4;

    #[allow(dead_code)]
    #[derive(Debug)]
    enum Languages {
        C,
        Rust,
    }
    let mut programs = Vec::new();
    #[cfg(feature = "bpf_c")]
    {
        programs.push((Languages::C, "invoke", "invoked"));
    }
    #[cfg(feature = "bpf_rust")]
    {
        programs.push((
            Languages::Rust,
            "solana_bpf_rust_invoke",
            "solana_bpf_rust_invoked",
        ));
    }
    for program in programs.iter() {
        println!("Test program: {:?}", program);

        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(50);
        let mut bank = Bank::new(&genesis_config);
        let (name, id, entrypoint) = solana_bpf_loader_program!();
        bank.add_builtin_loader(&name, id, entrypoint);
        let bank = Arc::new(bank);
        let bank_client = BankClient::new_shared(&bank);

        let invoke_program_id =
            load_bpf_program(&bank_client, &bpf_loader::id(), &mint_keypair, program.1);
        let invoked_program_id =
            load_bpf_program(&bank_client, &bpf_loader::id(), &mint_keypair, program.2);

        let argument_keypair = Keypair::new();
        let account = Account::new(42, 100, &invoke_program_id);
        bank.store_account(&argument_keypair.pubkey(), &account);

        let invoked_argument_keypair = Keypair::new();
        let account = Account::new(10, 10, &invoked_program_id);
        bank.store_account(&invoked_argument_keypair.pubkey(), &account);

        let from_keypair = Keypair::new();
        let account = Account::new(84, 0, &solana_sdk::system_program::id());
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
            AccountMeta::new_readonly(solana_sdk::system_program::id(), false),
            AccountMeta::new(from_keypair.pubkey(), true),
        ];

        // success cases

        let instruction = Instruction::new(
            invoke_program_id,
            &[TEST_SUCCESS, bump_seed1, bump_seed2, bump_seed3],
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
        let (result, inner_instructions) = process_transaction_and_record_inner(&bank, tx);
        assert!(result.is_ok());
        let invoked_programs: Vec<Pubkey> = inner_instructions[0]
            .iter()
            .map(|ix| message.account_keys[ix.program_id_index as usize].clone())
            .collect();

        let expected_invoked_programs = match program.0 {
            Languages::C => vec![
                solana_sdk::system_program::id(),
                solana_sdk::system_program::id(),
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
                solana_sdk::system_program::id(),
                solana_sdk::system_program::id(),
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
        };
        assert_eq!(invoked_programs, expected_invoked_programs);

        // failure cases

        let instruction = Instruction::new(
            invoke_program_id,
            &[
                TEST_PRIVILEGE_ESCALATION_SIGNER,
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

        let (result, inner_instructions) = process_transaction_and_record_inner(&bank, tx);
        let invoked_programs: Vec<Pubkey> = inner_instructions[0]
            .iter()
            .map(|ix| message.account_keys[ix.program_id_index as usize].clone())
            .collect();
        assert_eq!(invoked_programs, vec![invoked_program_id.clone()]);
        assert_eq!(
            result.unwrap_err(),
            TransactionError::InstructionError(0, InstructionError::Custom(194969602))
        );

        let instruction = Instruction::new(
            invoke_program_id,
            &[
                TEST_PRIVILEGE_ESCALATION_WRITABLE,
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
        let (result, inner_instructions) = process_transaction_and_record_inner(&bank, tx);
        let invoked_programs: Vec<Pubkey> = inner_instructions[0]
            .iter()
            .map(|ix| message.account_keys[ix.program_id_index as usize].clone())
            .collect();
        assert_eq!(invoked_programs, vec![invoked_program_id.clone()]);
        assert_eq!(
            result.unwrap_err(),
            TransactionError::InstructionError(0, InstructionError::Custom(194969602))
        );

        let instruction = Instruction::new(
            invoke_program_id,
            &[
                TEST_PPROGRAM_NOT_EXECUTABLE,
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
        let (result, inner_instructions) = process_transaction_and_record_inner(&bank, tx);
        let invoked_programs: Vec<Pubkey> = inner_instructions[0]
            .iter()
            .map(|ix| message.account_keys[ix.program_id_index as usize].clone())
            .collect();
        assert_eq!(invoked_programs, vec![argument_keypair.pubkey().clone()]);
        assert_eq!(
            result.unwrap_err(),
            TransactionError::InstructionError(0, InstructionError::AccountNotExecutable)
        );

        // Check final state

        assert_eq!(43, bank.get_balance(&derived_key1));
        let account = bank.get_account(&derived_key1).unwrap();
        assert_eq!(invoke_program_id, account.owner);
        assert_eq!(
            MAX_PERMITTED_DATA_INCREASE,
            bank.get_account(&derived_key1).unwrap().data.len()
        );
        for i in 0..20 {
            assert_eq!(i as u8, account.data[i]);
        }
    }
}

#[cfg(feature = "bpf_rust")]
#[test]
fn test_program_bpf_call_depth() {
    solana_logger::setup();

    println!("Test program: solana_bpf_rust_call_depth");

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50);
    let mut bank = Bank::new(&genesis_config);
    let (name, id, entrypoint) = solana_bpf_loader_program!();
    bank.add_builtin_loader(&name, id, entrypoint);
    let bank_client = BankClient::new(bank);
    let program_id = load_bpf_program(
        &bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_bpf_rust_call_depth",
    );

    let instruction = Instruction::new(
        program_id,
        &(ComputeBudget::default().max_call_depth - 1),
        vec![],
    );
    let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
    assert!(result.is_ok());

    let instruction =
        Instruction::new(program_id, &ComputeBudget::default().max_call_depth, vec![]);
    let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
    assert!(result.is_err());
}

#[test]
fn assert_instruction_count() {
    solana_logger::setup();

    let mut programs = Vec::new();
    #[cfg(feature = "bpf_c")]
    {
        programs.extend_from_slice(&[
            ("bpf_to_bpf", 13),
            ("multiple_static", 8),
            ("noop", 57),
            ("relative_call", 10),
            ("sanity", 1140),
            ("sanity++", 1140),
            ("struct_pass", 8),
            ("struct_ret", 22),
        ]);
    }
    #[cfg(feature = "bpf_rust")]
    {
        programs.extend_from_slice(&[
            ("solana_bpf_rust_128bit", 543),
            ("solana_bpf_rust_alloc", 19082),
            ("solana_bpf_rust_dep_crate", 2),
            ("solana_bpf_rust_external_spend", 538),
            ("solana_bpf_rust_iter", 723),
            ("solana_bpf_rust_many_args", 231),
            ("solana_bpf_rust_noop", 488),
            ("solana_bpf_rust_param_passing", 46),
            ("solana_bpf_rust_ristretto", 19399),
            ("solana_bpf_rust_sanity", 1965),
        ]);
    }

    for program in programs.iter() {
        println!("Test program: {:?}", program.0);
        let program_id = solana_sdk::pubkey::new_rand();
        let key = solana_sdk::pubkey::new_rand();
        let mut account = RefCell::new(Account::default());
        let parameter_accounts = vec![KeyedAccount::new(&key, false, &mut account)];
        let count = run_program(program.0, &program_id, &parameter_accounts[..], &[]).unwrap();
        println!("  {} : {:?} ({:?})", program.0, count, program.1,);
        assert!(count <= program.1);
    }
}

// Mock InvokeContext

#[derive(Debug, Default)]
struct MockInvokeContext {
    pub key: Pubkey,
    pub logger: MockLogger,
    pub compute_budget: ComputeBudget,
    pub compute_meter: MockComputeMeter,
}
impl InvokeContext for MockInvokeContext {
    fn push(&mut self, _key: &Pubkey) -> Result<(), InstructionError> {
        Ok(())
    }
    fn pop(&mut self) {}
    fn verify_and_update(
        &mut self,
        _message: &Message,
        _instruction: &CompiledInstruction,
        _accounts: &[Rc<RefCell<Account>>],
    ) -> Result<(), InstructionError> {
        Ok(())
    }
    fn get_caller(&self) -> Result<&Pubkey, InstructionError> {
        Ok(&self.key)
    }
    fn get_programs(&self) -> &[(Pubkey, ProcessInstruction)] {
        &[]
    }
    fn get_logger(&self) -> Rc<RefCell<dyn Logger>> {
        Rc::new(RefCell::new(self.logger.clone()))
    }
    fn get_compute_budget(&self) -> &ComputeBudget {
        &self.compute_budget
    }
    fn get_compute_meter(&self) -> Rc<RefCell<dyn ComputeMeter>> {
        Rc::new(RefCell::new(self.compute_meter.clone()))
    }
    fn add_executor(&mut self, _pubkey: &Pubkey, _executor: Arc<dyn Executor>) {}
    fn get_executor(&mut self, _pubkey: &Pubkey) -> Option<Arc<dyn Executor>> {
        None
    }
    fn record_instruction(&self, _instruction: &Instruction) {}
    fn is_feature_active(&self, _feature_id: &Pubkey) -> bool {
        true
    }
}

#[derive(Debug, Default, Clone)]
struct MockComputeMeter {}
impl ComputeMeter for MockComputeMeter {
    fn consume(&mut self, _amount: u64) -> Result<(), InstructionError> {
        Ok(())
    }
    fn get_remaining(&self) -> u64 {
        u64::MAX
    }
}
#[derive(Debug, Default, Clone)]
struct MockLogger {}
impl Logger for MockLogger {
    fn log_enabled(&self) -> bool {
        true
    }
    fn log(&mut self, _message: &str) {
        // println!("{}", message);
    }
}

struct TestInstructionMeter {}
impl InstructionMeter for TestInstructionMeter {
    fn consume(&mut self, _amount: u64) {}
    fn get_remaining(&self) -> u64 {
        u64::MAX
    }
}

#[cfg(any(feature = "bpf_rust"))]
#[test]
fn test_program_bpf_instruction_introspection() {
    solana_logger::setup();

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50_000);
    let mut bank = Bank::new(&genesis_config);

    let (name, id, entrypoint) = solana_bpf_loader_program!();
    bank.add_builtin_loader(&name, id, entrypoint);
    let bank = Arc::new(bank);
    let bank_client = BankClient::new_shared(&bank);

    let program_id = load_bpf_program(
        &bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_bpf_rust_instruction_introspection",
    );

    // Passing transaction
    let account_metas = vec![AccountMeta::new_readonly(
        solana_sdk::sysvar::instructions::id(),
        false,
    )];
    let instruction0 = Instruction::new(program_id, &[0u8, 0u8], account_metas.clone());
    let instruction1 = Instruction::new(program_id, &[0u8, 1u8], account_metas.clone());
    let instruction2 = Instruction::new(program_id, &[0u8, 2u8], account_metas);
    let message = Message::new(
        &[instruction0, instruction1, instruction2],
        Some(&mint_keypair.pubkey()),
    );
    let result = bank_client.send_and_confirm_message(&[&mint_keypair], message);
    println!("result: {:?}", result);
    assert!(result.is_ok());

    // writable special instructions11111 key, should not be allowed
    let account_metas = vec![AccountMeta::new(
        solana_sdk::sysvar::instructions::id(),
        false,
    )];
    let instruction = Instruction::new(program_id, &0u8, account_metas);
    let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
    assert_eq!(
        result.unwrap_err().unwrap(),
        TransactionError::InvalidAccountIndex
    );

    // No accounts, should error
    let instruction = Instruction::new(program_id, &0u8, vec![]);
    let result = bank_client.send_and_confirm_instruction(&mint_keypair, instruction);
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().unwrap(),
        TransactionError::InstructionError(
            0,
            solana_sdk::instruction::InstructionError::NotEnoughAccountKeys
        )
    );
    assert!(bank
        .get_account(&solana_sdk::sysvar::instructions::id())
        .is_none());
}

#![cfg(test)]

use {
    crate::mock_bank::MockBankCallback,
    solana_bpf_loader_program::syscalls::{SyscallAbort, SyscallLog, SyscallMemcpy, SyscallMemset},
    solana_program_runtime::{
        compute_budget::ComputeBudget,
        invoke_context::InvokeContext,
        loaded_programs::{
            BlockRelation, ForkGraph, LoadedProgram, ProgramCache, ProgramRuntimeEnvironments,
        },
        runtime_config::RuntimeConfig,
        solana_rbpf::{
            program::{BuiltinFunction, BuiltinProgram, FunctionRegistry},
            vm::Config,
        },
        timings::ExecuteTimings,
    },
    solana_sdk::{
        account::{AccountSharedData, WritableAccount},
        bpf_loader,
        clock::{Epoch, Slot},
        epoch_schedule::EpochSchedule,
        fee::FeeStructure,
        hash::Hash,
        instruction::CompiledInstruction,
        message::{Message, MessageHeader},
        native_loader,
        pubkey::Pubkey,
        signature::Signature,
        transaction::{SanitizedTransaction, Transaction},
    },
    solana_svm::{
        account_loader::TransactionCheckResult,
        transaction_error_metrics::TransactionErrorMetrics,
        transaction_processor::{ExecutionRecordingConfig, TransactionBatchProcessor},
    },
    std::{
        cmp::Ordering,
        env,
        fs::{self, File},
        io::Read,
        sync::{Arc, RwLock},
    },
};

// This module contains the implementation of TransactionProcessingCallback
mod mock_bank;

const BPF_LOADER_NAME: &str = "solana_bpf_loader_program";
const DEPLOYMENT_SLOT: u64 = 0;
const EXECUTION_SLOT: u64 = 5; // The execution slot must be greater than the deployment slot
const DEPLOYMENT_EPOCH: u64 = 0;
const EXECUTION_EPOCH: u64 = 2; // The execution epoch must be greater than the deployment epoch

struct MockForkGraph {}

impl ForkGraph for MockForkGraph {
    fn relationship(&self, a: Slot, b: Slot) -> BlockRelation {
        match a.cmp(&b) {
            Ordering::Less => BlockRelation::Ancestor,
            Ordering::Equal => BlockRelation::Equal,
            Ordering::Greater => BlockRelation::Descendant,
        }
    }

    fn slot_epoch(&self, _slot: Slot) -> Option<Epoch> {
        Some(0)
    }
}

fn create_custom_environment<'a>() -> BuiltinProgram<InvokeContext<'a>> {
    let compute_budget = ComputeBudget::default();
    let vm_config = Config {
        max_call_depth: compute_budget.max_call_depth,
        stack_frame_size: compute_budget.stack_frame_size,
        enable_address_translation: true,
        enable_stack_frame_gaps: true,
        instruction_meter_checkpoint_distance: 10000,
        enable_instruction_meter: true,
        enable_instruction_tracing: true,
        enable_symbol_and_section_labels: true,
        reject_broken_elfs: true,
        noop_instruction_rate: 256,
        sanitize_user_provided_values: true,
        external_internal_function_hash_collision: false,
        reject_callx_r10: false,
        enable_sbpf_v1: true,
        enable_sbpf_v2: false,
        optimize_rodata: false,
        new_elf_parser: false,
        aligned_memory_mapping: true,
    };

    // These functions are system calls the compile contract calls during execution, so they
    // need to be registered.
    let mut function_registry = FunctionRegistry::<BuiltinFunction<InvokeContext>>::default();
    function_registry
        .register_function_hashed(*b"abort", SyscallAbort::vm)
        .expect("Registration failed");
    function_registry
        .register_function_hashed(*b"sol_log_", SyscallLog::vm)
        .expect("Registration failed");
    function_registry
        .register_function_hashed(*b"sol_memcpy_", SyscallMemcpy::vm)
        .expect("Registration failed");
    function_registry
        .register_function_hashed(*b"sol_memset_", SyscallMemset::vm)
        .expect("Registration failed");

    BuiltinProgram::new_loader(vm_config, function_registry)
}

fn create_executable_environment(
    mock_bank: &mut MockBankCallback,
) -> (ProgramCache<MockForkGraph>, Vec<Pubkey>) {
    let mut program_cache = ProgramCache::<MockForkGraph>::new(0, 20);

    // We must register the bpf loader account as a loadable account, otherwise programs
    // won't execute.
    let account_data = native_loader::create_loadable_account_with_fields(
        BPF_LOADER_NAME,
        (5000, DEPLOYMENT_EPOCH),
    );
    mock_bank
        .account_shared_data
        .insert(bpf_loader::id(), account_data);

    // The bpf loader needs an executable as well
    program_cache.assign_program(
        bpf_loader::id(),
        Arc::new(LoadedProgram::new_builtin(
            DEPLOYMENT_SLOT,
            BPF_LOADER_NAME.len(),
            solana_bpf_loader_program::Entrypoint::vm,
        )),
    );

    program_cache.environments = ProgramRuntimeEnvironments {
        program_runtime_v1: Arc::new(create_custom_environment()),
        // We are not using program runtime v2
        program_runtime_v2: Arc::new(BuiltinProgram::new_loader(
            Config::default(),
            FunctionRegistry::default(),
        )),
    };

    program_cache.fork_graph = Some(Arc::new(RwLock::new(MockForkGraph {})));

    // Inform SVM of the registered builins
    let registered_built_ins = vec![bpf_loader::id()];
    (program_cache, registered_built_ins)
}

fn prepare_transactions(
    mock_bank: &mut MockBankCallback,
) -> (Vec<SanitizedTransaction>, Vec<TransactionCheckResult>) {
    let mut all_transactions = Vec::new();
    let mut transaction_checks = Vec::new();

    // A transaction that works without any account
    let key1 = Pubkey::new_unique();
    let fee_payer = Pubkey::new_unique();
    let message = Message {
        account_keys: vec![fee_payer, key1],
        header: MessageHeader {
            num_required_signatures: 1,
            num_readonly_signed_accounts: 0,
            num_readonly_unsigned_accounts: 0,
        },
        instructions: vec![CompiledInstruction {
            program_id_index: 1,
            accounts: vec![],
            data: vec![],
        }],
        recent_blockhash: Hash::default(),
    };

    let transaction = Transaction {
        signatures: vec![Signature::new_unique()],
        message,
    };
    let sanitized_transaction =
        SanitizedTransaction::try_from_legacy_transaction(transaction).unwrap();
    all_transactions.push(sanitized_transaction);
    transaction_checks.push((Ok(()), None, Some(20)));

    // Loading the program file
    let mut dir = env::current_dir().unwrap();
    dir.push("tests");
    // File compiled from
    // https://github.com/solana-developers/program-examples/blob/feb82f254a4633ce2107d06060f2d0558dc987f5/basics/hello-solana/native/program/src/lib.rs
    dir.push("hello_solana_program.so");
    let mut file = File::open(dir.clone()).expect("file not found");
    let metadata = fs::metadata(dir).expect("Unable to read metadata");
    let mut buffer = vec![0; metadata.len() as usize];
    file.read_exact(&mut buffer).expect("Buffer overflow");

    // The program account must have funds and hold the executable binary
    let mut account_data = AccountSharedData::default();
    // The executable account owner must be one of the loaders.
    account_data.set_owner(bpf_loader::id());
    account_data.set_data(buffer);
    account_data.set_executable(true);
    account_data.set_lamports(25);
    mock_bank.account_shared_data.insert(key1, account_data);

    // The transaction fee payer must have enough funds
    let mut account_data = AccountSharedData::default();
    account_data.set_lamports(80000);
    mock_bank
        .account_shared_data
        .insert(fee_payer, account_data);

    // TODO: Include these examples as well:
    // A simple funds transfer between accounts
    // A transaction that fails
    // A transaction whose verification has already failed

    (all_transactions, transaction_checks)
}

#[test]
fn svm_integration() {
    let mut mock_bank = MockBankCallback::default();
    let (transactions, mut check_results) = prepare_transactions(&mut mock_bank);
    let (program_cache, builtins) = create_executable_environment(&mut mock_bank);
    let program_cache = Arc::new(RwLock::new(program_cache));
    let batch_processor = TransactionBatchProcessor::<MockForkGraph>::new(
        EXECUTION_SLOT,
        EXECUTION_EPOCH,
        EpochSchedule::default(),
        FeeStructure::default(),
        Arc::new(RuntimeConfig::default()),
        program_cache.clone(),
    );

    let mut error_counter = TransactionErrorMetrics::default();
    let recording_config = ExecutionRecordingConfig {
        enable_log_recording: true,
        enable_return_data_recording: false,
        enable_cpi_recording: false,
    };
    let mut timings = ExecuteTimings::default();

    let result = batch_processor.load_and_execute_sanitized_transactions(
        &mock_bank,
        &transactions,
        check_results.as_mut_slice(),
        &mut error_counter,
        recording_config,
        &mut timings,
        None,
        builtins.iter(),
        None,
        false,
    );

    assert_eq!(result.execution_results.len(), 1);
    assert!(result.execution_results[0]
        .details()
        .unwrap()
        .status
        .is_ok());
    let logs = result.execution_results[0]
        .details()
        .unwrap()
        .log_messages
        .as_ref()
        .unwrap();
    assert!(logs.contains(&"Program log: Hello, Solana!".to_string()));
}

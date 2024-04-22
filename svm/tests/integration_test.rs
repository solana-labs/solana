#![cfg(test)]

use {
    crate::{mock_bank::MockBankCallback, transaction_builder::SanitizedTransactionBuilder},
    solana_bpf_loader_program::syscalls::{
        SyscallAbort, SyscallGetClockSysvar, SyscallInvokeSignedRust, SyscallLog, SyscallMemcpy,
        SyscallMemset, SyscallSetReturnData,
    },
    solana_program_runtime::{
        compute_budget::ComputeBudget,
        invoke_context::InvokeContext,
        loaded_programs::{
            BlockRelation, ForkGraph, ProgramCache, ProgramCacheEntry, ProgramRuntimeEnvironments,
        },
        solana_rbpf::{
            program::{BuiltinFunction, BuiltinProgram, FunctionRegistry},
            vm::Config,
        },
        timings::ExecuteTimings,
    },
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount, WritableAccount},
        bpf_loader_upgradeable::{self, UpgradeableLoaderState},
        clock::{Clock, Epoch, Slot, UnixTimestamp},
        epoch_schedule::EpochSchedule,
        hash::Hash,
        instruction::AccountMeta,
        pubkey::Pubkey,
        signature::Signature,
        sysvar::SysvarId,
        transaction::{SanitizedTransaction, TransactionError},
    },
    solana_svm::{
        account_loader::TransactionCheckResult,
        runtime_config::RuntimeConfig,
        transaction_error_metrics::TransactionErrorMetrics,
        transaction_processor::{
            ExecutionRecordingConfig, TransactionBatchProcessor, TransactionProcessingCallback,
        },
        transaction_results::TransactionExecutionResult,
    },
    std::{
        cmp::Ordering,
        collections::{HashMap, HashSet},
        env,
        fs::{self, File},
        io::Read,
        sync::{Arc, RwLock},
        time::{SystemTime, UNIX_EPOCH},
    },
};

// This module contains the implementation of TransactionProcessingCallback
mod mock_bank;
mod transaction_builder;

const BPF_LOADER_NAME: &str = "solana_bpf_loader_upgradeable_program";
const SYSTEM_PROGRAM_NAME: &str = "system_program";
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

    function_registry
        .register_function_hashed(*b"sol_invoke_signed_rust", SyscallInvokeSignedRust::vm)
        .expect("Registration failed");

    function_registry
        .register_function_hashed(*b"sol_set_return_data", SyscallSetReturnData::vm)
        .expect("Registration failed");

    function_registry
        .register_function_hashed(*b"sol_get_clock_sysvar", SyscallGetClockSysvar::vm)
        .expect("Registration failed");

    BuiltinProgram::new_loader(vm_config, function_registry)
}

fn create_executable_environment(mock_bank: &mut MockBankCallback) -> ProgramCache<MockForkGraph> {
    let mut program_cache = ProgramCache::<MockForkGraph>::new(0, 20);

    program_cache.environments = ProgramRuntimeEnvironments {
        program_runtime_v1: Arc::new(create_custom_environment()),
        // We are not using program runtime v2
        program_runtime_v2: Arc::new(BuiltinProgram::new_loader(
            Config::default(),
            FunctionRegistry::default(),
        )),
    };

    program_cache.fork_graph = Some(Arc::new(RwLock::new(MockForkGraph {})));

    // We must fill in the sysvar cache entries
    let time_now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs() as i64;
    let clock = Clock {
        slot: DEPLOYMENT_SLOT,
        epoch_start_timestamp: time_now.saturating_sub(10) as UnixTimestamp,
        epoch: DEPLOYMENT_EPOCH,
        leader_schedule_epoch: DEPLOYMENT_EPOCH,
        unix_timestamp: time_now as UnixTimestamp,
    };

    let mut account_data = AccountSharedData::default();
    account_data.set_data(bincode::serialize(&clock).unwrap());
    mock_bank
        .account_shared_data
        .borrow_mut()
        .insert(Clock::id(), account_data);

    program_cache
}

fn load_program(name: String) -> Vec<u8> {
    // Loading the program file
    let mut dir = env::current_dir().unwrap();
    dir.push("tests");
    dir.push("example-programs");
    dir.push(name.as_str());
    let name = name.replace('-', "_");
    dir.push(name + "_program.so");
    let mut file = File::open(dir.clone()).expect("file not found");
    let metadata = fs::metadata(dir).expect("Unable to read metadata");
    let mut buffer = vec![0; metadata.len() as usize];
    file.read_exact(&mut buffer).expect("Buffer overflow");
    buffer
}

fn deploy_program(name: String, mock_bank: &mut MockBankCallback) -> Pubkey {
    let program_account = Pubkey::new_unique();
    let program_data_account = Pubkey::new_unique();
    let state = UpgradeableLoaderState::Program {
        programdata_address: program_data_account,
    };

    // The program account must have funds and hold the executable binary
    let mut account_data = AccountSharedData::default();
    account_data.set_data(bincode::serialize(&state).unwrap());
    account_data.set_lamports(25);
    account_data.set_owner(bpf_loader_upgradeable::id());
    mock_bank
        .account_shared_data
        .borrow_mut()
        .insert(program_account, account_data);

    let mut account_data = AccountSharedData::default();
    let state = UpgradeableLoaderState::ProgramData {
        slot: DEPLOYMENT_SLOT,
        upgrade_authority_address: None,
    };
    let mut header = bincode::serialize(&state).unwrap();
    let mut complement = vec![
        0;
        std::cmp::max(
            0,
            UpgradeableLoaderState::size_of_programdata_metadata().saturating_sub(header.len())
        )
    ];
    let mut buffer = load_program(name);
    header.append(&mut complement);
    header.append(&mut buffer);
    account_data.set_data(header);
    mock_bank
        .account_shared_data
        .borrow_mut()
        .insert(program_data_account, account_data);

    program_account
}

fn register_builtins(
    mock_bank: &MockBankCallback,
    batch_processor: &TransactionBatchProcessor<MockForkGraph>,
) {
    // We must register the bpf loader account as a loadable account, otherwise programs
    // won't execute.
    batch_processor.add_builtin(
        mock_bank,
        bpf_loader_upgradeable::id(),
        BPF_LOADER_NAME,
        ProgramCacheEntry::new_builtin(
            DEPLOYMENT_SLOT,
            BPF_LOADER_NAME.len(),
            solana_bpf_loader_program::Entrypoint::vm,
        ),
    );

    // In order to perform a transference of native tokens using the system instruction,
    // the system program builtin must be registered.
    batch_processor.add_builtin(
        mock_bank,
        solana_system_program::id(),
        SYSTEM_PROGRAM_NAME,
        ProgramCacheEntry::new_builtin(
            DEPLOYMENT_SLOT,
            SYSTEM_PROGRAM_NAME.len(),
            solana_system_program::system_processor::Entrypoint::vm,
        ),
    );
}

fn prepare_transactions(
    mock_bank: &mut MockBankCallback,
) -> (Vec<SanitizedTransaction>, Vec<TransactionCheckResult>) {
    let mut transaction_builder = SanitizedTransactionBuilder::default();
    let mut all_transactions = Vec::new();
    let mut transaction_checks = Vec::new();

    // A transaction that works without any account
    let hello_program = deploy_program("hello-solana".to_string(), mock_bank);
    let fee_payer = Pubkey::new_unique();
    transaction_builder.create_instruction(hello_program, Vec::new(), HashMap::new(), Vec::new());

    let sanitized_transaction =
        transaction_builder.build(Hash::default(), (fee_payer, Signature::new_unique()));

    all_transactions.push(sanitized_transaction);
    transaction_checks.push((Ok(()), None, Some(20)));

    // The transaction fee payer must have enough funds
    let mut account_data = AccountSharedData::default();
    account_data.set_lamports(80000);
    mock_bank
        .account_shared_data
        .borrow_mut()
        .insert(fee_payer, account_data);

    // A simple funds transfer between accounts
    let transfer_program_account = deploy_program("simple-transfer".to_string(), mock_bank);
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
        transaction_builder.build(Hash::default(), (fee_payer, Signature::new_unique()));
    all_transactions.push(sanitized_transaction);
    transaction_checks.push((Ok(()), None, Some(20)));

    // Setting up the accounts for the transfer

    // fee payer
    let mut account_data = AccountSharedData::default();
    account_data.set_lamports(80000);
    mock_bank
        .account_shared_data
        .borrow_mut()
        .insert(fee_payer, account_data);

    // sender
    let mut account_data = AccountSharedData::default();
    account_data.set_lamports(900000);
    mock_bank
        .account_shared_data
        .borrow_mut()
        .insert(sender, account_data);

    // recipient
    let mut account_data = AccountSharedData::default();
    account_data.set_lamports(900000);
    mock_bank
        .account_shared_data
        .borrow_mut()
        .insert(recipient, account_data);

    // The system account is set in `create_executable_environment`

    // A program that utilizes a Sysvar
    let program_account = deploy_program("clock-sysvar".to_string(), mock_bank);
    let fee_payer = Pubkey::new_unique();
    transaction_builder.create_instruction(program_account, Vec::new(), HashMap::new(), Vec::new());

    let sanitized_transaction =
        transaction_builder.build(Hash::default(), (fee_payer, Signature::new_unique()));

    all_transactions.push(sanitized_transaction);
    transaction_checks.push((Ok(()), None, Some(20)));

    let mut account_data = AccountSharedData::default();
    account_data.set_lamports(80000);
    mock_bank
        .account_shared_data
        .borrow_mut()
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
        transaction_builder.build(Hash::default(), (fee_payer, Signature::new_unique()));
    all_transactions.push(sanitized_transaction.clone());
    transaction_checks.push((Ok(()), None, Some(20)));

    // fee payer
    let mut account_data = AccountSharedData::default();
    account_data.set_lamports(80000);
    mock_bank
        .account_shared_data
        .borrow_mut()
        .insert(fee_payer, account_data);

    // Sender without enough funds
    let mut account_data = AccountSharedData::default();
    account_data.set_lamports(900000);
    mock_bank
        .account_shared_data
        .borrow_mut()
        .insert(sender, account_data);

    // recipient
    let mut account_data = AccountSharedData::default();
    account_data.set_lamports(900000);
    mock_bank
        .account_shared_data
        .borrow_mut()
        .insert(recipient, account_data);

    // A transaction whose verification has already failed
    all_transactions.push(sanitized_transaction);
    transaction_checks.push((Err(TransactionError::BlockhashNotFound), None, Some(20)));

    (all_transactions, transaction_checks)
}

#[test]
fn svm_integration() {
    let mut mock_bank = MockBankCallback::default();
    let (transactions, mut check_results) = prepare_transactions(&mut mock_bank);
    let program_cache = create_executable_environment(&mut mock_bank);
    let program_cache = Arc::new(RwLock::new(program_cache));
    let batch_processor = TransactionBatchProcessor::<MockForkGraph>::new(
        EXECUTION_SLOT,
        EXECUTION_EPOCH,
        EpochSchedule::default(),
        Arc::new(RuntimeConfig::default()),
        program_cache.clone(),
        HashSet::default(),
    );

    // The sysvars must be put in the cache
    batch_processor.fill_missing_sysvar_cache_entries(&mock_bank);
    register_builtins(&mock_bank, &batch_processor);

    let mut error_counter = TransactionErrorMetrics::default();
    let recording_config = ExecutionRecordingConfig {
        enable_log_recording: true,
        enable_return_data_recording: true,
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
        None,
        false,
    );

    assert_eq!(result.execution_results.len(), 5);
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

    assert!(result.execution_results[1]
        .details()
        .unwrap()
        .status
        .is_ok());

    // The SVM does not commit the account changes in MockBank
    let recipient_key = transactions[1].message().account_keys()[2];
    let recipient_data = result.loaded_transactions[1]
        .0
        .as_ref()
        .unwrap()
        .accounts
        .iter()
        .find(|key| key.0 == recipient_key)
        .unwrap();
    assert_eq!(recipient_data.1.lamports(), 900010);

    let return_data = result.execution_results[2]
        .details()
        .unwrap()
        .return_data
        .as_ref()
        .unwrap();
    let time = i64::from_be_bytes(return_data.data[0..8].try_into().unwrap());
    let clock_data = mock_bank.get_account_shared_data(&Clock::id()).unwrap();
    let clock_info: Clock = bincode::deserialize(clock_data.data()).unwrap();
    assert_eq!(clock_info.unix_timestamp, time);

    assert!(result.execution_results[3]
        .details()
        .unwrap()
        .status
        .is_err());
    assert!(result.execution_results[3]
        .details()
        .unwrap()
        .log_messages
        .as_ref()
        .unwrap()
        .contains(&"Transfer: insufficient lamports 900000, need 900050".to_string()));

    assert!(matches!(
        result.execution_results[4],
        TransactionExecutionResult::NotExecuted(TransactionError::BlockhashNotFound)
    ));
}

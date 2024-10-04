use {
    log::*,
    solana_bpf_loader_program::syscalls::{
        SyscallAbort, SyscallGetClockSysvar, SyscallInvokeSignedRust, SyscallLog,
        SyscallLogBpfComputeUnits, SyscallLogPubkey, SyscallLogU64, SyscallMemcpy, SyscallMemset,
        SyscallSetReturnData,
    },
    solana_compute_budget::compute_budget::ComputeBudget,
    solana_program_runtime::{
        invoke_context::InvokeContext,
        loaded_programs::{
            BlockRelation, ForkGraph, LoadProgramMetrics, ProgramCacheEntry,
            ProgramRuntimeEnvironments,
        },
        solana_rbpf::{
            program::{BuiltinFunction, BuiltinProgram, FunctionRegistry},
            vm::Config,
        },
    },
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        clock::{Clock, Slot, UnixTimestamp},
        feature_set::FeatureSet,
        message::AccountKeys,
        native_loader,
        pubkey::Pubkey,
        sysvar::SysvarId,
        transaction::SanitizedTransaction,
    },
    solana_svm::{
        transaction_processing_callback::TransactionProcessingCallback,
        transaction_processing_result::TransactionProcessingResult,
        transaction_processor::TransactionBatchProcessor,
    },
    std::{
        collections::HashMap,
        sync::{Arc, RwLock},
        time::{SystemTime, UNIX_EPOCH},
    },
};

const DEPLOYMENT_SLOT: u64 = 0;
const DEPLOYMENT_EPOCH: u64 = 0;

pub struct MockForkGraph {}

impl ForkGraph for MockForkGraph {
    fn relationship(&self, a: Slot, b: Slot) -> BlockRelation {
        match a.cmp(&b) {
            std::cmp::Ordering::Less => BlockRelation::Ancestor,
            std::cmp::Ordering::Equal => BlockRelation::Equal,
            std::cmp::Ordering::Greater => BlockRelation::Descendant,
        }
    }
}

pub struct MockBankCallback {
    pub feature_set: Arc<FeatureSet>,
    pub account_shared_data: RwLock<HashMap<Pubkey, AccountSharedData>>,
}

impl TransactionProcessingCallback for MockBankCallback {
    fn account_matches_owners(&self, account: &Pubkey, owners: &[Pubkey]) -> Option<usize> {
        if let Some(data) = self.account_shared_data.read().unwrap().get(account) {
            if data.lamports() == 0 {
                None
            } else {
                owners.iter().position(|entry| data.owner() == entry)
            }
        } else {
            None
        }
    }

    fn get_account_shared_data(&self, pubkey: &Pubkey) -> Option<AccountSharedData> {
        debug!(
            "Get account {pubkey} shared data, thread {:?}",
            std::thread::current().name()
        );
        self.account_shared_data
            .read()
            .unwrap()
            .get(pubkey)
            .cloned()
    }

    fn add_builtin_account(&self, name: &str, program_id: &Pubkey) {
        let account_data = native_loader::create_loadable_account_with_fields(name, (5000, 0));

        self.account_shared_data
            .write()
            .unwrap()
            .insert(*program_id, account_data);
    }
}

impl MockBankCallback {
    pub fn new(account_map: Vec<(Pubkey, AccountSharedData)>) -> Self {
        Self {
            feature_set: Arc::new(FeatureSet::default()),
            account_shared_data: RwLock::new(HashMap::from_iter(account_map)),
        }
    }

    #[allow(dead_code)]
    pub fn override_feature_set(&mut self, new_set: FeatureSet) {
        self.feature_set = Arc::new(new_set)
    }
}

pub struct LoadAndExecuteTransactionsOutput {
    // Vector of results indicating whether a transaction was executed or could not
    // be executed. Note executed transactions can still have failed!
    pub processing_results: Vec<TransactionProcessingResult>,
}

pub struct TransactionBatch<'a> {
    lock_results: Vec<solana_sdk::transaction::Result<()>>,
    sanitized_txs: std::borrow::Cow<'a, [SanitizedTransaction]>,
}

impl<'a> TransactionBatch<'a> {
    pub fn new(
        lock_results: Vec<solana_sdk::transaction::Result<()>>,
        sanitized_txs: std::borrow::Cow<'a, [SanitizedTransaction]>,
    ) -> Self {
        assert_eq!(lock_results.len(), sanitized_txs.len());
        Self {
            lock_results,
            sanitized_txs,
        }
    }

    pub fn lock_results(&self) -> &Vec<solana_sdk::transaction::Result<()>> {
        &self.lock_results
    }

    pub fn sanitized_transactions(&self) -> &[SanitizedTransaction] {
        &self.sanitized_txs
    }
}

pub fn create_custom_environment<'a>() -> BuiltinProgram<InvokeContext<'a>> {
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
        aligned_memory_mapping: true,
    };

    // Register system calls that the compiled contract calls during execution.
    let mut function_registry = FunctionRegistry::<BuiltinFunction<InvokeContext>>::default();
    function_registry
        .register_function_hashed(*b"abort", SyscallAbort::vm)
        .expect("Registration failed");
    function_registry
        .register_function_hashed(*b"sol_log_", SyscallLog::vm)
        .expect("Registration failed");
    function_registry
        .register_function_hashed(*b"sol_log_64_", SyscallLogU64::vm)
        .expect("Registration failed");
    function_registry
        .register_function_hashed(*b"sol_log_compute_units_", SyscallLogBpfComputeUnits::vm)
        .expect("Registration failed");
    function_registry
        .register_function_hashed(*b"sol_log_pubkey", SyscallLogPubkey::vm)
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

pub fn create_executable_environment(
    fork_graph: Arc<RwLock<MockForkGraph>>,
    account_keys: &AccountKeys,
    mock_bank: &mut MockBankCallback,
    transaction_processor: &TransactionBatchProcessor<MockForkGraph>,
) {
    let mut program_cache = transaction_processor.program_cache.write().unwrap();

    program_cache.environments = ProgramRuntimeEnvironments {
        program_runtime_v1: Arc::new(create_custom_environment()),
        // We are not using program runtime v2
        program_runtime_v2: Arc::new(BuiltinProgram::new_loader(
            Config::default(),
            FunctionRegistry::default(),
        )),
    };

    program_cache.fork_graph = Some(Arc::downgrade(&fork_graph));
    // add programs to cache
    for key in account_keys.iter() {
        if let Some(account) = mock_bank.get_account_shared_data(key) {
            if account.executable() && *account.owner() == solana_sdk::bpf_loader_upgradeable::id()
            {
                let data = account.data();
                let program_data_account_key = Pubkey::try_from(data[4..].to_vec()).unwrap();
                let program_data_account = mock_bank
                    .get_account_shared_data(&program_data_account_key)
                    .unwrap();
                let program_data = program_data_account.data();
                let elf_bytes = program_data[45..].to_vec();

                let program_runtime_environment =
                    program_cache.environments.program_runtime_v1.clone();
                program_cache.assign_program(
                    *key,
                    Arc::new(
                        ProgramCacheEntry::new(
                            &solana_sdk::bpf_loader_upgradeable::id(),
                            program_runtime_environment,
                            0,
                            0,
                            &elf_bytes,
                            elf_bytes.len(),
                            &mut LoadProgramMetrics::default(),
                        )
                        .unwrap(),
                    ),
                );
            }
        }
    }

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
    account_data.set_data_from_slice(bincode::serialize(&clock).unwrap().as_slice());
    mock_bank
        .account_shared_data
        .write()
        .unwrap()
        .insert(Clock::id(), account_data);
}

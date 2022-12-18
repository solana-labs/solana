//! The solana-program-test provides a BanksClient-based test framework SBF programs
#![allow(clippy::integer_arithmetic)]

// Export tokio for test clients
pub use tokio;
use {
    async_trait::async_trait,
    chrono_humanize::{Accuracy, HumanTime, Tense},
    log::*,
    solana_banks_client::start_client,
    solana_banks_server::banks_server::start_local_server,
    solana_bpf_loader_program::serialization::serialize_parameters,
    solana_program_runtime::{
        compute_budget::ComputeBudget, ic_msg, invoke_context::ProcessInstructionWithContext,
        stable_log, timings::ExecuteTimings,
    },
    solana_runtime::{
        accounts_background_service::{AbsRequestSender, SnapshotRequestType},
        bank::Bank,
        bank_forks::BankForks,
        builtins::Builtin,
        commitment::BlockCommitmentCache,
        epoch_accounts_hash::EpochAccountsHash,
        genesis_utils::{create_genesis_config_with_leader_ex, GenesisConfigInfo},
        runtime_config::RuntimeConfig,
    },
    solana_sdk::{
        account::{Account, AccountSharedData},
        account_info::AccountInfo,
        clock::Slot,
        entrypoint::{deserialize, ProgramResult, SUCCESS},
        feature_set::FEATURE_NAMES,
        fee_calculator::{FeeCalculator, FeeRateGovernor, DEFAULT_TARGET_LAMPORTS_PER_SIGNATURE},
        genesis_config::{ClusterType, GenesisConfig},
        hash::Hash,
        instruction::{Instruction, InstructionError},
        native_token::sol_to_lamports,
        poh_config::PohConfig,
        program_error::{ProgramError, UNSUPPORTED_SYSVAR},
        pubkey::Pubkey,
        rent::Rent,
        signature::{Keypair, Signer},
        sysvar::{Sysvar, SysvarId},
    },
    solana_vote_program::vote_state::{self, VoteState, VoteStateVersions},
    std::{
        cell::RefCell,
        collections::{HashMap, HashSet},
        convert::TryFrom,
        fs::File,
        io::{self, Read},
        mem::transmute,
        path::{Path, PathBuf},
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
        time::{Duration, Instant},
    },
    thiserror::Error,
    tokio::task::JoinHandle,
};
// Export types so test clients can limit their solana crate dependencies
pub use {
    solana_banks_client::{BanksClient, BanksClientError},
    solana_banks_interface::BanksTransactionResultWithMetadata,
    solana_program_runtime::invoke_context::InvokeContext,
    solana_sdk::transaction_context::IndexOfAccount,
};

pub mod programs;

#[macro_use]
extern crate solana_bpf_loader_program;

/// Errors from the program test environment
#[derive(Error, Debug, PartialEq, Eq)]
pub enum ProgramTestError {
    /// The chosen warp slot is not in the future, so warp is not performed
    #[error("Warp slot not in the future")]
    InvalidWarpSlot,
}

thread_local! {
    static INVOKE_CONTEXT: RefCell<Option<usize>> = RefCell::new(None);
}
fn set_invoke_context(new: &mut InvokeContext) {
    INVOKE_CONTEXT
        .with(|invoke_context| unsafe { invoke_context.replace(Some(transmute::<_, usize>(new))) });
}
fn get_invoke_context<'a, 'b>() -> &'a mut InvokeContext<'b> {
    let ptr = INVOKE_CONTEXT.with(|invoke_context| match *invoke_context.borrow() {
        Some(val) => val,
        None => panic!("Invoke context not set!"),
    });
    unsafe { transmute::<usize, &mut InvokeContext>(ptr) }
}

pub fn builtin_process_instruction(
    process_instruction: solana_sdk::entrypoint::ProcessInstruction,
    _first_instruction_account: IndexOfAccount,
    invoke_context: &mut InvokeContext,
) -> Result<(), InstructionError> {
    set_invoke_context(invoke_context);

    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let instruction_data = instruction_context.get_instruction_data();
    let instruction_account_indices = 0..instruction_context.get_number_of_instruction_accounts();

    let log_collector = invoke_context.get_log_collector();
    let program_id = instruction_context.get_last_program_key(transaction_context)?;
    stable_log::program_invoke(
        &log_collector,
        program_id,
        invoke_context.get_stack_height(),
    );

    // Copy indices_in_instruction into a HashSet to ensure there are no duplicates
    let deduplicated_indices: HashSet<IndexOfAccount> = instruction_account_indices.collect();

    // Serialize entrypoint parameters with SBF ABI
    let (mut parameter_bytes, _regions, _account_lengths) = serialize_parameters(
        invoke_context.transaction_context,
        invoke_context
            .transaction_context
            .get_current_instruction_context()?,
        true,
    )?;

    // Deserialize data back into instruction params
    let (program_id, account_infos, _input) =
        unsafe { deserialize(&mut parameter_bytes.as_slice_mut()[0] as *mut u8) };

    // Execute the program
    process_instruction(program_id, &account_infos, instruction_data).map_err(|err| {
        let err = u64::from(err);
        stable_log::program_failure(&log_collector, program_id, &err.into());
        err
    })?;
    stable_log::program_success(&log_collector, program_id);

    // Lookup table for AccountInfo
    let account_info_map: HashMap<_, _> = account_infos.into_iter().map(|a| (a.key, a)).collect();

    // Re-fetch the instruction context. The previous reference may have been
    // invalidated due to the `set_invoke_context` in a CPI.
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;

    // Commit AccountInfo changes back into KeyedAccounts
    for i in deduplicated_indices.into_iter() {
        let mut borrowed_account =
            instruction_context.try_borrow_instruction_account(transaction_context, i)?;
        if borrowed_account.is_writable() {
            if let Some(account_info) = account_info_map.get(borrowed_account.get_key()) {
                if borrowed_account.get_lamports() != account_info.lamports() {
                    borrowed_account.set_lamports(account_info.lamports())?;
                }

                if borrowed_account
                    .can_data_be_resized(account_info.data_len())
                    .is_ok()
                    && borrowed_account.can_data_be_changed().is_ok()
                {
                    borrowed_account.set_data_from_slice(&account_info.data.borrow())?;
                }
                if borrowed_account.get_owner() != account_info.owner {
                    borrowed_account.set_owner(account_info.owner.as_ref())?;
                }
            }
        }
    }

    Ok(())
}

/// Converts a `solana-program`-style entrypoint into the runtime's entrypoint style, for
/// use with `ProgramTest::add_program`
#[macro_export]
macro_rules! processor {
    ($process_instruction:expr) => {
        Some(
            |first_instruction_account: $crate::IndexOfAccount,
             invoke_context: &mut solana_program_test::InvokeContext| {
                $crate::builtin_process_instruction(
                    $process_instruction,
                    first_instruction_account,
                    invoke_context,
                )
            },
        )
    };
}

fn get_sysvar<T: Default + Sysvar + Sized + serde::de::DeserializeOwned + Clone>(
    sysvar: Result<Arc<T>, InstructionError>,
    var_addr: *mut u8,
) -> u64 {
    let invoke_context = get_invoke_context();
    if invoke_context
        .consume_checked(invoke_context.get_compute_budget().sysvar_base_cost + T::size_of() as u64)
        .is_err()
    {
        panic!("Exceeded compute budget");
    }

    match sysvar {
        Ok(sysvar_data) => unsafe {
            *(var_addr as *mut _ as *mut T) = T::clone(&sysvar_data);
            SUCCESS
        },
        Err(_) => UNSUPPORTED_SYSVAR,
    }
}

struct SyscallStubs {}
impl solana_sdk::program_stubs::SyscallStubs for SyscallStubs {
    fn sol_log(&self, message: &str) {
        let invoke_context = get_invoke_context();
        ic_msg!(invoke_context, "Program log: {}", message);
    }

    fn sol_invoke_signed(
        &self,
        instruction: &Instruction,
        account_infos: &[AccountInfo],
        signers_seeds: &[&[&[u8]]],
    ) -> ProgramResult {
        let invoke_context = get_invoke_context();
        let log_collector = invoke_context.get_log_collector();
        let transaction_context = &invoke_context.transaction_context;
        let instruction_context = transaction_context
            .get_current_instruction_context()
            .unwrap();
        let caller = instruction_context
            .get_last_program_key(transaction_context)
            .unwrap();

        stable_log::program_invoke(
            &log_collector,
            &instruction.program_id,
            invoke_context.get_stack_height(),
        );

        let signers = signers_seeds
            .iter()
            .map(|seeds| Pubkey::create_program_address(seeds, caller).unwrap())
            .collect::<Vec<_>>();

        let (instruction_accounts, program_indices) = invoke_context
            .prepare_instruction(instruction, &signers)
            .unwrap();

        // Copy caller's account_info modifications into invoke_context accounts
        let transaction_context = &invoke_context.transaction_context;
        let instruction_context = transaction_context
            .get_current_instruction_context()
            .unwrap();
        let mut account_indices = Vec::with_capacity(instruction_accounts.len());
        for instruction_account in instruction_accounts.iter() {
            let account_key = transaction_context
                .get_key_of_account_at_index(instruction_account.index_in_transaction)
                .unwrap();
            let account_info_index = account_infos
                .iter()
                .position(|account_info| account_info.unsigned_key() == account_key)
                .ok_or(InstructionError::MissingAccount)
                .unwrap();
            let account_info = &account_infos[account_info_index];
            let mut borrowed_account = instruction_context
                .try_borrow_instruction_account(
                    transaction_context,
                    instruction_account.index_in_caller,
                )
                .unwrap();
            if borrowed_account.get_lamports() != account_info.lamports() {
                borrowed_account
                    .set_lamports(account_info.lamports())
                    .unwrap();
            }
            let account_info_data = account_info.try_borrow_data().unwrap();
            // The redundant check helps to avoid the expensive data comparison if we can
            match borrowed_account
                .can_data_be_resized(account_info_data.len())
                .and_then(|_| borrowed_account.can_data_be_changed())
            {
                Ok(()) => borrowed_account
                    .set_data_from_slice(&account_info_data)
                    .unwrap(),
                Err(err) if borrowed_account.get_data() != *account_info_data => {
                    panic!("{err:?}");
                }
                _ => {}
            }
            // Change the owner at the end so that we are allowed to change the lamports and data before
            if borrowed_account.get_owner() != account_info.owner {
                borrowed_account
                    .set_owner(account_info.owner.as_ref())
                    .unwrap();
            }
            if instruction_account.is_writable {
                account_indices.push((instruction_account.index_in_caller, account_info_index));
            }
        }

        let mut compute_units_consumed = 0;
        invoke_context
            .process_instruction(
                &instruction.data,
                &instruction_accounts,
                &program_indices,
                &mut compute_units_consumed,
                &mut ExecuteTimings::default(),
            )
            .map_err(|err| ProgramError::try_from(err).unwrap_or_else(|err| panic!("{}", err)))?;

        // Copy invoke_context accounts modifications into caller's account_info
        let transaction_context = &invoke_context.transaction_context;
        let instruction_context = transaction_context
            .get_current_instruction_context()
            .unwrap();
        for (index_in_caller, account_info_index) in account_indices.into_iter() {
            let borrowed_account = instruction_context
                .try_borrow_instruction_account(transaction_context, index_in_caller)
                .unwrap();
            let account_info = &account_infos[account_info_index];
            **account_info.try_borrow_mut_lamports().unwrap() = borrowed_account.get_lamports();
            if account_info.owner != borrowed_account.get_owner() {
                // TODO Figure out a better way to allow the System Program to set the account owner
                #[allow(clippy::transmute_ptr_to_ptr)]
                #[allow(mutable_transmutes)]
                let account_info_mut =
                    unsafe { transmute::<&Pubkey, &mut Pubkey>(account_info.owner) };
                *account_info_mut = *borrowed_account.get_owner();
            }

            let new_data = borrowed_account.get_data();
            let new_len = new_data.len();

            // Resize account_info data (grow-only)
            if account_info.data_len() < new_len {
                account_info.realloc(new_len, false)?;
            }

            // Clone the data
            let mut data = account_info.try_borrow_mut_data()?;
            data.clone_from_slice(new_data);
        }

        stable_log::program_success(&log_collector, &instruction.program_id);
        Ok(())
    }

    fn sol_get_clock_sysvar(&self, var_addr: *mut u8) -> u64 {
        get_sysvar(
            get_invoke_context().get_sysvar_cache().get_clock(),
            var_addr,
        )
    }

    fn sol_get_epoch_schedule_sysvar(&self, var_addr: *mut u8) -> u64 {
        get_sysvar(
            get_invoke_context().get_sysvar_cache().get_epoch_schedule(),
            var_addr,
        )
    }

    #[allow(deprecated)]
    fn sol_get_fees_sysvar(&self, var_addr: *mut u8) -> u64 {
        get_sysvar(get_invoke_context().get_sysvar_cache().get_fees(), var_addr)
    }

    fn sol_get_rent_sysvar(&self, var_addr: *mut u8) -> u64 {
        get_sysvar(get_invoke_context().get_sysvar_cache().get_rent(), var_addr)
    }

    fn sol_get_return_data(&self) -> Option<(Pubkey, Vec<u8>)> {
        let (program_id, data) = get_invoke_context().transaction_context.get_return_data();
        Some((*program_id, data.to_vec()))
    }

    fn sol_set_return_data(&self, data: &[u8]) {
        let invoke_context = get_invoke_context();
        let transaction_context = &mut invoke_context.transaction_context;
        let instruction_context = transaction_context
            .get_current_instruction_context()
            .unwrap();
        let caller = *instruction_context
            .get_last_program_key(transaction_context)
            .unwrap();
        transaction_context
            .set_return_data(caller, data.to_vec())
            .unwrap();
    }

    fn sol_get_stack_height(&self) -> u64 {
        let invoke_context = get_invoke_context();
        invoke_context.get_stack_height().try_into().unwrap()
    }
}

pub fn find_file(filename: &str) -> Option<PathBuf> {
    for dir in default_shared_object_dirs() {
        let candidate = dir.join(filename);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn default_shared_object_dirs() -> Vec<PathBuf> {
    let mut search_path = vec![];
    if let Ok(bpf_out_dir) = std::env::var("BPF_OUT_DIR") {
        search_path.push(PathBuf::from(bpf_out_dir));
    } else if let Ok(bpf_out_dir) = std::env::var("SBF_OUT_DIR") {
        search_path.push(PathBuf::from(bpf_out_dir));
    }
    search_path.push(PathBuf::from("tests/fixtures"));
    if let Ok(dir) = std::env::current_dir() {
        search_path.push(dir);
    }
    trace!("SBF .so search path: {:?}", search_path);
    search_path
}

pub fn read_file<P: AsRef<Path>>(path: P) -> Vec<u8> {
    let path = path.as_ref();
    let mut file = File::open(path)
        .unwrap_or_else(|err| panic!("Failed to open \"{}\": {}", path.display(), err));

    let mut file_data = Vec::new();
    file.read_to_end(&mut file_data)
        .unwrap_or_else(|err| panic!("Failed to read \"{}\": {}", path.display(), err));
    file_data
}

pub struct ProgramTest {
    accounts: Vec<(Pubkey, AccountSharedData)>,
    builtins: Vec<Builtin>,
    compute_max_units: Option<u64>,
    prefer_bpf: bool,
    use_bpf_jit: bool,
    deactivate_feature_set: HashSet<Pubkey>,
    transaction_account_lock_limit: Option<usize>,
}

impl Default for ProgramTest {
    /// Initialize a new ProgramTest
    ///
    /// If the `BPF_OUT_DIR` environment variable is defined, BPF programs will be preferred over
    /// over a native instruction processor.  The `ProgramTest::prefer_bpf()` method may be
    /// used to override this preference at runtime.  `cargo test-bpf` will set `BPF_OUT_DIR`
    /// automatically.
    ///
    /// SBF program shared objects and account data files are searched for in
    /// * the value of the `BPF_OUT_DIR` environment variable
    /// * the `tests/fixtures` sub-directory
    /// * the current working directory
    ///
    fn default() -> Self {
        solana_logger::setup_with_default(
            "solana_rbpf::vm=debug,\
             solana_runtime::message_processor=debug,\
             solana_runtime::system_instruction_processor=trace,\
             solana_program_test=info",
        );
        let prefer_bpf =
            std::env::var("BPF_OUT_DIR").is_ok() || std::env::var("SBF_OUT_DIR").is_ok();

        Self {
            accounts: vec![],
            builtins: vec![],
            compute_max_units: None,
            prefer_bpf,
            use_bpf_jit: false,
            deactivate_feature_set: HashSet::default(),
            transaction_account_lock_limit: None,
        }
    }
}

impl ProgramTest {
    /// Create a `ProgramTest`.
    ///
    /// This is a wrapper around [`default`] and [`add_program`]. See their documentation for more
    /// details.
    ///
    /// [`default`]: #method.default
    /// [`add_program`]: #method.add_program
    pub fn new(
        program_name: &str,
        program_id: Pubkey,
        process_instruction: Option<ProcessInstructionWithContext>,
    ) -> Self {
        let mut me = Self::default();
        me.add_program(program_name, program_id, process_instruction);
        me
    }

    /// Override default SBF program selection
    pub fn prefer_bpf(&mut self, prefer_bpf: bool) {
        self.prefer_bpf = prefer_bpf;
    }

    /// Override the default maximum compute units
    pub fn set_compute_max_units(&mut self, compute_max_units: u64) {
        self.compute_max_units = Some(compute_max_units);
    }

    /// Override the default transaction account lock limit
    pub fn set_transaction_account_lock_limit(&mut self, transaction_account_lock_limit: usize) {
        self.transaction_account_lock_limit = Some(transaction_account_lock_limit);
    }

    /// Override the SBF compute budget
    #[allow(deprecated)]
    #[deprecated(since = "1.8.0", note = "please use `set_compute_max_units` instead")]
    pub fn set_bpf_compute_max_units(&mut self, bpf_compute_max_units: u64) {
        self.compute_max_units = Some(bpf_compute_max_units);
    }

    /// Execute the SBF program with JIT if true, interpreted if false
    pub fn use_bpf_jit(&mut self, use_bpf_jit: bool) {
        self.use_bpf_jit = use_bpf_jit;
    }

    /// Add an account to the test environment
    pub fn add_account(&mut self, address: Pubkey, account: Account) {
        self.accounts
            .push((address, AccountSharedData::from(account)));
    }

    /// Add an account to the test environment with the account data in the provided `filename`
    pub fn add_account_with_file_data(
        &mut self,
        address: Pubkey,
        lamports: u64,
        owner: Pubkey,
        filename: &str,
    ) {
        self.add_account(
            address,
            Account {
                lamports,
                data: read_file(find_file(filename).unwrap_or_else(|| {
                    panic!("Unable to locate {filename}");
                })),
                owner,
                executable: false,
                rent_epoch: 0,
            },
        );
    }

    /// Add an account to the test environment with the account data in the provided as a base 64
    /// string
    pub fn add_account_with_base64_data(
        &mut self,
        address: Pubkey,
        lamports: u64,
        owner: Pubkey,
        data_base64: &str,
    ) {
        self.add_account(
            address,
            Account {
                lamports,
                data: base64::decode(data_base64)
                    .unwrap_or_else(|err| panic!("Failed to base64 decode: {err}")),
                owner,
                executable: false,
                rent_epoch: 0,
            },
        );
    }

    /// Add a SBF program to the test environment.
    ///
    /// `program_name` will also be used to locate the SBF shared object in the current or fixtures
    /// directory.
    ///
    /// If `process_instruction` is provided, the natively built-program may be used instead of the
    /// SBF shared object depending on the `BPF_OUT_DIR` environment variable.
    pub fn add_program(
        &mut self,
        program_name: &str,
        program_id: Pubkey,
        process_instruction: Option<ProcessInstructionWithContext>,
    ) {
        let add_bpf = |this: &mut ProgramTest, program_file: PathBuf| {
            let data = read_file(&program_file);
            info!(
                "\"{}\" SBF program from {}{}",
                program_name,
                program_file.display(),
                std::fs::metadata(&program_file)
                    .map(|metadata| {
                        metadata
                            .modified()
                            .map(|time| {
                                format!(
                                    ", modified {}",
                                    HumanTime::from(time)
                                        .to_text_en(Accuracy::Precise, Tense::Past)
                                )
                            })
                            .ok()
                    })
                    .ok()
                    .flatten()
                    .unwrap_or_default()
            );

            this.add_account(
                program_id,
                Account {
                    lamports: Rent::default().minimum_balance(data.len()).min(1),
                    data,
                    owner: solana_sdk::bpf_loader::id(),
                    executable: true,
                    rent_epoch: 0,
                },
            );
        };

        let add_native = |this: &mut ProgramTest, process_fn: ProcessInstructionWithContext| {
            info!("\"{}\" program loaded as native code", program_name);
            this.builtins
                .push(Builtin::new(program_name, program_id, process_fn));
        };

        let warn_invalid_program_name = || {
            let valid_program_names = default_shared_object_dirs()
                .iter()
                .filter_map(|dir| dir.read_dir().ok())
                .flat_map(|read_dir| {
                    read_dir.filter_map(|entry| {
                        let path = entry.ok()?.path();
                        if !path.is_file() {
                            return None;
                        }
                        match path.extension()?.to_str()? {
                            "so" => Some(path.file_stem()?.to_os_string()),
                            _ => None,
                        }
                    })
                })
                .collect::<Vec<_>>();

            if valid_program_names.is_empty() {
                // This should be unreachable as `test-bpf` should guarantee at least one shared
                // object exists somewhere.
                warn!("No SBF shared objects found.");
                return;
            }

            warn!(
                "Possible bogus program name. Ensure the program name ({}) \
                matches one of the following recognizable program names:",
                program_name,
            );
            for name in valid_program_names {
                warn!(" - {}", name.to_str().unwrap());
            }
        };

        let program_file = find_file(&format!("{program_name}.so"));
        match (self.prefer_bpf, program_file, process_instruction) {
            // If SBF is preferred (i.e., `test-sbf` is invoked) and a BPF shared object exists,
            // use that as the program data.
            (true, Some(file), _) => add_bpf(self, file),

            // If SBF is not required (i.e., we were invoked with `test`), use the provided
            // processor function as is.
            //
            // TODO: figure out why tests hang if a processor panics when running native code.
            (false, _, Some(process)) => add_native(self, process),

            // Invalid: `test-sbf` invocation with no matching SBF shared object.
            (true, None, _) => {
                warn_invalid_program_name();
                panic!("Program file data not available for {program_name} ({program_id})");
            }

            // Invalid: regular `test` invocation without a processor.
            (false, _, None) => {
                panic!("Program processor not available for {program_name} ({program_id})");
            }
        }
    }

    /// Add a builtin program to the test environment.
    ///
    /// Note that builtin programs are responsible for their own `stable_log` output.
    pub fn add_builtin_program(
        &mut self,
        program_name: &str,
        program_id: Pubkey,
        process_instruction: ProcessInstructionWithContext,
    ) {
        info!("\"{}\" builtin program", program_name);
        self.builtins
            .push(Builtin::new(program_name, program_id, process_instruction));
    }

    /// Deactivate a runtime feature.
    ///
    /// Note that all features are activated by default.
    pub fn deactivate_feature(&mut self, feature_id: Pubkey) {
        self.deactivate_feature_set.insert(feature_id);
    }

    fn setup_bank(
        &self,
    ) -> (
        Arc<RwLock<BankForks>>,
        Arc<RwLock<BlockCommitmentCache>>,
        Hash,
        GenesisConfigInfo,
    ) {
        {
            use std::sync::Once;
            static ONCE: Once = Once::new();

            ONCE.call_once(|| {
                solana_sdk::program_stubs::set_syscall_stubs(Box::new(SyscallStubs {}));
            });
        }

        let rent = Rent::default();
        let fee_rate_governor = FeeRateGovernor {
            // Initialize with a non-zero fee
            lamports_per_signature: DEFAULT_TARGET_LAMPORTS_PER_SIGNATURE / 2,
            ..FeeRateGovernor::default()
        };
        let bootstrap_validator_pubkey = Pubkey::new_unique();
        let bootstrap_validator_stake_lamports =
            rent.minimum_balance(VoteState::size_of()) + sol_to_lamports(1_000_000.0);

        let mint_keypair = Keypair::new();
        let voting_keypair = Keypair::new();

        let mut genesis_config = create_genesis_config_with_leader_ex(
            sol_to_lamports(1_000_000.0),
            &mint_keypair.pubkey(),
            &bootstrap_validator_pubkey,
            &voting_keypair.pubkey(),
            &Pubkey::new_unique(),
            bootstrap_validator_stake_lamports,
            42,
            fee_rate_governor,
            rent,
            ClusterType::Development,
            vec![],
        );

        // Remove features tagged to deactivate
        for deactivate_feature_pk in &self.deactivate_feature_set {
            if FEATURE_NAMES.contains_key(deactivate_feature_pk) {
                match genesis_config.accounts.remove(deactivate_feature_pk) {
                    Some(_) => debug!("Feature for {:?} deactivated", deactivate_feature_pk),
                    None => warn!(
                        "Feature {:?} set for deactivation not found in genesis_config account list, ignored.",
                        deactivate_feature_pk
                    ),
                }
            } else {
                warn!(
                    "Feature {:?} set for deactivation is not a known Feature public key",
                    deactivate_feature_pk
                );
            }
        }

        let target_tick_duration = Duration::from_micros(100);
        genesis_config.poh_config = PohConfig::new_sleep(target_tick_duration);
        debug!("Payer address: {}", mint_keypair.pubkey());
        debug!("Genesis config: {}", genesis_config);

        let mut bank = Bank::new_with_runtime_config_for_tests(
            &genesis_config,
            Arc::new(RuntimeConfig {
                bpf_jit: self.use_bpf_jit,
                compute_budget: self.compute_max_units.map(|max_units| ComputeBudget {
                    compute_unit_limit: max_units,
                    ..ComputeBudget::default()
                }),
                transaction_account_lock_limit: self.transaction_account_lock_limit,
                ..RuntimeConfig::default()
            }),
        );

        // Add loaders
        macro_rules! add_builtin {
            ($b:expr) => {
                bank.add_builtin(&$b.0, &$b.1, $b.2)
            };
        }
        add_builtin!(solana_bpf_loader_deprecated_program!());
        if self.use_bpf_jit {
            add_builtin!(solana_bpf_loader_program_with_jit!());
            add_builtin!(solana_bpf_loader_upgradeable_program_with_jit!());
        } else {
            add_builtin!(solana_bpf_loader_program!());
            add_builtin!(solana_bpf_loader_upgradeable_program!());
        }

        // Add commonly-used SPL programs as a convenience to the user
        for (program_id, account) in programs::spl_programs(&Rent::default()).iter() {
            bank.store_account(program_id, account);
        }

        // User-supplied additional builtins
        for builtin in self.builtins.iter() {
            bank.add_builtin(
                &builtin.name,
                &builtin.id,
                builtin.process_instruction_with_context,
            );
        }

        for (address, account) in self.accounts.iter() {
            if bank.get_account(address).is_some() {
                info!("Overriding account at {}", address);
            }
            bank.store_account(address, account);
        }
        bank.set_capitalization();
        // Advance beyond slot 0 for a slightly more realistic test environment
        let bank = {
            let bank = Arc::new(bank);
            bank.fill_bank_with_ticks_for_tests();
            let bank = Bank::new_from_parent(&bank, bank.collector_id(), bank.slot() + 1);
            debug!("Bank slot: {}", bank.slot());
            bank
        };
        let slot = bank.slot();
        let last_blockhash = bank.last_blockhash();
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let block_commitment_cache = Arc::new(RwLock::new(
            BlockCommitmentCache::new_for_tests_with_slots(slot, slot),
        ));

        (
            bank_forks,
            block_commitment_cache,
            last_blockhash,
            GenesisConfigInfo {
                genesis_config,
                mint_keypair,
                voting_keypair,
                validator_pubkey: bootstrap_validator_pubkey,
            },
        )
    }

    pub async fn start(self) -> (BanksClient, Keypair, Hash) {
        let (bank_forks, block_commitment_cache, last_blockhash, gci) = self.setup_bank();
        let target_tick_duration = gci.genesis_config.poh_config.target_tick_duration;
        let target_slot_duration = target_tick_duration * gci.genesis_config.ticks_per_slot as u32;
        let transport = start_local_server(
            bank_forks.clone(),
            block_commitment_cache.clone(),
            target_tick_duration,
        )
        .await;
        let banks_client = start_client(transport)
            .await
            .unwrap_or_else(|err| panic!("Failed to start banks client: {err}"));

        // Run a simulated PohService to provide the client with new blockhashes.  New blockhashes
        // are required when sending multiple otherwise identical transactions in series from a
        // test
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(target_slot_duration).await;
                bank_forks
                    .read()
                    .unwrap()
                    .working_bank()
                    .register_recent_blockhash(&Hash::new_unique());
            }
        });

        (banks_client, gci.mint_keypair, last_blockhash)
    }

    /// Start the test client
    ///
    /// Returns a `BanksClient` interface into the test environment as well as a payer `Keypair`
    /// with SOL for sending transactions
    pub async fn start_with_context(self) -> ProgramTestContext {
        let (bank_forks, block_commitment_cache, last_blockhash, gci) = self.setup_bank();
        let target_tick_duration = gci.genesis_config.poh_config.target_tick_duration;
        let transport = start_local_server(
            bank_forks.clone(),
            block_commitment_cache.clone(),
            target_tick_duration,
        )
        .await;
        let banks_client = start_client(transport)
            .await
            .unwrap_or_else(|err| panic!("Failed to start banks client: {err}"));

        ProgramTestContext::new(
            bank_forks,
            block_commitment_cache,
            banks_client,
            last_blockhash,
            gci,
        )
    }
}

#[async_trait]
pub trait ProgramTestBanksClientExt {
    /// Get a new blockhash, similar in spirit to RpcClient::get_new_blockhash()
    ///
    /// This probably should eventually be moved into BanksClient proper in some form
    #[deprecated(
        since = "1.9.0",
        note = "Please use `get_new_latest_blockhash `instead"
    )]
    async fn get_new_blockhash(&mut self, blockhash: &Hash) -> io::Result<(Hash, FeeCalculator)>;
    /// Get a new latest blockhash, similar in spirit to RpcClient::get_latest_blockhash()
    async fn get_new_latest_blockhash(&mut self, blockhash: &Hash) -> io::Result<Hash>;
}

#[async_trait]
impl ProgramTestBanksClientExt for BanksClient {
    async fn get_new_blockhash(&mut self, blockhash: &Hash) -> io::Result<(Hash, FeeCalculator)> {
        let mut num_retries = 0;
        let start = Instant::now();
        while start.elapsed().as_secs() < 5 {
            #[allow(deprecated)]
            if let Ok((fee_calculator, new_blockhash, _slot)) = self.get_fees().await {
                if new_blockhash != *blockhash {
                    return Ok((new_blockhash, fee_calculator));
                }
            }
            debug!("Got same blockhash ({:?}), will retry...", blockhash);

            tokio::time::sleep(Duration::from_millis(200)).await;
            num_retries += 1;
        }

        Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "Unable to get new blockhash after {}ms (retried {} times), stuck at {}",
                start.elapsed().as_millis(),
                num_retries,
                blockhash
            ),
        ))
    }

    async fn get_new_latest_blockhash(&mut self, blockhash: &Hash) -> io::Result<Hash> {
        let mut num_retries = 0;
        let start = Instant::now();
        while start.elapsed().as_secs() < 5 {
            let new_blockhash = self.get_latest_blockhash().await?;
            if new_blockhash != *blockhash {
                return Ok(new_blockhash);
            }
            debug!("Got same blockhash ({:?}), will retry...", blockhash);

            tokio::time::sleep(Duration::from_millis(200)).await;
            num_retries += 1;
        }

        Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "Unable to get new blockhash after {}ms (retried {} times), stuck at {}",
                start.elapsed().as_millis(),
                num_retries,
                blockhash
            ),
        ))
    }
}

struct DroppableTask<T>(Arc<AtomicBool>, JoinHandle<T>);

impl<T> Drop for DroppableTask<T> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

pub struct ProgramTestContext {
    pub banks_client: BanksClient,
    pub last_blockhash: Hash,
    pub payer: Keypair,
    genesis_config: GenesisConfig,
    bank_forks: Arc<RwLock<BankForks>>,
    block_commitment_cache: Arc<RwLock<BlockCommitmentCache>>,
    _bank_task: DroppableTask<()>,
}

impl ProgramTestContext {
    fn new(
        bank_forks: Arc<RwLock<BankForks>>,
        block_commitment_cache: Arc<RwLock<BlockCommitmentCache>>,
        banks_client: BanksClient,
        last_blockhash: Hash,
        genesis_config_info: GenesisConfigInfo,
    ) -> Self {
        // Run a simulated PohService to provide the client with new blockhashes.  New blockhashes
        // are required when sending multiple otherwise identical transactions in series from a
        // test
        let running_bank_forks = bank_forks.clone();
        let target_tick_duration = genesis_config_info
            .genesis_config
            .poh_config
            .target_tick_duration;
        let target_slot_duration =
            target_tick_duration * genesis_config_info.genesis_config.ticks_per_slot as u32;
        let exit = Arc::new(AtomicBool::new(false));
        let bank_task = DroppableTask(
            exit.clone(),
            tokio::spawn(async move {
                loop {
                    if exit.load(Ordering::Relaxed) {
                        break;
                    }
                    tokio::time::sleep(target_slot_duration).await;
                    running_bank_forks
                        .read()
                        .unwrap()
                        .working_bank()
                        .register_recent_blockhash(&Hash::new_unique());
                }
            }),
        );

        Self {
            banks_client,
            last_blockhash,
            payer: genesis_config_info.mint_keypair,
            genesis_config: genesis_config_info.genesis_config,
            bank_forks,
            block_commitment_cache,
            _bank_task: bank_task,
        }
    }

    pub fn genesis_config(&self) -> &GenesisConfig {
        &self.genesis_config
    }

    /// Manually increment vote credits for the current epoch in the specified vote account to simulate validator voting activity
    pub fn increment_vote_account_credits(
        &mut self,
        vote_account_address: &Pubkey,
        number_of_credits: u64,
    ) {
        let bank_forks = self.bank_forks.read().unwrap();
        let bank = bank_forks.working_bank();

        // generate some vote activity for rewards
        let mut vote_account = bank.get_account(vote_account_address).unwrap();
        let mut vote_state = vote_state::from(&vote_account).unwrap();

        let epoch = bank.epoch();
        for _ in 0..number_of_credits {
            vote_state.increment_credits(epoch, 1);
        }
        let versioned = VoteStateVersions::new_current(vote_state);
        vote_state::to(&versioned, &mut vote_account).unwrap();
        bank.store_account(vote_account_address, &vote_account);
    }

    /// Create or overwrite an account, subverting normal runtime checks.
    ///
    /// This method exists to make it easier to set up artificial situations
    /// that would be difficult to replicate by sending individual transactions.
    /// Beware that it can be used to create states that would not be reachable
    /// by sending transactions!
    pub fn set_account(&mut self, address: &Pubkey, account: &AccountSharedData) {
        let bank_forks = self.bank_forks.read().unwrap();
        let bank = bank_forks.working_bank();
        bank.store_account(address, account);
    }

    /// Create or overwrite a sysvar, subverting normal runtime checks.
    ///
    /// This method exists to make it easier to set up artificial situations
    /// that would be difficult to replicate on a new test cluster. Beware
    /// that it can be used to create states that would not be reachable
    /// under normal conditions!
    pub fn set_sysvar<T: SysvarId + Sysvar>(&self, sysvar: &T) {
        let bank_forks = self.bank_forks.read().unwrap();
        let bank = bank_forks.working_bank();
        bank.set_sysvar_for_tests(sysvar);
    }

    /// Force the working bank ahead to a new slot
    pub fn warp_to_slot(&mut self, warp_slot: Slot) -> Result<(), ProgramTestError> {
        let mut bank_forks = self.bank_forks.write().unwrap();
        let bank = bank_forks.working_bank();

        // Fill ticks until a new blockhash is recorded, otherwise retried transactions will have
        // the same signature
        bank.fill_bank_with_ticks_for_tests();

        // Ensure that we are actually progressing forward
        let working_slot = bank.slot();
        if warp_slot <= working_slot {
            return Err(ProgramTestError::InvalidWarpSlot);
        }

        // Warp ahead to one slot *before* the desired slot because the bank
        // from Bank::warp_from_parent() is frozen. If the desired slot is one
        // slot *after* the working_slot, no need to warp at all.
        let pre_warp_slot = warp_slot - 1;
        let warp_bank = if pre_warp_slot == working_slot {
            bank.freeze();
            bank
        } else {
            bank_forks.insert(Bank::warp_from_parent(
                &bank,
                &Pubkey::default(),
                pre_warp_slot,
                // some warping tests cannot use the append vecs because of the sequence of adding roots and flushing
                solana_runtime::accounts_db::CalcAccountsHashDataSource::IndexForTests,
            ))
        };

        let (snapshot_request_sender, snapshot_request_receiver) = crossbeam_channel::unbounded();
        let abs_request_sender = AbsRequestSender::new(snapshot_request_sender);

        bank_forks.set_root(pre_warp_slot, &abs_request_sender, Some(pre_warp_slot));

        // The call to `set_root()` above will send an EAH request.  Need to intercept and handle
        // all EpochAccountsHash requests so future rooted banks do not hang in Bank::freeze()
        // waiting for an in-flight EAH calculation to complete.
        snapshot_request_receiver
            .try_iter()
            .filter(|snapshot_request| {
                snapshot_request.request_type == SnapshotRequestType::EpochAccountsHash
            })
            .for_each(|snapshot_request| {
                snapshot_request
                    .snapshot_root_bank
                    .rc
                    .accounts
                    .accounts_db
                    .epoch_accounts_hash_manager
                    .set_valid(
                        EpochAccountsHash::new(Hash::new_unique()),
                        snapshot_request.snapshot_root_bank.slot(),
                    )
            });

        // warp_bank is frozen so go forward to get unfrozen bank at warp_slot
        bank_forks.insert(Bank::new_from_parent(
            &warp_bank,
            &Pubkey::default(),
            warp_slot,
        ));

        // Update block commitment cache, otherwise banks server will poll at
        // the wrong slot
        let mut w_block_commitment_cache = self.block_commitment_cache.write().unwrap();
        // HACK: The root set here should be `pre_warp_slot`, but since we're
        // in a testing environment, the root bank never updates after a warp.
        // The ticking thread only updates the working bank, and never the root
        // bank.
        w_block_commitment_cache.set_all_slots(warp_slot, warp_slot);

        let bank = bank_forks.working_bank();
        self.last_blockhash = bank.last_blockhash();
        Ok(())
    }

    /// Get a new latest blockhash, similar in spirit to RpcClient::get_latest_blockhash()
    pub async fn get_new_latest_blockhash(&mut self) -> io::Result<Hash> {
        let blockhash = self
            .banks_client
            .get_new_latest_blockhash(&self.last_blockhash)
            .await?;
        self.last_blockhash = blockhash;
        Ok(blockhash)
    }
}

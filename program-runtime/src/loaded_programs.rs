use {
    crate::invoke_context::{BuiltinFunctionWithContext, InvokeContext},
    log::{debug, error, log_enabled, trace},
    percentage::PercentageInteger,
    solana_measure::measure::Measure,
    solana_rbpf::{
        elf::Executable,
        program::{BuiltinProgram, FunctionRegistry},
        verifier::RequisiteVerifier,
        vm::Config,
    },
    solana_sdk::{
        bpf_loader, bpf_loader_deprecated, bpf_loader_upgradeable,
        clock::{Epoch, Slot},
        loader_v4, native_loader,
        pubkey::Pubkey,
        saturating_add_assign,
    },
    solana_timings::ExecuteDetailsTimings,
    solana_type_overrides::{
        rand::{thread_rng, Rng},
        sync::{
            atomic::{AtomicU64, Ordering},
            Arc, Condvar, Mutex, RwLock,
        },
        thread,
    },
    std::{
        collections::{hash_map::Entry, HashMap},
        fmt::{Debug, Formatter},
        sync::Weak,
    },
};

pub type ProgramRuntimeEnvironment = Arc<BuiltinProgram<InvokeContext<'static>>>;
pub const MAX_LOADED_ENTRY_COUNT: usize = 256;
pub const DELAY_VISIBILITY_SLOT_OFFSET: Slot = 1;

/// Relationship between two fork IDs
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum BlockRelation {
    /// The slot is on the same fork and is an ancestor of the other slot
    Ancestor,
    /// The two slots are equal and are on the same fork
    Equal,
    /// The slot is on the same fork and is a descendant of the other slot
    Descendant,
    /// The slots are on two different forks and may have had a common ancestor at some point
    Unrelated,
    /// Either one or both of the slots are either older than the latest root, or are in future
    Unknown,
}

/// Maps relationship between two slots.
pub trait ForkGraph {
    /// Returns the BlockRelation of A to B
    fn relationship(&self, a: Slot, b: Slot) -> BlockRelation;
}

/// The owner of a programs accounts, thus the loader of a program
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProgramCacheEntryOwner {
    #[default]
    NativeLoader,
    LoaderV1,
    LoaderV2,
    LoaderV3,
    LoaderV4,
}

impl TryFrom<&Pubkey> for ProgramCacheEntryOwner {
    type Error = ();
    fn try_from(loader_key: &Pubkey) -> Result<Self, ()> {
        if native_loader::check_id(loader_key) {
            Ok(ProgramCacheEntryOwner::NativeLoader)
        } else if bpf_loader_deprecated::check_id(loader_key) {
            Ok(ProgramCacheEntryOwner::LoaderV1)
        } else if bpf_loader::check_id(loader_key) {
            Ok(ProgramCacheEntryOwner::LoaderV2)
        } else if bpf_loader_upgradeable::check_id(loader_key) {
            Ok(ProgramCacheEntryOwner::LoaderV3)
        } else if loader_v4::check_id(loader_key) {
            Ok(ProgramCacheEntryOwner::LoaderV4)
        } else {
            Err(())
        }
    }
}

impl From<ProgramCacheEntryOwner> for Pubkey {
    fn from(program_cache_entry_owner: ProgramCacheEntryOwner) -> Self {
        match program_cache_entry_owner {
            ProgramCacheEntryOwner::NativeLoader => native_loader::id(),
            ProgramCacheEntryOwner::LoaderV1 => bpf_loader_deprecated::id(),
            ProgramCacheEntryOwner::LoaderV2 => bpf_loader::id(),
            ProgramCacheEntryOwner::LoaderV3 => bpf_loader_upgradeable::id(),
            ProgramCacheEntryOwner::LoaderV4 => loader_v4::id(),
        }
    }
}

/*
    The possible ProgramCacheEntryType transitions:

    DelayVisibility is special in that it is never stored in the cache.
    It is only returned by ProgramCacheForTxBatch::find() when a Loaded entry
    is encountered which is not effective yet.

    Builtin re/deployment:
    - Empty => Builtin in TransactionBatchProcessor::add_builtin
    - Builtin => Builtin in TransactionBatchProcessor::add_builtin

    Un/re/deployment (with delay and cooldown):
    - Empty / Closed => Loaded in UpgradeableLoaderInstruction::DeployWithMaxDataLen
    - Loaded / FailedVerification => Loaded in UpgradeableLoaderInstruction::Upgrade
    - Loaded / FailedVerification => Closed in UpgradeableLoaderInstruction::Close

    Eviction and unloading (in the same slot):
    - Unloaded => Loaded in ProgramCache::assign_program
    - Loaded => Unloaded in ProgramCache::unload_program_entry

    At epoch boundary (when feature set and environment changes):
    - Loaded => FailedVerification in Bank::_new_from_parent
    - FailedVerification => Loaded in Bank::_new_from_parent

    Through pruning (when on orphan fork or overshadowed on the rooted fork):
    - Closed / Unloaded / Loaded / Builtin => Empty in ProgramCache::prune
*/

/// Actual payload of [ProgramCacheEntry].
#[derive(Default)]
pub enum ProgramCacheEntryType {
    /// Tombstone for programs which currently do not pass the verifier but could if the feature set changed.
    FailedVerification(ProgramRuntimeEnvironment),
    /// Tombstone for programs that were either explicitly closed or never deployed.
    ///
    /// It's also used for accounts belonging to program loaders, that don't actually contain program code (e.g. buffer accounts for LoaderV3 programs).
    #[default]
    Closed,
    /// Tombstone for programs which have recently been modified but the new version is not visible yet.
    DelayVisibility,
    /// Successfully verified but not currently compiled.
    ///
    /// It continues to track usage statistics even when the compiled executable of the program is evicted from memory.
    Unloaded(ProgramRuntimeEnvironment),
    /// Verified and compiled program
    Loaded(Executable<InvokeContext<'static>>),
    /// A built-in program which is not stored on-chain but backed into and distributed with the validator
    Builtin(BuiltinProgram<InvokeContext<'static>>),
}

impl Debug for ProgramCacheEntryType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ProgramCacheEntryType::FailedVerification(_) => {
                write!(f, "ProgramCacheEntryType::FailedVerification")
            }
            ProgramCacheEntryType::Closed => write!(f, "ProgramCacheEntryType::Closed"),
            ProgramCacheEntryType::DelayVisibility => {
                write!(f, "ProgramCacheEntryType::DelayVisibility")
            }
            ProgramCacheEntryType::Unloaded(_) => write!(f, "ProgramCacheEntryType::Unloaded"),
            ProgramCacheEntryType::Loaded(_) => write!(f, "ProgramCacheEntryType::Loaded"),
            ProgramCacheEntryType::Builtin(_) => write!(f, "ProgramCacheEntryType::Builtin"),
        }
    }
}

impl ProgramCacheEntryType {
    /// Returns a reference to its environment if it has one
    pub fn get_environment(&self) -> Option<&ProgramRuntimeEnvironment> {
        match self {
            ProgramCacheEntryType::Loaded(program) => Some(program.get_loader()),
            ProgramCacheEntryType::FailedVerification(env)
            | ProgramCacheEntryType::Unloaded(env) => Some(env),
            _ => None,
        }
    }
}

/// Holds a program version at a specific address and on a specific slot / fork.
///
/// It contains the actual program in [ProgramCacheEntryType] and a bunch of meta-data.
#[derive(Debug, Default)]
pub struct ProgramCacheEntry {
    /// The program of this entry
    pub program: ProgramCacheEntryType,
    /// The loader of this entry
    pub account_owner: ProgramCacheEntryOwner,
    /// Size of account that stores the program and program data
    pub account_size: usize,
    /// Slot in which the program was (re)deployed
    pub deployment_slot: Slot,
    /// Slot in which this entry will become active (can be in the future)
    pub effective_slot: Slot,
    /// How often this entry was used by a transaction
    pub tx_usage_counter: AtomicU64,
    /// How often this entry was used by an instruction
    pub ix_usage_counter: AtomicU64,
    /// Latest slot in which the entry was used
    pub latest_access_slot: AtomicU64,
}

/// Global cache statistics for [ProgramCache].
#[derive(Debug, Default)]
pub struct ProgramCacheStats {
    /// a program was already in the cache
    pub hits: AtomicU64,
    /// a program was not found and loaded instead
    pub misses: AtomicU64,
    /// a compiled executable was unloaded
    pub evictions: HashMap<Pubkey, u64>,
    /// an unloaded program was loaded again (opposite of eviction)
    pub reloads: AtomicU64,
    /// a program was loaded or un/re/deployed
    pub insertions: AtomicU64,
    /// a program was loaded but can not be extracted on its own fork anymore
    pub lost_insertions: AtomicU64,
    /// a program which was already in the cache was reloaded by mistake
    pub replacements: AtomicU64,
    /// a program was only used once before being unloaded
    pub one_hit_wonders: AtomicU64,
    /// a program became unreachable in the fork graph because of rerooting
    pub prunes_orphan: AtomicU64,
    /// a program got pruned because it was not recompiled for the next epoch
    pub prunes_environment: AtomicU64,
    /// a program had no entries because all slot versions got pruned
    pub empty_entries: AtomicU64,
    /// water level of loaded entries currently cached
    pub water_level: AtomicU64,
}

impl ProgramCacheStats {
    pub fn reset(&mut self) {
        *self = ProgramCacheStats::default();
    }
    pub fn log(&self) {
        let hits = self.hits.load(Ordering::Relaxed);
        let misses = self.misses.load(Ordering::Relaxed);
        let evictions: u64 = self.evictions.values().sum();
        let reloads = self.reloads.load(Ordering::Relaxed);
        let insertions = self.insertions.load(Ordering::Relaxed);
        let lost_insertions = self.lost_insertions.load(Ordering::Relaxed);
        let replacements = self.replacements.load(Ordering::Relaxed);
        let one_hit_wonders = self.one_hit_wonders.load(Ordering::Relaxed);
        let prunes_orphan = self.prunes_orphan.load(Ordering::Relaxed);
        let prunes_environment = self.prunes_environment.load(Ordering::Relaxed);
        let empty_entries = self.empty_entries.load(Ordering::Relaxed);
        let water_level = self.water_level.load(Ordering::Relaxed);
        debug!(
            "Loaded Programs Cache Stats -- Hits: {}, Misses: {}, Evictions: {}, Reloads: {}, Insertions: {}, Lost-Insertions: {}, Replacements: {}, One-Hit-Wonders: {}, Prunes-Orphan: {}, Prunes-Environment: {}, Empty: {}, Water-Level: {}",
            hits, misses, evictions, reloads, insertions, lost_insertions, replacements, one_hit_wonders, prunes_orphan, prunes_environment, empty_entries, water_level
        );
        if log_enabled!(log::Level::Trace) && !self.evictions.is_empty() {
            let mut evictions = self.evictions.iter().collect::<Vec<_>>();
            evictions.sort_by_key(|e| e.1);
            let evictions = evictions
                .into_iter()
                .rev()
                .map(|(program_id, evictions)| {
                    format!("  {:<44}  {}", program_id.to_string(), evictions)
                })
                .collect::<Vec<_>>();
            let evictions = evictions.join("\n");
            trace!(
                "Eviction Details:\n  {:<44}  {}\n{}",
                "Program",
                "Count",
                evictions
            );
        }
    }
}

/// Time measurements for loading a single [ProgramCacheEntry].
#[derive(Debug, Default)]
pub struct LoadProgramMetrics {
    /// Program address, but as text
    pub program_id: String,
    /// Microseconds it took to `create_program_runtime_environment`
    pub register_syscalls_us: u64,
    /// Microseconds it took to `Executable::<InvokeContext>::load`
    pub load_elf_us: u64,
    /// Microseconds it took to `executable.verify::<RequisiteVerifier>`
    pub verify_code_us: u64,
    /// Microseconds it took to `executable.jit_compile`
    pub jit_compile_us: u64,
}

impl LoadProgramMetrics {
    pub fn submit_datapoint(&self, timings: &mut ExecuteDetailsTimings) {
        saturating_add_assign!(
            timings.create_executor_register_syscalls_us,
            self.register_syscalls_us
        );
        saturating_add_assign!(timings.create_executor_load_elf_us, self.load_elf_us);
        saturating_add_assign!(timings.create_executor_verify_code_us, self.verify_code_us);
        saturating_add_assign!(timings.create_executor_jit_compile_us, self.jit_compile_us);
        datapoint_trace!(
            "create_executor_trace",
            ("program_id", self.program_id, String),
            ("register_syscalls_us", self.register_syscalls_us, i64),
            ("load_elf_us", self.load_elf_us, i64),
            ("verify_code_us", self.verify_code_us, i64),
            ("jit_compile_us", self.jit_compile_us, i64),
        );
    }
}

impl PartialEq for ProgramCacheEntry {
    fn eq(&self, other: &Self) -> bool {
        self.effective_slot == other.effective_slot
            && self.deployment_slot == other.deployment_slot
            && self.is_tombstone() == other.is_tombstone()
    }
}

impl ProgramCacheEntry {
    /// Creates a new user program
    pub fn new(
        loader_key: &Pubkey,
        program_runtime_environment: ProgramRuntimeEnvironment,
        deployment_slot: Slot,
        effective_slot: Slot,
        elf_bytes: &[u8],
        account_size: usize,
        metrics: &mut LoadProgramMetrics,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Self::new_internal(
            loader_key,
            program_runtime_environment,
            deployment_slot,
            effective_slot,
            elf_bytes,
            account_size,
            metrics,
            false, /* reloading */
        )
    }

    /// Reloads a user program, *without* running the verifier.
    ///
    /// # Safety
    ///
    /// This method is unsafe since it assumes that the program has already been verified. Should
    /// only be called when the program was previously verified and loaded in the cache, but was
    /// unloaded due to inactivity. It should also be checked that the `program_runtime_environment`
    /// hasn't changed since it was unloaded.
    pub unsafe fn reload(
        loader_key: &Pubkey,
        program_runtime_environment: Arc<BuiltinProgram<InvokeContext<'static>>>,
        deployment_slot: Slot,
        effective_slot: Slot,
        elf_bytes: &[u8],
        account_size: usize,
        metrics: &mut LoadProgramMetrics,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Self::new_internal(
            loader_key,
            program_runtime_environment,
            deployment_slot,
            effective_slot,
            elf_bytes,
            account_size,
            metrics,
            true, /* reloading */
        )
    }

    fn new_internal(
        loader_key: &Pubkey,
        program_runtime_environment: Arc<BuiltinProgram<InvokeContext<'static>>>,
        deployment_slot: Slot,
        effective_slot: Slot,
        elf_bytes: &[u8],
        account_size: usize,
        metrics: &mut LoadProgramMetrics,
        reloading: bool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let load_elf_time = Measure::start("load_elf_time");
        // The following unused_mut exception is needed for architectures that do not
        // support JIT compilation.
        #[allow(unused_mut)]
        let mut executable = Executable::load(elf_bytes, program_runtime_environment.clone())?;
        metrics.load_elf_us = load_elf_time.end_as_us();

        if !reloading {
            let verify_code_time = Measure::start("verify_code_time");
            executable.verify::<RequisiteVerifier>()?;
            metrics.verify_code_us = verify_code_time.end_as_us();
        }

        #[cfg(all(not(target_os = "windows"), target_arch = "x86_64"))]
        {
            let jit_compile_time = Measure::start("jit_compile_time");
            executable.jit_compile()?;
            metrics.jit_compile_us = jit_compile_time.end_as_us();
        }

        Ok(Self {
            deployment_slot,
            account_owner: ProgramCacheEntryOwner::try_from(loader_key).unwrap(),
            account_size,
            effective_slot,
            tx_usage_counter: AtomicU64::new(0),
            program: ProgramCacheEntryType::Loaded(executable),
            ix_usage_counter: AtomicU64::new(0),
            latest_access_slot: AtomicU64::new(0),
        })
    }

    pub fn to_unloaded(&self) -> Option<Self> {
        match &self.program {
            ProgramCacheEntryType::Loaded(_) => {}
            ProgramCacheEntryType::FailedVerification(_)
            | ProgramCacheEntryType::Closed
            | ProgramCacheEntryType::DelayVisibility
            | ProgramCacheEntryType::Unloaded(_)
            | ProgramCacheEntryType::Builtin(_) => {
                return None;
            }
        }
        Some(Self {
            program: ProgramCacheEntryType::Unloaded(self.program.get_environment()?.clone()),
            account_owner: self.account_owner,
            account_size: self.account_size,
            deployment_slot: self.deployment_slot,
            effective_slot: self.effective_slot,
            tx_usage_counter: AtomicU64::new(self.tx_usage_counter.load(Ordering::Relaxed)),
            ix_usage_counter: AtomicU64::new(self.ix_usage_counter.load(Ordering::Relaxed)),
            latest_access_slot: AtomicU64::new(self.latest_access_slot.load(Ordering::Relaxed)),
        })
    }

    /// Creates a new built-in program
    pub fn new_builtin(
        deployment_slot: Slot,
        account_size: usize,
        builtin_function: BuiltinFunctionWithContext,
    ) -> Self {
        let mut function_registry = FunctionRegistry::default();
        function_registry
            .register_function_hashed(*b"entrypoint", builtin_function)
            .unwrap();
        Self {
            deployment_slot,
            account_owner: ProgramCacheEntryOwner::NativeLoader,
            account_size,
            effective_slot: deployment_slot,
            tx_usage_counter: AtomicU64::new(0),
            program: ProgramCacheEntryType::Builtin(BuiltinProgram::new_builtin(function_registry)),
            ix_usage_counter: AtomicU64::new(0),
            latest_access_slot: AtomicU64::new(0),
        }
    }

    pub fn new_tombstone(
        slot: Slot,
        account_owner: ProgramCacheEntryOwner,
        reason: ProgramCacheEntryType,
    ) -> Self {
        let tombstone = Self {
            program: reason,
            account_owner,
            account_size: 0,
            deployment_slot: slot,
            effective_slot: slot,
            tx_usage_counter: AtomicU64::default(),
            ix_usage_counter: AtomicU64::default(),
            latest_access_slot: AtomicU64::new(0),
        };
        debug_assert!(tombstone.is_tombstone());
        tombstone
    }

    pub fn is_tombstone(&self) -> bool {
        matches!(
            self.program,
            ProgramCacheEntryType::FailedVerification(_)
                | ProgramCacheEntryType::Closed
                | ProgramCacheEntryType::DelayVisibility
        )
    }

    fn is_implicit_delay_visibility_tombstone(&self, slot: Slot) -> bool {
        !matches!(self.program, ProgramCacheEntryType::Builtin(_))
            && self.effective_slot.saturating_sub(self.deployment_slot)
                == DELAY_VISIBILITY_SLOT_OFFSET
            && slot >= self.deployment_slot
            && slot < self.effective_slot
    }

    pub fn update_access_slot(&self, slot: Slot) {
        let _ = self.latest_access_slot.fetch_max(slot, Ordering::Relaxed);
    }

    pub fn decayed_usage_counter(&self, now: Slot) -> u64 {
        let last_access = self.latest_access_slot.load(Ordering::Relaxed);
        // Shifting the u64 value for more than 63 will cause an overflow.
        let decaying_for = std::cmp::min(63, now.saturating_sub(last_access));
        self.tx_usage_counter.load(Ordering::Relaxed) >> decaying_for
    }

    pub fn account_owner(&self) -> Pubkey {
        self.account_owner.into()
    }
}

/// Globally shared RBPF config and syscall registry
///
/// This is only valid in an epoch range as long as no feature affecting RBPF is activated.
#[derive(Clone, Debug)]
pub struct ProgramRuntimeEnvironments {
    /// For program runtime V1
    pub program_runtime_v1: ProgramRuntimeEnvironment,
    /// For program runtime V2
    pub program_runtime_v2: ProgramRuntimeEnvironment,
}

impl Default for ProgramRuntimeEnvironments {
    fn default() -> Self {
        let empty_loader = Arc::new(BuiltinProgram::new_loader(
            Config::default(),
            FunctionRegistry::default(),
        ));
        Self {
            program_runtime_v1: empty_loader.clone(),
            program_runtime_v2: empty_loader,
        }
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct LoadingTaskCookie(u64);

impl LoadingTaskCookie {
    fn new() -> Self {
        Self(0)
    }

    fn update(&mut self) {
        let LoadingTaskCookie(cookie) = self;
        *cookie = cookie.wrapping_add(1);
    }
}

/// Suspends the thread in case no cooprative loading task was assigned
#[derive(Debug, Default)]
pub struct LoadingTaskWaiter {
    cookie: Mutex<LoadingTaskCookie>,
    cond: Condvar,
}

impl LoadingTaskWaiter {
    pub fn new() -> Self {
        Self {
            cookie: Mutex::new(LoadingTaskCookie::new()),
            cond: Condvar::new(),
        }
    }

    pub fn cookie(&self) -> LoadingTaskCookie {
        *self.cookie.lock().unwrap()
    }

    pub fn notify(&self) {
        let mut cookie = self.cookie.lock().unwrap();
        cookie.update();
        self.cond.notify_all();
    }

    pub fn wait(&self, cookie: LoadingTaskCookie) -> LoadingTaskCookie {
        let cookie_guard = self.cookie.lock().unwrap();
        *self
            .cond
            .wait_while(cookie_guard, |current_cookie| *current_cookie == cookie)
            .unwrap()
    }
}

#[derive(Debug)]
enum IndexImplementation {
    /// Fork-graph aware index implementation
    V1 {
        /// A two level index:
        ///
        /// - the first level is for the address at which programs are deployed
        /// - the second level for the slot (and thus also fork), sorted by slot number.
        entries: HashMap<Pubkey, Vec<Arc<ProgramCacheEntry>>>,
        /// The entries that are getting loaded and have not yet finished loading.
        ///
        /// The key is the program address, the value is a tuple of the slot in which the program is
        /// being loaded and the thread ID doing the load.
        ///
        /// It is possible that multiple TX batches from different slots need different versions of a
        /// program. The deployment slot of a program is only known after load tho,
        /// so all loads for a given program key are serialized.
        loading_entries: Mutex<HashMap<Pubkey, (Slot, thread::ThreadId)>>,
    },
}

/// This structure is the global cache of loaded, verified and compiled programs.
///
/// It ...
/// - is validator global and fork graph aware, so it can optimize the commonalities across banks.
/// - handles the visibility rules of un/re/deployments.
/// - stores the usage statistics and verification status of each program.
/// - is elastic and uses a probabilistic eviction stragety based on the usage statistics.
/// - also keeps the compiled executables around, but only for the most used programs.
/// - supports various kinds of tombstones to avoid loading programs which can not be loaded.
/// - cleans up entries on orphan branches when the block store is rerooted.
/// - supports the cache preparation phase before feature activations which can change cached programs.
/// - manages the environments of the programs and upcoming environments for the next epoch.
/// - allows for cooperative loading of TX batches which hit the same missing programs simultaneously.
/// - enforces that all programs used in a batch are eagerly loaded ahead of execution.
/// - is not persisted to disk or a snapshot, so it needs to cold start and warm up first.
pub struct ProgramCache<FG: ForkGraph> {
    /// Index of the cached entries and cooperative loading tasks
    index: IndexImplementation,
    /// The slot of the last rerooting
    pub latest_root_slot: Slot,
    /// The epoch of the last rerooting
    pub latest_root_epoch: Epoch,
    /// Environments of the current epoch
    pub environments: ProgramRuntimeEnvironments,
    /// Anticipated replacement for `environments` at the next epoch
    ///
    /// This is `None` during most of an epoch, and only `Some` around the boundaries (at the end and beginning of an epoch).
    /// More precisely, it starts with the cache preparation phase a few hundred slots before the epoch boundary,
    /// and it ends with the first rerooting after the epoch boundary.
    pub upcoming_environments: Option<ProgramRuntimeEnvironments>,
    /// List of loaded programs which should be recompiled before the next epoch (but don't have to).
    pub programs_to_recompile: Vec<(Pubkey, Arc<ProgramCacheEntry>)>,
    /// Statistics counters
    pub stats: ProgramCacheStats,
    /// Reference to the block store
    pub fork_graph: Option<Weak<RwLock<FG>>>,
    /// Coordinates TX batches waiting for others to complete their task during cooperative loading
    pub loading_task_waiter: Arc<LoadingTaskWaiter>,
}

impl<FG: ForkGraph> Debug for ProgramCache<FG> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProgramCache")
            .field("root slot", &self.latest_root_slot)
            .field("root epoch", &self.latest_root_epoch)
            .field("stats", &self.stats)
            .field("index", &self.index)
            .finish()
    }
}

/// Local view into [ProgramCache] which was extracted for a specific TX batch.
///
/// This isolation enables the global [ProgramCache] to continue to evolve (e.g. evictions),
/// while the TX batch is guaranteed it will continue to find all the programs it requires.
/// For program management instructions this also buffers them before they are merged back into the global [ProgramCache].
#[derive(Clone, Debug, Default)]
pub struct ProgramCacheForTxBatch {
    /// Pubkey is the address of a program.
    /// ProgramCacheEntry is the corresponding program entry valid for the slot in which a transaction is being executed.
    entries: HashMap<Pubkey, Arc<ProgramCacheEntry>>,
    /// Program entries modified during the transaction batch.
    modified_entries: HashMap<Pubkey, Arc<ProgramCacheEntry>>,
    slot: Slot,
    pub environments: ProgramRuntimeEnvironments,
    /// Anticipated replacement for `environments` at the next epoch.
    ///
    /// This is `None` during most of an epoch, and only `Some` around the boundaries (at the end and beginning of an epoch).
    /// More precisely, it starts with the cache preparation phase a few hundred slots before the epoch boundary,
    /// and it ends with the first rerooting after the epoch boundary.
    /// Needed when a program is deployed at the last slot of an epoch, becomes effective in the next epoch.
    /// So needs to be compiled with the environment for the next epoch.
    pub upcoming_environments: Option<ProgramRuntimeEnvironments>,
    /// The epoch of the last rerooting
    pub latest_root_epoch: Epoch,
    pub hit_max_limit: bool,
    pub loaded_missing: bool,
    pub merged_modified: bool,
}

impl ProgramCacheForTxBatch {
    pub fn new(
        slot: Slot,
        environments: ProgramRuntimeEnvironments,
        upcoming_environments: Option<ProgramRuntimeEnvironments>,
        latest_root_epoch: Epoch,
    ) -> Self {
        Self {
            entries: HashMap::new(),
            modified_entries: HashMap::new(),
            slot,
            environments,
            upcoming_environments,
            latest_root_epoch,
            hit_max_limit: false,
            loaded_missing: false,
            merged_modified: false,
        }
    }

    pub fn new_from_cache<FG: ForkGraph>(
        slot: Slot,
        epoch: Epoch,
        cache: &ProgramCache<FG>,
    ) -> Self {
        Self {
            entries: HashMap::new(),
            modified_entries: HashMap::new(),
            slot,
            environments: cache.get_environments_for_epoch(epoch),
            upcoming_environments: cache.get_upcoming_environments_for_epoch(epoch),
            latest_root_epoch: cache.latest_root_epoch,
            hit_max_limit: false,
            loaded_missing: false,
            merged_modified: false,
        }
    }

    /// Returns the current environments depending on the given epoch
    pub fn get_environments_for_epoch(&self, epoch: Epoch) -> &ProgramRuntimeEnvironments {
        if epoch != self.latest_root_epoch {
            if let Some(upcoming_environments) = self.upcoming_environments.as_ref() {
                return upcoming_environments;
            }
        }
        &self.environments
    }

    /// Refill the cache with a single entry. It's typically called during transaction loading, and
    /// transaction processing (for program management instructions).
    /// It replaces the existing entry (if any) with the provided entry. The return value contains
    /// `true` if an entry existed.
    /// The function also returns the newly inserted value.
    pub fn replenish(
        &mut self,
        key: Pubkey,
        entry: Arc<ProgramCacheEntry>,
    ) -> (bool, Arc<ProgramCacheEntry>) {
        (self.entries.insert(key, entry.clone()).is_some(), entry)
    }

    /// Store an entry in `modified_entries` for a program modified during the
    /// transaction batch.
    pub fn store_modified_entry(&mut self, key: Pubkey, entry: Arc<ProgramCacheEntry>) {
        self.modified_entries.insert(key, entry);
    }

    /// Drain the program cache's modified entries, returning the owned
    /// collection.
    pub fn drain_modified_entries(&mut self) -> HashMap<Pubkey, Arc<ProgramCacheEntry>> {
        std::mem::take(&mut self.modified_entries)
    }

    pub fn find(&self, key: &Pubkey) -> Option<Arc<ProgramCacheEntry>> {
        // First lookup the cache of the programs modified by the current
        // transaction. If not found, lookup the cache of the cache of the
        // programs that are loaded for the transaction batch.
        self.modified_entries
            .get(key)
            .or(self.entries.get(key))
            .map(|entry| {
                if entry.is_implicit_delay_visibility_tombstone(self.slot) {
                    // Found a program entry on the current fork, but it's not effective
                    // yet. It indicates that the program has delayed visibility. Return
                    // the tombstone to reflect that.
                    Arc::new(ProgramCacheEntry::new_tombstone(
                        entry.deployment_slot,
                        entry.account_owner,
                        ProgramCacheEntryType::DelayVisibility,
                    ))
                } else {
                    entry.clone()
                }
            })
    }

    pub fn slot(&self) -> Slot {
        self.slot
    }

    pub fn set_slot_for_tests(&mut self, slot: Slot) {
        self.slot = slot;
    }

    pub fn merge(&mut self, modified_entries: &HashMap<Pubkey, Arc<ProgramCacheEntry>>) {
        modified_entries.iter().for_each(|(key, entry)| {
            self.merged_modified = true;
            self.replenish(*key, entry.clone());
        })
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

pub enum ProgramCacheMatchCriteria {
    DeployedOnOrAfterSlot(Slot),
    Tombstone,
    NoCriteria,
}

impl<FG: ForkGraph> ProgramCache<FG> {
    pub fn new(root_slot: Slot, root_epoch: Epoch) -> Self {
        Self {
            index: IndexImplementation::V1 {
                entries: HashMap::new(),
                loading_entries: Mutex::new(HashMap::new()),
            },
            latest_root_slot: root_slot,
            latest_root_epoch: root_epoch,
            environments: ProgramRuntimeEnvironments::default(),
            upcoming_environments: None,
            programs_to_recompile: Vec::default(),
            stats: ProgramCacheStats::default(),
            fork_graph: None,
            loading_task_waiter: Arc::new(LoadingTaskWaiter::default()),
        }
    }

    pub fn set_fork_graph(&mut self, fork_graph: Weak<RwLock<FG>>) {
        self.fork_graph = Some(fork_graph);
    }

    /// Returns the current environments depending on the given epoch
    pub fn get_environments_for_epoch(&self, epoch: Epoch) -> ProgramRuntimeEnvironments {
        if epoch != self.latest_root_epoch {
            if let Some(upcoming_environments) = self.upcoming_environments.as_ref() {
                return upcoming_environments.clone();
            }
        }
        self.environments.clone()
    }

    /// Returns the upcoming environments depending on the given epoch
    pub fn get_upcoming_environments_for_epoch(
        &self,
        epoch: Epoch,
    ) -> Option<ProgramRuntimeEnvironments> {
        if epoch == self.latest_root_epoch {
            return self.upcoming_environments.clone();
        }
        None
    }

    /// Insert a single entry. It's typically called during transaction loading,
    /// when the cache doesn't contain the entry corresponding to program `key`.
    pub fn assign_program(&mut self, key: Pubkey, entry: Arc<ProgramCacheEntry>) -> bool {
        debug_assert!(!matches!(
            &entry.program,
            ProgramCacheEntryType::DelayVisibility
        ));
        // This function always returns `true` during normal operation.
        // Only during the cache preparation phase this can return `false`
        // for entries with `upcoming_environments`.
        fn is_current_env(
            environments: &ProgramRuntimeEnvironments,
            env_opt: Option<&ProgramRuntimeEnvironment>,
        ) -> bool {
            env_opt
                .map(|env| {
                    Arc::ptr_eq(env, &environments.program_runtime_v1)
                        || Arc::ptr_eq(env, &environments.program_runtime_v2)
                })
                .unwrap_or(true)
        }
        match &mut self.index {
            IndexImplementation::V1 { entries, .. } => {
                let slot_versions = &mut entries.entry(key).or_default();
                match slot_versions.binary_search_by(|at| {
                    at.effective_slot
                        .cmp(&entry.effective_slot)
                        .then(at.deployment_slot.cmp(&entry.deployment_slot))
                        .then(
                            // This `.then()` has no effect during normal operation.
                            // Only during the cache preparation phase this does allow entries
                            // which only differ in their environment to be interleaved in `slot_versions`.
                            is_current_env(&self.environments, at.program.get_environment()).cmp(
                                &is_current_env(
                                    &self.environments,
                                    entry.program.get_environment(),
                                ),
                            ),
                        )
                }) {
                    Ok(index) => {
                        let existing = slot_versions.get_mut(index).unwrap();
                        match (&existing.program, &entry.program) {
                            (
                                ProgramCacheEntryType::Builtin(_),
                                ProgramCacheEntryType::Builtin(_),
                            )
                            | (
                                ProgramCacheEntryType::Unloaded(_),
                                ProgramCacheEntryType::Loaded(_),
                            ) => {}
                            _ => {
                                // Something is wrong, I can feel it ...
                                error!("ProgramCache::assign_program() failed key={:?} existing={:?} entry={:?}", key, slot_versions, entry);
                                debug_assert!(false, "Unexpected replacement of an entry");
                                self.stats.replacements.fetch_add(1, Ordering::Relaxed);
                                return true;
                            }
                        }
                        // Copy over the usage counter to the new entry
                        entry.tx_usage_counter.fetch_add(
                            existing.tx_usage_counter.load(Ordering::Relaxed),
                            Ordering::Relaxed,
                        );
                        entry.ix_usage_counter.fetch_add(
                            existing.ix_usage_counter.load(Ordering::Relaxed),
                            Ordering::Relaxed,
                        );
                        *existing = Arc::clone(&entry);
                        self.stats.reloads.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(index) => {
                        self.stats.insertions.fetch_add(1, Ordering::Relaxed);
                        slot_versions.insert(index, Arc::clone(&entry));
                    }
                }
            }
        }
        false
    }

    pub fn prune_by_deployment_slot(&mut self, slot: Slot) {
        match &mut self.index {
            IndexImplementation::V1 { entries, .. } => {
                for second_level in entries.values_mut() {
                    second_level.retain(|entry| entry.deployment_slot != slot);
                }
                self.remove_programs_with_no_entries();
            }
        }
    }

    /// Before rerooting the blockstore this removes all superfluous entries
    pub fn prune(&mut self, new_root_slot: Slot, new_root_epoch: Epoch) {
        let Some(fork_graph) = self.fork_graph.clone() else {
            error!("Program cache doesn't have fork graph.");
            return;
        };
        let fork_graph = fork_graph.upgrade().unwrap();
        let Ok(fork_graph) = fork_graph.read() else {
            error!("Failed to lock fork graph for reading.");
            return;
        };
        let mut preparation_phase_ends = false;
        if self.latest_root_epoch != new_root_epoch {
            self.latest_root_epoch = new_root_epoch;
            if let Some(upcoming_environments) = self.upcoming_environments.take() {
                preparation_phase_ends = true;
                self.environments = upcoming_environments;
                self.programs_to_recompile.clear();
            }
        }
        match &mut self.index {
            IndexImplementation::V1 { entries, .. } => {
                for second_level in entries.values_mut() {
                    // Remove entries un/re/deployed on orphan forks
                    let mut first_ancestor_found = false;
                    let mut first_ancestor_env = None;
                    *second_level = second_level
                        .iter()
                        .rev()
                        .filter(|entry| {
                            let relation =
                                fork_graph.relationship(entry.deployment_slot, new_root_slot);
                            if entry.deployment_slot >= new_root_slot {
                                matches!(relation, BlockRelation::Equal | BlockRelation::Descendant)
                            } else if matches!(relation, BlockRelation::Ancestor)
                                || entry.deployment_slot <= self.latest_root_slot
                            {
                                if !first_ancestor_found {
                                    first_ancestor_found = true;
                                    first_ancestor_env = entry.program.get_environment();
                                    return true;
                                }
                                // Do not prune the entry if the runtime environment of the entry is different
                                // than the entry that was previously found (stored in first_ancestor_env).
                                // Different environment indicates that this entry might belong to an older
                                // epoch that had a different environment (e.g. different feature set).
                                // Once the root moves to the new/current epoch, the entry will get pruned.
                                // But, until then the entry might still be getting used by an older slot.
                                if let Some(entry_env) = entry.program.get_environment() {
                                    if let Some(env) = first_ancestor_env {
                                        if !Arc::ptr_eq(entry_env, env) {
                                            return true;
                                        }
                                    }
                                }
                                self.stats.prunes_orphan.fetch_add(1, Ordering::Relaxed);
                                false
                            } else {
                                self.stats.prunes_orphan.fetch_add(1, Ordering::Relaxed);
                                false
                            }
                        })
                        .filter(|entry| {
                            // Remove outdated environment of previous feature set
                            if preparation_phase_ends
                                && !Self::matches_environment(entry, &self.environments)
                            {
                                self.stats
                                    .prunes_environment
                                    .fetch_add(1, Ordering::Relaxed);
                                return false;
                            }
                            true
                        })
                        .cloned()
                        .collect();
                    second_level.reverse();
                }
            }
        }
        self.remove_programs_with_no_entries();
        debug_assert!(self.latest_root_slot <= new_root_slot);
        self.latest_root_slot = new_root_slot;
    }

    fn matches_environment(
        entry: &Arc<ProgramCacheEntry>,
        environments: &ProgramRuntimeEnvironments,
    ) -> bool {
        let Some(environment) = entry.program.get_environment() else {
            return true;
        };
        Arc::ptr_eq(environment, &environments.program_runtime_v1)
            || Arc::ptr_eq(environment, &environments.program_runtime_v2)
    }

    fn matches_criteria(
        program: &Arc<ProgramCacheEntry>,
        criteria: &ProgramCacheMatchCriteria,
    ) -> bool {
        match criteria {
            ProgramCacheMatchCriteria::DeployedOnOrAfterSlot(slot) => {
                program.deployment_slot >= *slot
            }
            ProgramCacheMatchCriteria::Tombstone => program.is_tombstone(),
            ProgramCacheMatchCriteria::NoCriteria => true,
        }
    }

    /// Extracts a subset of the programs relevant to a transaction batch
    /// and returns which program accounts the accounts DB needs to load.
    pub fn extract(
        &self,
        search_for: &mut Vec<(Pubkey, (ProgramCacheMatchCriteria, u64))>,
        loaded_programs_for_tx_batch: &mut ProgramCacheForTxBatch,
        is_first_round: bool,
    ) -> Option<(Pubkey, u64)> {
        debug_assert!(self.fork_graph.is_some());
        let fork_graph = self.fork_graph.as_ref().unwrap().upgrade().unwrap();
        let locked_fork_graph = fork_graph.read().unwrap();
        let mut cooperative_loading_task = None;
        match &self.index {
            IndexImplementation::V1 {
                entries,
                loading_entries,
            } => {
                search_for.retain(|(key, (match_criteria, usage_count))| {
                    if let Some(second_level) = entries.get(key) {
                        for entry in second_level.iter().rev() {
                            if entry.deployment_slot <= self.latest_root_slot
                                || matches!(
                                    locked_fork_graph.relationship(
                                        entry.deployment_slot,
                                        loaded_programs_for_tx_batch.slot
                                    ),
                                    BlockRelation::Equal | BlockRelation::Ancestor
                                )
                            {
                                let entry_to_return = if loaded_programs_for_tx_batch.slot
                                    >= entry.effective_slot
                                    && Self::matches_environment(
                                        entry,
                                        &loaded_programs_for_tx_batch.environments,
                                    ) {
                                    if !Self::matches_criteria(entry, match_criteria) {
                                        break;
                                    }
                                    if let ProgramCacheEntryType::Unloaded(_environment) =
                                        &entry.program
                                    {
                                        break;
                                    }
                                    entry.clone()
                                } else if entry.is_implicit_delay_visibility_tombstone(
                                    loaded_programs_for_tx_batch.slot,
                                ) {
                                    // Found a program entry on the current fork, but it's not effective
                                    // yet. It indicates that the program has delayed visibility. Return
                                    // the tombstone to reflect that.
                                    Arc::new(ProgramCacheEntry::new_tombstone(
                                        entry.deployment_slot,
                                        entry.account_owner,
                                        ProgramCacheEntryType::DelayVisibility,
                                    ))
                                } else {
                                    continue;
                                };
                                entry_to_return
                                    .update_access_slot(loaded_programs_for_tx_batch.slot);
                                entry_to_return
                                    .tx_usage_counter
                                    .fetch_add(*usage_count, Ordering::Relaxed);
                                loaded_programs_for_tx_batch
                                    .entries
                                    .insert(*key, entry_to_return);
                                return false;
                            }
                        }
                    }
                    if cooperative_loading_task.is_none() {
                        let mut loading_entries = loading_entries.lock().unwrap();
                        let entry = loading_entries.entry(*key);
                        if let Entry::Vacant(entry) = entry {
                            entry.insert((
                                loaded_programs_for_tx_batch.slot,
                                thread::current().id(),
                            ));
                            cooperative_loading_task = Some((*key, *usage_count));
                        }
                    }
                    true
                });
            }
        }
        drop(locked_fork_graph);
        if is_first_round {
            self.stats
                .misses
                .fetch_add(search_for.len() as u64, Ordering::Relaxed);
            self.stats.hits.fetch_add(
                loaded_programs_for_tx_batch.entries.len() as u64,
                Ordering::Relaxed,
            );
        }
        cooperative_loading_task
    }

    /// Called by Bank::replenish_program_cache() for each program that is done loading.
    pub fn finish_cooperative_loading_task(
        &mut self,
        slot: Slot,
        key: Pubkey,
        loaded_program: Arc<ProgramCacheEntry>,
    ) -> bool {
        match &mut self.index {
            IndexImplementation::V1 {
                loading_entries, ..
            } => {
                let loading_thread = loading_entries.get_mut().unwrap().remove(&key);
                debug_assert_eq!(loading_thread, Some((slot, thread::current().id())));
                // Check that it will be visible to our own fork once inserted
                if loaded_program.deployment_slot > self.latest_root_slot
                    && !matches!(
                        self.fork_graph
                            .as_ref()
                            .unwrap()
                            .upgrade()
                            .unwrap()
                            .read()
                            .unwrap()
                            .relationship(loaded_program.deployment_slot, slot),
                        BlockRelation::Equal | BlockRelation::Ancestor
                    )
                {
                    self.stats.lost_insertions.fetch_add(1, Ordering::Relaxed);
                }
                let was_occupied = self.assign_program(key, loaded_program);
                self.loading_task_waiter.notify();
                was_occupied
            }
        }
    }

    pub fn merge(&mut self, modified_entries: &HashMap<Pubkey, Arc<ProgramCacheEntry>>) {
        modified_entries.iter().for_each(|(key, entry)| {
            self.assign_program(*key, entry.clone());
        })
    }

    /// Returns the list of entries which are verified and compiled.
    pub fn get_flattened_entries(
        &self,
        include_program_runtime_v1: bool,
        _include_program_runtime_v2: bool,
    ) -> Vec<(Pubkey, Arc<ProgramCacheEntry>)> {
        match &self.index {
            IndexImplementation::V1 { entries, .. } => entries
                .iter()
                .flat_map(|(id, second_level)| {
                    second_level
                        .iter()
                        .filter_map(move |program| match program.program {
                            ProgramCacheEntryType::Loaded(_) => {
                                if include_program_runtime_v1 {
                                    Some((*id, program.clone()))
                                } else {
                                    None
                                }
                            }
                            _ => None,
                        })
                })
                .collect(),
        }
    }

    /// Returns the list of all entries in the cache.
    pub fn get_flattened_entries_for_tests(&self) -> Vec<(Pubkey, Arc<ProgramCacheEntry>)> {
        match &self.index {
            IndexImplementation::V1 { entries, .. } => entries
                .iter()
                .flat_map(|(id, second_level)| {
                    second_level.iter().map(|program| (*id, program.clone()))
                })
                .collect(),
        }
    }

    /// Returns the slot versions for the given program id.
    pub fn get_slot_versions_for_tests(&self, key: &Pubkey) -> &[Arc<ProgramCacheEntry>] {
        match &self.index {
            IndexImplementation::V1 { entries, .. } => entries
                .get(key)
                .map(|second_level| second_level.as_ref())
                .unwrap_or(&[]),
        }
    }

    /// Unloads programs which were used infrequently
    pub fn sort_and_unload(&mut self, shrink_to: PercentageInteger) {
        let mut sorted_candidates = self.get_flattened_entries(true, true);
        sorted_candidates
            .sort_by_cached_key(|(_id, program)| program.tx_usage_counter.load(Ordering::Relaxed));
        let num_to_unload = sorted_candidates
            .len()
            .saturating_sub(shrink_to.apply_to(MAX_LOADED_ENTRY_COUNT));
        self.unload_program_entries(sorted_candidates.iter().take(num_to_unload));
    }

    /// Evicts programs using 2's random selection, choosing the least used program out of the two entries.
    /// The eviction is performed enough number of times to reduce the cache usage to the given percentage.
    pub fn evict_using_2s_random_selection(&mut self, shrink_to: PercentageInteger, now: Slot) {
        let mut candidates = self.get_flattened_entries(true, true);
        self.stats
            .water_level
            .store(candidates.len() as u64, Ordering::Relaxed);
        let num_to_unload = candidates
            .len()
            .saturating_sub(shrink_to.apply_to(MAX_LOADED_ENTRY_COUNT));
        fn random_index_and_usage_counter(
            candidates: &[(Pubkey, Arc<ProgramCacheEntry>)],
            now: Slot,
        ) -> (usize, u64) {
            let mut rng = thread_rng();
            let index = rng.gen_range(0..candidates.len());
            let usage_counter = candidates
                .get(index)
                .expect("Failed to get cached entry")
                .1
                .decayed_usage_counter(now);
            (index, usage_counter)
        }

        for _ in 0..num_to_unload {
            let (index1, usage_counter1) = random_index_and_usage_counter(&candidates, now);
            let (index2, usage_counter2) = random_index_and_usage_counter(&candidates, now);

            let (program, entry) = if usage_counter1 < usage_counter2 {
                candidates.swap_remove(index1)
            } else {
                candidates.swap_remove(index2)
            };
            self.unload_program_entry(&program, &entry);
        }
    }

    /// Removes all the entries at the given keys, if they exist
    pub fn remove_programs(&mut self, keys: impl Iterator<Item = Pubkey>) {
        match &mut self.index {
            IndexImplementation::V1 { entries, .. } => {
                for k in keys {
                    entries.remove(&k);
                }
            }
        }
    }

    /// This function removes the given entry for the given program from the cache.
    /// The function expects that the program and entry exists in the cache. Otherwise it'll panic.
    fn unload_program_entry(&mut self, program: &Pubkey, remove_entry: &Arc<ProgramCacheEntry>) {
        match &mut self.index {
            IndexImplementation::V1 { entries, .. } => {
                let second_level = entries.get_mut(program).expect("Cache lookup failed");
                let candidate = second_level
                    .iter_mut()
                    .find(|entry| entry == &remove_entry)
                    .expect("Program entry not found");

                // Certain entry types cannot be unloaded, such as tombstones, or already unloaded entries.
                // For such entries, `to_unloaded()` will return None.
                // These entry types do not occupy much memory.
                if let Some(unloaded) = candidate.to_unloaded() {
                    if candidate.tx_usage_counter.load(Ordering::Relaxed) == 1 {
                        self.stats.one_hit_wonders.fetch_add(1, Ordering::Relaxed);
                    }
                    self.stats
                        .evictions
                        .entry(*program)
                        .and_modify(|c| saturating_add_assign!(*c, 1))
                        .or_insert(1);
                    *candidate = Arc::new(unloaded);
                }
            }
        }
    }

    fn unload_program_entries<'a>(
        &mut self,
        remove: impl Iterator<Item = &'a (Pubkey, Arc<ProgramCacheEntry>)>,
    ) {
        for (program, entry) in remove {
            self.unload_program_entry(program, entry);
        }
    }

    fn remove_programs_with_no_entries(&mut self) {
        match &mut self.index {
            IndexImplementation::V1 { entries, .. } => {
                let num_programs_before_removal = entries.len();
                entries.retain(|_key, second_level| !second_level.is_empty());
                if entries.len() < num_programs_before_removal {
                    self.stats.empty_entries.fetch_add(
                        num_programs_before_removal.saturating_sub(entries.len()) as u64,
                        Ordering::Relaxed,
                    );
                }
            }
        }
    }
}

#[cfg(feature = "frozen-abi")]
impl solana_frozen_abi::abi_example::AbiExample for ProgramCacheEntry {
    fn example() -> Self {
        // ProgramCacheEntry isn't serializable by definition.
        Self::default()
    }
}

#[cfg(feature = "frozen-abi")]
impl<FG: ForkGraph> solana_frozen_abi::abi_example::AbiExample for ProgramCache<FG> {
    fn example() -> Self {
        // ProgramCache isn't serializable by definition.
        Self::new(Slot::default(), Epoch::default())
    }
}

#[cfg(test)]
mod tests {
    use {
        crate::loaded_programs::{
            BlockRelation, ForkGraph, ProgramCache, ProgramCacheEntry, ProgramCacheEntryOwner,
            ProgramCacheEntryType, ProgramCacheForTxBatch, ProgramCacheMatchCriteria,
            ProgramRuntimeEnvironment, ProgramRuntimeEnvironments, DELAY_VISIBILITY_SLOT_OFFSET,
        },
        assert_matches::assert_matches,
        percentage::Percentage,
        solana_rbpf::{elf::Executable, program::BuiltinProgram},
        solana_sdk::{clock::Slot, pubkey::Pubkey},
        std::{
            fs::File,
            io::Read,
            ops::ControlFlow,
            sync::{
                atomic::{AtomicU64, Ordering},
                Arc, RwLock,
            },
        },
        test_case::{test_case, test_matrix},
    };

    static MOCK_ENVIRONMENT: std::sync::OnceLock<ProgramRuntimeEnvironment> =
        std::sync::OnceLock::<ProgramRuntimeEnvironment>::new();

    fn get_mock_env() -> ProgramRuntimeEnvironment {
        MOCK_ENVIRONMENT
            .get_or_init(|| Arc::new(BuiltinProgram::new_mock()))
            .clone()
    }

    fn new_mock_cache<FG: ForkGraph>() -> ProgramCache<FG> {
        let mut cache = ProgramCache::new(0, 0);
        cache.environments.program_runtime_v1 = get_mock_env();
        cache
    }

    fn new_test_entry(deployment_slot: Slot, effective_slot: Slot) -> Arc<ProgramCacheEntry> {
        new_test_entry_with_usage(deployment_slot, effective_slot, AtomicU64::default())
    }

    fn new_loaded_entry(env: ProgramRuntimeEnvironment) -> ProgramCacheEntryType {
        let mut elf = Vec::new();
        File::open("../programs/bpf_loader/test_elfs/out/noop_aligned.so")
            .unwrap()
            .read_to_end(&mut elf)
            .unwrap();
        let executable = Executable::load(&elf, env).unwrap();
        ProgramCacheEntryType::Loaded(executable)
    }

    fn new_test_entry_with_usage(
        deployment_slot: Slot,
        effective_slot: Slot,
        usage_counter: AtomicU64,
    ) -> Arc<ProgramCacheEntry> {
        Arc::new(ProgramCacheEntry {
            program: new_loaded_entry(get_mock_env()),
            account_owner: ProgramCacheEntryOwner::LoaderV2,
            account_size: 0,
            deployment_slot,
            effective_slot,
            tx_usage_counter: usage_counter,
            ix_usage_counter: AtomicU64::default(),
            latest_access_slot: AtomicU64::new(deployment_slot),
        })
    }

    fn new_test_builtin_entry(
        deployment_slot: Slot,
        effective_slot: Slot,
    ) -> Arc<ProgramCacheEntry> {
        Arc::new(ProgramCacheEntry {
            program: ProgramCacheEntryType::Builtin(BuiltinProgram::new_mock()),
            account_owner: ProgramCacheEntryOwner::NativeLoader,
            account_size: 0,
            deployment_slot,
            effective_slot,
            tx_usage_counter: AtomicU64::default(),
            ix_usage_counter: AtomicU64::default(),
            latest_access_slot: AtomicU64::default(),
        })
    }

    fn set_tombstone<FG: ForkGraph>(
        cache: &mut ProgramCache<FG>,
        key: Pubkey,
        slot: Slot,
        reason: ProgramCacheEntryType,
    ) -> Arc<ProgramCacheEntry> {
        let program = Arc::new(ProgramCacheEntry::new_tombstone(
            slot,
            ProgramCacheEntryOwner::LoaderV2,
            reason,
        ));
        cache.assign_program(key, program.clone());
        program
    }

    fn insert_unloaded_entry<FG: ForkGraph>(
        cache: &mut ProgramCache<FG>,
        key: Pubkey,
        slot: Slot,
    ) -> Arc<ProgramCacheEntry> {
        let loaded = new_test_entry_with_usage(slot, slot.saturating_add(1), AtomicU64::default());
        let unloaded = Arc::new(loaded.to_unloaded().expect("Failed to unload the program"));
        cache.assign_program(key, unloaded.clone());
        unloaded
    }

    fn num_matching_entries<P, FG>(cache: &ProgramCache<FG>, predicate: P) -> usize
    where
        P: Fn(&ProgramCacheEntryType) -> bool,
        FG: ForkGraph,
    {
        cache
            .get_flattened_entries_for_tests()
            .iter()
            .filter(|(_key, program)| predicate(&program.program))
            .count()
    }

    #[test]
    fn test_usage_counter_decay() {
        let _cache = new_mock_cache::<TestForkGraph>();
        let program = new_test_entry_with_usage(10, 11, AtomicU64::new(32));
        program.update_access_slot(15);
        assert_eq!(program.decayed_usage_counter(15), 32);
        assert_eq!(program.decayed_usage_counter(16), 16);
        assert_eq!(program.decayed_usage_counter(17), 8);
        assert_eq!(program.decayed_usage_counter(18), 4);
        assert_eq!(program.decayed_usage_counter(19), 2);
        assert_eq!(program.decayed_usage_counter(20), 1);
        assert_eq!(program.decayed_usage_counter(21), 0);
        assert_eq!(program.decayed_usage_counter(15), 32);
        assert_eq!(program.decayed_usage_counter(14), 32);

        program.update_access_slot(18);
        assert_eq!(program.decayed_usage_counter(15), 32);
        assert_eq!(program.decayed_usage_counter(16), 32);
        assert_eq!(program.decayed_usage_counter(17), 32);
        assert_eq!(program.decayed_usage_counter(18), 32);
        assert_eq!(program.decayed_usage_counter(19), 16);
        assert_eq!(program.decayed_usage_counter(20), 8);
        assert_eq!(program.decayed_usage_counter(21), 4);

        // Decay for 63 or more slots
        assert_eq!(program.decayed_usage_counter(18 + 63), 0);
        assert_eq!(program.decayed_usage_counter(100), 0);
    }

    fn program_deploy_test_helper(
        cache: &mut ProgramCache<TestForkGraph>,
        program: Pubkey,
        deployment_slots: Vec<Slot>,
        usage_counters: Vec<u64>,
        programs: &mut Vec<(Pubkey, Slot, u64)>,
    ) {
        // Add multiple entries for program
        deployment_slots
            .iter()
            .enumerate()
            .for_each(|(i, deployment_slot)| {
                let usage_counter = *usage_counters.get(i).unwrap_or(&0);
                cache.assign_program(
                    program,
                    new_test_entry_with_usage(
                        *deployment_slot,
                        (*deployment_slot).saturating_add(2),
                        AtomicU64::new(usage_counter),
                    ),
                );
                programs.push((program, *deployment_slot, usage_counter));
            });

        // Add tombstones entries for program
        let env = Arc::new(BuiltinProgram::new_mock());
        for slot in 21..31 {
            set_tombstone(
                cache,
                program,
                slot,
                ProgramCacheEntryType::FailedVerification(env.clone()),
            );
        }

        // Add unloaded entries for program
        for slot in 31..41 {
            insert_unloaded_entry(cache, program, slot);
        }
    }

    #[test]
    fn test_random_eviction() {
        let mut programs = vec![];

        let mut cache = new_mock_cache::<TestForkGraph>();

        // This test adds different kind of entries to the cache.
        // Tombstones and unloaded entries are expected to not be evicted.
        // It also adds multiple entries for three programs as it tries to create a typical cache instance.

        // Program 1
        program_deploy_test_helper(
            &mut cache,
            Pubkey::new_unique(),
            vec![0, 10, 20],
            vec![4, 5, 25],
            &mut programs,
        );

        // Program 2
        program_deploy_test_helper(
            &mut cache,
            Pubkey::new_unique(),
            vec![5, 11],
            vec![0, 2],
            &mut programs,
        );

        // Program 3
        program_deploy_test_helper(
            &mut cache,
            Pubkey::new_unique(),
            vec![0, 5, 15],
            vec![100, 3, 20],
            &mut programs,
        );

        // 1 for each deployment slot
        let num_loaded_expected = 8;
        // 10 for each program
        let num_unloaded_expected = 30;
        // 10 for each program
        let num_tombstones_expected = 30;

        // Count the number of loaded, unloaded and tombstone entries.
        programs.sort_by_key(|(_id, _slot, usage_count)| *usage_count);
        let num_loaded = num_matching_entries(&cache, |program_type| {
            matches!(program_type, ProgramCacheEntryType::Loaded(_))
        });
        let num_unloaded = num_matching_entries(&cache, |program_type| {
            matches!(program_type, ProgramCacheEntryType::Unloaded(_))
        });
        let num_tombstones = num_matching_entries(&cache, |program_type| {
            matches!(
                program_type,
                ProgramCacheEntryType::DelayVisibility
                    | ProgramCacheEntryType::FailedVerification(_)
                    | ProgramCacheEntryType::Closed
            )
        });

        // Test that the cache is constructed with the expected number of entries.
        assert_eq!(num_loaded, num_loaded_expected);
        assert_eq!(num_unloaded, num_unloaded_expected);
        assert_eq!(num_tombstones, num_tombstones_expected);

        // Evict entries from the cache
        let eviction_pct = 2;

        let num_loaded_expected =
            Percentage::from(eviction_pct).apply_to(crate::loaded_programs::MAX_LOADED_ENTRY_COUNT);
        let num_unloaded_expected = num_unloaded_expected + num_loaded - num_loaded_expected;
        cache.evict_using_2s_random_selection(Percentage::from(eviction_pct), 21);

        // Count the number of loaded, unloaded and tombstone entries.
        let num_loaded = num_matching_entries(&cache, |program_type| {
            matches!(program_type, ProgramCacheEntryType::Loaded(_))
        });
        let num_unloaded = num_matching_entries(&cache, |program_type| {
            matches!(program_type, ProgramCacheEntryType::Unloaded(_))
        });
        let num_tombstones = num_matching_entries(&cache, |program_type| {
            matches!(program_type, ProgramCacheEntryType::FailedVerification(_))
        });

        // However many entries are left after the shrink
        assert_eq!(num_loaded, num_loaded_expected);
        // The original unloaded entries + the evicted loaded entries
        assert_eq!(num_unloaded, num_unloaded_expected);
        // The original tombstones are not evicted
        assert_eq!(num_tombstones, num_tombstones_expected);
    }

    #[test]
    fn test_eviction() {
        let mut programs = vec![];
        let mut cache = new_mock_cache::<TestForkGraph>();

        // Program 1
        program_deploy_test_helper(
            &mut cache,
            Pubkey::new_unique(),
            vec![0, 10, 20],
            vec![4, 5, 25],
            &mut programs,
        );

        // Program 2
        program_deploy_test_helper(
            &mut cache,
            Pubkey::new_unique(),
            vec![5, 11],
            vec![0, 2],
            &mut programs,
        );

        // Program 3
        program_deploy_test_helper(
            &mut cache,
            Pubkey::new_unique(),
            vec![0, 5, 15],
            vec![100, 3, 20],
            &mut programs,
        );

        // 1 for each deployment slot
        let num_loaded_expected = 8;
        // 10 for each program
        let num_unloaded_expected = 30;
        // 10 for each program
        let num_tombstones_expected = 30;

        // Count the number of loaded, unloaded and tombstone entries.
        programs.sort_by_key(|(_id, _slot, usage_count)| *usage_count);
        let num_loaded = num_matching_entries(&cache, |program_type| {
            matches!(program_type, ProgramCacheEntryType::Loaded(_))
        });
        let num_unloaded = num_matching_entries(&cache, |program_type| {
            matches!(program_type, ProgramCacheEntryType::Unloaded(_))
        });
        let num_tombstones = num_matching_entries(&cache, |program_type| {
            matches!(program_type, ProgramCacheEntryType::FailedVerification(_))
        });

        // Test that the cache is constructed with the expected number of entries.
        assert_eq!(num_loaded, num_loaded_expected);
        assert_eq!(num_unloaded, num_unloaded_expected);
        assert_eq!(num_tombstones, num_tombstones_expected);

        // Evict entries from the cache
        let eviction_pct = 2;

        let num_loaded_expected =
            Percentage::from(eviction_pct).apply_to(crate::loaded_programs::MAX_LOADED_ENTRY_COUNT);
        let num_unloaded_expected = num_unloaded_expected + num_loaded - num_loaded_expected;

        cache.sort_and_unload(Percentage::from(eviction_pct));

        // Check that every program is still in the cache.
        let entries = cache.get_flattened_entries_for_tests();
        programs.iter().for_each(|entry| {
            assert!(entries.iter().any(|(key, _entry)| key == &entry.0));
        });

        let unloaded = entries
            .iter()
            .filter_map(|(key, program)| {
                matches!(program.program, ProgramCacheEntryType::Unloaded(_))
                    .then_some((*key, program.tx_usage_counter.load(Ordering::Relaxed)))
            })
            .collect::<Vec<(Pubkey, u64)>>();

        for index in 0..3 {
            let expected = programs.get(index).expect("Missing program");
            assert!(unloaded.contains(&(expected.0, expected.2)));
        }

        // Count the number of loaded, unloaded and tombstone entries.
        let num_loaded = num_matching_entries(&cache, |program_type| {
            matches!(program_type, ProgramCacheEntryType::Loaded(_))
        });
        let num_unloaded = num_matching_entries(&cache, |program_type| {
            matches!(program_type, ProgramCacheEntryType::Unloaded(_))
        });
        let num_tombstones = num_matching_entries(&cache, |program_type| {
            matches!(
                program_type,
                ProgramCacheEntryType::DelayVisibility
                    | ProgramCacheEntryType::FailedVerification(_)
                    | ProgramCacheEntryType::Closed
            )
        });

        // However many entries are left after the shrink
        assert_eq!(num_loaded, num_loaded_expected);
        // The original unloaded entries + the evicted loaded entries
        assert_eq!(num_unloaded, num_unloaded_expected);
        // The original tombstones are not evicted
        assert_eq!(num_tombstones, num_tombstones_expected);
    }

    #[test]
    fn test_usage_count_of_unloaded_program() {
        let mut cache = new_mock_cache::<TestForkGraph>();

        let program = Pubkey::new_unique();
        let evict_to_pct = 2;
        let cache_capacity_after_shrink =
            Percentage::from(evict_to_pct).apply_to(crate::loaded_programs::MAX_LOADED_ENTRY_COUNT);
        // Add enough programs to the cache to trigger 1 eviction after shrinking.
        let num_total_programs = (cache_capacity_after_shrink + 1) as u64;
        (0..num_total_programs).for_each(|i| {
            cache.assign_program(
                program,
                new_test_entry_with_usage(i, i + 2, AtomicU64::new(i + 10)),
            );
        });

        cache.sort_and_unload(Percentage::from(evict_to_pct));

        let num_unloaded = num_matching_entries(&cache, |program_type| {
            matches!(program_type, ProgramCacheEntryType::Unloaded(_))
        });
        assert_eq!(num_unloaded, 1);

        cache
            .get_flattened_entries_for_tests()
            .iter()
            .for_each(|(_key, program)| {
                if matches!(program.program, ProgramCacheEntryType::Unloaded(_)) {
                    // Test that the usage counter is retained for the unloaded program
                    assert_eq!(program.tx_usage_counter.load(Ordering::Relaxed), 10);
                    assert_eq!(program.deployment_slot, 0);
                    assert_eq!(program.effective_slot, 2);
                }
            });

        // Replenish the program that was just unloaded. Use 0 as the usage counter. This should be
        // updated with the usage counter from the unloaded program.
        cache.assign_program(program, new_test_entry_with_usage(0, 2, AtomicU64::new(0)));

        cache
            .get_flattened_entries_for_tests()
            .iter()
            .for_each(|(_key, program)| {
                if matches!(program.program, ProgramCacheEntryType::Unloaded(_))
                    && program.deployment_slot == 0
                    && program.effective_slot == 2
                {
                    // Test that the usage counter was correctly updated.
                    assert_eq!(program.tx_usage_counter.load(Ordering::Relaxed), 10);
                }
            });
    }

    #[test]
    fn test_fuzz_assign_program_order() {
        use rand::prelude::SliceRandom;
        const EXPECTED_ENTRIES: [(u64, u64); 7] =
            [(1, 2), (5, 5), (5, 6), (5, 10), (9, 10), (10, 10), (3, 12)];
        let mut rng = rand::thread_rng();
        let program_id = Pubkey::new_unique();
        for _ in 0..1000 {
            let mut entries = EXPECTED_ENTRIES.to_vec();
            entries.shuffle(&mut rng);
            let mut cache = new_mock_cache::<TestForkGraph>();
            for (deployment_slot, effective_slot) in entries {
                assert!(!cache
                    .assign_program(program_id, new_test_entry(deployment_slot, effective_slot)));
            }
            for ((deployment_slot, effective_slot), entry) in EXPECTED_ENTRIES
                .iter()
                .zip(cache.get_slot_versions_for_tests(&program_id).iter())
            {
                assert_eq!(entry.deployment_slot, *deployment_slot);
                assert_eq!(entry.effective_slot, *effective_slot);
            }
        }
    }

    #[test_matrix(
        (
            ProgramCacheEntryType::Closed,
            ProgramCacheEntryType::FailedVerification(get_mock_env()),
            new_loaded_entry(get_mock_env()),
        ),
        (
            ProgramCacheEntryType::FailedVerification(get_mock_env()),
            ProgramCacheEntryType::Closed,
            ProgramCacheEntryType::Unloaded(get_mock_env()),
            new_loaded_entry(get_mock_env()),
            ProgramCacheEntryType::Builtin(BuiltinProgram::new_mock()),
        )
    )]
    #[test_matrix(
        (
            ProgramCacheEntryType::Unloaded(get_mock_env()),
        ),
        (
            ProgramCacheEntryType::FailedVerification(get_mock_env()),
            ProgramCacheEntryType::Closed,
            ProgramCacheEntryType::Unloaded(get_mock_env()),
            ProgramCacheEntryType::Builtin(BuiltinProgram::new_mock()),
        )
    )]
    #[test_matrix(
        (ProgramCacheEntryType::Builtin(BuiltinProgram::new_mock()),),
        (
            ProgramCacheEntryType::FailedVerification(get_mock_env()),
            ProgramCacheEntryType::Closed,
            ProgramCacheEntryType::Unloaded(get_mock_env()),
            new_loaded_entry(get_mock_env()),
        )
    )]
    #[should_panic(expected = "Unexpected replacement of an entry")]
    fn test_assign_program_failure(old: ProgramCacheEntryType, new: ProgramCacheEntryType) {
        let mut cache = new_mock_cache::<TestForkGraph>();
        let program_id = Pubkey::new_unique();
        assert!(!cache.assign_program(
            program_id,
            Arc::new(ProgramCacheEntry {
                program: old,
                account_owner: ProgramCacheEntryOwner::LoaderV2,
                account_size: 0,
                deployment_slot: 10,
                effective_slot: 11,
                tx_usage_counter: AtomicU64::default(),
                ix_usage_counter: AtomicU64::default(),
                latest_access_slot: AtomicU64::default(),
            }),
        ));
        cache.assign_program(
            program_id,
            Arc::new(ProgramCacheEntry {
                program: new,
                account_owner: ProgramCacheEntryOwner::LoaderV2,
                account_size: 0,
                deployment_slot: 10,
                effective_slot: 11,
                tx_usage_counter: AtomicU64::default(),
                ix_usage_counter: AtomicU64::default(),
                latest_access_slot: AtomicU64::default(),
            }),
        );
    }

    #[test_case(
        ProgramCacheEntryType::Unloaded(Arc::new(BuiltinProgram::new_mock())),
        new_loaded_entry(get_mock_env())
    )]
    #[test_case(
        ProgramCacheEntryType::Builtin(BuiltinProgram::new_mock()),
        ProgramCacheEntryType::Builtin(BuiltinProgram::new_mock())
    )]
    fn test_assign_program_success(old: ProgramCacheEntryType, new: ProgramCacheEntryType) {
        let mut cache = new_mock_cache::<TestForkGraph>();
        let program_id = Pubkey::new_unique();
        assert!(!cache.assign_program(
            program_id,
            Arc::new(ProgramCacheEntry {
                program: old,
                account_owner: ProgramCacheEntryOwner::LoaderV2,
                account_size: 0,
                deployment_slot: 10,
                effective_slot: 11,
                tx_usage_counter: AtomicU64::default(),
                ix_usage_counter: AtomicU64::default(),
                latest_access_slot: AtomicU64::default(),
            }),
        ));
        assert!(!cache.assign_program(
            program_id,
            Arc::new(ProgramCacheEntry {
                program: new,
                account_owner: ProgramCacheEntryOwner::LoaderV2,
                account_size: 0,
                deployment_slot: 10,
                effective_slot: 11,
                tx_usage_counter: AtomicU64::default(),
                ix_usage_counter: AtomicU64::default(),
                latest_access_slot: AtomicU64::default(),
            }),
        ));
    }

    #[test]
    fn test_tombstone() {
        let env = Arc::new(BuiltinProgram::new_mock());
        let tombstone = ProgramCacheEntry::new_tombstone(
            0,
            ProgramCacheEntryOwner::LoaderV2,
            ProgramCacheEntryType::FailedVerification(env.clone()),
        );
        assert_matches!(
            tombstone.program,
            ProgramCacheEntryType::FailedVerification(_)
        );
        assert!(tombstone.is_tombstone());
        assert_eq!(tombstone.deployment_slot, 0);
        assert_eq!(tombstone.effective_slot, 0);

        let tombstone = ProgramCacheEntry::new_tombstone(
            100,
            ProgramCacheEntryOwner::LoaderV2,
            ProgramCacheEntryType::Closed,
        );
        assert_matches!(tombstone.program, ProgramCacheEntryType::Closed);
        assert!(tombstone.is_tombstone());
        assert_eq!(tombstone.deployment_slot, 100);
        assert_eq!(tombstone.effective_slot, 100);

        let mut cache = new_mock_cache::<TestForkGraph>();
        let program1 = Pubkey::new_unique();
        let tombstone = set_tombstone(
            &mut cache,
            program1,
            10,
            ProgramCacheEntryType::FailedVerification(env.clone()),
        );
        let slot_versions = cache.get_slot_versions_for_tests(&program1);
        assert_eq!(slot_versions.len(), 1);
        assert!(slot_versions.first().unwrap().is_tombstone());
        assert_eq!(tombstone.deployment_slot, 10);
        assert_eq!(tombstone.effective_slot, 10);

        // Add a program at slot 50, and a tombstone for the program at slot 60
        let program2 = Pubkey::new_unique();
        cache.assign_program(program2, new_test_builtin_entry(50, 51));
        let slot_versions = cache.get_slot_versions_for_tests(&program2);
        assert_eq!(slot_versions.len(), 1);
        assert!(!slot_versions.first().unwrap().is_tombstone());

        let tombstone = set_tombstone(
            &mut cache,
            program2,
            60,
            ProgramCacheEntryType::FailedVerification(env),
        );
        let slot_versions = cache.get_slot_versions_for_tests(&program2);
        assert_eq!(slot_versions.len(), 2);
        assert!(!slot_versions.first().unwrap().is_tombstone());
        assert!(slot_versions.get(1).unwrap().is_tombstone());
        assert!(tombstone.is_tombstone());
        assert_eq!(tombstone.deployment_slot, 60);
        assert_eq!(tombstone.effective_slot, 60);
    }

    struct TestForkGraph {
        relation: BlockRelation,
    }
    impl ForkGraph for TestForkGraph {
        fn relationship(&self, _a: Slot, _b: Slot) -> BlockRelation {
            self.relation
        }
    }

    #[test]
    fn test_prune_empty() {
        let mut cache = new_mock_cache::<TestForkGraph>();
        let fork_graph = Arc::new(RwLock::new(TestForkGraph {
            relation: BlockRelation::Unrelated,
        }));

        cache.set_fork_graph(Arc::downgrade(&fork_graph));

        cache.prune(0, 0);
        assert!(cache.get_flattened_entries_for_tests().is_empty());

        cache.prune(10, 0);
        assert!(cache.get_flattened_entries_for_tests().is_empty());

        let mut cache = new_mock_cache::<TestForkGraph>();
        let fork_graph = Arc::new(RwLock::new(TestForkGraph {
            relation: BlockRelation::Ancestor,
        }));

        cache.set_fork_graph(Arc::downgrade(&fork_graph));

        cache.prune(0, 0);
        assert!(cache.get_flattened_entries_for_tests().is_empty());

        cache.prune(10, 0);
        assert!(cache.get_flattened_entries_for_tests().is_empty());

        let mut cache = new_mock_cache::<TestForkGraph>();
        let fork_graph = Arc::new(RwLock::new(TestForkGraph {
            relation: BlockRelation::Descendant,
        }));

        cache.set_fork_graph(Arc::downgrade(&fork_graph));

        cache.prune(0, 0);
        assert!(cache.get_flattened_entries_for_tests().is_empty());

        cache.prune(10, 0);
        assert!(cache.get_flattened_entries_for_tests().is_empty());

        let mut cache = new_mock_cache::<TestForkGraph>();
        let fork_graph = Arc::new(RwLock::new(TestForkGraph {
            relation: BlockRelation::Unknown,
        }));
        cache.set_fork_graph(Arc::downgrade(&fork_graph));

        cache.prune(0, 0);
        assert!(cache.get_flattened_entries_for_tests().is_empty());

        cache.prune(10, 0);
        assert!(cache.get_flattened_entries_for_tests().is_empty());
    }

    #[test]
    fn test_prune_different_env() {
        let mut cache = new_mock_cache::<TestForkGraph>();

        let fork_graph = Arc::new(RwLock::new(TestForkGraph {
            relation: BlockRelation::Ancestor,
        }));

        cache.set_fork_graph(Arc::downgrade(&fork_graph));

        let program1 = Pubkey::new_unique();
        cache.assign_program(program1, new_test_entry(10, 10));

        let new_env = Arc::new(BuiltinProgram::new_mock());
        cache.upcoming_environments = Some(ProgramRuntimeEnvironments {
            program_runtime_v1: new_env.clone(),
            program_runtime_v2: new_env.clone(),
        });
        let updated_program = Arc::new(ProgramCacheEntry {
            program: new_loaded_entry(new_env.clone()),
            account_owner: ProgramCacheEntryOwner::LoaderV2,
            account_size: 0,
            deployment_slot: 20,
            effective_slot: 20,
            tx_usage_counter: AtomicU64::default(),
            ix_usage_counter: AtomicU64::default(),
            latest_access_slot: AtomicU64::default(),
        });
        cache.assign_program(program1, updated_program.clone());

        // Test that there are 2 entries for the program
        assert_eq!(cache.get_slot_versions_for_tests(&program1).len(), 2);

        cache.prune(21, cache.latest_root_epoch);

        // Test that prune didn't remove the entry, since environments are different.
        assert_eq!(cache.get_slot_versions_for_tests(&program1).len(), 2);

        cache.prune(22, cache.latest_root_epoch.saturating_add(1));

        // Test that prune removed 1 entry, since epoch changed
        assert_eq!(cache.get_slot_versions_for_tests(&program1).len(), 1);

        let entry = cache
            .get_slot_versions_for_tests(&program1)
            .first()
            .expect("Failed to get the program")
            .clone();
        // Test that the correct entry remains in the cache
        assert_eq!(entry, updated_program);
    }

    #[derive(Default)]
    struct TestForkGraphSpecific {
        forks: Vec<Vec<Slot>>,
    }

    impl TestForkGraphSpecific {
        fn insert_fork(&mut self, fork: &[Slot]) {
            let mut fork = fork.to_vec();
            fork.sort();
            self.forks.push(fork)
        }
    }

    impl ForkGraph for TestForkGraphSpecific {
        fn relationship(&self, a: Slot, b: Slot) -> BlockRelation {
            match self.forks.iter().try_for_each(|fork| {
                let relation = fork
                    .iter()
                    .position(|x| *x == a)
                    .and_then(|a_pos| {
                        fork.iter().position(|x| *x == b).and_then(|b_pos| {
                            (a_pos == b_pos)
                                .then_some(BlockRelation::Equal)
                                .or_else(|| (a_pos < b_pos).then_some(BlockRelation::Ancestor))
                                .or(Some(BlockRelation::Descendant))
                        })
                    })
                    .unwrap_or(BlockRelation::Unrelated);

                if relation != BlockRelation::Unrelated {
                    return ControlFlow::Break(relation);
                }

                ControlFlow::Continue(())
            }) {
                ControlFlow::Break(relation) => relation,
                _ => BlockRelation::Unrelated,
            }
        }
    }

    fn get_entries_to_load(
        cache: &ProgramCache<TestForkGraphSpecific>,
        loading_slot: Slot,
        keys: &[Pubkey],
    ) -> Vec<(Pubkey, (ProgramCacheMatchCriteria, u64))> {
        let fork_graph = cache.fork_graph.as_ref().unwrap().upgrade().unwrap();
        let locked_fork_graph = fork_graph.read().unwrap();
        let entries = cache.get_flattened_entries_for_tests();
        keys.iter()
            .filter_map(|key| {
                entries
                    .iter()
                    .rev()
                    .find(|(program_id, entry)| {
                        program_id == key
                            && matches!(
                                locked_fork_graph.relationship(entry.deployment_slot, loading_slot),
                                BlockRelation::Equal | BlockRelation::Ancestor,
                            )
                    })
                    .map(|(program_id, _entry)| {
                        (*program_id, (ProgramCacheMatchCriteria::NoCriteria, 1))
                    })
            })
            .collect()
    }

    fn match_slot(
        extracted: &ProgramCacheForTxBatch,
        program: &Pubkey,
        deployment_slot: Slot,
        working_slot: Slot,
    ) -> bool {
        assert_eq!(extracted.slot, working_slot);
        extracted
            .entries
            .get(program)
            .map(|entry| entry.deployment_slot == deployment_slot)
            .unwrap_or(false)
    }

    fn match_missing(
        missing: &[(Pubkey, (ProgramCacheMatchCriteria, u64))],
        program: &Pubkey,
        expected_result: bool,
    ) -> bool {
        missing.iter().any(|(key, _)| key == program) == expected_result
    }

    #[test]
    fn test_fork_extract_and_prune() {
        let mut cache = new_mock_cache::<TestForkGraphSpecific>();

        // Fork graph created for the test
        //                   0
        //                 /   \
        //                10    5
        //                |     |
        //                20    11
        //                |     | \
        //                22   15  25
        //                      |   |
        //                     16  27
        //                      |
        //                     19
        //                      |
        //                     23

        let mut fork_graph = TestForkGraphSpecific::default();
        fork_graph.insert_fork(&[0, 10, 20, 22]);
        fork_graph.insert_fork(&[0, 5, 11, 15, 16, 18, 19, 21, 23]);
        fork_graph.insert_fork(&[0, 5, 11, 25, 27]);

        let fork_graph = Arc::new(RwLock::new(fork_graph));
        cache.set_fork_graph(Arc::downgrade(&fork_graph));

        let program1 = Pubkey::new_unique();
        cache.assign_program(program1, new_test_entry(0, 1));
        cache.assign_program(program1, new_test_entry(10, 11));
        cache.assign_program(program1, new_test_entry(20, 21));

        let program2 = Pubkey::new_unique();
        cache.assign_program(program2, new_test_entry(5, 6));
        cache.assign_program(
            program2,
            new_test_entry(11, 11 + DELAY_VISIBILITY_SLOT_OFFSET),
        );

        let program3 = Pubkey::new_unique();
        cache.assign_program(program3, new_test_entry(25, 26));

        let program4 = Pubkey::new_unique();
        cache.assign_program(program4, new_test_entry(0, 1));
        cache.assign_program(program4, new_test_entry(5, 6));
        // The following is a special case, where effective slot is 3 slots in the future
        cache.assign_program(
            program4,
            new_test_entry(15, 15 + DELAY_VISIBILITY_SLOT_OFFSET),
        );

        // Current fork graph
        //                   0
        //                 /   \
        //                10    5
        //                |     |
        //                20    11
        //                |     | \
        //                22   15  25
        //                      |   |
        //                     16  27
        //                      |
        //                     19
        //                      |
        //                     23

        // Testing fork 0 - 10 - 12 - 22 with current slot at 22
        let mut missing =
            get_entries_to_load(&cache, 22, &[program1, program2, program3, program4]);
        assert!(match_missing(&missing, &program2, false));
        assert!(match_missing(&missing, &program3, false));
        let mut extracted = ProgramCacheForTxBatch::new(22, cache.environments.clone(), None, 0);
        cache.extract(&mut missing, &mut extracted, true);
        assert!(match_slot(&extracted, &program1, 20, 22));
        assert!(match_slot(&extracted, &program4, 0, 22));

        // Testing fork 0 - 5 - 11 - 15 - 16 with current slot at 15
        let mut missing =
            get_entries_to_load(&cache, 15, &[program1, program2, program3, program4]);
        assert!(match_missing(&missing, &program3, false));
        let mut extracted = ProgramCacheForTxBatch::new(15, cache.environments.clone(), None, 0);
        cache.extract(&mut missing, &mut extracted, true);
        assert!(match_slot(&extracted, &program1, 0, 15));
        assert!(match_slot(&extracted, &program2, 11, 15));
        // The effective slot of program4 deployed in slot 15 is 19. So it should not be usable in slot 16.
        // A delay visibility tombstone should be returned here.
        let tombstone = extracted
            .find(&program4)
            .expect("Failed to find the tombstone");
        assert_matches!(tombstone.program, ProgramCacheEntryType::DelayVisibility);
        assert_eq!(tombstone.deployment_slot, 15);

        // Testing the same fork above, but current slot is now 18 (equal to effective slot of program4).
        let mut missing =
            get_entries_to_load(&cache, 18, &[program1, program2, program3, program4]);
        assert!(match_missing(&missing, &program3, false));
        let mut extracted = ProgramCacheForTxBatch::new(18, cache.environments.clone(), None, 0);
        cache.extract(&mut missing, &mut extracted, true);
        assert!(match_slot(&extracted, &program1, 0, 18));
        assert!(match_slot(&extracted, &program2, 11, 18));
        // The effective slot of program4 deployed in slot 15 is 18. So it should be usable in slot 18.
        assert!(match_slot(&extracted, &program4, 15, 18));

        // Testing the same fork above, but current slot is now 23 (future slot than effective slot of program4).
        let mut missing =
            get_entries_to_load(&cache, 23, &[program1, program2, program3, program4]);
        assert!(match_missing(&missing, &program3, false));
        let mut extracted = ProgramCacheForTxBatch::new(23, cache.environments.clone(), None, 0);
        cache.extract(&mut missing, &mut extracted, true);
        assert!(match_slot(&extracted, &program1, 0, 23));
        assert!(match_slot(&extracted, &program2, 11, 23));
        // The effective slot of program4 deployed in slot 15 is 19. So it should be usable in slot 23.
        assert!(match_slot(&extracted, &program4, 15, 23));

        // Testing fork 0 - 5 - 11 - 15 - 16 with current slot at 11
        let mut missing =
            get_entries_to_load(&cache, 11, &[program1, program2, program3, program4]);
        assert!(match_missing(&missing, &program3, false));
        let mut extracted = ProgramCacheForTxBatch::new(11, cache.environments.clone(), None, 0);
        cache.extract(&mut missing, &mut extracted, true);
        assert!(match_slot(&extracted, &program1, 0, 11));
        // program2 was updated at slot 11, but is not effective till slot 12. The result should contain a tombstone.
        let tombstone = extracted
            .find(&program2)
            .expect("Failed to find the tombstone");
        assert_matches!(tombstone.program, ProgramCacheEntryType::DelayVisibility);
        assert_eq!(tombstone.deployment_slot, 11);
        assert!(match_slot(&extracted, &program4, 5, 11));

        cache.prune(5, 0);

        // Fork graph after pruning
        //                   0
        //                   |
        //                   5
        //                   |
        //                   11
        //                   | \
        //                  15  25
        //                   |   |
        //                  16  27
        //                   |
        //                  19
        //                   |
        //                  23

        // Testing fork 11 - 15 - 16- 19 - 22 with root at 5 and current slot at 22
        let mut missing =
            get_entries_to_load(&cache, 21, &[program1, program2, program3, program4]);
        assert!(match_missing(&missing, &program3, false));
        let mut extracted = ProgramCacheForTxBatch::new(21, cache.environments.clone(), None, 0);
        cache.extract(&mut missing, &mut extracted, true);
        // Since the fork was pruned, we should not find the entry deployed at slot 20.
        assert!(match_slot(&extracted, &program1, 0, 21));
        assert!(match_slot(&extracted, &program2, 11, 21));
        assert!(match_slot(&extracted, &program4, 15, 21));

        // Testing fork 0 - 5 - 11 - 25 - 27 with current slot at 27
        let mut missing =
            get_entries_to_load(&cache, 27, &[program1, program2, program3, program4]);
        let mut extracted = ProgramCacheForTxBatch::new(27, cache.environments.clone(), None, 0);
        cache.extract(&mut missing, &mut extracted, true);
        assert!(match_slot(&extracted, &program1, 0, 27));
        assert!(match_slot(&extracted, &program2, 11, 27));
        assert!(match_slot(&extracted, &program3, 25, 27));
        assert!(match_slot(&extracted, &program4, 5, 27));

        cache.prune(15, 0);

        // Fork graph after pruning
        //                  0
        //                  |
        //                  5
        //                  |
        //                  11
        //                  |
        //                  15
        //                  |
        //                  16
        //                  |
        //                  19
        //                  |
        //                  23

        // Testing fork 16, 19, 23, with root at 15, current slot at 23
        let mut missing =
            get_entries_to_load(&cache, 23, &[program1, program2, program3, program4]);
        assert!(match_missing(&missing, &program3, false));
        let mut extracted = ProgramCacheForTxBatch::new(23, cache.environments.clone(), None, 0);
        cache.extract(&mut missing, &mut extracted, true);
        assert!(match_slot(&extracted, &program1, 0, 23));
        assert!(match_slot(&extracted, &program2, 11, 23));
        assert!(match_slot(&extracted, &program4, 15, 23));
    }

    #[test]
    fn test_extract_using_deployment_slot() {
        let mut cache = new_mock_cache::<TestForkGraphSpecific>();

        // Fork graph created for the test
        //                   0
        //                 /   \
        //                10    5
        //                |     |
        //                20    11
        //                |     | \
        //                22   15  25
        //                      |   |
        //                     16  27
        //                      |
        //                     19
        //                      |
        //                     23

        let mut fork_graph = TestForkGraphSpecific::default();
        fork_graph.insert_fork(&[0, 10, 20, 22]);
        fork_graph.insert_fork(&[0, 5, 11, 12, 15, 16, 18, 19, 21, 23]);
        fork_graph.insert_fork(&[0, 5, 11, 25, 27]);

        let fork_graph = Arc::new(RwLock::new(fork_graph));
        cache.set_fork_graph(Arc::downgrade(&fork_graph));

        let program1 = Pubkey::new_unique();
        cache.assign_program(program1, new_test_entry(0, 1));
        cache.assign_program(program1, new_test_entry(20, 21));

        let program2 = Pubkey::new_unique();
        cache.assign_program(program2, new_test_entry(5, 6));
        cache.assign_program(program2, new_test_entry(11, 12));

        let program3 = Pubkey::new_unique();
        cache.assign_program(program3, new_test_entry(25, 26));

        // Testing fork 0 - 5 - 11 - 15 - 16 - 19 - 21 - 23 with current slot at 19
        let mut missing = get_entries_to_load(&cache, 12, &[program1, program2, program3]);
        assert!(match_missing(&missing, &program3, false));
        let mut extracted = ProgramCacheForTxBatch::new(12, cache.environments.clone(), None, 0);
        cache.extract(&mut missing, &mut extracted, true);
        assert!(match_slot(&extracted, &program1, 0, 12));
        assert!(match_slot(&extracted, &program2, 11, 12));

        // Test the same fork, but request the program modified at a later slot than what's in the cache.
        let mut missing = get_entries_to_load(&cache, 12, &[program1, program2, program3]);
        missing.get_mut(0).unwrap().1 .0 = ProgramCacheMatchCriteria::DeployedOnOrAfterSlot(5);
        missing.get_mut(1).unwrap().1 .0 = ProgramCacheMatchCriteria::DeployedOnOrAfterSlot(5);
        assert!(match_missing(&missing, &program3, false));
        let mut extracted = ProgramCacheForTxBatch::new(12, cache.environments.clone(), None, 0);
        cache.extract(&mut missing, &mut extracted, true);
        assert!(match_missing(&missing, &program1, true));
        assert!(match_slot(&extracted, &program2, 11, 12));
    }

    #[test]
    fn test_extract_unloaded() {
        let mut cache = new_mock_cache::<TestForkGraphSpecific>();

        // Fork graph created for the test
        //                   0
        //                 /   \
        //                10    5
        //                |     |
        //                20    11
        //                |     | \
        //                22   15  25
        //                      |   |
        //                     16  27
        //                      |
        //                     19
        //                      |
        //                     23

        let mut fork_graph = TestForkGraphSpecific::default();
        fork_graph.insert_fork(&[0, 10, 20, 22]);
        fork_graph.insert_fork(&[0, 5, 11, 15, 16, 19, 21, 23]);
        fork_graph.insert_fork(&[0, 5, 11, 25, 27]);

        let fork_graph = Arc::new(RwLock::new(fork_graph));
        cache.set_fork_graph(Arc::downgrade(&fork_graph));

        let program1 = Pubkey::new_unique();
        cache.assign_program(program1, new_test_entry(0, 1));
        cache.assign_program(program1, new_test_entry(20, 21));

        let program2 = Pubkey::new_unique();
        cache.assign_program(program2, new_test_entry(5, 6));
        cache.assign_program(program2, new_test_entry(11, 12));

        let program3 = Pubkey::new_unique();
        // Insert an unloaded program with correct/cache's environment at slot 25
        let _ = insert_unloaded_entry(&mut cache, program3, 25);

        // Insert another unloaded program with a different environment at slot 20
        // Since this entry's environment won't match cache's environment, looking up this
        // entry should return missing instead of unloaded entry.
        cache.assign_program(
            program3,
            Arc::new(
                new_test_entry(20, 21)
                    .to_unloaded()
                    .expect("Failed to create unloaded program"),
            ),
        );

        // Testing fork 0 - 5 - 11 - 15 - 16 - 19 - 21 - 23 with current slot at 19
        let mut missing = get_entries_to_load(&cache, 19, &[program1, program2, program3]);
        assert!(match_missing(&missing, &program3, false));
        let mut extracted = ProgramCacheForTxBatch::new(19, cache.environments.clone(), None, 0);
        cache.extract(&mut missing, &mut extracted, true);
        assert!(match_slot(&extracted, &program1, 0, 19));
        assert!(match_slot(&extracted, &program2, 11, 19));

        // Testing fork 0 - 5 - 11 - 25 - 27 with current slot at 27
        let mut missing = get_entries_to_load(&cache, 27, &[program1, program2, program3]);
        let mut extracted = ProgramCacheForTxBatch::new(27, cache.environments.clone(), None, 0);
        cache.extract(&mut missing, &mut extracted, true);
        assert!(match_slot(&extracted, &program1, 0, 27));
        assert!(match_slot(&extracted, &program2, 11, 27));
        assert!(match_missing(&missing, &program3, true));

        // Testing fork 0 - 10 - 20 - 22 with current slot at 22
        let mut missing = get_entries_to_load(&cache, 22, &[program1, program2, program3]);
        assert!(match_missing(&missing, &program2, false));
        let mut extracted = ProgramCacheForTxBatch::new(22, cache.environments.clone(), None, 0);
        cache.extract(&mut missing, &mut extracted, true);
        assert!(match_slot(&extracted, &program1, 20, 22));
        assert!(match_missing(&missing, &program3, true));
    }

    #[test]
    fn test_extract_nonexistent() {
        let mut cache = new_mock_cache::<TestForkGraphSpecific>();
        let fork_graph = TestForkGraphSpecific::default();
        let fork_graph = Arc::new(RwLock::new(fork_graph));
        cache.set_fork_graph(Arc::downgrade(&fork_graph));

        let program1 = Pubkey::new_unique();
        let mut missing = vec![(program1, (ProgramCacheMatchCriteria::NoCriteria, 1))];
        let mut extracted = ProgramCacheForTxBatch::new(0, cache.environments.clone(), None, 0);
        cache.extract(&mut missing, &mut extracted, true);
        assert!(match_missing(&missing, &program1, true));
    }

    #[test]
    fn test_unloaded() {
        let mut cache = new_mock_cache::<TestForkGraph>();
        for program_cache_entry_type in [
            ProgramCacheEntryType::FailedVerification(
                cache.environments.program_runtime_v1.clone(),
            ),
            ProgramCacheEntryType::Closed,
            ProgramCacheEntryType::Unloaded(cache.environments.program_runtime_v1.clone()),
            ProgramCacheEntryType::Builtin(BuiltinProgram::new_mock()),
        ] {
            let entry = Arc::new(ProgramCacheEntry {
                program: program_cache_entry_type,
                account_owner: ProgramCacheEntryOwner::LoaderV2,
                account_size: 0,
                deployment_slot: 0,
                effective_slot: 0,
                tx_usage_counter: AtomicU64::default(),
                ix_usage_counter: AtomicU64::default(),
                latest_access_slot: AtomicU64::default(),
            });
            assert!(entry.to_unloaded().is_none());

            // Check that unload_program_entry() does nothing for this entry
            let program_id = Pubkey::new_unique();
            cache.assign_program(program_id, entry.clone());
            cache.unload_program_entry(&program_id, &entry);
            assert_eq!(cache.get_slot_versions_for_tests(&program_id).len(), 1);
            assert!(cache.stats.evictions.is_empty());
        }

        let entry = new_test_entry_with_usage(1, 2, AtomicU64::new(3));
        let unloaded_entry = entry.to_unloaded().unwrap();
        assert_eq!(unloaded_entry.deployment_slot, 1);
        assert_eq!(unloaded_entry.effective_slot, 2);
        assert_eq!(unloaded_entry.latest_access_slot.load(Ordering::Relaxed), 1);
        assert_eq!(unloaded_entry.tx_usage_counter.load(Ordering::Relaxed), 3);

        // Check that unload_program_entry() does its work
        let program_id = Pubkey::new_unique();
        cache.assign_program(program_id, entry.clone());
        cache.unload_program_entry(&program_id, &entry);
        assert!(cache.stats.evictions.contains_key(&program_id));
    }

    #[test]
    fn test_fork_prune_find_first_ancestor() {
        let mut cache = new_mock_cache::<TestForkGraphSpecific>();

        // Fork graph created for the test
        //                   0
        //                 /   \
        //                10    5
        //                |
        //                20

        // Deploy program on slot 0, and slot 5.
        // Prune the fork that has slot 5. The cache should still have the program
        // deployed at slot 0.
        let mut fork_graph = TestForkGraphSpecific::default();
        fork_graph.insert_fork(&[0, 10, 20]);
        fork_graph.insert_fork(&[0, 5]);
        let fork_graph = Arc::new(RwLock::new(fork_graph));
        cache.set_fork_graph(Arc::downgrade(&fork_graph));

        let program1 = Pubkey::new_unique();
        cache.assign_program(program1, new_test_entry(0, 1));
        cache.assign_program(program1, new_test_entry(5, 6));

        cache.prune(10, 0);

        let mut missing = get_entries_to_load(&cache, 20, &[program1]);
        let mut extracted = ProgramCacheForTxBatch::new(20, cache.environments.clone(), None, 0);
        cache.extract(&mut missing, &mut extracted, true);

        // The cache should have the program deployed at slot 0
        assert_eq!(
            extracted
                .find(&program1)
                .expect("Did not find the program")
                .deployment_slot,
            0
        );
    }

    #[test]
    fn test_prune_by_deployment_slot() {
        let mut cache = new_mock_cache::<TestForkGraphSpecific>();

        // Fork graph created for the test
        //                   0
        //                 /   \
        //                10    5
        //                |
        //                20

        // Deploy program on slot 0, and slot 5.
        // Prune the fork that has slot 5. The cache should still have the program
        // deployed at slot 0.
        let mut fork_graph = TestForkGraphSpecific::default();
        fork_graph.insert_fork(&[0, 10, 20]);
        fork_graph.insert_fork(&[0, 5, 6]);
        let fork_graph = Arc::new(RwLock::new(fork_graph));
        cache.set_fork_graph(Arc::downgrade(&fork_graph));

        let program1 = Pubkey::new_unique();
        cache.assign_program(program1, new_test_entry(0, 1));
        cache.assign_program(program1, new_test_entry(5, 6));

        let program2 = Pubkey::new_unique();
        cache.assign_program(program2, new_test_entry(10, 11));

        let mut missing = get_entries_to_load(&cache, 20, &[program1, program2]);
        let mut extracted = ProgramCacheForTxBatch::new(20, cache.environments.clone(), None, 0);
        cache.extract(&mut missing, &mut extracted, true);
        assert!(match_slot(&extracted, &program1, 0, 20));
        assert!(match_slot(&extracted, &program2, 10, 20));

        let mut missing = get_entries_to_load(&cache, 6, &[program1, program2]);
        assert!(match_missing(&missing, &program2, false));
        let mut extracted = ProgramCacheForTxBatch::new(6, cache.environments.clone(), None, 0);
        cache.extract(&mut missing, &mut extracted, true);
        assert!(match_slot(&extracted, &program1, 5, 6));

        // Pruning slot 5 will remove program1 entry deployed at slot 5.
        // On fork chaining from slot 5, the entry deployed at slot 0 will become visible.
        cache.prune_by_deployment_slot(5);

        let mut missing = get_entries_to_load(&cache, 20, &[program1, program2]);
        let mut extracted = ProgramCacheForTxBatch::new(20, cache.environments.clone(), None, 0);
        cache.extract(&mut missing, &mut extracted, true);
        assert!(match_slot(&extracted, &program1, 0, 20));
        assert!(match_slot(&extracted, &program2, 10, 20));

        let mut missing = get_entries_to_load(&cache, 6, &[program1, program2]);
        assert!(match_missing(&missing, &program2, false));
        let mut extracted = ProgramCacheForTxBatch::new(6, cache.environments.clone(), None, 0);
        cache.extract(&mut missing, &mut extracted, true);
        assert!(match_slot(&extracted, &program1, 0, 6));

        // Pruning slot 10 will remove program2 entry deployed at slot 10.
        // As there is no other entry for program2, extract() will return it as missing.
        cache.prune_by_deployment_slot(10);

        let mut missing = get_entries_to_load(&cache, 20, &[program1, program2]);
        assert!(match_missing(&missing, &program2, false));
        let mut extracted = ProgramCacheForTxBatch::new(20, cache.environments.clone(), None, 0);
        cache.extract(&mut missing, &mut extracted, true);
        assert!(match_slot(&extracted, &program1, 0, 20));
    }

    #[test]
    fn test_usable_entries_for_slot() {
        new_mock_cache::<TestForkGraph>();
        let tombstone = Arc::new(ProgramCacheEntry::new_tombstone(
            0,
            ProgramCacheEntryOwner::LoaderV2,
            ProgramCacheEntryType::Closed,
        ));

        assert!(ProgramCache::<TestForkGraph>::matches_criteria(
            &tombstone,
            &ProgramCacheMatchCriteria::NoCriteria
        ));

        assert!(ProgramCache::<TestForkGraph>::matches_criteria(
            &tombstone,
            &ProgramCacheMatchCriteria::Tombstone
        ));

        assert!(ProgramCache::<TestForkGraph>::matches_criteria(
            &tombstone,
            &ProgramCacheMatchCriteria::DeployedOnOrAfterSlot(0)
        ));

        assert!(!ProgramCache::<TestForkGraph>::matches_criteria(
            &tombstone,
            &ProgramCacheMatchCriteria::DeployedOnOrAfterSlot(1)
        ));

        let program = new_test_entry(0, 1);

        assert!(ProgramCache::<TestForkGraph>::matches_criteria(
            &program,
            &ProgramCacheMatchCriteria::NoCriteria
        ));

        assert!(!ProgramCache::<TestForkGraph>::matches_criteria(
            &program,
            &ProgramCacheMatchCriteria::Tombstone
        ));

        assert!(ProgramCache::<TestForkGraph>::matches_criteria(
            &program,
            &ProgramCacheMatchCriteria::DeployedOnOrAfterSlot(0)
        ));

        assert!(!ProgramCache::<TestForkGraph>::matches_criteria(
            &program,
            &ProgramCacheMatchCriteria::DeployedOnOrAfterSlot(1)
        ));

        let program = Arc::new(new_test_entry_with_usage(0, 1, AtomicU64::default()));

        assert!(ProgramCache::<TestForkGraph>::matches_criteria(
            &program,
            &ProgramCacheMatchCriteria::NoCriteria
        ));

        assert!(!ProgramCache::<TestForkGraph>::matches_criteria(
            &program,
            &ProgramCacheMatchCriteria::Tombstone
        ));

        assert!(ProgramCache::<TestForkGraph>::matches_criteria(
            &program,
            &ProgramCacheMatchCriteria::DeployedOnOrAfterSlot(0)
        ));

        assert!(!ProgramCache::<TestForkGraph>::matches_criteria(
            &program,
            &ProgramCacheMatchCriteria::DeployedOnOrAfterSlot(1)
        ));
    }
}

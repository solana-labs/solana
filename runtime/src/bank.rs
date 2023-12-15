//! The `bank` module tracks client accounts and the progress of on-chain
//! programs.
//!
//! A single bank relates to a block produced by a single leader and each bank
//! except for the genesis bank points back to a parent bank.
//!
//! The bank is the main entrypoint for processing verified transactions with the function
//! `Bank::process_transactions`
//!
//! It does this by loading the accounts using the reference it holds on the account store,
//! and then passing those to an InvokeContext which handles loading the programs specified
//! by the Transaction and executing it.
//!
//! The bank then stores the results to the accounts store.
//!
//! It then has APIs for retrieving if a transaction has been processed and it's status.
//! See `get_signature_status` et al.
//!
//! Bank lifecycle:
//!
//! A bank is newly created and open to transactions. Transactions are applied
//! until either the bank reached the tick count when the node is the leader for that slot, or the
//! node has applied all transactions present in all `Entry`s in the slot.
//!
//! Once it is complete, the bank can then be frozen. After frozen, no more transactions can
//! be applied or state changes made. At the frozen step, rent will be applied and various
//! sysvar special accounts update to the new state of the system.
//!
//! After frozen, and the bank has had the appropriate number of votes on it, then it can become
//! rooted. At this point, it will not be able to be removed from the chain and the
//! state is finalized.
//!
//! It offers a high-level API that signs transactions
//! on behalf of the caller, and a low-level API for when they have
//! already been signed and verified.
#[allow(deprecated)]
use solana_sdk::recent_blockhashes_account;
pub use solana_sdk::reward_type::RewardType;
use {
    crate::{
        bank::metrics::*,
        bank_forks::BankForks,
        builtins::{BuiltinPrototype, BUILTINS},
        epoch_rewards_hasher::hash_rewards_into_partitions,
        epoch_stakes::{EpochStakes, NodeVoteAccounts},
        runtime_config::RuntimeConfig,
        serde_snapshot::BankIncrementalSnapshotPersistence,
        snapshot_hash::SnapshotHash,
        stake_account::StakeAccount,
        stake_history::StakeHistory,
        stake_weighted_timestamp::{
            calculate_stake_weighted_timestamp, MaxAllowableDrift,
            MAX_ALLOWABLE_DRIFT_PERCENTAGE_FAST, MAX_ALLOWABLE_DRIFT_PERCENTAGE_SLOW_V2,
        },
        stakes::{InvalidCacheEntryReason, Stakes, StakesCache, StakesEnum},
        status_cache::{SlotDelta, StatusCache},
        transaction_batch::TransactionBatch,
    },
    byteorder::{ByteOrder, LittleEndian},
    dashmap::{DashMap, DashSet},
    itertools::izip,
    log::*,
    percentage::Percentage,
    rayon::{
        iter::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator},
        slice::ParallelSlice,
        ThreadPool, ThreadPoolBuilder,
    },
    solana_accounts_db::{
        account_overrides::AccountOverrides,
        accounts::{
            AccountAddressFilter, Accounts, LoadedTransaction, PubkeyAccountSlot, RewardInterval,
            TransactionLoadResult,
        },
        accounts_db::{
            AccountShrinkThreshold, AccountStorageEntry, AccountsDbConfig,
            CalcAccountsHashDataSource, IncludeSlotInHash, VerifyAccountsHashAndLamportsConfig,
            ACCOUNTS_DB_CONFIG_FOR_BENCHMARKS, ACCOUNTS_DB_CONFIG_FOR_TESTING,
        },
        accounts_hash::{AccountsHash, CalcAccountsHashConfig, HashStats, IncrementalAccountsHash},
        accounts_index::{AccountSecondaryIndexes, IndexKey, ScanConfig, ScanResult, ZeroLamport},
        accounts_partition::{self, Partition, PartitionIndex},
        accounts_update_notifier_interface::AccountsUpdateNotifier,
        ancestors::{Ancestors, AncestorsForSerialization},
        blockhash_queue::BlockhashQueue,
        epoch_accounts_hash::EpochAccountsHash,
        nonce_info::{NonceInfo, NoncePartial},
        partitioned_rewards::PartitionedEpochRewardsConfig,
        rent_collector::{CollectedInfo, RentCollector},
        rent_debits::RentDebits,
        sorted_storages::SortedStorages,
        stake_rewards::{RewardInfo, StakeReward},
        storable_accounts::StorableAccounts,
        transaction_error_metrics::TransactionErrorMetrics,
        transaction_results::{
            inner_instructions_list_from_instruction_trace, DurableNonceFee,
            TransactionCheckResult, TransactionExecutionDetails, TransactionExecutionResult,
            TransactionResults,
        },
    },
    solana_bpf_loader_program::syscalls::create_program_runtime_environment_v1,
    solana_cost_model::cost_tracker::CostTracker,
    solana_loader_v4_program::create_program_runtime_environment_v2,
    solana_measure::{measure, measure::Measure, measure_us},
    solana_perf::perf_libs,
    solana_program_runtime::{
        accounts_data_meter::MAX_ACCOUNTS_DATA_LEN,
        compute_budget::{self, ComputeBudget},
        invoke_context::BuiltinFunctionWithContext,
        loaded_programs::{
            LoadProgramMetrics, LoadedProgram, LoadedProgramMatchCriteria, LoadedProgramType,
            LoadedPrograms, LoadedProgramsForTxBatch, WorkingSlot, DELAY_VISIBILITY_SLOT_OFFSET,
        },
        log_collector::LogCollector,
        message_processor::MessageProcessor,
        sysvar_cache::SysvarCache,
        timings::{ExecuteDetailsTimings, ExecuteTimingType, ExecuteTimings},
    },
    solana_sdk::{
        account::{
            create_account_shared_data_with_fields as create_account, from_account, Account,
            AccountSharedData, InheritableAccountFields, ReadableAccount, WritableAccount,
        },
        account_utils::StateMut,
        bpf_loader, bpf_loader_deprecated,
        bpf_loader_upgradeable::{self, UpgradeableLoaderState},
        clock::{
            BankId, Epoch, Slot, SlotCount, SlotIndex, UnixTimestamp, DEFAULT_HASHES_PER_TICK,
            DEFAULT_TICKS_PER_SECOND, INITIAL_RENT_EPOCH, MAX_PROCESSING_AGE,
            MAX_TRANSACTION_FORWARDING_DELAY, MAX_TRANSACTION_FORWARDING_DELAY_GPU,
            SECONDS_PER_DAY, UPDATED_HASHES_PER_TICK2, UPDATED_HASHES_PER_TICK3,
            UPDATED_HASHES_PER_TICK4, UPDATED_HASHES_PER_TICK5, UPDATED_HASHES_PER_TICK6,
        },
        epoch_info::EpochInfo,
        epoch_schedule::EpochSchedule,
        feature,
        feature_set::{
            self, add_set_tx_loaded_accounts_data_size_instruction,
            enable_early_verification_of_account_modifications,
            include_loaded_accounts_data_size_in_fee_calculation,
            remove_congestion_multiplier_from_fee_calculation, remove_deprecated_request_unit_ix,
            FeatureSet,
        },
        fee::FeeStructure,
        fee_calculator::{FeeCalculator, FeeRateGovernor},
        genesis_config::{ClusterType, GenesisConfig},
        hard_forks::HardForks,
        hash::{extend_and_hash, hashv, Hash},
        incinerator,
        inflation::Inflation,
        instruction::InstructionError,
        loader_v4::{self, LoaderV4State, LoaderV4Status},
        message::{AccountKeys, SanitizedMessage},
        native_loader,
        native_token::LAMPORTS_PER_SOL,
        nonce::{self, state::DurableNonce, NONCED_TX_MARKER_IX_INDEX},
        nonce_account,
        packet::PACKET_DATA_SIZE,
        precompiles::get_precompiles,
        pubkey::Pubkey,
        saturating_add_assign,
        signature::{Keypair, Signature},
        slot_hashes::SlotHashes,
        slot_history::{Check, SlotHistory},
        stake::state::Delegation,
        system_transaction,
        sysvar::{self, last_restart_slot::LastRestartSlot, Sysvar, SysvarId},
        timing::years_as_slots,
        transaction::{
            self, MessageHash, Result, SanitizedTransaction, Transaction, TransactionError,
            TransactionVerificationMode, VersionedTransaction, MAX_TX_ACCOUNT_LOCKS,
        },
        transaction_context::{
            ExecutionRecord, TransactionAccount, TransactionContext, TransactionReturnData,
        },
    },
    solana_stake_program::stake_state::{
        self, InflationPointCalculationEvent, PointValue, StakeStateV2,
    },
    solana_system_program::{get_system_account_kind, SystemAccountKind},
    solana_vote::vote_account::{VoteAccount, VoteAccounts, VoteAccountsHashMap},
    solana_vote_program::vote_state::VoteState,
    std::{
        borrow::Cow,
        cell::RefCell,
        collections::{HashMap, HashSet},
        convert::TryFrom,
        fmt, mem,
        ops::{AddAssign, RangeInclusive},
        path::PathBuf,
        rc::Rc,
        slice,
        sync::{
            atomic::{
                AtomicBool, AtomicI64, AtomicU64, AtomicUsize,
                Ordering::{self, AcqRel, Acquire, Relaxed},
            },
            Arc, LockResult, RwLock, RwLockReadGuard, RwLockWriteGuard,
        },
        thread::Builder,
        time::{Duration, Instant},
    },
};

/// params to `verify_accounts_hash`
struct VerifyAccountsHashConfig {
    test_hash_calculation: bool,
    ignore_mismatch: bool,
    require_rooted_bank: bool,
    run_in_background: bool,
    store_hash_raw_data_for_debug: bool,
}

mod address_lookup_table;
pub mod bank_hash_details;
mod builtin_programs;
pub mod epoch_accounts_hash_utils;
mod fee_distribution;
mod metrics;
mod serde_snapshot;
mod sysvar_cache;
#[cfg(test)]
mod tests;
mod transaction_account_state_info;

pub const SECONDS_PER_YEAR: f64 = 365.25 * 24.0 * 60.0 * 60.0;

pub const MAX_LEADER_SCHEDULE_STAKES: Epoch = 5;

#[derive(Default)]
struct RentMetrics {
    hold_range_us: AtomicU64,
    load_us: AtomicU64,
    collect_us: AtomicU64,
    hash_us: AtomicU64,
    store_us: AtomicU64,
    count: AtomicUsize,
}

pub type BankStatusCache = StatusCache<Result<()>>;
#[frozen_abi(digest = "EzAXfE2xG3ZqdAj8KMC8CeqoSxjo5hxrEaP7fta8LT9u")]
pub type BankSlotDelta = SlotDelta<Result<()>>;

#[derive(Default, Copy, Clone, Debug, PartialEq, Eq)]
pub struct SquashTiming {
    pub squash_accounts_ms: u64,
    pub squash_accounts_cache_ms: u64,
    pub squash_accounts_index_ms: u64,
    pub squash_accounts_store_ms: u64,

    pub squash_cache_ms: u64,
}

impl AddAssign for SquashTiming {
    fn add_assign(&mut self, rhs: Self) {
        self.squash_accounts_ms += rhs.squash_accounts_ms;
        self.squash_accounts_cache_ms += rhs.squash_accounts_cache_ms;
        self.squash_accounts_index_ms += rhs.squash_accounts_index_ms;
        self.squash_accounts_store_ms += rhs.squash_accounts_store_ms;
        self.squash_cache_ms += rhs.squash_cache_ms;
    }
}

#[derive(Debug)]
pub struct BankRc {
    /// where all the Accounts are stored
    pub accounts: Arc<Accounts>,

    /// Previous checkpoint of this bank
    pub(crate) parent: RwLock<Option<Arc<Bank>>>,

    /// Current slot
    pub(crate) slot: Slot,

    pub(crate) bank_id_generator: Arc<AtomicU64>,
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
use solana_frozen_abi::abi_example::AbiExample;

#[cfg(RUSTC_WITH_SPECIALIZATION)]
impl AbiExample for BankRc {
    fn example() -> Self {
        BankRc {
            // Set parent to None to cut the recursion into another Bank
            parent: RwLock::new(None),
            // AbiExample for Accounts is specially implemented to contain a storage example
            accounts: AbiExample::example(),
            slot: AbiExample::example(),
            bank_id_generator: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl BankRc {
    pub(crate) fn new(accounts: Accounts, slot: Slot) -> Self {
        Self {
            accounts: Arc::new(accounts),
            parent: RwLock::new(None),
            slot,
            bank_id_generator: Arc::new(AtomicU64::new(0)),
        }
    }
}

enum ProgramAccountLoadResult {
    AccountNotFound,
    InvalidAccountData,
    InvalidV4Program,
    ProgramOfLoaderV1orV2(AccountSharedData),
    ProgramOfLoaderV3(AccountSharedData, AccountSharedData, Slot),
    ProgramOfLoaderV4(AccountSharedData, Slot),
}

pub struct LoadAndExecuteTransactionsOutput {
    pub loaded_transactions: Vec<TransactionLoadResult>,
    // Vector of results indicating whether a transaction was executed or could not
    // be executed. Note executed transactions can still have failed!
    pub execution_results: Vec<TransactionExecutionResult>,
    pub retryable_transaction_indexes: Vec<usize>,
    // Total number of transactions that were executed
    pub executed_transactions_count: usize,
    // Number of non-vote transactions that were executed
    pub executed_non_vote_transactions_count: usize,
    // Total number of the executed transactions that returned success/not
    // an error.
    pub executed_with_successful_result_count: usize,
    pub signature_count: u64,
    pub error_counters: TransactionErrorMetrics,
}

pub struct TransactionSimulationResult {
    pub result: Result<()>,
    pub logs: TransactionLogMessages,
    pub post_simulation_accounts: Vec<TransactionAccount>,
    pub units_consumed: u64,
    pub return_data: Option<TransactionReturnData>,
}
pub struct TransactionBalancesSet {
    pub pre_balances: TransactionBalances,
    pub post_balances: TransactionBalances,
}

impl TransactionBalancesSet {
    pub fn new(pre_balances: TransactionBalances, post_balances: TransactionBalances) -> Self {
        assert_eq!(pre_balances.len(), post_balances.len());
        Self {
            pre_balances,
            post_balances,
        }
    }
}
pub type TransactionBalances = Vec<Vec<u64>>;

/// A list of log messages emitted during a transaction
pub type TransactionLogMessages = Vec<String>;

#[derive(Serialize, Deserialize, AbiExample, AbiEnumVisitor, Debug, PartialEq, Eq)]
pub enum TransactionLogCollectorFilter {
    All,
    AllWithVotes,
    None,
    OnlyMentionedAddresses,
}

impl Default for TransactionLogCollectorFilter {
    fn default() -> Self {
        Self::None
    }
}

#[derive(AbiExample, Debug, Default)]
pub struct TransactionLogCollectorConfig {
    pub mentioned_addresses: HashSet<Pubkey>,
    pub filter: TransactionLogCollectorFilter,
}

#[derive(AbiExample, Clone, Debug, PartialEq, Eq)]
pub struct TransactionLogInfo {
    pub signature: Signature,
    pub result: Result<()>,
    pub is_vote: bool,
    pub log_messages: TransactionLogMessages,
}

#[derive(AbiExample, Default, Debug)]
pub struct TransactionLogCollector {
    // All the logs collected for from this Bank.  Exact contents depend on the
    // active `TransactionLogCollectorFilter`
    pub logs: Vec<TransactionLogInfo>,

    // For each `mentioned_addresses`, maintain a list of indices into `logs` to easily
    // locate the logs from transactions that included the mentioned addresses.
    pub mentioned_address_map: HashMap<Pubkey, Vec<usize>>,
}

impl TransactionLogCollector {
    pub fn get_logs_for_address(
        &self,
        address: Option<&Pubkey>,
    ) -> Option<Vec<TransactionLogInfo>> {
        match address {
            None => Some(self.logs.clone()),
            Some(address) => self.mentioned_address_map.get(address).map(|log_indices| {
                log_indices
                    .iter()
                    .filter_map(|i| self.logs.get(*i).cloned())
                    .collect()
            }),
        }
    }
}

/// Bank's common fields shared by all supported snapshot versions for deserialization.
/// Sync fields with BankFieldsToSerialize! This is paired with it.
/// All members are made public to remain Bank's members private and to make versioned deserializer workable on this
/// Note that some fields are missing from the serializer struct. This is because of fields added later.
/// Since it is difficult to insert fields to serialize/deserialize against existing code already deployed,
/// new fields can be optionally serialized and optionally deserialized. At some point, the serialization and
/// deserialization will use a new mechanism or otherwise be in sync more clearly.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BankFieldsToDeserialize {
    pub(crate) blockhash_queue: BlockhashQueue,
    pub(crate) ancestors: AncestorsForSerialization,
    pub(crate) hash: Hash,
    pub(crate) parent_hash: Hash,
    pub(crate) parent_slot: Slot,
    pub(crate) hard_forks: HardForks,
    pub(crate) transaction_count: u64,
    pub(crate) tick_height: u64,
    pub(crate) signature_count: u64,
    pub(crate) capitalization: u64,
    pub(crate) max_tick_height: u64,
    pub(crate) hashes_per_tick: Option<u64>,
    pub(crate) ticks_per_slot: u64,
    pub(crate) ns_per_slot: u128,
    pub(crate) genesis_creation_time: UnixTimestamp,
    pub(crate) slots_per_year: f64,
    pub(crate) slot: Slot,
    pub(crate) epoch: Epoch,
    pub(crate) block_height: u64,
    pub(crate) collector_id: Pubkey,
    pub(crate) collector_fees: u64,
    pub(crate) fee_calculator: FeeCalculator,
    pub(crate) fee_rate_governor: FeeRateGovernor,
    pub(crate) collected_rent: u64,
    pub(crate) rent_collector: RentCollector,
    pub(crate) epoch_schedule: EpochSchedule,
    pub(crate) inflation: Inflation,
    pub(crate) stakes: Stakes<Delegation>,
    pub(crate) epoch_stakes: HashMap<Epoch, EpochStakes>,
    pub(crate) is_delta: bool,
    pub(crate) accounts_data_len: u64,
    pub(crate) incremental_snapshot_persistence: Option<BankIncrementalSnapshotPersistence>,
    pub(crate) epoch_accounts_hash: Option<Hash>,
    pub(crate) epoch_reward_status: EpochRewardStatus,
}

/// Bank's common fields shared by all supported snapshot versions for serialization.
/// This is separated from BankFieldsToDeserialize to avoid cloning by using refs.
/// So, sync fields with BankFieldsToDeserialize!
/// all members are made public to keep Bank private and to make versioned serializer workable on this.
/// Note that some fields are missing from the serializer struct. This is because of fields added later.
/// Since it is difficult to insert fields to serialize/deserialize against existing code already deployed,
/// new fields can be optionally serialized and optionally deserialized. At some point, the serialization and
/// deserialization will use a new mechanism or otherwise be in sync more clearly.
#[derive(Debug)]
pub(crate) struct BankFieldsToSerialize<'a> {
    pub(crate) blockhash_queue: &'a RwLock<BlockhashQueue>,
    pub(crate) ancestors: &'a AncestorsForSerialization,
    pub(crate) hash: Hash,
    pub(crate) parent_hash: Hash,
    pub(crate) parent_slot: Slot,
    pub(crate) hard_forks: &'a RwLock<HardForks>,
    pub(crate) transaction_count: u64,
    pub(crate) tick_height: u64,
    pub(crate) signature_count: u64,
    pub(crate) capitalization: u64,
    pub(crate) max_tick_height: u64,
    pub(crate) hashes_per_tick: Option<u64>,
    pub(crate) ticks_per_slot: u64,
    pub(crate) ns_per_slot: u128,
    pub(crate) genesis_creation_time: UnixTimestamp,
    pub(crate) slots_per_year: f64,
    pub(crate) slot: Slot,
    pub(crate) epoch: Epoch,
    pub(crate) block_height: u64,
    pub(crate) collector_id: Pubkey,
    pub(crate) collector_fees: u64,
    pub(crate) fee_calculator: FeeCalculator,
    pub(crate) fee_rate_governor: FeeRateGovernor,
    pub(crate) collected_rent: u64,
    pub(crate) rent_collector: RentCollector,
    pub(crate) epoch_schedule: EpochSchedule,
    pub(crate) inflation: Inflation,
    pub(crate) stakes: &'a StakesCache,
    pub(crate) epoch_stakes: &'a HashMap<Epoch, EpochStakes>,
    pub(crate) is_delta: bool,
    pub(crate) accounts_data_len: u64,
}

// Can't derive PartialEq because RwLock doesn't implement PartialEq
impl PartialEq for Bank {
    fn eq(&self, other: &Self) -> bool {
        if std::ptr::eq(self, other) {
            return true;
        }
        let Self {
            rc: _,
            status_cache: _,
            blockhash_queue,
            ancestors,
            hash,
            parent_hash,
            parent_slot,
            hard_forks,
            transaction_count,
            non_vote_transaction_count_since_restart: _,
            transaction_error_count: _,
            transaction_entries_count: _,
            transactions_per_entry_max: _,
            tick_height,
            signature_count,
            capitalization,
            max_tick_height,
            hashes_per_tick,
            ticks_per_slot,
            ns_per_slot,
            genesis_creation_time,
            slots_per_year,
            slot,
            bank_id: _,
            epoch,
            block_height,
            collector_id,
            collector_fees,
            fee_rate_governor,
            collected_rent,
            rent_collector,
            epoch_schedule,
            inflation,
            stakes_cache,
            epoch_stakes,
            is_delta,
            // TODO: Confirm if all these fields are intentionally ignored!
            builtin_programs: _,
            runtime_config: _,
            rewards: _,
            cluster_type: _,
            lazy_rent_collection: _,
            rewards_pool_pubkeys: _,
            transaction_debug_keys: _,
            transaction_log_collector_config: _,
            transaction_log_collector: _,
            feature_set: _,
            drop_callback: _,
            freeze_started: _,
            vote_only_bank: _,
            cost_tracker: _,
            sysvar_cache: _,
            accounts_data_size_initial: _,
            accounts_data_size_delta_on_chain: _,
            accounts_data_size_delta_off_chain: _,
            fee_structure: _,
            incremental_snapshot_persistence: _,
            loaded_programs_cache: _,
            check_program_modification_slot: _,
            epoch_reward_status: _,
            // Ignore new fields explicitly if they do not impact PartialEq.
            // Adding ".." will remove compile-time checks that if a new field
            // is added to the struct, this PartialEq is accordingly updated.
        } = self;
        *blockhash_queue.read().unwrap() == *other.blockhash_queue.read().unwrap()
            && ancestors == &other.ancestors
            && *hash.read().unwrap() == *other.hash.read().unwrap()
            && parent_hash == &other.parent_hash
            && parent_slot == &other.parent_slot
            && *hard_forks.read().unwrap() == *other.hard_forks.read().unwrap()
            && transaction_count.load(Relaxed) == other.transaction_count.load(Relaxed)
            && tick_height.load(Relaxed) == other.tick_height.load(Relaxed)
            && signature_count.load(Relaxed) == other.signature_count.load(Relaxed)
            && capitalization.load(Relaxed) == other.capitalization.load(Relaxed)
            && max_tick_height == &other.max_tick_height
            && hashes_per_tick == &other.hashes_per_tick
            && ticks_per_slot == &other.ticks_per_slot
            && ns_per_slot == &other.ns_per_slot
            && genesis_creation_time == &other.genesis_creation_time
            && slots_per_year == &other.slots_per_year
            && slot == &other.slot
            && epoch == &other.epoch
            && block_height == &other.block_height
            && collector_id == &other.collector_id
            && collector_fees.load(Relaxed) == other.collector_fees.load(Relaxed)
            && fee_rate_governor == &other.fee_rate_governor
            && collected_rent.load(Relaxed) == other.collected_rent.load(Relaxed)
            && rent_collector == &other.rent_collector
            && epoch_schedule == &other.epoch_schedule
            && *inflation.read().unwrap() == *other.inflation.read().unwrap()
            && *stakes_cache.stakes() == *other.stakes_cache.stakes()
            && epoch_stakes == &other.epoch_stakes
            && is_delta.load(Relaxed) == other.is_delta.load(Relaxed)
    }
}

#[derive(Debug)]
pub enum RewardCalculationEvent<'a, 'b> {
    Staking(&'a Pubkey, &'b InflationPointCalculationEvent),
}

/// type alias is not supported for trait in rust yet. As a workaround, we define the
/// `RewardCalcTracer` trait explicitly and implement it on any type that implement
/// `Fn(&RewardCalculationEvent) + Send + Sync`.
pub trait RewardCalcTracer: Fn(&RewardCalculationEvent) + Send + Sync {}

impl<T: Fn(&RewardCalculationEvent) + Send + Sync> RewardCalcTracer for T {}

fn null_tracer() -> Option<impl RewardCalcTracer> {
    None::<fn(&RewardCalculationEvent)>
}

pub trait DropCallback: fmt::Debug {
    fn callback(&self, b: &Bank);
    fn clone_box(&self) -> Box<dyn DropCallback + Send + Sync>;
}

#[derive(Debug, Default)]
pub struct OptionalDropCallback(Option<Box<dyn DropCallback + Send + Sync>>);

#[cfg(RUSTC_WITH_SPECIALIZATION)]
impl AbiExample for OptionalDropCallback {
    fn example() -> Self {
        Self(None)
    }
}

#[derive(AbiExample, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct StartBlockHeightAndRewards {
    /// the block height of the slot at which rewards distribution began
    pub(crate) start_block_height: u64,
    /// calculated epoch rewards pending distribution, outer Vec is by partition (one partition per block)
    pub(crate) stake_rewards_by_partition: Arc<Vec<StakeRewards>>,
}

/// Represent whether bank is in the reward phase or not.
#[derive(AbiExample, AbiEnumVisitor, Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub(crate) enum EpochRewardStatus {
    /// this bank is in the reward phase.
    /// Contents are the start point for epoch reward calculation,
    /// i.e. parent_slot and parent_block height for the starting
    /// block of the current epoch.
    Active(StartBlockHeightAndRewards),
    /// this bank is outside of the rewarding phase.
    #[default]
    Inactive,
}

/// Manager for the state of all accounts and programs after processing its entries.
/// AbiExample is needed even without Serialize/Deserialize; actual (de-)serialization
/// are implemented elsewhere for versioning
#[derive(AbiExample, Debug)]
pub struct Bank {
    /// References to accounts, parent and signature status
    pub rc: BankRc,

    /// A cache of signature statuses
    pub status_cache: Arc<RwLock<BankStatusCache>>,

    /// FIFO queue of `recent_blockhash` items
    blockhash_queue: RwLock<BlockhashQueue>,

    /// The set of parents including this bank
    pub ancestors: Ancestors,

    /// Hash of this Bank's state. Only meaningful after freezing.
    hash: RwLock<Hash>,

    /// Hash of this Bank's parent's state
    parent_hash: Hash,

    /// parent's slot
    parent_slot: Slot,

    /// slots to hard fork at
    hard_forks: Arc<RwLock<HardForks>>,

    /// The number of transactions processed without error
    transaction_count: AtomicU64,

    /// The number of non-vote transactions processed without error since the most recent boot from
    /// snapshot or genesis. This value is not shared though the network, nor retained within
    /// snapshots, but is preserved in `Bank::new_from_parent`.
    non_vote_transaction_count_since_restart: AtomicU64,

    /// The number of transaction errors in this slot
    transaction_error_count: AtomicU64,

    /// The number of transaction entries in this slot
    transaction_entries_count: AtomicU64,

    /// The max number of transaction in an entry in this slot
    transactions_per_entry_max: AtomicU64,

    /// Bank tick height
    tick_height: AtomicU64,

    /// The number of signatures from valid transactions in this slot
    signature_count: AtomicU64,

    /// Total capitalization, used to calculate inflation
    capitalization: AtomicU64,

    // Bank max_tick_height
    max_tick_height: u64,

    /// The number of hashes in each tick. None value means hashing is disabled.
    hashes_per_tick: Option<u64>,

    /// The number of ticks in each slot.
    ticks_per_slot: u64,

    /// length of a slot in ns
    pub ns_per_slot: u128,

    /// genesis time, used for computed clock
    genesis_creation_time: UnixTimestamp,

    /// The number of slots per year, used for inflation
    slots_per_year: f64,

    /// Bank slot (i.e. block)
    slot: Slot,

    bank_id: BankId,

    /// Bank epoch
    epoch: Epoch,

    /// Bank block_height
    block_height: u64,

    /// The pubkey to send transactions fees to.
    collector_id: Pubkey,

    /// Fees that have been collected
    collector_fees: AtomicU64,

    /// Track cluster signature throughput and adjust fee rate
    pub(crate) fee_rate_governor: FeeRateGovernor,

    /// Rent that has been collected
    collected_rent: AtomicU64,

    /// latest rent collector, knows the epoch
    rent_collector: RentCollector,

    /// initialized from genesis
    epoch_schedule: EpochSchedule,

    /// inflation specs
    inflation: Arc<RwLock<Inflation>>,

    /// cache of vote_account and stake_account state for this fork
    stakes_cache: StakesCache,

    /// staked nodes on epoch boundaries, saved off when a bank.slot() is at
    ///   a leader schedule calculation boundary
    epoch_stakes: HashMap<Epoch, EpochStakes>,

    /// A boolean reflecting whether any entries were recorded into the PoH
    /// stream for the slot == self.slot
    is_delta: AtomicBool,

    builtin_programs: HashSet<Pubkey>,

    /// Optional config parameters that can override runtime behavior
    runtime_config: Arc<RuntimeConfig>,

    /// Protocol-level rewards that were distributed by this bank
    pub rewards: RwLock<Vec<(Pubkey, RewardInfo)>>,

    pub cluster_type: Option<ClusterType>,

    pub lazy_rent_collection: AtomicBool,

    // this is temporary field only to remove rewards_pool entirely
    pub rewards_pool_pubkeys: Arc<HashSet<Pubkey>>,

    transaction_debug_keys: Option<Arc<HashSet<Pubkey>>>,

    // Global configuration for how transaction logs should be collected across all banks
    pub transaction_log_collector_config: Arc<RwLock<TransactionLogCollectorConfig>>,

    // Logs from transactions that this Bank executed collected according to the criteria in
    // `transaction_log_collector_config`
    pub transaction_log_collector: Arc<RwLock<TransactionLogCollector>>,

    pub feature_set: Arc<FeatureSet>,

    /// callback function only to be called when dropping and should only be called once
    pub drop_callback: RwLock<OptionalDropCallback>,

    pub freeze_started: AtomicBool,

    vote_only_bank: bool,

    cost_tracker: RwLock<CostTracker>,

    sysvar_cache: RwLock<SysvarCache>,

    /// The initial accounts data size at the start of this Bank, before processing any transactions/etc
    accounts_data_size_initial: u64,
    /// The change to accounts data size in this Bank, due on-chain events (i.e. transactions)
    accounts_data_size_delta_on_chain: AtomicI64,
    /// The change to accounts data size in this Bank, due to off-chain events (i.e. rent collection)
    accounts_data_size_delta_off_chain: AtomicI64,

    /// Transaction fee structure
    pub fee_structure: FeeStructure,

    pub incremental_snapshot_persistence: Option<BankIncrementalSnapshotPersistence>,

    pub loaded_programs_cache: Arc<RwLock<LoadedPrograms<BankForks>>>,

    pub check_program_modification_slot: bool,

    epoch_reward_status: EpochRewardStatus,
}

struct VoteWithStakeDelegations {
    vote_state: Arc<VoteState>,
    vote_account: AccountSharedData,
    delegations: Vec<(Pubkey, StakeAccount<Delegation>)>,
}

type VoteWithStakeDelegationsMap = DashMap<Pubkey, VoteWithStakeDelegations>;

type InvalidCacheKeyMap = DashMap<Pubkey, InvalidCacheEntryReason>;

struct LoadVoteAndStakeAccountsResult {
    vote_with_stake_delegations_map: VoteWithStakeDelegationsMap,
    invalid_vote_keys: InvalidCacheKeyMap,
    vote_accounts_cache_miss_count: usize,
}

#[derive(Debug)]
struct VoteReward {
    vote_account: AccountSharedData,
    commission: u8,
    vote_rewards: u64,
    vote_needs_store: bool,
}

type VoteRewards = DashMap<Pubkey, VoteReward>;
#[derive(Debug, Default)]
struct VoteRewardsAccounts {
    /// reward info for each vote account pubkey.
    /// This type is used by `update_reward_history()`
    rewards: Vec<(Pubkey, RewardInfo)>,
    /// corresponds to pubkey in `rewards`
    /// Some if account is to be stored.
    /// None if to be skipped.
    accounts_to_store: Vec<Option<AccountSharedData>>,
}

/// hold reward calc info to avoid recalculation across functions
struct EpochRewardCalculateParamInfo<'a> {
    stake_history: StakeHistory,
    stake_delegations: Vec<(&'a Pubkey, &'a StakeAccount<Delegation>)>,
    cached_vote_accounts: &'a VoteAccounts,
}

/// Hold all results from calculating the rewards for partitioned distribution.
/// This struct exists so we can have a function which does all the calculation with no
/// side effects.
struct PartitionedRewardsCalculation {
    vote_account_rewards: VoteRewardsAccounts,
    stake_rewards_by_partition: StakeRewardCalculationPartitioned,
    old_vote_balance_and_staked: u64,
    validator_rewards: u64,
    validator_rate: f64,
    foundation_rate: f64,
    prev_epoch_duration_in_years: f64,
    capitalization: u64,
}

/// result of calculating the stake rewards at beginning of new epoch
struct StakeRewardCalculationPartitioned {
    /// each individual stake account to reward, grouped by partition
    stake_rewards_by_partition: Vec<StakeRewards>,
    /// total lamports across all `stake_rewards`
    total_stake_rewards_lamports: u64,
}

struct CalculateRewardsAndDistributeVoteRewardsResult {
    /// total rewards for the epoch (including both vote rewards and stake rewards)
    total_rewards: u64,
    /// distributed vote rewards
    distributed_rewards: u64,
    /// stake rewards that still need to be distributed, grouped by partition
    stake_rewards_by_partition: Vec<StakeRewards>,
}

pub(crate) type StakeRewards = Vec<StakeReward>;

#[derive(Debug, Default)]
pub struct NewBankOptions {
    pub vote_only_bank: bool,
}

#[derive(Debug, Default)]
pub struct BankTestConfig {
    pub secondary_indexes: AccountSecondaryIndexes,
}

#[derive(Debug)]
struct PrevEpochInflationRewards {
    validator_rewards: u64,
    prev_epoch_duration_in_years: f64,
    validator_rate: f64,
    foundation_rate: f64,
}

pub struct CommitTransactionCounts {
    pub committed_transactions_count: u64,
    pub committed_non_vote_transactions_count: u64,
    pub committed_with_failure_result_count: u64,
    pub signature_count: u64,
}

impl WorkingSlot for Bank {
    fn current_slot(&self) -> Slot {
        self.slot
    }

    fn current_epoch(&self) -> Epoch {
        self.epoch
    }

    fn is_ancestor(&self, other: Slot) -> bool {
        self.ancestors.contains_key(&other)
    }
}
#[derive(Debug, Default)]
/// result of calculating the stake rewards at end of epoch
struct StakeRewardCalculation {
    /// each individual stake account to reward
    stake_rewards: StakeRewards,
    /// total lamports across all `stake_rewards`
    total_stake_rewards_lamports: u64,
}

impl Bank {
    pub fn default_for_tests() -> Self {
        Self::default_with_accounts(Accounts::default_for_tests())
    }

    pub fn new_for_benches(genesis_config: &GenesisConfig) -> Self {
        Self::new_with_paths_for_benches(genesis_config, Vec::new())
    }

    pub fn new_for_tests(genesis_config: &GenesisConfig) -> Self {
        Self::new_for_tests_with_config(genesis_config, BankTestConfig::default())
    }

    pub fn new_for_tests_with_config(
        genesis_config: &GenesisConfig,
        test_config: BankTestConfig,
    ) -> Self {
        Self::new_with_config_for_tests(
            genesis_config,
            test_config.secondary_indexes,
            AccountShrinkThreshold::default(),
        )
    }

    /// Intended for use by tests only.
    /// create new bank with the given configs.
    pub fn new_with_runtime_config_for_tests(
        genesis_config: &GenesisConfig,
        runtime_config: Arc<RuntimeConfig>,
    ) -> Self {
        Self::new_with_paths_for_tests(
            genesis_config,
            runtime_config,
            Vec::new(),
            AccountSecondaryIndexes::default(),
            AccountShrinkThreshold::default(),
        )
    }

    pub fn new_no_wallclock_throttle_for_tests(genesis_config: &GenesisConfig) -> Self {
        let mut bank = Self::new_for_tests(genesis_config);

        bank.ns_per_slot = std::u128::MAX;
        bank
    }

    pub(crate) fn new_with_config_for_tests(
        genesis_config: &GenesisConfig,
        account_indexes: AccountSecondaryIndexes,
        shrink_ratio: AccountShrinkThreshold,
    ) -> Self {
        Self::new_with_paths_for_tests(
            genesis_config,
            Arc::new(RuntimeConfig::default()),
            Vec::new(),
            account_indexes,
            shrink_ratio,
        )
    }

    fn default_with_accounts(accounts: Accounts) -> Self {
        let mut bank = Self {
            incremental_snapshot_persistence: None,
            rc: BankRc::new(accounts, Slot::default()),
            status_cache: Arc::<RwLock<BankStatusCache>>::default(),
            blockhash_queue: RwLock::<BlockhashQueue>::default(),
            ancestors: Ancestors::default(),
            hash: RwLock::<Hash>::default(),
            parent_hash: Hash::default(),
            parent_slot: Slot::default(),
            hard_forks: Arc::<RwLock<HardForks>>::default(),
            transaction_count: AtomicU64::default(),
            non_vote_transaction_count_since_restart: AtomicU64::default(),
            transaction_error_count: AtomicU64::default(),
            transaction_entries_count: AtomicU64::default(),
            transactions_per_entry_max: AtomicU64::default(),
            tick_height: AtomicU64::default(),
            signature_count: AtomicU64::default(),
            capitalization: AtomicU64::default(),
            max_tick_height: u64::default(),
            hashes_per_tick: Option::<u64>::default(),
            ticks_per_slot: u64::default(),
            ns_per_slot: u128::default(),
            genesis_creation_time: UnixTimestamp::default(),
            slots_per_year: f64::default(),
            slot: Slot::default(),
            bank_id: BankId::default(),
            epoch: Epoch::default(),
            block_height: u64::default(),
            collector_id: Pubkey::default(),
            collector_fees: AtomicU64::default(),
            fee_rate_governor: FeeRateGovernor::default(),
            collected_rent: AtomicU64::default(),
            rent_collector: RentCollector::default(),
            epoch_schedule: EpochSchedule::default(),
            inflation: Arc::<RwLock<Inflation>>::default(),
            stakes_cache: StakesCache::default(),
            epoch_stakes: HashMap::<Epoch, EpochStakes>::default(),
            is_delta: AtomicBool::default(),
            builtin_programs: HashSet::<Pubkey>::default(),
            runtime_config: Arc::<RuntimeConfig>::default(),
            rewards: RwLock::<Vec<(Pubkey, RewardInfo)>>::default(),
            cluster_type: Option::<ClusterType>::default(),
            lazy_rent_collection: AtomicBool::default(),
            rewards_pool_pubkeys: Arc::<HashSet<Pubkey>>::default(),
            transaction_debug_keys: Option::<Arc<HashSet<Pubkey>>>::default(),
            transaction_log_collector_config: Arc::<RwLock<TransactionLogCollectorConfig>>::default(
            ),
            transaction_log_collector: Arc::<RwLock<TransactionLogCollector>>::default(),
            feature_set: Arc::<FeatureSet>::default(),
            drop_callback: RwLock::new(OptionalDropCallback(None)),
            freeze_started: AtomicBool::default(),
            vote_only_bank: false,
            cost_tracker: RwLock::<CostTracker>::default(),
            sysvar_cache: RwLock::<SysvarCache>::default(),
            accounts_data_size_initial: 0,
            accounts_data_size_delta_on_chain: AtomicI64::new(0),
            accounts_data_size_delta_off_chain: AtomicI64::new(0),
            fee_structure: FeeStructure::default(),
            loaded_programs_cache: Arc::new(RwLock::new(LoadedPrograms::new(
                Slot::default(),
                Epoch::default(),
            ))),
            check_program_modification_slot: false,
            epoch_reward_status: EpochRewardStatus::default(),
        };

        let accounts_data_size_initial = bank.get_total_accounts_stats().unwrap().data_len as u64;
        bank.accounts_data_size_initial = accounts_data_size_initial;

        bank
    }

    pub fn new_with_paths_for_tests(
        genesis_config: &GenesisConfig,
        runtime_config: Arc<RuntimeConfig>,
        paths: Vec<PathBuf>,
        account_indexes: AccountSecondaryIndexes,
        shrink_ratio: AccountShrinkThreshold,
    ) -> Self {
        Self::new_with_paths(
            genesis_config,
            runtime_config,
            paths,
            None,
            None,
            account_indexes,
            shrink_ratio,
            false,
            Some(ACCOUNTS_DB_CONFIG_FOR_TESTING),
            None,
            Arc::default(),
        )
    }

    /// Intended for use by benches only.
    /// create new bank with the given config and paths.
    pub fn new_with_paths_for_benches(genesis_config: &GenesisConfig, paths: Vec<PathBuf>) -> Self {
        Self::new_with_paths(
            genesis_config,
            Arc::<RuntimeConfig>::default(),
            paths,
            None,
            None,
            AccountSecondaryIndexes::default(),
            AccountShrinkThreshold::default(),
            false,
            Some(ACCOUNTS_DB_CONFIG_FOR_BENCHMARKS),
            None,
            Arc::default(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_paths(
        genesis_config: &GenesisConfig,
        runtime_config: Arc<RuntimeConfig>,
        paths: Vec<PathBuf>,
        debug_keys: Option<Arc<HashSet<Pubkey>>>,
        additional_builtins: Option<&[BuiltinPrototype]>,
        account_indexes: AccountSecondaryIndexes,
        shrink_ratio: AccountShrinkThreshold,
        debug_do_not_add_builtins: bool,
        accounts_db_config: Option<AccountsDbConfig>,
        accounts_update_notifier: Option<AccountsUpdateNotifier>,
        exit: Arc<AtomicBool>,
    ) -> Self {
        let accounts = Accounts::new_with_config(
            paths,
            &genesis_config.cluster_type,
            account_indexes,
            shrink_ratio,
            accounts_db_config,
            accounts_update_notifier,
            exit,
        );
        let mut bank = Self::default_with_accounts(accounts);
        bank.ancestors = Ancestors::from(vec![bank.slot()]);
        bank.transaction_debug_keys = debug_keys;
        bank.runtime_config = runtime_config;
        bank.cluster_type = Some(genesis_config.cluster_type);

        bank.process_genesis_config(genesis_config);
        bank.finish_init(
            genesis_config,
            additional_builtins,
            debug_do_not_add_builtins,
        );

        // genesis needs stakes for all epochs up to the epoch implied by
        //  slot = 0 and genesis configuration
        {
            let stakes = bank.stakes_cache.stakes().clone();
            let stakes = Arc::new(StakesEnum::from(stakes));
            for epoch in 0..=bank.get_leader_schedule_epoch(bank.slot) {
                bank.epoch_stakes
                    .insert(epoch, EpochStakes::new(stakes.clone(), epoch));
            }
            bank.update_stake_history(None);
        }
        bank.update_clock(None);
        bank.update_rent();
        bank.update_epoch_schedule();
        bank.update_recent_blockhashes();
        bank.update_last_restart_slot();
        bank.fill_missing_sysvar_cache_entries();
        bank
    }

    /// Create a new bank that points to an immutable checkpoint of another bank.
    pub fn new_from_parent(parent: Arc<Bank>, collector_id: &Pubkey, slot: Slot) -> Self {
        Self::_new_from_parent(
            parent,
            collector_id,
            slot,
            null_tracer(),
            NewBankOptions::default(),
        )
    }

    pub fn new_from_parent_with_options(
        parent: Arc<Bank>,
        collector_id: &Pubkey,
        slot: Slot,
        new_bank_options: NewBankOptions,
    ) -> Self {
        Self::_new_from_parent(parent, collector_id, slot, null_tracer(), new_bank_options)
    }

    pub fn new_from_parent_with_tracer(
        parent: Arc<Bank>,
        collector_id: &Pubkey,
        slot: Slot,
        reward_calc_tracer: impl RewardCalcTracer,
    ) -> Self {
        Self::_new_from_parent(
            parent,
            collector_id,
            slot,
            Some(reward_calc_tracer),
            NewBankOptions::default(),
        )
    }

    fn get_rent_collector_from(rent_collector: &RentCollector, epoch: Epoch) -> RentCollector {
        rent_collector.clone_with_epoch(epoch)
    }

    fn is_partitioned_rewards_feature_enabled(&self) -> bool {
        self.feature_set
            .is_active(&feature_set::enable_partitioned_epoch_reward::id())
    }

    pub(crate) fn set_epoch_reward_status_active(
        &mut self,
        stake_rewards_by_partition: Vec<StakeRewards>,
    ) {
        self.epoch_reward_status = EpochRewardStatus::Active(StartBlockHeightAndRewards {
            start_block_height: self.block_height,
            stake_rewards_by_partition: Arc::new(stake_rewards_by_partition),
        });
    }

    fn partitioned_epoch_rewards_config(&self) -> &PartitionedEpochRewardsConfig {
        &self
            .rc
            .accounts
            .accounts_db
            .partitioned_epoch_rewards_config
    }

    /// # stake accounts to store in one block during partitioned reward interval
    fn partitioned_rewards_stake_account_stores_per_block(&self) -> u64 {
        self.partitioned_epoch_rewards_config()
            .stake_account_stores_per_block
    }

    /// reward calculation happens synchronously during the first block of the epoch boundary.
    /// So, # blocks for reward calculation is 1.
    fn get_reward_calculation_num_blocks(&self) -> Slot {
        self.partitioned_epoch_rewards_config()
            .reward_calculation_num_blocks
    }

    /// Calculate the number of blocks required to distribute rewards to all stake accounts.
    fn get_reward_distribution_num_blocks(&self, rewards: &StakeRewards) -> u64 {
        let total_stake_accounts = rewards.len();
        if self.epoch_schedule.warmup && self.epoch < self.first_normal_epoch() {
            1
        } else {
            const MAX_FACTOR_OF_REWARD_BLOCKS_IN_EPOCH: u64 = 10;
            let num_chunks = solana_accounts_db::accounts_hash::AccountsHasher::div_ceil(
                total_stake_accounts,
                self.partitioned_rewards_stake_account_stores_per_block() as usize,
            ) as u64;

            // Limit the reward credit interval to 10% of the total number of slots in a epoch
            num_chunks.clamp(
                1,
                (self.epoch_schedule.slots_per_epoch / MAX_FACTOR_OF_REWARD_BLOCKS_IN_EPOCH).max(1),
            )
        }
    }

    /// Return `RewardInterval` enum for current bank
    fn get_reward_interval(&self) -> RewardInterval {
        if matches!(self.epoch_reward_status, EpochRewardStatus::Active(_)) {
            RewardInterval::InsideInterval
        } else {
            RewardInterval::OutsideInterval
        }
    }

    /// For testing only
    pub fn force_reward_interval_end_for_tests(&mut self) {
        self.epoch_reward_status = EpochRewardStatus::Inactive;
    }

    fn _new_from_parent(
        parent: Arc<Bank>,
        collector_id: &Pubkey,
        slot: Slot,
        reward_calc_tracer: Option<impl RewardCalcTracer>,
        new_bank_options: NewBankOptions,
    ) -> Self {
        let mut time = Measure::start("bank::new_from_parent");
        let NewBankOptions { vote_only_bank } = new_bank_options;

        parent.freeze();
        assert_ne!(slot, parent.slot());

        let epoch_schedule = parent.epoch_schedule;
        let epoch = epoch_schedule.get_epoch(slot);

        let (rc, bank_rc_creation_time_us) = measure_us!({
            let accounts_db = Arc::clone(&parent.rc.accounts.accounts_db);
            accounts_db.insert_default_bank_hash_stats(slot, parent.slot());
            BankRc {
                accounts: Arc::new(Accounts::new(accounts_db)),
                parent: RwLock::new(Some(Arc::clone(&parent))),
                slot,
                bank_id_generator: Arc::clone(&parent.rc.bank_id_generator),
            }
        });

        let (status_cache, status_cache_time_us) = measure_us!(Arc::clone(&parent.status_cache));

        let (fee_rate_governor, fee_components_time_us) = measure_us!(
            FeeRateGovernor::new_derived(&parent.fee_rate_governor, parent.signature_count())
        );

        let bank_id = rc.bank_id_generator.fetch_add(1, Relaxed) + 1;
        let (blockhash_queue, blockhash_queue_time_us) =
            measure_us!(RwLock::new(parent.blockhash_queue.read().unwrap().clone()));

        let (stakes_cache, stakes_cache_time_us) =
            measure_us!(StakesCache::new(parent.stakes_cache.stakes().clone()));

        let (epoch_stakes, epoch_stakes_time_us) = measure_us!(parent.epoch_stakes.clone());

        let (builtin_programs, builtin_programs_time_us) =
            measure_us!(parent.builtin_programs.clone());

        let (rewards_pool_pubkeys, rewards_pool_pubkeys_time_us) =
            measure_us!(parent.rewards_pool_pubkeys.clone());

        let (transaction_debug_keys, transaction_debug_keys_time_us) =
            measure_us!(parent.transaction_debug_keys.clone());

        let (transaction_log_collector_config, transaction_log_collector_config_time_us) =
            measure_us!(parent.transaction_log_collector_config.clone());

        let (feature_set, feature_set_time_us) = measure_us!(parent.feature_set.clone());

        let accounts_data_size_initial = parent.load_accounts_data_size();
        let mut new = Self {
            incremental_snapshot_persistence: None,
            rc,
            status_cache,
            slot,
            bank_id,
            epoch,
            blockhash_queue,

            // TODO: clean this up, so much special-case copying...
            hashes_per_tick: parent.hashes_per_tick,
            ticks_per_slot: parent.ticks_per_slot,
            ns_per_slot: parent.ns_per_slot,
            genesis_creation_time: parent.genesis_creation_time,
            slots_per_year: parent.slots_per_year,
            epoch_schedule,
            collected_rent: AtomicU64::new(0),
            rent_collector: Self::get_rent_collector_from(&parent.rent_collector, epoch),
            max_tick_height: (slot + 1) * parent.ticks_per_slot,
            block_height: parent.block_height + 1,
            fee_rate_governor,
            capitalization: AtomicU64::new(parent.capitalization()),
            vote_only_bank,
            inflation: parent.inflation.clone(),
            transaction_count: AtomicU64::new(parent.transaction_count()),
            non_vote_transaction_count_since_restart: AtomicU64::new(
                parent.non_vote_transaction_count_since_restart(),
            ),
            transaction_error_count: AtomicU64::new(0),
            transaction_entries_count: AtomicU64::new(0),
            transactions_per_entry_max: AtomicU64::new(0),
            // we will .clone_with_epoch() this soon after stake data update; so just .clone() for now
            stakes_cache,
            epoch_stakes,
            parent_hash: parent.hash(),
            parent_slot: parent.slot(),
            collector_id: *collector_id,
            collector_fees: AtomicU64::new(0),
            ancestors: Ancestors::default(),
            hash: RwLock::new(Hash::default()),
            is_delta: AtomicBool::new(false),
            builtin_programs,
            tick_height: AtomicU64::new(parent.tick_height.load(Relaxed)),
            signature_count: AtomicU64::new(0),
            runtime_config: parent.runtime_config.clone(),
            hard_forks: parent.hard_forks.clone(),
            rewards: RwLock::new(vec![]),
            cluster_type: parent.cluster_type,
            lazy_rent_collection: AtomicBool::new(parent.lazy_rent_collection.load(Relaxed)),
            rewards_pool_pubkeys,
            transaction_debug_keys,
            transaction_log_collector_config,
            transaction_log_collector: Arc::new(RwLock::new(TransactionLogCollector::default())),
            feature_set: Arc::clone(&feature_set),
            drop_callback: RwLock::new(OptionalDropCallback(
                parent
                    .drop_callback
                    .read()
                    .unwrap()
                    .0
                    .as_ref()
                    .map(|drop_callback| drop_callback.clone_box()),
            )),
            freeze_started: AtomicBool::new(false),
            cost_tracker: RwLock::new(CostTracker::new_with_account_data_size_limit(
                feature_set
                    .is_active(&feature_set::cap_accounts_data_len::id())
                    .then(|| {
                        parent
                            .accounts_data_size_limit()
                            .saturating_sub(accounts_data_size_initial)
                    }),
            )),
            sysvar_cache: RwLock::new(SysvarCache::default()),
            accounts_data_size_initial,
            accounts_data_size_delta_on_chain: AtomicI64::new(0),
            accounts_data_size_delta_off_chain: AtomicI64::new(0),
            fee_structure: parent.fee_structure.clone(),
            loaded_programs_cache: parent.loaded_programs_cache.clone(),
            check_program_modification_slot: false,
            epoch_reward_status: parent.epoch_reward_status.clone(),
        };

        let (_, ancestors_time_us) = measure_us!({
            let mut ancestors = Vec::with_capacity(1 + new.parents().len());
            ancestors.push(new.slot());
            new.parents().iter().for_each(|p| {
                ancestors.push(p.slot());
            });
            new.ancestors = Ancestors::from(ancestors);
        });

        // Following code may touch AccountsDb, requiring proper ancestors
        let (_, update_epoch_time_us) = measure_us!({
            if parent.epoch() < new.epoch() {
                new.process_new_epoch(
                    parent.epoch(),
                    parent.slot(),
                    parent.block_height(),
                    reward_calc_tracer,
                );
            } else {
                // Save a snapshot of stakes for use in consensus and stake weighted networking
                let leader_schedule_epoch = epoch_schedule.get_leader_schedule_epoch(slot);
                new.update_epoch_stakes(leader_schedule_epoch);
            }
            if new.is_partitioned_rewards_code_enabled() {
                new.distribute_partitioned_epoch_rewards();
            }
        });

        let (_, recompilation_time_us) = measure_us!({
            // Recompile loaded programs one at a time before the next epoch hits
            let (_epoch, slot_index) = new.get_epoch_and_slot_index(new.slot());
            let slots_in_epoch = new.get_slots_in_epoch(new.epoch());
            let slots_in_recompilation_phase =
                (solana_program_runtime::loaded_programs::MAX_LOADED_ENTRY_COUNT as u64)
                    .min(slots_in_epoch)
                    .checked_div(2)
                    .unwrap();
            let mut loaded_programs_cache = new.loaded_programs_cache.write().unwrap();
            if loaded_programs_cache.upcoming_environments.is_some() {
                if let Some((key, program_to_recompile)) =
                    loaded_programs_cache.programs_to_recompile.pop()
                {
                    drop(loaded_programs_cache);
                    let recompiled = new.load_program(&key, false, Some(program_to_recompile));
                    let mut loaded_programs_cache = new.loaded_programs_cache.write().unwrap();
                    loaded_programs_cache.replenish(key, recompiled);
                }
            } else if new.epoch() != loaded_programs_cache.latest_root_epoch
                || slot_index.saturating_add(slots_in_recompilation_phase) >= slots_in_epoch
            {
                // Anticipate the upcoming program runtime environment for the next epoch,
                // so we can try to recompile loaded programs before the feature transition hits.
                drop(loaded_programs_cache);
                let (feature_set, _new_feature_activations) = new.compute_active_feature_set(true);
                let mut loaded_programs_cache = new.loaded_programs_cache.write().unwrap();
                let program_runtime_environment_v1 = create_program_runtime_environment_v1(
                    &feature_set,
                    &new.runtime_config.compute_budget.unwrap_or_default(),
                    false, /* deployment */
                    false, /* debugging_features */
                )
                .unwrap();
                let program_runtime_environment_v2 = create_program_runtime_environment_v2(
                    &new.runtime_config.compute_budget.unwrap_or_default(),
                    false, /* debugging_features */
                );
                let mut upcoming_environments = loaded_programs_cache.environments.clone();
                let changed_program_runtime_v1 =
                    *upcoming_environments.program_runtime_v1 != program_runtime_environment_v1;
                let changed_program_runtime_v2 =
                    *upcoming_environments.program_runtime_v2 != program_runtime_environment_v2;
                if changed_program_runtime_v1 {
                    upcoming_environments.program_runtime_v1 =
                        Arc::new(program_runtime_environment_v1);
                }
                if changed_program_runtime_v2 {
                    upcoming_environments.program_runtime_v2 =
                        Arc::new(program_runtime_environment_v2);
                }
                loaded_programs_cache.upcoming_environments = Some(upcoming_environments);
                loaded_programs_cache.programs_to_recompile = loaded_programs_cache
                    .get_entries_sorted_by_tx_usage(
                        changed_program_runtime_v1,
                        changed_program_runtime_v2,
                    );
            }
        });

        // Update sysvars before processing transactions
        let (_, update_sysvars_time_us) = measure_us!({
            new.update_slot_hashes();
            new.update_stake_history(Some(parent.epoch()));
            new.update_clock(Some(parent.epoch()));
            new.update_fees();
            new.update_last_restart_slot()
        });

        let (_, fill_sysvar_cache_time_us) = measure_us!(new.fill_missing_sysvar_cache_entries());
        time.stop();

        report_new_bank_metrics(
            slot,
            parent.slot(),
            new.block_height,
            NewBankTimings {
                bank_rc_creation_time_us,
                total_elapsed_time_us: time.as_us(),
                status_cache_time_us,
                fee_components_time_us,
                blockhash_queue_time_us,
                stakes_cache_time_us,
                epoch_stakes_time_us,
                builtin_programs_time_us,
                rewards_pool_pubkeys_time_us,
                executor_cache_time_us: 0,
                transaction_debug_keys_time_us,
                transaction_log_collector_config_time_us,
                feature_set_time_us,
                ancestors_time_us,
                update_epoch_time_us,
                recompilation_time_us,
                update_sysvars_time_us,
                fill_sysvar_cache_time_us,
            },
        );

        parent
            .loaded_programs_cache
            .read()
            .unwrap()
            .stats
            .submit(parent.slot());

        new.loaded_programs_cache.write().unwrap().stats.reset();
        new
    }

    /// Epoch in which the new cooldown warmup rate for stake was activated
    pub fn new_warmup_cooldown_rate_epoch(&self) -> Option<Epoch> {
        self.feature_set
            .new_warmup_cooldown_rate_epoch(&self.epoch_schedule)
    }

    /// process for the start of a new epoch
    fn process_new_epoch(
        &mut self,
        parent_epoch: Epoch,
        parent_slot: Slot,
        parent_height: u64,
        reward_calc_tracer: Option<impl RewardCalcTracer>,
    ) {
        let epoch = self.epoch();
        let slot = self.slot();
        let (thread_pool, thread_pool_time) = measure!(
            ThreadPoolBuilder::new().build().unwrap(),
            "thread_pool_creation",
        );

        let (_, apply_feature_activations_time) = measure!(
            self.apply_feature_activations(ApplyFeatureActivationsCaller::NewFromParent, false),
            "apply_feature_activation",
        );

        // Add new entry to stakes.stake_history, set appropriate epoch and
        // update vote accounts with warmed up stakes before saving a
        // snapshot of stakes in epoch stakes
        let (_, activate_epoch_time) = measure!(
            self.stakes_cache.activate_epoch(
                epoch,
                &thread_pool,
                self.new_warmup_cooldown_rate_epoch()
            ),
            "activate_epoch",
        );

        // Save a snapshot of stakes for use in consensus and stake weighted networking
        let leader_schedule_epoch = self.epoch_schedule.get_leader_schedule_epoch(slot);
        let (_, update_epoch_stakes_time) = measure!(
            self.update_epoch_stakes(leader_schedule_epoch),
            "update_epoch_stakes",
        );

        let mut rewards_metrics = RewardsMetrics::default();
        // After saving a snapshot of stakes, apply stake rewards and commission
        let (_, update_rewards_with_thread_pool_time) = measure!(
            {
                if self.is_partitioned_rewards_feature_enabled()
                    || self
                        .partitioned_epoch_rewards_config()
                        .test_enable_partitioned_rewards
                {
                    self.begin_partitioned_rewards(
                        reward_calc_tracer,
                        &thread_pool,
                        parent_epoch,
                        parent_slot,
                        parent_height,
                        &mut rewards_metrics,
                    );
                } else {
                    self.update_rewards_with_thread_pool(
                        parent_epoch,
                        reward_calc_tracer,
                        &thread_pool,
                        &mut rewards_metrics,
                    )
                }
            },
            "update_rewards_with_thread_pool",
        );

        report_new_epoch_metrics(
            epoch,
            slot,
            parent_slot,
            NewEpochTimings {
                thread_pool_time_us: thread_pool_time.as_us(),
                apply_feature_activations_time_us: apply_feature_activations_time.as_us(),
                activate_epoch_time_us: activate_epoch_time.as_us(),
                update_epoch_stakes_time_us: update_epoch_stakes_time.as_us(),
                update_rewards_with_thread_pool_time_us: update_rewards_with_thread_pool_time
                    .as_us(),
            },
            rewards_metrics,
        );
    }

    /// partitioned reward distribution is complete.
    /// So, deactivate the epoch rewards sysvar.
    fn deactivate_epoch_reward_status(&mut self) {
        assert!(matches!(
            self.epoch_reward_status,
            EpochRewardStatus::Active(_)
        ));
        self.epoch_reward_status = EpochRewardStatus::Inactive;
        if let Some(account) = self.get_account(&sysvar::epoch_rewards::id()) {
            if account.lamports() > 0 {
                info!(
                    "burning {} extra lamports in EpochRewards sysvar account at slot {}",
                    account.lamports(),
                    self.slot()
                );
                self.log_epoch_rewards_sysvar("burn");
                self.burn_and_purge_account(&sysvar::epoch_rewards::id(), account);
            }
        }
    }

    /// Begin the process of calculating and distributing rewards.
    /// This process can take multiple slots.
    fn begin_partitioned_rewards(
        &mut self,
        reward_calc_tracer: Option<impl Fn(&RewardCalculationEvent) + Send + Sync>,
        thread_pool: &ThreadPool,
        parent_epoch: Epoch,
        parent_slot: Slot,
        parent_block_height: u64,
        rewards_metrics: &mut RewardsMetrics,
    ) {
        let CalculateRewardsAndDistributeVoteRewardsResult {
            total_rewards,
            distributed_rewards,
            stake_rewards_by_partition,
        } = self.calculate_rewards_and_distribute_vote_rewards(
            parent_epoch,
            reward_calc_tracer,
            thread_pool,
            rewards_metrics,
        );

        let slot = self.slot();
        let credit_start = self.block_height() + self.get_reward_calculation_num_blocks();
        let credit_end_exclusive = credit_start + stake_rewards_by_partition.len() as u64;

        self.set_epoch_reward_status_active(stake_rewards_by_partition);

        // create EpochRewards sysvar that holds the balance of undistributed rewards with
        // (total_rewards, distributed_rewards, credit_end_exclusive), total capital will increase by (total_rewards - distributed_rewards)
        self.create_epoch_rewards_sysvar(total_rewards, distributed_rewards, credit_end_exclusive);

        datapoint_info!(
            "epoch-rewards-status-update",
            ("start_slot", slot, i64),
            ("start_block_height", self.block_height(), i64),
            ("active", 1, i64),
            ("parent_slot", parent_slot, i64),
            ("parent_block_height", parent_block_height, i64),
        );
    }

    /// Process reward distribution for the block if it is inside reward interval.
    fn distribute_partitioned_epoch_rewards(&mut self) {
        let EpochRewardStatus::Active(status) = &self.epoch_reward_status else {
            return;
        };

        let height = self.block_height();
        let start_block_height = status.start_block_height;
        let credit_start = start_block_height + self.get_reward_calculation_num_blocks();
        let credit_end_exclusive = credit_start + status.stake_rewards_by_partition.len() as u64;
        assert!(
            self.epoch_schedule.get_slots_in_epoch(self.epoch)
                > credit_end_exclusive.saturating_sub(credit_start)
        );

        if height >= credit_start && height < credit_end_exclusive {
            let partition_index = height - credit_start;
            self.distribute_epoch_rewards_in_partition(
                &status.stake_rewards_by_partition,
                partition_index,
            );
        }

        if height.saturating_add(1) >= credit_end_exclusive {
            datapoint_info!(
                "epoch-rewards-status-update",
                ("slot", self.slot(), i64),
                ("block_height", height, i64),
                ("active", 0, i64),
                ("start_block_height", start_block_height, i64),
            );

            self.deactivate_epoch_reward_status();
        }
    }

    pub fn byte_limit_for_scans(&self) -> Option<usize> {
        self.rc
            .accounts
            .accounts_db
            .accounts_index
            .scan_results_limit_bytes
    }

    pub fn proper_ancestors_set(&self) -> HashSet<Slot> {
        HashSet::from_iter(self.proper_ancestors())
    }

    /// Returns all ancestors excluding self.slot.
    pub(crate) fn proper_ancestors(&self) -> impl Iterator<Item = Slot> + '_ {
        self.ancestors
            .keys()
            .into_iter()
            .filter(move |slot| *slot != self.slot)
    }

    pub fn set_callback(&self, callback: Option<Box<dyn DropCallback + Send + Sync>>) {
        *self.drop_callback.write().unwrap() = OptionalDropCallback(callback);
    }

    pub fn vote_only_bank(&self) -> bool {
        self.vote_only_bank
    }

    /// Like `new_from_parent` but additionally:
    /// * Doesn't assume that the parent is anywhere near `slot`, parent could be millions of slots
    /// in the past
    /// * Adjusts the new bank's tick height to avoid having to run PoH for millions of slots
    /// * Freezes the new bank, assuming that the user will `Bank::new_from_parent` from this bank
    /// * Calculates and sets the epoch accounts hash from the parent
    pub fn warp_from_parent(
        parent: Arc<Bank>,
        collector_id: &Pubkey,
        slot: Slot,
        data_source: CalcAccountsHashDataSource,
    ) -> Self {
        parent.freeze();
        parent
            .rc
            .accounts
            .accounts_db
            .epoch_accounts_hash_manager
            .set_in_flight(parent.slot());
        let accounts_hash = parent.update_accounts_hash(data_source, false, true);
        let epoch_accounts_hash = accounts_hash.into();
        parent
            .rc
            .accounts
            .accounts_db
            .epoch_accounts_hash_manager
            .set_valid(epoch_accounts_hash, parent.slot());

        let parent_timestamp = parent.clock().unix_timestamp;
        let mut new = Bank::new_from_parent(parent, collector_id, slot);
        new.apply_feature_activations(ApplyFeatureActivationsCaller::WarpFromParent, false);
        new.update_epoch_stakes(new.epoch_schedule().get_epoch(slot));
        new.tick_height.store(new.max_tick_height(), Relaxed);

        let mut clock = new.clock();
        clock.epoch_start_timestamp = parent_timestamp;
        clock.unix_timestamp = parent_timestamp;
        new.update_sysvar_account(&sysvar::clock::id(), |account| {
            create_account(
                &clock,
                new.inherit_specially_retained_account_fields(account),
            )
        });
        new.fill_missing_sysvar_cache_entries();
        new.freeze();
        new
    }

    /// Create a bank from explicit arguments and deserialized fields from snapshot
    #[allow(clippy::float_cmp)]
    pub(crate) fn new_from_fields(
        bank_rc: BankRc,
        genesis_config: &GenesisConfig,
        runtime_config: Arc<RuntimeConfig>,
        fields: BankFieldsToDeserialize,
        debug_keys: Option<Arc<HashSet<Pubkey>>>,
        additional_builtins: Option<&[BuiltinPrototype]>,
        debug_do_not_add_builtins: bool,
        accounts_data_size_initial: u64,
    ) -> Self {
        let now = Instant::now();
        let ancestors = Ancestors::from(&fields.ancestors);
        // For backward compatibility, we can only serialize and deserialize
        // Stakes<Delegation> in BankFieldsTo{Serialize,Deserialize}. But Bank
        // caches Stakes<StakeAccount>. Below Stakes<StakeAccount> is obtained
        // from Stakes<Delegation> by reading the full account state from
        // accounts-db. Note that it is crucial that these accounts are loaded
        // at the right slot and match precisely with serialized Delegations.
        let stakes = Stakes::new(&fields.stakes, |pubkey| {
            let (account, _slot) = bank_rc.accounts.load_with_fixed_root(&ancestors, pubkey)?;
            Some(account)
        })
        .expect(
            "Stakes cache is inconsistent with accounts-db. This can indicate \
            a corrupted snapshot or bugs in cached accounts or accounts-db.",
        );
        let stakes_accounts_load_duration = now.elapsed();
        fn new<T: Default>() -> T {
            T::default()
        }
        let feature_set = new();
        let mut bank = Self {
            incremental_snapshot_persistence: fields.incremental_snapshot_persistence,
            rc: bank_rc,
            status_cache: new(),
            blockhash_queue: RwLock::new(fields.blockhash_queue),
            ancestors,
            hash: RwLock::new(fields.hash),
            parent_hash: fields.parent_hash,
            parent_slot: fields.parent_slot,
            hard_forks: Arc::new(RwLock::new(fields.hard_forks)),
            transaction_count: AtomicU64::new(fields.transaction_count),
            non_vote_transaction_count_since_restart: new(),
            transaction_error_count: new(),
            transaction_entries_count: new(),
            transactions_per_entry_max: new(),
            tick_height: AtomicU64::new(fields.tick_height),
            signature_count: AtomicU64::new(fields.signature_count),
            capitalization: AtomicU64::new(fields.capitalization),
            max_tick_height: fields.max_tick_height,
            hashes_per_tick: fields.hashes_per_tick,
            ticks_per_slot: fields.ticks_per_slot,
            ns_per_slot: fields.ns_per_slot,
            genesis_creation_time: fields.genesis_creation_time,
            slots_per_year: fields.slots_per_year,
            slot: fields.slot,
            bank_id: 0,
            epoch: fields.epoch,
            block_height: fields.block_height,
            collector_id: fields.collector_id,
            collector_fees: AtomicU64::new(fields.collector_fees),
            fee_rate_governor: fields.fee_rate_governor,
            collected_rent: AtomicU64::new(fields.collected_rent),
            // clone()-ing is needed to consider a gated behavior in rent_collector
            rent_collector: Self::get_rent_collector_from(&fields.rent_collector, fields.epoch),
            epoch_schedule: fields.epoch_schedule,
            inflation: Arc::new(RwLock::new(fields.inflation)),
            stakes_cache: StakesCache::new(stakes),
            epoch_stakes: fields.epoch_stakes,
            is_delta: AtomicBool::new(fields.is_delta),
            builtin_programs: new(),
            runtime_config,
            rewards: new(),
            cluster_type: Some(genesis_config.cluster_type),
            lazy_rent_collection: new(),
            rewards_pool_pubkeys: new(),
            transaction_debug_keys: debug_keys,
            transaction_log_collector_config: new(),
            transaction_log_collector: new(),
            feature_set: Arc::clone(&feature_set),
            drop_callback: RwLock::new(OptionalDropCallback(None)),
            freeze_started: AtomicBool::new(fields.hash != Hash::default()),
            vote_only_bank: false,
            cost_tracker: RwLock::new(CostTracker::default()),
            sysvar_cache: RwLock::new(SysvarCache::default()),
            accounts_data_size_initial,
            accounts_data_size_delta_on_chain: AtomicI64::new(0),
            accounts_data_size_delta_off_chain: AtomicI64::new(0),
            fee_structure: FeeStructure::default(),
            loaded_programs_cache: Arc::new(RwLock::new(LoadedPrograms::new(
                fields.slot,
                fields.epoch,
            ))),
            check_program_modification_slot: false,
            epoch_reward_status: EpochRewardStatus::default(),
        };
        bank.finish_init(
            genesis_config,
            additional_builtins,
            debug_do_not_add_builtins,
        );
        bank.fill_missing_sysvar_cache_entries();

        // Sanity assertions between bank snapshot and genesis config
        // Consider removing from serializable bank state
        // (BankFieldsToSerialize/BankFieldsToDeserialize) and initializing
        // from the passed in genesis_config instead (as new()/new_with_paths() already do)
        assert_eq!(
            bank.genesis_creation_time, genesis_config.creation_time,
            "Bank snapshot genesis creation time does not match genesis.bin creation time.\
             The snapshot and genesis.bin might pertain to different clusters"
        );
        assert_eq!(bank.ticks_per_slot, genesis_config.ticks_per_slot);
        assert_eq!(
            bank.ns_per_slot,
            genesis_config.poh_config.target_tick_duration.as_nanos()
                * genesis_config.ticks_per_slot as u128
        );
        assert_eq!(bank.max_tick_height, (bank.slot + 1) * bank.ticks_per_slot);
        assert_eq!(
            bank.slots_per_year,
            years_as_slots(
                1.0,
                &genesis_config.poh_config.target_tick_duration,
                bank.ticks_per_slot,
            )
        );
        assert_eq!(bank.epoch_schedule, genesis_config.epoch_schedule);
        assert_eq!(bank.epoch, bank.epoch_schedule.get_epoch(bank.slot));

        datapoint_info!(
            "bank-new-from-fields",
            (
                "accounts_data_len-from-snapshot",
                fields.accounts_data_len as i64,
                i64
            ),
            (
                "accounts_data_len-from-generate_index",
                accounts_data_size_initial as i64,
                i64
            ),
            (
                "stakes_accounts_load_duration_us",
                stakes_accounts_load_duration.as_micros(),
                i64
            ),
        );
        bank
    }

    /// Return subset of bank fields representing serializable state
    pub(crate) fn get_fields_to_serialize<'a>(
        &'a self,
        ancestors: &'a HashMap<Slot, usize>,
    ) -> BankFieldsToSerialize<'a> {
        BankFieldsToSerialize {
            blockhash_queue: &self.blockhash_queue,
            ancestors,
            hash: *self.hash.read().unwrap(),
            parent_hash: self.parent_hash,
            parent_slot: self.parent_slot,
            hard_forks: &self.hard_forks,
            transaction_count: self.transaction_count.load(Relaxed),
            tick_height: self.tick_height.load(Relaxed),
            signature_count: self.signature_count.load(Relaxed),
            capitalization: self.capitalization.load(Relaxed),
            max_tick_height: self.max_tick_height,
            hashes_per_tick: self.hashes_per_tick,
            ticks_per_slot: self.ticks_per_slot,
            ns_per_slot: self.ns_per_slot,
            genesis_creation_time: self.genesis_creation_time,
            slots_per_year: self.slots_per_year,
            slot: self.slot,
            epoch: self.epoch,
            block_height: self.block_height,
            collector_id: self.collector_id,
            collector_fees: self.collector_fees.load(Relaxed),
            fee_calculator: FeeCalculator::default(),
            fee_rate_governor: self.fee_rate_governor.clone(),
            collected_rent: self.collected_rent.load(Relaxed),
            rent_collector: self.rent_collector.clone(),
            epoch_schedule: self.epoch_schedule,
            inflation: *self.inflation.read().unwrap(),
            stakes: &self.stakes_cache,
            epoch_stakes: &self.epoch_stakes,
            is_delta: self.is_delta.load(Relaxed),
            accounts_data_len: self.load_accounts_data_size(),
        }
    }

    pub fn collector_id(&self) -> &Pubkey {
        &self.collector_id
    }

    pub fn genesis_creation_time(&self) -> UnixTimestamp {
        self.genesis_creation_time
    }

    pub fn slot(&self) -> Slot {
        self.slot
    }

    pub fn bank_id(&self) -> BankId {
        self.bank_id
    }

    pub fn epoch(&self) -> Epoch {
        self.epoch
    }

    pub fn first_normal_epoch(&self) -> Epoch {
        self.epoch_schedule().first_normal_epoch
    }

    pub fn freeze_lock(&self) -> RwLockReadGuard<Hash> {
        self.hash.read().unwrap()
    }

    pub fn hash(&self) -> Hash {
        *self.hash.read().unwrap()
    }

    pub fn is_frozen(&self) -> bool {
        *self.hash.read().unwrap() != Hash::default()
    }

    pub fn freeze_started(&self) -> bool {
        self.freeze_started.load(Relaxed)
    }

    pub fn status_cache_ancestors(&self) -> Vec<u64> {
        let mut roots = self.status_cache.read().unwrap().roots().clone();
        let min = roots.iter().min().cloned().unwrap_or(0);
        for ancestor in self.ancestors.keys() {
            if ancestor >= min {
                roots.insert(ancestor);
            }
        }

        let mut ancestors: Vec<_> = roots.into_iter().collect();
        #[allow(clippy::stable_sort_primitive)]
        ancestors.sort();
        ancestors
    }

    /// computed unix_timestamp at this slot height
    pub fn unix_timestamp_from_genesis(&self) -> i64 {
        self.genesis_creation_time + ((self.slot as u128 * self.ns_per_slot) / 1_000_000_000) as i64
    }

    fn update_sysvar_account<F>(&self, pubkey: &Pubkey, updater: F)
    where
        F: Fn(&Option<AccountSharedData>) -> AccountSharedData,
    {
        let old_account = self.get_account_with_fixed_root(pubkey);
        let mut new_account = updater(&old_account);

        // When new sysvar comes into existence (with RENT_UNADJUSTED_INITIAL_BALANCE lamports),
        // this code ensures that the sysvar's balance is adjusted to be rent-exempt.
        //
        // More generally, this code always re-calculates for possible sysvar data size change,
        // although there is no such sysvars currently.
        self.adjust_sysvar_balance_for_rent(&mut new_account);
        self.store_account_and_update_capitalization(pubkey, &new_account);
    }

    fn inherit_specially_retained_account_fields(
        &self,
        old_account: &Option<AccountSharedData>,
    ) -> InheritableAccountFields {
        const RENT_UNADJUSTED_INITIAL_BALANCE: u64 = 1;

        (
            old_account
                .as_ref()
                .map(|a| a.lamports())
                .unwrap_or(RENT_UNADJUSTED_INITIAL_BALANCE),
            old_account
                .as_ref()
                .map(|a| a.rent_epoch())
                .unwrap_or(INITIAL_RENT_EPOCH),
        )
    }

    pub fn clock(&self) -> sysvar::clock::Clock {
        from_account(&self.get_account(&sysvar::clock::id()).unwrap_or_default())
            .unwrap_or_default()
    }

    fn update_clock(&self, parent_epoch: Option<Epoch>) {
        let mut unix_timestamp = self.clock().unix_timestamp;
        // set epoch_start_timestamp to None to warp timestamp
        let epoch_start_timestamp = {
            let epoch = if let Some(epoch) = parent_epoch {
                epoch
            } else {
                self.epoch()
            };
            let first_slot_in_epoch = self.epoch_schedule().get_first_slot_in_epoch(epoch);
            Some((first_slot_in_epoch, self.clock().epoch_start_timestamp))
        };
        let max_allowable_drift = MaxAllowableDrift {
            fast: MAX_ALLOWABLE_DRIFT_PERCENTAGE_FAST,
            slow: MAX_ALLOWABLE_DRIFT_PERCENTAGE_SLOW_V2,
        };

        let ancestor_timestamp = self.clock().unix_timestamp;
        if let Some(timestamp_estimate) =
            self.get_timestamp_estimate(max_allowable_drift, epoch_start_timestamp)
        {
            unix_timestamp = timestamp_estimate;
            if timestamp_estimate < ancestor_timestamp {
                unix_timestamp = ancestor_timestamp;
            }
        }
        datapoint_info!(
            "bank-timestamp-correction",
            ("slot", self.slot(), i64),
            ("from_genesis", self.unix_timestamp_from_genesis(), i64),
            ("corrected", unix_timestamp, i64),
            ("ancestor_timestamp", ancestor_timestamp, i64),
        );
        let mut epoch_start_timestamp =
            // On epoch boundaries, update epoch_start_timestamp
            if parent_epoch.is_some() && parent_epoch.unwrap() != self.epoch() {
                unix_timestamp
            } else {
                self.clock().epoch_start_timestamp
            };
        if self.slot == 0 {
            unix_timestamp = self.unix_timestamp_from_genesis();
            epoch_start_timestamp = self.unix_timestamp_from_genesis();
        }
        let clock = sysvar::clock::Clock {
            slot: self.slot,
            epoch_start_timestamp,
            epoch: self.epoch_schedule().get_epoch(self.slot),
            leader_schedule_epoch: self.epoch_schedule().get_leader_schedule_epoch(self.slot),
            unix_timestamp,
        };
        self.update_sysvar_account(&sysvar::clock::id(), |account| {
            create_account(
                &clock,
                self.inherit_specially_retained_account_fields(account),
            )
        });
    }

    pub fn update_last_restart_slot(&self) {
        let feature_flag = self
            .feature_set
            .is_active(&feature_set::last_restart_slot_sysvar::id());

        if feature_flag {
            // First, see what the currently stored last restart slot is. This
            // account may not exist yet if the feature was just activated.
            let current_last_restart_slot = self
                .get_account(&sysvar::last_restart_slot::id())
                .and_then(|account| {
                    let lrs: Option<LastRestartSlot> = from_account(&account);
                    lrs
                })
                .map(|account| account.last_restart_slot);

            let last_restart_slot = {
                let slot = self.slot;
                let hard_forks_r = self.hard_forks.read().unwrap();

                // Only consider hard forks <= this bank's slot to avoid prematurely applying
                // a hard fork that is set to occur in the future.
                hard_forks_r
                    .iter()
                    .rev()
                    .find(|(hard_fork, _)| *hard_fork <= slot)
                    .map(|(slot, _)| *slot)
                    .unwrap_or(0)
            };

            // Only need to write if the last restart has changed
            if current_last_restart_slot != Some(last_restart_slot) {
                self.update_sysvar_account(&sysvar::last_restart_slot::id(), |account| {
                    create_account(
                        &LastRestartSlot { last_restart_slot },
                        self.inherit_specially_retained_account_fields(account),
                    )
                });
            }
        }
    }

    pub fn set_sysvar_for_tests<T>(&self, sysvar: &T)
    where
        T: Sysvar + SysvarId,
    {
        self.update_sysvar_account(&T::id(), |account| {
            create_account(
                sysvar,
                self.inherit_specially_retained_account_fields(account),
            )
        });
        // Simply force fill sysvar cache rather than checking which sysvar was
        // actually updated since tests don't need to be optimized for performance.
        self.reset_sysvar_cache();
        self.fill_missing_sysvar_cache_entries();
    }

    fn update_slot_history(&self) {
        self.update_sysvar_account(&sysvar::slot_history::id(), |account| {
            let mut slot_history = account
                .as_ref()
                .map(|account| from_account::<SlotHistory, _>(account).unwrap())
                .unwrap_or_default();
            slot_history.add(self.slot());
            create_account(
                &slot_history,
                self.inherit_specially_retained_account_fields(account),
            )
        });
    }

    fn update_slot_hashes(&self) {
        self.update_sysvar_account(&sysvar::slot_hashes::id(), |account| {
            let mut slot_hashes = account
                .as_ref()
                .map(|account| from_account::<SlotHashes, _>(account).unwrap())
                .unwrap_or_default();
            slot_hashes.add(self.parent_slot, self.parent_hash);
            create_account(
                &slot_hashes,
                self.inherit_specially_retained_account_fields(account),
            )
        });
    }

    pub fn get_slot_history(&self) -> SlotHistory {
        from_account(&self.get_account(&sysvar::slot_history::id()).unwrap()).unwrap()
    }

    fn update_epoch_stakes(&mut self, leader_schedule_epoch: Epoch) {
        // update epoch_stakes cache
        //  if my parent didn't populate for this staker's epoch, we've
        //  crossed a boundary
        if self.epoch_stakes.get(&leader_schedule_epoch).is_none() {
            self.epoch_stakes.retain(|&epoch, _| {
                epoch >= leader_schedule_epoch.saturating_sub(MAX_LEADER_SCHEDULE_STAKES)
            });
            let stakes = self.stakes_cache.stakes().clone();
            let stakes = Arc::new(StakesEnum::from(stakes));
            let new_epoch_stakes = EpochStakes::new(stakes, leader_schedule_epoch);
            info!(
                "new epoch stakes, epoch: {}, total_stake: {}",
                leader_schedule_epoch,
                new_epoch_stakes.total_stake(),
            );

            // It is expensive to log the details of epoch stakes. Only log them at "trace"
            // level for debugging purpose.
            if log::log_enabled!(log::Level::Trace) {
                let vote_stakes: HashMap<_, _> = self
                    .stakes_cache
                    .stakes()
                    .vote_accounts()
                    .delegated_stakes()
                    .map(|(pubkey, stake)| (*pubkey, stake))
                    .collect();
                trace!("new epoch stakes, stakes: {vote_stakes:#?}");
            }
            self.epoch_stakes
                .insert(leader_schedule_epoch, new_epoch_stakes);
        }
    }

    #[allow(deprecated)]
    fn update_fees(&self) {
        if !self
            .feature_set
            .is_active(&feature_set::disable_fees_sysvar::id())
        {
            self.update_sysvar_account(&sysvar::fees::id(), |account| {
                create_account(
                    &sysvar::fees::Fees::new(&self.fee_rate_governor.create_fee_calculator()),
                    self.inherit_specially_retained_account_fields(account),
                )
            });
        }
    }

    fn update_rent(&self) {
        self.update_sysvar_account(&sysvar::rent::id(), |account| {
            create_account(
                &self.rent_collector.rent,
                self.inherit_specially_retained_account_fields(account),
            )
        });
    }

    fn update_epoch_schedule(&self) {
        self.update_sysvar_account(&sysvar::epoch_schedule::id(), |account| {
            create_account(
                self.epoch_schedule(),
                self.inherit_specially_retained_account_fields(account),
            )
        });
    }

    fn update_stake_history(&self, epoch: Option<Epoch>) {
        if epoch == Some(self.epoch()) {
            return;
        }
        // if I'm the first Bank in an epoch, ensure stake_history is updated
        self.update_sysvar_account(&sysvar::stake_history::id(), |account| {
            create_account::<sysvar::stake_history::StakeHistory>(
                self.stakes_cache.stakes().history(),
                self.inherit_specially_retained_account_fields(account),
            )
        });
    }

    pub fn epoch_duration_in_years(&self, prev_epoch: Epoch) -> f64 {
        // period: time that has passed as a fraction of a year, basically the length of
        //  an epoch as a fraction of a year
        //  calculated as: slots_elapsed / (slots / year)
        self.epoch_schedule().get_slots_in_epoch(prev_epoch) as f64 / self.slots_per_year
    }

    // Calculates the starting-slot for inflation from the activation slot.
    // This method assumes that `pico_inflation` will be enabled before `full_inflation`, giving
    // precedence to the latter. However, since `pico_inflation` is fixed-rate Inflation, should
    // `pico_inflation` be enabled 2nd, the incorrect start slot provided here should have no
    // effect on the inflation calculation.
    fn get_inflation_start_slot(&self) -> Slot {
        let mut slots = self
            .feature_set
            .full_inflation_features_enabled()
            .iter()
            .filter_map(|id| self.feature_set.activated_slot(id))
            .collect::<Vec<_>>();
        slots.sort_unstable();
        slots.first().cloned().unwrap_or_else(|| {
            self.feature_set
                .activated_slot(&feature_set::pico_inflation::id())
                .unwrap_or(0)
        })
    }

    fn get_inflation_num_slots(&self) -> u64 {
        let inflation_activation_slot = self.get_inflation_start_slot();
        // Normalize inflation_start to align with the start of rewards accrual.
        let inflation_start_slot = self.epoch_schedule().get_first_slot_in_epoch(
            self.epoch_schedule()
                .get_epoch(inflation_activation_slot)
                .saturating_sub(1),
        );
        self.epoch_schedule().get_first_slot_in_epoch(self.epoch()) - inflation_start_slot
    }

    pub fn slot_in_year_for_inflation(&self) -> f64 {
        let num_slots = self.get_inflation_num_slots();

        // calculated as: num_slots / (slots / year)
        num_slots as f64 / self.slots_per_year
    }

    fn calculate_previous_epoch_inflation_rewards(
        &self,
        prev_epoch_capitalization: u64,
        prev_epoch: Epoch,
    ) -> PrevEpochInflationRewards {
        let slot_in_year = self.slot_in_year_for_inflation();
        let (validator_rate, foundation_rate) = {
            let inflation = self.inflation.read().unwrap();
            (
                (*inflation).validator(slot_in_year),
                (*inflation).foundation(slot_in_year),
            )
        };

        let prev_epoch_duration_in_years = self.epoch_duration_in_years(prev_epoch);
        let validator_rewards = (validator_rate
            * prev_epoch_capitalization as f64
            * prev_epoch_duration_in_years) as u64;

        PrevEpochInflationRewards {
            validator_rewards,
            prev_epoch_duration_in_years,
            validator_rate,
            foundation_rate,
        }
    }

    /// Calculate rewards from previous epoch to prepare for partitioned distribution.
    fn calculate_rewards_for_partitioning(
        &self,
        prev_epoch: Epoch,
        reward_calc_tracer: Option<impl Fn(&RewardCalculationEvent) + Send + Sync>,
        thread_pool: &ThreadPool,
        metrics: &mut RewardsMetrics,
    ) -> PartitionedRewardsCalculation {
        let capitalization = self.capitalization();
        let PrevEpochInflationRewards {
            validator_rewards,
            prev_epoch_duration_in_years,
            validator_rate,
            foundation_rate,
        } = self.calculate_previous_epoch_inflation_rewards(capitalization, prev_epoch);

        let old_vote_balance_and_staked = self.stakes_cache.stakes().vote_balance_and_staked();

        let (vote_account_rewards, mut stake_rewards) = self
            .calculate_validator_rewards(
                prev_epoch,
                validator_rewards,
                reward_calc_tracer,
                thread_pool,
                metrics,
            )
            .unwrap_or_default();

        let num_partitions = self.get_reward_distribution_num_blocks(&stake_rewards.stake_rewards);
        let stake_rewards_by_partition = hash_rewards_into_partitions(
            std::mem::take(&mut stake_rewards.stake_rewards),
            &self.parent_hash(),
            num_partitions as usize,
        );

        PartitionedRewardsCalculation {
            vote_account_rewards,
            stake_rewards_by_partition: StakeRewardCalculationPartitioned {
                stake_rewards_by_partition,
                total_stake_rewards_lamports: stake_rewards.total_stake_rewards_lamports,
            },
            old_vote_balance_and_staked,
            validator_rewards,
            validator_rate,
            foundation_rate,
            prev_epoch_duration_in_years,
            capitalization,
        }
    }

    // Calculate rewards from previous epoch and distribute vote rewards
    fn calculate_rewards_and_distribute_vote_rewards(
        &self,
        prev_epoch: Epoch,
        reward_calc_tracer: Option<impl Fn(&RewardCalculationEvent) + Send + Sync>,
        thread_pool: &ThreadPool,
        metrics: &mut RewardsMetrics,
    ) -> CalculateRewardsAndDistributeVoteRewardsResult {
        let PartitionedRewardsCalculation {
            vote_account_rewards,
            stake_rewards_by_partition,
            old_vote_balance_and_staked,
            validator_rewards,
            validator_rate,
            foundation_rate,
            prev_epoch_duration_in_years,
            capitalization,
        } = self.calculate_rewards_for_partitioning(
            prev_epoch,
            reward_calc_tracer,
            thread_pool,
            metrics,
        );
        let vote_rewards = self.store_vote_accounts_partitioned(vote_account_rewards, metrics);

        // update reward history of JUST vote_rewards, stake_rewards is vec![] here
        self.update_reward_history(vec![], vote_rewards);

        let StakeRewardCalculationPartitioned {
            stake_rewards_by_partition,
            total_stake_rewards_lamports,
        } = stake_rewards_by_partition;

        // the remaining code mirrors `update_rewards_with_thread_pool()`

        let new_vote_balance_and_staked = self.stakes_cache.stakes().vote_balance_and_staked();

        // This is for vote rewards only.
        let validator_rewards_paid = new_vote_balance_and_staked - old_vote_balance_and_staked;
        self.assert_validator_rewards_paid(validator_rewards_paid);

        // verify that we didn't pay any more than we expected to
        assert!(validator_rewards >= validator_rewards_paid + total_stake_rewards_lamports);

        info!(
            "distributed vote rewards: {} out of {}, remaining {}",
            validator_rewards_paid, validator_rewards, total_stake_rewards_lamports
        );

        let (num_stake_accounts, num_vote_accounts) = {
            let stakes = self.stakes_cache.stakes();
            (
                stakes.stake_delegations().len(),
                stakes.vote_accounts().len(),
            )
        };
        self.capitalization
            .fetch_add(validator_rewards_paid, Relaxed);

        let active_stake = if let Some(stake_history_entry) =
            self.stakes_cache.stakes().history().get(prev_epoch)
        {
            stake_history_entry.effective
        } else {
            0
        };

        datapoint_info!(
            "epoch_rewards",
            ("slot", self.slot, i64),
            ("epoch", prev_epoch, i64),
            ("validator_rate", validator_rate, f64),
            ("foundation_rate", foundation_rate, f64),
            ("epoch_duration_in_years", prev_epoch_duration_in_years, f64),
            ("validator_rewards", validator_rewards_paid, i64),
            ("active_stake", active_stake, i64),
            ("pre_capitalization", capitalization, i64),
            ("post_capitalization", self.capitalization(), i64),
            ("num_stake_accounts", num_stake_accounts, i64),
            ("num_vote_accounts", num_vote_accounts, i64),
        );

        CalculateRewardsAndDistributeVoteRewardsResult {
            total_rewards: validator_rewards_paid + total_stake_rewards_lamports,
            distributed_rewards: validator_rewards_paid,
            stake_rewards_by_partition,
        }
    }

    fn assert_validator_rewards_paid(&self, validator_rewards_paid: u64) {
        assert_eq!(
            validator_rewards_paid,
            u64::try_from(
                self.rewards
                    .read()
                    .unwrap()
                    .par_iter()
                    .map(|(_address, reward_info)| {
                        match reward_info.reward_type {
                            RewardType::Voting | RewardType::Staking => reward_info.lamports,
                            _ => 0,
                        }
                    })
                    .sum::<i64>()
            )
            .unwrap()
        );
    }

    // update rewards based on the previous epoch
    fn update_rewards_with_thread_pool(
        &mut self,
        prev_epoch: Epoch,
        reward_calc_tracer: Option<impl Fn(&RewardCalculationEvent) + Send + Sync>,
        thread_pool: &ThreadPool,
        metrics: &mut RewardsMetrics,
    ) {
        let capitalization = self.capitalization();
        let PrevEpochInflationRewards {
            validator_rewards,
            prev_epoch_duration_in_years,
            validator_rate,
            foundation_rate,
        } = self.calculate_previous_epoch_inflation_rewards(capitalization, prev_epoch);

        let old_vote_balance_and_staked = self.stakes_cache.stakes().vote_balance_and_staked();

        self.pay_validator_rewards_with_thread_pool(
            prev_epoch,
            validator_rewards,
            reward_calc_tracer,
            thread_pool,
            metrics,
        );

        let new_vote_balance_and_staked = self.stakes_cache.stakes().vote_balance_and_staked();
        let validator_rewards_paid = new_vote_balance_and_staked - old_vote_balance_and_staked;
        assert_eq!(
            validator_rewards_paid,
            u64::try_from(
                self.rewards
                    .read()
                    .unwrap()
                    .iter()
                    .map(|(_address, reward_info)| {
                        match reward_info.reward_type {
                            RewardType::Voting | RewardType::Staking => reward_info.lamports,
                            _ => 0,
                        }
                    })
                    .sum::<i64>()
            )
            .unwrap()
        );

        // verify that we didn't pay any more than we expected to
        assert!(validator_rewards >= validator_rewards_paid);

        info!(
            "distributed inflation: {} (rounded from: {})",
            validator_rewards_paid, validator_rewards
        );
        let (num_stake_accounts, num_vote_accounts) = {
            let stakes = self.stakes_cache.stakes();
            (
                stakes.stake_delegations().len(),
                stakes.vote_accounts().len(),
            )
        };
        self.capitalization
            .fetch_add(validator_rewards_paid, Relaxed);

        let active_stake = if let Some(stake_history_entry) =
            self.stakes_cache.stakes().history().get(prev_epoch)
        {
            stake_history_entry.effective
        } else {
            0
        };

        datapoint_warn!(
            "epoch_rewards",
            ("slot", self.slot, i64),
            ("epoch", prev_epoch, i64),
            ("validator_rate", validator_rate, f64),
            ("foundation_rate", foundation_rate, f64),
            ("epoch_duration_in_years", prev_epoch_duration_in_years, f64),
            ("validator_rewards", validator_rewards_paid, i64),
            ("active_stake", active_stake, i64),
            ("pre_capitalization", capitalization, i64),
            ("post_capitalization", self.capitalization(), i64),
            ("num_stake_accounts", num_stake_accounts, i64),
            ("num_vote_accounts", num_vote_accounts, i64),
        );
    }

    fn filter_stake_delegations<'a>(
        &self,
        stakes: &'a Stakes<StakeAccount<Delegation>>,
    ) -> Vec<(&'a Pubkey, &'a StakeAccount<Delegation>)> {
        if self
            .feature_set
            .is_active(&feature_set::stake_minimum_delegation_for_rewards::id())
        {
            let num_stake_delegations = stakes.stake_delegations().len();
            let min_stake_delegation =
                solana_stake_program::get_minimum_delegation(&self.feature_set)
                    .max(LAMPORTS_PER_SOL);

            let (stake_delegations, filter_timer) = measure!(stakes
                .stake_delegations()
                .iter()
                .filter(|(_stake_pubkey, cached_stake_account)| {
                    cached_stake_account.delegation().stake >= min_stake_delegation
                })
                .collect::<Vec<_>>());

            datapoint_info!(
                "stake_account_filter_time",
                ("filter_time_us", filter_timer.as_us(), i64),
                ("num_stake_delegations_before", num_stake_delegations, i64),
                ("num_stake_delegations_after", stake_delegations.len(), i64)
            );
            stake_delegations
        } else {
            stakes.stake_delegations().iter().collect()
        }
    }

    fn _load_vote_and_stake_accounts(
        &self,
        thread_pool: &ThreadPool,
        reward_calc_tracer: Option<impl RewardCalcTracer>,
    ) -> LoadVoteAndStakeAccountsResult {
        let stakes = self.stakes_cache.stakes();
        let stake_delegations = self.filter_stake_delegations(&stakes);

        // Obtain all unique voter pubkeys from stake delegations.
        fn merge(mut acc: HashSet<Pubkey>, other: HashSet<Pubkey>) -> HashSet<Pubkey> {
            if acc.len() < other.len() {
                return merge(other, acc);
            }
            acc.extend(other);
            acc
        }
        let voter_pubkeys = thread_pool.install(|| {
            stake_delegations
                .par_iter()
                .fold(
                    HashSet::default,
                    |mut voter_pubkeys, (_stake_pubkey, stake_account)| {
                        let delegation = stake_account.delegation();
                        voter_pubkeys.insert(delegation.voter_pubkey);
                        voter_pubkeys
                    },
                )
                .reduce(HashSet::default, merge)
        });
        // Obtain vote-accounts for unique voter pubkeys.
        let cached_vote_accounts = stakes.vote_accounts();
        let solana_vote_program: Pubkey = solana_vote_program::id();
        let vote_accounts_cache_miss_count = AtomicUsize::default();
        let get_vote_account = |vote_pubkey: &Pubkey| -> Option<VoteAccount> {
            if let Some(vote_account) = cached_vote_accounts.get(vote_pubkey) {
                return Some(vote_account.clone());
            }
            // If accounts-db contains a valid vote account, then it should
            // already have been cached in cached_vote_accounts; so the code
            // below is only for sanity check, and can be removed once
            // vote_accounts_cache_miss_count is shown to be always zero.
            let account = self.get_account_with_fixed_root(vote_pubkey)?;
            if account.owner() == &solana_vote_program
                && VoteState::deserialize(account.data()).is_ok()
            {
                vote_accounts_cache_miss_count.fetch_add(1, Relaxed);
            }
            VoteAccount::try_from(account).ok()
        };
        let invalid_vote_keys = DashMap::<Pubkey, InvalidCacheEntryReason>::new();
        let make_vote_delegations_entry = |vote_pubkey| {
            let Some(vote_account) = get_vote_account(&vote_pubkey) else {
                invalid_vote_keys.insert(vote_pubkey, InvalidCacheEntryReason::Missing);
                return None;
            };
            if vote_account.owner() != &solana_vote_program {
                invalid_vote_keys.insert(vote_pubkey, InvalidCacheEntryReason::WrongOwner);
                return None;
            }
            let Ok(vote_state) = vote_account.vote_state().cloned() else {
                invalid_vote_keys.insert(vote_pubkey, InvalidCacheEntryReason::BadState);
                return None;
            };
            let vote_with_stake_delegations = VoteWithStakeDelegations {
                vote_state: Arc::new(vote_state),
                vote_account: AccountSharedData::from(vote_account),
                delegations: Vec::default(),
            };
            Some((vote_pubkey, vote_with_stake_delegations))
        };
        let vote_with_stake_delegations_map: DashMap<Pubkey, VoteWithStakeDelegations> =
            thread_pool.install(|| {
                voter_pubkeys
                    .into_par_iter()
                    .filter_map(make_vote_delegations_entry)
                    .collect()
            });
        // Join stake accounts with vote-accounts.
        let push_stake_delegation = |(stake_pubkey, stake_account): (&Pubkey, &StakeAccount<_>)| {
            let delegation = stake_account.delegation();
            let Some(mut vote_delegations) =
                vote_with_stake_delegations_map.get_mut(&delegation.voter_pubkey)
            else {
                return;
            };
            if let Some(reward_calc_tracer) = reward_calc_tracer.as_ref() {
                let delegation =
                    InflationPointCalculationEvent::Delegation(delegation, solana_vote_program);
                let event = RewardCalculationEvent::Staking(stake_pubkey, &delegation);
                reward_calc_tracer(&event);
            }
            let stake_delegation = (*stake_pubkey, stake_account.clone());
            vote_delegations.delegations.push(stake_delegation);
        };
        thread_pool.install(|| {
            stake_delegations
                .into_par_iter()
                .for_each(push_stake_delegation);
        });
        LoadVoteAndStakeAccountsResult {
            vote_with_stake_delegations_map,
            invalid_vote_keys,
            vote_accounts_cache_miss_count: vote_accounts_cache_miss_count.into_inner(),
        }
    }

    /// calculate and return some reward calc info to avoid recalculation across functions
    fn get_epoch_reward_calculate_param_info<'a>(
        &self,
        stakes: &'a Stakes<StakeAccount<Delegation>>,
    ) -> EpochRewardCalculateParamInfo<'a> {
        let stake_history = self.stakes_cache.stakes().history().clone();

        let stake_delegations = self.filter_stake_delegations(stakes);

        let cached_vote_accounts = stakes.vote_accounts();

        EpochRewardCalculateParamInfo {
            stake_history,
            stake_delegations,
            cached_vote_accounts,
        }
    }

    /// Calculate epoch reward and return vote and stake rewards.
    fn calculate_validator_rewards(
        &self,
        rewarded_epoch: Epoch,
        rewards: u64,
        reward_calc_tracer: Option<impl RewardCalcTracer>,
        thread_pool: &ThreadPool,
        metrics: &mut RewardsMetrics,
    ) -> Option<(VoteRewardsAccounts, StakeRewardCalculation)> {
        let stakes = self.stakes_cache.stakes();
        let reward_calculate_param = self.get_epoch_reward_calculate_param_info(&stakes);

        self.calculate_reward_points_partitioned(
            &reward_calculate_param,
            rewards,
            thread_pool,
            metrics,
        )
        .map(|point_value| {
            self.calculate_stake_vote_rewards(
                &reward_calculate_param,
                rewarded_epoch,
                point_value,
                thread_pool,
                reward_calc_tracer,
                metrics,
            )
        })
    }

    /// Load, calculate and payout epoch rewards for stake and vote accounts
    fn pay_validator_rewards_with_thread_pool(
        &mut self,
        rewarded_epoch: Epoch,
        rewards: u64,
        reward_calc_tracer: Option<impl RewardCalcTracer>,
        thread_pool: &ThreadPool,
        metrics: &mut RewardsMetrics,
    ) {
        let stake_history = self.stakes_cache.stakes().history().clone();
        let vote_with_stake_delegations_map =
            self.load_vote_and_stake_accounts(thread_pool, reward_calc_tracer.as_ref(), metrics);

        let point_value = self.calculate_reward_points(
            &vote_with_stake_delegations_map,
            rewards,
            &stake_history,
            thread_pool,
            metrics,
        );

        if let Some(point_value) = point_value {
            let (vote_account_rewards, stake_rewards) = self.redeem_rewards(
                vote_with_stake_delegations_map,
                rewarded_epoch,
                point_value,
                &stake_history,
                thread_pool,
                reward_calc_tracer.as_ref(),
                metrics,
            );

            // this checking of an unactivated feature can be enabled in tests or with a validator by passing `--partitioned-epoch-rewards-compare-calculation`
            if self
                .partitioned_epoch_rewards_config()
                .test_compare_partitioned_epoch_rewards
            {
                // immutable `&self` to avoid side effects
                (self as &Bank).compare_with_partitioned_rewards(
                    &stake_rewards,
                    &vote_account_rewards,
                    rewarded_epoch,
                    thread_pool,
                    null_tracer(),
                );
            }

            self.store_stake_accounts(thread_pool, &stake_rewards, metrics);
            let vote_rewards = self.store_vote_accounts(vote_account_rewards, metrics);
            self.update_reward_history(stake_rewards, vote_rewards);
        }
    }

    /// compare the vote and stake accounts between the normal rewards calculation code
    /// and the partitioned rewards calculation code
    /// `stake_rewards_expected` and `vote_rewards_expected` are the results of the normal rewards calculation code
    /// This fn should have NO side effects.
    /// This fn is only called in tests or with a debug cli arg prior to partitioned rewards feature activation.
    fn compare_with_partitioned_rewards_results(
        stake_rewards_expected: &[StakeReward],
        vote_rewards_expected: &DashMap<Pubkey, VoteReward>,
        partitioned_rewards: PartitionedRewardsCalculation,
    ) {
        // put partitioned stake rewards in a hashmap
        let mut stake_rewards: HashMap<Pubkey, &StakeReward> = HashMap::default();
        partitioned_rewards
            .stake_rewards_by_partition
            .stake_rewards_by_partition
            .iter()
            .flatten()
            .for_each(|stake_reward| {
                stake_rewards.insert(stake_reward.stake_pubkey, stake_reward);
            });

        // verify stake rewards match expected
        stake_rewards_expected.iter().for_each(|stake_reward| {
            let partitioned = stake_rewards.remove(&stake_reward.stake_pubkey).unwrap();
            assert_eq!(partitioned, stake_reward);
        });
        assert!(stake_rewards.is_empty(), "{stake_rewards:?}");

        let mut vote_rewards: HashMap<Pubkey, (RewardInfo, AccountSharedData)> = HashMap::default();
        partitioned_rewards
            .vote_account_rewards
            .accounts_to_store
            .iter()
            .enumerate()
            .for_each(|(i, account)| {
                if let Some(account) = account {
                    let reward = &partitioned_rewards.vote_account_rewards.rewards[i];
                    vote_rewards.insert(reward.0, (reward.1, account.clone()));
                }
            });

        // verify vote rewards match expected
        vote_rewards_expected.iter().for_each(|entry| {
            if entry.value().vote_needs_store {
                let partitioned = vote_rewards.remove(entry.key()).unwrap();
                let mut to_store_partitioned = partitioned.1.clone();
                to_store_partitioned.set_lamports(partitioned.0.post_balance);
                let mut to_store_normal = entry.value().vote_account.clone();
                _ = to_store_normal.checked_add_lamports(entry.value().vote_rewards);
                assert_eq!(to_store_partitioned, to_store_normal, "{:?}", entry.key());
            }
        });
        assert!(vote_rewards.is_empty(), "{vote_rewards:?}");
        info!(
            "verified partitioned rewards calculation matching: {}, {}",
            partitioned_rewards
                .stake_rewards_by_partition
                .stake_rewards_by_partition
                .iter()
                .map(|rewards| rewards.len())
                .sum::<usize>(),
            partitioned_rewards
                .vote_account_rewards
                .accounts_to_store
                .len()
        );
    }

    /// compare the vote and stake accounts between the normal rewards calculation code
    /// and the partitioned rewards calculation code
    /// `stake_rewards_expected` and `vote_rewards_expected` are the results of the normal rewards calculation code
    /// This fn should have NO side effects.
    fn compare_with_partitioned_rewards(
        &self,
        stake_rewards_expected: &[StakeReward],
        vote_rewards_expected: &DashMap<Pubkey, VoteReward>,
        rewarded_epoch: Epoch,
        thread_pool: &ThreadPool,
        reward_calc_tracer: Option<impl RewardCalcTracer>,
    ) {
        let partitioned_rewards = self.calculate_rewards_for_partitioning(
            rewarded_epoch,
            reward_calc_tracer,
            thread_pool,
            &mut RewardsMetrics::default(),
        );
        Self::compare_with_partitioned_rewards_results(
            stake_rewards_expected,
            vote_rewards_expected,
            partitioned_rewards,
        );
    }

    fn load_vote_and_stake_accounts(
        &mut self,
        thread_pool: &ThreadPool,
        reward_calc_tracer: Option<impl RewardCalcTracer>,
        metrics: &mut RewardsMetrics,
    ) -> VoteWithStakeDelegationsMap {
        let (
            LoadVoteAndStakeAccountsResult {
                vote_with_stake_delegations_map,
                invalid_vote_keys,
                vote_accounts_cache_miss_count,
            },
            measure,
        ) = measure!({
            self._load_vote_and_stake_accounts(thread_pool, reward_calc_tracer.as_ref())
        });
        metrics
            .load_vote_and_stake_accounts_us
            .fetch_add(measure.as_us(), Relaxed);
        metrics.vote_accounts_cache_miss_count += vote_accounts_cache_miss_count;
        self.stakes_cache
            .handle_invalid_keys(invalid_vote_keys, self.slot());
        vote_with_stake_delegations_map
    }

    /// Calculates epoch reward points from stake/vote accounts.
    /// Returns reward lamports and points for the epoch or none if points == 0.
    fn calculate_reward_points_partitioned(
        &self,
        reward_calculate_params: &EpochRewardCalculateParamInfo,
        rewards: u64,
        thread_pool: &ThreadPool,
        metrics: &RewardsMetrics,
    ) -> Option<PointValue> {
        let EpochRewardCalculateParamInfo {
            stake_history,
            stake_delegations,
            cached_vote_accounts,
        } = reward_calculate_params;

        let solana_vote_program: Pubkey = solana_vote_program::id();

        let get_vote_account = |vote_pubkey: &Pubkey| -> Option<VoteAccount> {
            if let Some(vote_account) = cached_vote_accounts.get(vote_pubkey) {
                return Some(vote_account.clone());
            }
            // If accounts-db contains a valid vote account, then it should
            // already have been cached in cached_vote_accounts; so the code
            // below is only for sanity checking, and can be removed once
            // the cache is deemed to be reliable.
            let account = self.get_account_with_fixed_root(vote_pubkey)?;
            VoteAccount::try_from(account).ok()
        };

        let new_warmup_cooldown_rate_epoch = self.new_warmup_cooldown_rate_epoch();
        let (points, measure_us) = measure_us!(thread_pool.install(|| {
            stake_delegations
                .par_iter()
                .map(|(_stake_pubkey, stake_account)| {
                    let delegation = stake_account.delegation();
                    let vote_pubkey = delegation.voter_pubkey;

                    let Some(vote_account) = get_vote_account(&vote_pubkey) else {
                        return 0;
                    };
                    if vote_account.owner() != &solana_vote_program {
                        return 0;
                    }
                    let Ok(vote_state) = vote_account.vote_state() else {
                        return 0;
                    };

                    stake_state::calculate_points(
                        stake_account.stake_state(),
                        vote_state,
                        Some(stake_history),
                        new_warmup_cooldown_rate_epoch,
                    )
                    .unwrap_or(0)
                })
                .sum::<u128>()
        }));
        metrics.calculate_points_us.fetch_add(measure_us, Relaxed);

        (points > 0).then_some(PointValue { rewards, points })
    }

    fn calculate_reward_points(
        &self,
        vote_with_stake_delegations_map: &VoteWithStakeDelegationsMap,
        rewards: u64,
        stake_history: &StakeHistory,
        thread_pool: &ThreadPool,
        metrics: &RewardsMetrics,
    ) -> Option<PointValue> {
        let new_warmup_cooldown_rate_epoch = self.new_warmup_cooldown_rate_epoch();
        let (points, measure) = measure!(thread_pool.install(|| {
            vote_with_stake_delegations_map
                .par_iter()
                .map(|entry| {
                    let VoteWithStakeDelegations {
                        vote_state,
                        delegations,
                        ..
                    } = entry.value();

                    delegations
                        .par_iter()
                        .map(|(_stake_pubkey, stake_account)| {
                            stake_state::calculate_points(
                                stake_account.stake_state(),
                                vote_state,
                                Some(stake_history),
                                new_warmup_cooldown_rate_epoch,
                            )
                            .unwrap_or(0)
                        })
                        .sum::<u128>()
                })
                .sum()
        }));
        metrics
            .calculate_points_us
            .fetch_add(measure.as_us(), Relaxed);

        (points > 0).then_some(PointValue { rewards, points })
    }

    /// Calculates epoch rewards for stake/vote accounts
    /// Returns vote rewards, stake rewards, and the sum of all stake rewards in lamports
    fn calculate_stake_vote_rewards(
        &self,
        reward_calculate_params: &EpochRewardCalculateParamInfo,
        rewarded_epoch: Epoch,
        point_value: PointValue,
        thread_pool: &ThreadPool,
        reward_calc_tracer: Option<impl RewardCalcTracer>,
        metrics: &mut RewardsMetrics,
    ) -> (VoteRewardsAccounts, StakeRewardCalculation) {
        let EpochRewardCalculateParamInfo {
            stake_history,
            stake_delegations,
            cached_vote_accounts,
        } = reward_calculate_params;

        let solana_vote_program: Pubkey = solana_vote_program::id();

        let get_vote_account = |vote_pubkey: &Pubkey| -> Option<VoteAccount> {
            if let Some(vote_account) = cached_vote_accounts.get(vote_pubkey) {
                return Some(vote_account.clone());
            }
            // If accounts-db contains a valid vote account, then it should
            // already have been cached in cached_vote_accounts; so the code
            // below is only for sanity checking, and can be removed once
            // the cache is deemed to be reliable.
            let account = self.get_account_with_fixed_root(vote_pubkey)?;
            VoteAccount::try_from(account).ok()
        };

        let new_warmup_cooldown_rate_epoch = self.new_warmup_cooldown_rate_epoch();
        let vote_account_rewards: VoteRewards = DashMap::new();
        let total_stake_rewards = AtomicU64::default();
        let (stake_rewards, measure_stake_rewards_us) = measure_us!(thread_pool.install(|| {
            stake_delegations
                .par_iter()
                .filter_map(|(stake_pubkey, stake_account)| {
                    // curry closure to add the contextual stake_pubkey
                    let reward_calc_tracer = reward_calc_tracer.as_ref().map(|outer| {
                        // inner
                        move |inner_event: &_| {
                            outer(&RewardCalculationEvent::Staking(stake_pubkey, inner_event))
                        }
                    });

                    let stake_pubkey = **stake_pubkey;
                    let stake_account = (*stake_account).to_owned();

                    let delegation = stake_account.delegation();
                    let (mut stake_account, stake_state) =
                        <(AccountSharedData, StakeStateV2)>::from(stake_account);
                    let vote_pubkey = delegation.voter_pubkey;
                    let Some(vote_account) = get_vote_account(&vote_pubkey) else {
                        return None;
                    };
                    if vote_account.owner() != &solana_vote_program {
                        return None;
                    }
                    let Ok(vote_state) = vote_account.vote_state().cloned() else {
                        return None;
                    };

                    let pre_lamport = stake_account.lamports();

                    let redeemed = stake_state::redeem_rewards(
                        rewarded_epoch,
                        stake_state,
                        &mut stake_account,
                        &vote_state,
                        &point_value,
                        Some(stake_history),
                        reward_calc_tracer.as_ref(),
                        new_warmup_cooldown_rate_epoch,
                    );

                    let post_lamport = stake_account.lamports();

                    if let Ok((stakers_reward, voters_reward)) = redeemed {
                        debug!(
                            "calculated reward: {} {} {} {}",
                            stake_pubkey, pre_lamport, post_lamport, stakers_reward
                        );

                        // track voter rewards
                        let mut voters_reward_entry = vote_account_rewards
                            .entry(vote_pubkey)
                            .or_insert(VoteReward {
                                vote_account: vote_account.into(),
                                commission: vote_state.commission,
                                vote_rewards: 0,
                                vote_needs_store: false,
                            });

                        voters_reward_entry.vote_needs_store = true;
                        voters_reward_entry.vote_rewards = voters_reward_entry
                            .vote_rewards
                            .saturating_add(voters_reward);

                        let post_balance = stake_account.lamports();
                        total_stake_rewards.fetch_add(stakers_reward, Relaxed);
                        return Some(StakeReward {
                            stake_pubkey,
                            stake_reward_info: RewardInfo {
                                reward_type: RewardType::Staking,
                                lamports: i64::try_from(stakers_reward).unwrap(),
                                post_balance,
                                commission: Some(vote_state.commission),
                            },
                            stake_account,
                        });
                    } else {
                        debug!(
                            "stake_state::redeem_rewards() failed for {}: {:?}",
                            stake_pubkey, redeemed
                        );
                    }
                    None
                })
                .collect()
        }));
        let (vote_rewards, measure_vote_rewards_us) =
            measure_us!(Self::calc_vote_accounts_to_store(vote_account_rewards));

        metrics.redeem_rewards_us += measure_stake_rewards_us + measure_vote_rewards_us;

        (
            vote_rewards,
            StakeRewardCalculation {
                stake_rewards,
                total_stake_rewards_lamports: total_stake_rewards.load(Relaxed),
            },
        )
    }

    fn redeem_rewards(
        &self,
        vote_with_stake_delegations_map: DashMap<Pubkey, VoteWithStakeDelegations>,
        rewarded_epoch: Epoch,
        point_value: PointValue,
        stake_history: &StakeHistory,
        thread_pool: &ThreadPool,
        reward_calc_tracer: Option<impl RewardCalcTracer>,
        metrics: &mut RewardsMetrics,
    ) -> (VoteRewards, StakeRewards) {
        let new_warmup_cooldown_rate_epoch = self.new_warmup_cooldown_rate_epoch();
        let vote_account_rewards: VoteRewards =
            DashMap::with_capacity(vote_with_stake_delegations_map.len());
        let stake_delegation_iterator = vote_with_stake_delegations_map.into_par_iter().flat_map(
            |(
                vote_pubkey,
                VoteWithStakeDelegations {
                    vote_state,
                    vote_account,
                    delegations,
                },
            )| {
                vote_account_rewards.insert(
                    vote_pubkey,
                    VoteReward {
                        vote_account,
                        commission: vote_state.commission,
                        vote_rewards: 0,
                        vote_needs_store: false,
                    },
                );
                delegations
                    .into_par_iter()
                    .map(move |delegation| (vote_pubkey, Arc::clone(&vote_state), delegation))
            },
        );

        let (stake_rewards, measure) = measure!(thread_pool.install(|| {
            stake_delegation_iterator
                .filter_map(|(vote_pubkey, vote_state, (stake_pubkey, stake_account))| {
                    // curry closure to add the contextual stake_pubkey
                    let reward_calc_tracer = reward_calc_tracer.as_ref().map(|outer| {
                        // inner
                        move |inner_event: &_| {
                            outer(&RewardCalculationEvent::Staking(&stake_pubkey, inner_event))
                        }
                    });
                    let (mut stake_account, stake_state) =
                        <(AccountSharedData, StakeStateV2)>::from(stake_account);
                    let redeemed = stake_state::redeem_rewards(
                        rewarded_epoch,
                        stake_state,
                        &mut stake_account,
                        &vote_state,
                        &point_value,
                        Some(stake_history),
                        reward_calc_tracer.as_ref(),
                        new_warmup_cooldown_rate_epoch,
                    );
                    if let Ok((stakers_reward, voters_reward)) = redeemed {
                        // track voter rewards
                        if let Some(VoteReward {
                            vote_account: _,
                            commission: _,
                            vote_rewards: vote_rewards_sum,
                            vote_needs_store,
                        }) = vote_account_rewards.get_mut(&vote_pubkey).as_deref_mut()
                        {
                            *vote_needs_store = true;
                            *vote_rewards_sum = vote_rewards_sum.saturating_add(voters_reward);
                        }

                        let post_balance = stake_account.lamports();
                        return Some(StakeReward {
                            stake_pubkey,
                            stake_reward_info: RewardInfo {
                                reward_type: RewardType::Staking,
                                lamports: i64::try_from(stakers_reward).unwrap(),
                                post_balance,
                                commission: Some(vote_state.commission),
                            },
                            stake_account,
                        });
                    } else {
                        debug!(
                            "stake_state::redeem_rewards() failed for {}: {:?}",
                            stake_pubkey, redeemed
                        );
                    }
                    None
                })
                .collect()
        }));
        metrics.redeem_rewards_us += measure.as_us();
        (vote_account_rewards, stake_rewards)
    }

    fn store_stake_accounts(
        &self,
        thread_pool: &ThreadPool,
        stake_rewards: &[StakeReward],
        metrics: &RewardsMetrics,
    ) {
        // store stake account even if stake_reward is 0
        // because credits observed has changed
        let now = Instant::now();
        let slot = self.slot();
        let include_slot_in_hash = self.include_slot_in_hash();
        self.stakes_cache.update_stake_accounts(
            thread_pool,
            stake_rewards,
            self.new_warmup_cooldown_rate_epoch(),
        );
        assert!(!self.freeze_started());
        thread_pool.install(|| {
            stake_rewards.par_chunks(512).for_each(|chunk| {
                self.rc
                    .accounts
                    .store_accounts_cached((slot, chunk, include_slot_in_hash))
            })
        });
        metrics
            .store_stake_accounts_us
            .fetch_add(now.elapsed().as_micros() as u64, Relaxed);
    }

    /// store stake rewards in partition
    /// return the sum of all the stored rewards
    ///
    /// Note: even if staker's reward is 0, the stake account still needs to be stored because
    /// credits observed has changed
    fn store_stake_accounts_in_partition(&self, stake_rewards: &[StakeReward]) -> u64 {
        // Verify that stake account `lamports + reward_amount` matches what we have in the
        // rewarded account. This code will have a performance hit - an extra load and compare of
        // the stake accounts. This is for debugging. Once we are confident, we can disable the
        // check.
        const VERIFY_REWARD_LAMPORT: bool = true;

        if VERIFY_REWARD_LAMPORT {
            for r in stake_rewards {
                let stake_pubkey = r.stake_pubkey;
                let reward_amount = r.get_stake_reward();
                let post_stake_account = &r.stake_account;
                if let Some(curr_stake_account) = self.get_account_with_fixed_root(&stake_pubkey) {
                    let pre_lamport = curr_stake_account.lamports();
                    let post_lamport = post_stake_account.lamports();
                    assert_eq!(pre_lamport + u64::try_from(reward_amount).unwrap(), post_lamport,
                        "stake account balance has changed since the reward calculation! account: {stake_pubkey}, pre balance: {pre_lamport}, post balance: {post_lamport}, rewards: {reward_amount}");
                }
            }
        }

        self.store_accounts((self.slot(), stake_rewards, self.include_slot_in_hash()));
        stake_rewards
            .iter()
            .map(|stake_reward| stake_reward.stake_reward_info.lamports)
            .sum::<i64>() as u64
    }

    fn store_vote_accounts_partitioned(
        &self,
        vote_account_rewards: VoteRewardsAccounts,
        metrics: &RewardsMetrics,
    ) -> Vec<(Pubkey, RewardInfo)> {
        let (_, measure_us) = measure_us!({
            // reformat data to make it not sparse.
            // `StorableAccounts` does not efficiently handle sparse data.
            // Not all entries in `vote_account_rewards.accounts_to_store` have a Some(account) to store.
            let to_store = vote_account_rewards
                .accounts_to_store
                .iter()
                .filter_map(|account| account.as_ref())
                .enumerate()
                .map(|(i, account)| (&vote_account_rewards.rewards[i].0, account))
                .collect::<Vec<_>>();
            self.store_accounts((self.slot(), &to_store[..], self.include_slot_in_hash()));
        });

        metrics
            .store_vote_accounts_us
            .fetch_add(measure_us, Relaxed);

        vote_account_rewards.rewards
    }

    fn store_vote_accounts(
        &self,
        vote_account_rewards: VoteRewards,
        metrics: &RewardsMetrics,
    ) -> Vec<(Pubkey, RewardInfo)> {
        let (vote_rewards, measure) = measure!(vote_account_rewards
            .into_iter()
            .filter_map(
                |(
                    vote_pubkey,
                    VoteReward {
                        mut vote_account,
                        commission,
                        vote_rewards,
                        vote_needs_store,
                    },
                )| {
                    if let Err(err) = vote_account.checked_add_lamports(vote_rewards) {
                        debug!("reward redemption failed for {}: {:?}", vote_pubkey, err);
                        return None;
                    }

                    if vote_needs_store {
                        self.store_account(&vote_pubkey, &vote_account);
                    }

                    Some((
                        vote_pubkey,
                        RewardInfo {
                            reward_type: RewardType::Voting,
                            lamports: vote_rewards as i64,
                            post_balance: vote_account.lamports(),
                            commission: Some(commission),
                        },
                    ))
                },
            )
            .collect::<Vec<_>>());

        metrics
            .store_vote_accounts_us
            .fetch_add(measure.as_us(), Relaxed);
        vote_rewards
    }

    /// return reward info for each vote account
    /// return account data for each vote account that needs to be stored
    /// This return value is a little awkward at the moment so that downstream existing code in the non-partitioned rewards code path can be re-used without duplication or modification.
    /// This function is copied from the existing code path's `store_vote_accounts`.
    /// The primary differences:
    /// - we want this fn to have no side effects (such as actually storing vote accounts) so that we
    ///   can compare the expected results with the current code path
    /// - we want to be able to batch store the vote accounts later for improved performance/cache updating
    fn calc_vote_accounts_to_store(
        vote_account_rewards: DashMap<Pubkey, VoteReward>,
    ) -> VoteRewardsAccounts {
        let len = vote_account_rewards.len();
        let mut result = VoteRewardsAccounts {
            rewards: Vec::with_capacity(len),
            accounts_to_store: Vec::with_capacity(len),
        };
        vote_account_rewards.into_iter().for_each(
            |(
                vote_pubkey,
                VoteReward {
                    mut vote_account,
                    commission,
                    vote_rewards,
                    vote_needs_store,
                },
            )| {
                if let Err(err) = vote_account.checked_add_lamports(vote_rewards) {
                    debug!("reward redemption failed for {}: {:?}", vote_pubkey, err);
                    return;
                }

                result.rewards.push((
                    vote_pubkey,
                    RewardInfo {
                        reward_type: RewardType::Voting,
                        lamports: vote_rewards as i64,
                        post_balance: vote_account.lamports(),
                        commission: Some(commission),
                    },
                ));
                result
                    .accounts_to_store
                    .push(vote_needs_store.then_some(vote_account));
            },
        );
        result
    }

    fn update_reward_history(
        &self,
        stake_rewards: StakeRewards,
        mut vote_rewards: Vec<(Pubkey, RewardInfo)>,
    ) {
        let additional_reserve = stake_rewards.len() + vote_rewards.len();
        let mut rewards = self.rewards.write().unwrap();
        rewards.reserve(additional_reserve);
        rewards.append(&mut vote_rewards);
        stake_rewards
            .into_iter()
            .filter(|x| x.get_stake_reward() > 0)
            .for_each(|x| rewards.push((x.stake_pubkey, x.stake_reward_info)));
    }

    /// insert non-zero stake rewards to self.rewards
    /// Return the number of rewards inserted
    fn update_reward_history_in_partition(&self, stake_rewards: &[StakeReward]) -> usize {
        let mut rewards = self.rewards.write().unwrap();
        rewards.reserve(stake_rewards.len());
        let initial_len = rewards.len();
        stake_rewards
            .iter()
            .filter(|x| x.get_stake_reward() > 0)
            .for_each(|x| rewards.push((x.stake_pubkey, x.stake_reward_info)));
        rewards.len().saturating_sub(initial_len)
    }

    /// Process reward credits for a partition of rewards
    /// Store the rewards to AccountsDB, update reward history record and total capitalization.
    fn distribute_epoch_rewards_in_partition(
        &self,
        all_stake_rewards: &[Vec<StakeReward>],
        partition_index: u64,
    ) {
        let pre_capitalization = self.capitalization();
        let this_partition_stake_rewards = &all_stake_rewards[partition_index as usize];

        let (total_rewards_in_lamports, store_stake_accounts_us) =
            measure_us!(self.store_stake_accounts_in_partition(this_partition_stake_rewards));

        // increase total capitalization by the distributed rewards
        self.capitalization
            .fetch_add(total_rewards_in_lamports, Relaxed);

        // decrease distributed capital from epoch rewards sysvar
        self.update_epoch_rewards_sysvar(total_rewards_in_lamports);

        // update reward history for this partitioned distribution
        self.update_reward_history_in_partition(this_partition_stake_rewards);

        let metrics = RewardsStoreMetrics {
            pre_capitalization,
            post_capitalization: self.capitalization(),
            total_stake_accounts_count: all_stake_rewards.len(),
            partition_index,
            store_stake_accounts_us,
            store_stake_accounts_count: this_partition_stake_rewards.len(),
            distributed_rewards: total_rewards_in_lamports,
        };

        report_partitioned_reward_metrics(self, metrics);
    }

    /// true if it is ok to run partitioned rewards code.
    /// This means the feature is activated or certain testing situations.
    fn is_partitioned_rewards_code_enabled(&self) -> bool {
        self.is_partitioned_rewards_feature_enabled()
            || self
                .partitioned_epoch_rewards_config()
                .test_enable_partitioned_rewards
    }

    /// Helper fn to log epoch_rewards sysvar
    fn log_epoch_rewards_sysvar(&self, prefix: &str) {
        if let Some(account) = self.get_account(&sysvar::epoch_rewards::id()) {
            let epoch_rewards: sysvar::epoch_rewards::EpochRewards =
                from_account(&account).unwrap();
            info!(
                "{prefix} epoch_rewards sysvar: {:?}",
                (account.lamports(), epoch_rewards)
            );
        } else {
            info!("{prefix} epoch_rewards sysvar: none");
        }
    }

    /// Create EpochRewards sysvar with calculated rewards
    fn create_epoch_rewards_sysvar(
        &self,
        total_rewards: u64,
        distributed_rewards: u64,
        distribution_complete_block_height: u64,
    ) {
        assert!(self.is_partitioned_rewards_code_enabled());

        let epoch_rewards = sysvar::epoch_rewards::EpochRewards {
            total_rewards,
            distributed_rewards,
            distribution_complete_block_height,
        };

        self.update_sysvar_account(&sysvar::epoch_rewards::id(), |account| {
            let mut inherited_account_fields =
                self.inherit_specially_retained_account_fields(account);

            assert!(total_rewards >= distributed_rewards);
            // set the account lamports to the undistributed rewards
            inherited_account_fields.0 = total_rewards - distributed_rewards;
            create_account(&epoch_rewards, inherited_account_fields)
        });

        self.log_epoch_rewards_sysvar("create");
    }

    /// Update EpochRewards sysvar with distributed rewards
    fn update_epoch_rewards_sysvar(&self, distributed: u64) {
        assert!(self.is_partitioned_rewards_code_enabled());

        let mut epoch_rewards: sysvar::epoch_rewards::EpochRewards =
            from_account(&self.get_account(&sysvar::epoch_rewards::id()).unwrap()).unwrap();
        epoch_rewards.distribute(distributed);

        self.update_sysvar_account(&sysvar::epoch_rewards::id(), |account| {
            let mut inherited_account_fields =
                self.inherit_specially_retained_account_fields(account);

            let lamports = inherited_account_fields.0;
            assert!(lamports >= distributed);
            inherited_account_fields.0 = lamports - distributed;
            create_account(&epoch_rewards, inherited_account_fields)
        });

        self.log_epoch_rewards_sysvar("update");
    }

    fn update_recent_blockhashes_locked(&self, locked_blockhash_queue: &BlockhashQueue) {
        #[allow(deprecated)]
        self.update_sysvar_account(&sysvar::recent_blockhashes::id(), |account| {
            let recent_blockhash_iter = locked_blockhash_queue.get_recent_blockhashes();
            recent_blockhashes_account::create_account_with_data_and_fields(
                recent_blockhash_iter,
                self.inherit_specially_retained_account_fields(account),
            )
        });
    }

    pub fn update_recent_blockhashes(&self) {
        let blockhash_queue = self.blockhash_queue.read().unwrap();
        self.update_recent_blockhashes_locked(&blockhash_queue);
    }

    fn get_timestamp_estimate(
        &self,
        max_allowable_drift: MaxAllowableDrift,
        epoch_start_timestamp: Option<(Slot, UnixTimestamp)>,
    ) -> Option<UnixTimestamp> {
        let mut get_timestamp_estimate_time = Measure::start("get_timestamp_estimate");
        let slots_per_epoch = self.epoch_schedule().slots_per_epoch;
        let vote_accounts = self.vote_accounts();
        let recent_timestamps = vote_accounts.iter().filter_map(|(pubkey, (_, account))| {
            let vote_state = account.vote_state();
            let vote_state = vote_state.as_ref().ok()?;
            let slot_delta = self.slot().checked_sub(vote_state.last_timestamp.slot)?;
            (slot_delta <= slots_per_epoch).then_some({
                (
                    *pubkey,
                    (
                        vote_state.last_timestamp.slot,
                        vote_state.last_timestamp.timestamp,
                    ),
                )
            })
        });
        let slot_duration = Duration::from_nanos(self.ns_per_slot as u64);
        let epoch = self.epoch_schedule().get_epoch(self.slot());
        let stakes = self.epoch_vote_accounts(epoch)?;
        let stake_weighted_timestamp = calculate_stake_weighted_timestamp(
            recent_timestamps,
            stakes,
            self.slot(),
            slot_duration,
            epoch_start_timestamp,
            max_allowable_drift,
            self.feature_set
                .is_active(&feature_set::warp_timestamp_again::id()),
        );
        get_timestamp_estimate_time.stop();
        datapoint_info!(
            "bank-timestamp",
            (
                "get_timestamp_estimate_us",
                get_timestamp_estimate_time.as_us(),
                i64
            ),
        );
        stake_weighted_timestamp
    }

    pub fn rehash(&self) {
        let mut hash = self.hash.write().unwrap();
        let new = self.hash_internal_state();
        if new != *hash {
            warn!("Updating bank hash to {}", new);
            *hash = new;
        }
    }

    pub fn freeze(&self) {
        // This lock prevents any new commits from BankingStage
        // `Consumer::execute_and_commit_transactions_locked()` from
        // coming in after the last tick is observed. This is because in
        // BankingStage, any transaction successfully recorded in
        // `record_transactions()` is recorded after this `hash` lock
        // is grabbed. At the time of the successful record,
        // this means the PoH has not yet reached the last tick,
        // so this means freeze() hasn't been called yet. And because
        // BankingStage doesn't release this hash lock until both
        // record and commit are finished, those transactions will be
        // committed before this write lock can be obtained here.
        let mut hash = self.hash.write().unwrap();
        if *hash == Hash::default() {
            // finish up any deferred changes to account state
            self.collect_rent_eagerly();
            self.distribute_transaction_fees();
            self.distribute_rent_fees();
            self.update_slot_history();
            self.run_incinerator();

            // freeze is a one-way trip, idempotent
            self.freeze_started.store(true, Relaxed);
            *hash = self.hash_internal_state();
            self.rc.accounts.accounts_db.mark_slot_frozen(self.slot());
        }
    }

    // dangerous; don't use this; this is only needed for ledger-tool's special command
    pub fn unfreeze_for_ledger_tool(&self) {
        self.freeze_started.store(false, Relaxed);
    }

    pub fn epoch_schedule(&self) -> &EpochSchedule {
        &self.epoch_schedule
    }

    /// squash the parent's state up into this Bank,
    ///   this Bank becomes a root
    /// Note that this function is not thread-safe. If it is called concurrently on the same bank
    /// by multiple threads, the end result could be inconsistent.
    /// Calling code does not currently call this concurrently.
    pub fn squash(&self) -> SquashTiming {
        self.freeze();

        //this bank and all its parents are now on the rooted path
        let mut roots = vec![self.slot()];
        roots.append(&mut self.parents().iter().map(|p| p.slot()).collect());

        let mut total_index_us = 0;
        let mut total_cache_us = 0;
        let mut total_store_us = 0;

        let mut squash_accounts_time = Measure::start("squash_accounts_time");
        for slot in roots.iter().rev() {
            // root forks cannot be purged
            let add_root_timing = self.rc.accounts.add_root(*slot);
            total_index_us += add_root_timing.index_us;
            total_cache_us += add_root_timing.cache_us;
            total_store_us += add_root_timing.store_us;
        }
        squash_accounts_time.stop();

        *self.rc.parent.write().unwrap() = None;

        let mut squash_cache_time = Measure::start("squash_cache_time");
        roots
            .iter()
            .for_each(|slot| self.status_cache.write().unwrap().add_root(*slot));
        squash_cache_time.stop();

        SquashTiming {
            squash_accounts_ms: squash_accounts_time.as_ms(),
            squash_accounts_index_ms: total_index_us / 1000,
            squash_accounts_cache_ms: total_cache_us / 1000,
            squash_accounts_store_ms: total_store_us / 1000,

            squash_cache_ms: squash_cache_time.as_ms(),
        }
    }

    /// Return the more recent checkpoint of this bank instance.
    pub fn parent(&self) -> Option<Arc<Bank>> {
        self.rc.parent.read().unwrap().clone()
    }

    pub fn parent_slot(&self) -> Slot {
        self.parent_slot
    }

    pub fn parent_hash(&self) -> Hash {
        self.parent_hash
    }

    fn process_genesis_config(&mut self, genesis_config: &GenesisConfig) {
        // Bootstrap validator collects fees until `new_from_parent` is called.
        self.fee_rate_governor = genesis_config.fee_rate_governor.clone();

        // Make sure to activate the account_hash_ignore_slot feature
        // before calculating any account hashes.
        if genesis_config
            .accounts
            .iter()
            .any(|(pubkey, _)| pubkey == &feature_set::account_hash_ignore_slot::id())
        {
            self.activate_feature(&feature_set::account_hash_ignore_slot::id());
        }

        for (pubkey, account) in genesis_config.accounts.iter() {
            assert!(
                self.get_account(pubkey).is_none(),
                "{pubkey} repeated in genesis config"
            );
            self.store_account(pubkey, account);
            self.capitalization.fetch_add(account.lamports(), Relaxed);
            self.accounts_data_size_initial += account.data().len() as u64;
        }
        // updating sysvars (the fees sysvar in this case) now depends on feature activations in
        // genesis_config.accounts above
        self.update_fees();

        for (pubkey, account) in genesis_config.rewards_pools.iter() {
            assert!(
                self.get_account(pubkey).is_none(),
                "{pubkey} repeated in genesis config"
            );
            self.store_account(pubkey, account);
            self.accounts_data_size_initial += account.data().len() as u64;
        }

        // Highest staked node is the first collector but if a genesis config
        // doesn't define any staked nodes, we assume this genesis config is for
        // testing and set the collector id to a unique pubkey.
        self.collector_id = self
            .stakes_cache
            .stakes()
            .highest_staked_node()
            .unwrap_or_else(Pubkey::new_unique);

        self.blockhash_queue.write().unwrap().genesis_hash(
            &genesis_config.hash(),
            self.fee_rate_governor.lamports_per_signature,
        );

        self.hashes_per_tick = genesis_config.hashes_per_tick();
        self.ticks_per_slot = genesis_config.ticks_per_slot();
        self.ns_per_slot = genesis_config.ns_per_slot();
        self.genesis_creation_time = genesis_config.creation_time;
        self.max_tick_height = (self.slot + 1) * self.ticks_per_slot;
        self.slots_per_year = genesis_config.slots_per_year();

        self.epoch_schedule = genesis_config.epoch_schedule;

        self.inflation = Arc::new(RwLock::new(genesis_config.inflation));

        self.rent_collector = RentCollector::new(
            self.epoch,
            *self.epoch_schedule(),
            self.slots_per_year,
            genesis_config.rent,
        );

        // Add additional builtin programs specified in the genesis config
        for (name, program_id) in &genesis_config.native_instruction_processors {
            self.add_builtin_account(name, program_id, false);
        }
    }

    fn burn_and_purge_account(&self, program_id: &Pubkey, mut account: AccountSharedData) {
        let old_data_size = account.data().len();
        self.capitalization.fetch_sub(account.lamports(), Relaxed);
        // Both resetting account balance to 0 and zeroing the account data
        // is needed to really purge from AccountsDb and flush the Stakes cache
        account.set_lamports(0);
        account.data_as_mut_slice().fill(0);
        self.store_account(program_id, &account);
        self.calculate_and_update_accounts_data_size_delta_off_chain(old_data_size, 0);
    }

    // NOTE: must hold idempotent for the same set of arguments
    /// Add a builtin program account
    pub fn add_builtin_account(&self, name: &str, program_id: &Pubkey, must_replace: bool) {
        let existing_genuine_program =
            self.get_account_with_fixed_root(program_id)
                .and_then(|account| {
                    // it's very unlikely to be squatted at program_id as non-system account because of burden to
                    // find victim's pubkey/hash. So, when account.owner is indeed native_loader's, it's
                    // safe to assume it's a genuine program.
                    if native_loader::check_id(account.owner()) {
                        Some(account)
                    } else {
                        // malicious account is pre-occupying at program_id
                        self.burn_and_purge_account(program_id, account);
                        None
                    }
                });

        if must_replace {
            // updating builtin program
            match &existing_genuine_program {
                None => panic!(
                    "There is no account to replace with builtin program ({name}, {program_id})."
                ),
                Some(account) => {
                    if *name == String::from_utf8_lossy(account.data()) {
                        // The existing account is well formed
                        return;
                    }
                }
            }
        } else {
            // introducing builtin program
            if existing_genuine_program.is_some() {
                // The existing account is sufficient
                return;
            }
        }

        assert!(
            !self.freeze_started(),
            "Can't change frozen bank by adding not-existing new builtin program ({name}, {program_id}). \
            Maybe, inconsistent program activation is detected on snapshot restore?"
        );

        // Add a bogus executable builtin account, which will be loaded and ignored.
        let account = native_loader::create_loadable_account_with_fields(
            name,
            self.inherit_specially_retained_account_fields(&existing_genuine_program),
        );
        self.store_account_and_update_capitalization(program_id, &account);
    }

    /// Add a precompiled program account
    pub fn add_precompiled_account(&self, program_id: &Pubkey) {
        self.add_precompiled_account_with_owner(program_id, native_loader::id())
    }

    // Used by tests to simulate clusters with precompiles that aren't owned by the native loader
    fn add_precompiled_account_with_owner(&self, program_id: &Pubkey, owner: Pubkey) {
        if let Some(account) = self.get_account_with_fixed_root(program_id) {
            if account.executable() {
                // The account is already executable, that's all we need
                return;
            } else {
                // malicious account is pre-occupying at program_id
                self.burn_and_purge_account(program_id, account);
            }
        };

        assert!(
            !self.freeze_started(),
            "Can't change frozen bank by adding not-existing new precompiled program ({program_id}). \
                Maybe, inconsistent program activation is detected on snapshot restore?"
        );

        // Add a bogus executable account, which will be loaded and ignored.
        let (lamports, rent_epoch) = self.inherit_specially_retained_account_fields(&None);
        let account = AccountSharedData::from(Account {
            lamports,
            owner,
            data: vec![],
            executable: true,
            rent_epoch,
        });
        self.store_account_and_update_capitalization(program_id, &account);
    }

    pub fn set_rent_burn_percentage(&mut self, burn_percent: u8) {
        self.rent_collector.rent.burn_percent = burn_percent;
    }

    pub fn set_hashes_per_tick(&mut self, hashes_per_tick: Option<u64>) {
        self.hashes_per_tick = hashes_per_tick;
    }

    /// Return the last block hash registered.
    pub fn last_blockhash(&self) -> Hash {
        self.blockhash_queue.read().unwrap().last_hash()
    }

    pub fn last_blockhash_and_lamports_per_signature(&self) -> (Hash, u64) {
        let blockhash_queue = self.blockhash_queue.read().unwrap();
        let last_hash = blockhash_queue.last_hash();
        let last_lamports_per_signature = blockhash_queue
            .get_lamports_per_signature(&last_hash)
            .unwrap(); // safe so long as the BlockhashQueue is consistent
        (last_hash, last_lamports_per_signature)
    }

    pub fn is_blockhash_valid(&self, hash: &Hash) -> bool {
        let blockhash_queue = self.blockhash_queue.read().unwrap();
        blockhash_queue.is_hash_valid(hash)
    }

    pub fn get_minimum_balance_for_rent_exemption(&self, data_len: usize) -> u64 {
        self.rent_collector.rent.minimum_balance(data_len).max(1)
    }

    pub fn get_lamports_per_signature(&self) -> u64 {
        self.fee_rate_governor.lamports_per_signature
    }

    pub fn get_lamports_per_signature_for_blockhash(&self, hash: &Hash) -> Option<u64> {
        let blockhash_queue = self.blockhash_queue.read().unwrap();
        blockhash_queue.get_lamports_per_signature(hash)
    }

    #[deprecated(since = "1.9.0", note = "Please use `get_fee_for_message` instead")]
    pub fn get_fee_rate_governor(&self) -> &FeeRateGovernor {
        &self.fee_rate_governor
    }

    pub fn get_fee_for_message(&self, message: &SanitizedMessage) -> Option<u64> {
        let lamports_per_signature = {
            let blockhash_queue = self.blockhash_queue.read().unwrap();
            blockhash_queue.get_lamports_per_signature(message.recent_blockhash())
        }
        .or_else(|| {
            self.check_message_for_nonce(message)
                .and_then(|(address, account)| {
                    NoncePartial::new(address, account).lamports_per_signature()
                })
        })?;
        Some(self.get_fee_for_message_with_lamports_per_signature(message, lamports_per_signature))
    }

    /// Returns true when startup accounts hash verification has completed or never had to run in background.
    pub fn get_startup_verification_complete(&self) -> &Arc<AtomicBool> {
        &self
            .rc
            .accounts
            .accounts_db
            .verify_accounts_hash_in_bg
            .verified
    }

    /// return true if bg hash verification is complete
    /// return false if bg hash verification has not completed yet
    /// if hash verification failed, a panic will occur
    pub fn is_startup_verification_complete(&self) -> bool {
        self.rc
            .accounts
            .accounts_db
            .verify_accounts_hash_in_bg
            .check_complete()
    }

    /// This can occur because it completed in the background
    /// or if the verification was run in the foreground.
    pub fn set_startup_verification_complete(&self) {
        self.rc
            .accounts
            .accounts_db
            .verify_accounts_hash_in_bg
            .verification_complete()
    }

    pub fn get_fee_for_message_with_lamports_per_signature(
        &self,
        message: &SanitizedMessage,
        lamports_per_signature: u64,
    ) -> u64 {
        self.fee_structure.calculate_fee(
            message,
            lamports_per_signature,
            &ComputeBudget::fee_budget_limits(
                message.program_instructions_iter(),
                &self.feature_set,
            ),
            self.feature_set
                .is_active(&remove_congestion_multiplier_from_fee_calculation::id()),
            self.feature_set
                .is_active(&include_loaded_accounts_data_size_in_fee_calculation::id()),
        )
    }

    #[deprecated(
        since = "1.6.11",
        note = "Please use `get_blockhash_last_valid_block_height`"
    )]
    pub fn get_blockhash_last_valid_slot(&self, blockhash: &Hash) -> Option<Slot> {
        let blockhash_queue = self.blockhash_queue.read().unwrap();
        // This calculation will need to be updated to consider epoch boundaries if BlockhashQueue
        // length is made variable by epoch
        blockhash_queue
            .get_hash_age(blockhash)
            .map(|age| self.slot + blockhash_queue.get_max_age() as u64 - age)
    }

    pub fn get_blockhash_last_valid_block_height(&self, blockhash: &Hash) -> Option<Slot> {
        let blockhash_queue = self.blockhash_queue.read().unwrap();
        // This calculation will need to be updated to consider epoch boundaries if BlockhashQueue
        // length is made variable by epoch
        blockhash_queue
            .get_hash_age(blockhash)
            .map(|age| self.block_height + blockhash_queue.get_max_age() as u64 - age)
    }

    pub fn confirmed_last_blockhash(&self) -> Hash {
        const NUM_BLOCKHASH_CONFIRMATIONS: usize = 3;

        let parents = self.parents();
        if parents.is_empty() {
            self.last_blockhash()
        } else {
            let index = NUM_BLOCKHASH_CONFIRMATIONS.min(parents.len() - 1);
            parents[index].last_blockhash()
        }
    }

    /// Forget all signatures. Useful for benchmarking.
    pub fn clear_signatures(&self) {
        self.status_cache.write().unwrap().clear();
    }

    pub fn clear_slot_signatures(&self, slot: Slot) {
        self.status_cache.write().unwrap().clear_slot_entries(slot);
    }

    fn update_transaction_statuses(
        &self,
        sanitized_txs: &[SanitizedTransaction],
        execution_results: &[TransactionExecutionResult],
    ) {
        let mut status_cache = self.status_cache.write().unwrap();
        assert_eq!(sanitized_txs.len(), execution_results.len());
        for (tx, execution_result) in sanitized_txs.iter().zip(execution_results) {
            if let Some(details) = execution_result.details() {
                // Add the message hash to the status cache to ensure that this message
                // won't be processed again with a different signature.
                status_cache.insert(
                    tx.message().recent_blockhash(),
                    tx.message_hash(),
                    self.slot(),
                    details.status.clone(),
                );
                // Add the transaction signature to the status cache so that transaction status
                // can be queried by transaction signature over RPC. In the future, this should
                // only be added for API nodes because voting validators don't need to do this.
                status_cache.insert(
                    tx.message().recent_blockhash(),
                    tx.signature(),
                    self.slot(),
                    details.status.clone(),
                );
            }
        }
    }

    /// Register a new recent blockhash in the bank's recent blockhash queue. Called when a bank
    /// reaches its max tick height. Can be called by tests to get new blockhashes for transaction
    /// processing without advancing to a new bank slot.
    pub fn register_recent_blockhash(&self, blockhash: &Hash) {
        // Only acquire the write lock for the blockhash queue on block boundaries because
        // readers can starve this write lock acquisition and ticks would be slowed down too
        // much if the write lock is acquired for each tick.
        let mut w_blockhash_queue = self.blockhash_queue.write().unwrap();
        w_blockhash_queue.register_hash(blockhash, self.fee_rate_governor.lamports_per_signature);
        self.update_recent_blockhashes_locked(&w_blockhash_queue);
    }

    /// Tell the bank which Entry IDs exist on the ledger. This function assumes subsequent calls
    /// correspond to later entries, and will boot the oldest ones once its internal cache is full.
    /// Once boot, the bank will reject transactions using that `hash`.
    ///
    /// This is NOT thread safe because if tick height is updated by two different threads, the
    /// block boundary condition could be missed.
    pub fn register_tick(&self, hash: &Hash) {
        assert!(
            !self.freeze_started(),
            "register_tick() working on a bank that is already frozen or is undergoing freezing!"
        );

        if self.is_block_boundary(self.tick_height.load(Relaxed) + 1) {
            self.register_recent_blockhash(hash);
        }

        // ReplayStage will start computing the accounts delta hash when it
        // detects the tick height has reached the boundary, so the system
        // needs to guarantee all account updates for the slot have been
        // committed before this tick height is incremented (like the blockhash
        // sysvar above)
        self.tick_height.fetch_add(1, Relaxed);
    }

    pub fn is_complete(&self) -> bool {
        self.tick_height() == self.max_tick_height()
    }

    pub fn is_block_boundary(&self, tick_height: u64) -> bool {
        tick_height == self.max_tick_height
    }

    /// Get the max number of accounts that a transaction may lock in this block
    pub fn get_transaction_account_lock_limit(&self) -> usize {
        if let Some(transaction_account_lock_limit) =
            self.runtime_config.transaction_account_lock_limit
        {
            transaction_account_lock_limit
        } else if self
            .feature_set
            .is_active(&feature_set::increase_tx_account_lock_limit::id())
        {
            MAX_TX_ACCOUNT_LOCKS
        } else {
            64
        }
    }

    /// Prepare a transaction batch from a list of legacy transactions. Used for tests only.
    pub fn prepare_batch_for_tests(&self, txs: Vec<Transaction>) -> TransactionBatch {
        let transaction_account_lock_limit = self.get_transaction_account_lock_limit();
        let sanitized_txs = txs
            .into_iter()
            .map(SanitizedTransaction::from_transaction_for_tests)
            .collect::<Vec<_>>();
        let lock_results = self
            .rc
            .accounts
            .lock_accounts(sanitized_txs.iter(), transaction_account_lock_limit);
        TransactionBatch::new(lock_results, self, Cow::Owned(sanitized_txs))
    }

    /// Prepare a transaction batch from a list of versioned transactions from
    /// an entry. Used for tests only.
    pub fn prepare_entry_batch(&self, txs: Vec<VersionedTransaction>) -> Result<TransactionBatch> {
        let sanitized_txs = txs
            .into_iter()
            .map(|tx| SanitizedTransaction::try_create(tx, MessageHash::Compute, None, self))
            .collect::<Result<Vec<_>>>()?;
        let tx_account_lock_limit = self.get_transaction_account_lock_limit();
        let lock_results = self
            .rc
            .accounts
            .lock_accounts(sanitized_txs.iter(), tx_account_lock_limit);
        Ok(TransactionBatch::new(
            lock_results,
            self,
            Cow::Owned(sanitized_txs),
        ))
    }

    /// Prepare a locked transaction batch from a list of sanitized transactions.
    pub fn prepare_sanitized_batch<'a, 'b>(
        &'a self,
        txs: &'b [SanitizedTransaction],
    ) -> TransactionBatch<'a, 'b> {
        let tx_account_lock_limit = self.get_transaction_account_lock_limit();
        let lock_results = self
            .rc
            .accounts
            .lock_accounts(txs.iter(), tx_account_lock_limit);
        TransactionBatch::new(lock_results, self, Cow::Borrowed(txs))
    }

    /// Prepare a locked transaction batch from a list of sanitized transactions, and their cost
    /// limited packing status
    pub fn prepare_sanitized_batch_with_results<'a, 'b>(
        &'a self,
        transactions: &'b [SanitizedTransaction],
        transaction_results: impl Iterator<Item = Result<()>>,
    ) -> TransactionBatch<'a, 'b> {
        // this lock_results could be: Ok, AccountInUse, WouldExceedBlockMaxLimit or WouldExceedAccountMaxLimit
        let tx_account_lock_limit = self.get_transaction_account_lock_limit();
        let lock_results = self.rc.accounts.lock_accounts_with_results(
            transactions.iter(),
            transaction_results,
            tx_account_lock_limit,
        );
        TransactionBatch::new(lock_results, self, Cow::Borrowed(transactions))
    }

    /// Prepare a transaction batch from a single transaction without locking accounts
    pub(crate) fn prepare_unlocked_batch_from_single_tx<'a>(
        &'a self,
        transaction: &'a SanitizedTransaction,
    ) -> TransactionBatch<'_, '_> {
        let tx_account_lock_limit = self.get_transaction_account_lock_limit();
        let lock_result = transaction
            .get_account_locks(tx_account_lock_limit)
            .map(|_| ());
        let mut batch = TransactionBatch::new(
            vec![lock_result],
            self,
            Cow::Borrowed(slice::from_ref(transaction)),
        );
        batch.set_needs_unlock(false);
        batch
    }

    /// Run transactions against a frozen bank without committing the results
    pub fn simulate_transaction(
        &self,
        transaction: SanitizedTransaction,
    ) -> TransactionSimulationResult {
        assert!(self.is_frozen(), "simulation bank must be frozen");

        self.simulate_transaction_unchecked(transaction)
    }

    /// Run transactions against a bank without committing the results; does not check if the bank
    /// is frozen, enabling use in single-Bank test frameworks
    pub fn simulate_transaction_unchecked(
        &self,
        transaction: SanitizedTransaction,
    ) -> TransactionSimulationResult {
        let account_keys = transaction.message().account_keys();
        let number_of_accounts = account_keys.len();
        let account_overrides = self.get_account_overrides_for_simulation(&account_keys);
        let batch = self.prepare_unlocked_batch_from_single_tx(&transaction);
        let mut timings = ExecuteTimings::default();

        let LoadAndExecuteTransactionsOutput {
            loaded_transactions,
            mut execution_results,
            ..
        } = self.load_and_execute_transactions(
            &batch,
            // After simulation, transactions will need to be forwarded to the leader
            // for processing. During forwarding, the transaction could expire if the
            // delay is not accounted for.
            MAX_PROCESSING_AGE - MAX_TRANSACTION_FORWARDING_DELAY,
            false,
            true,
            true,
            &mut timings,
            Some(&account_overrides),
            None,
        );

        let post_simulation_accounts = loaded_transactions
            .into_iter()
            .next()
            .unwrap()
            .0
            .ok()
            .map(|loaded_transaction| {
                loaded_transaction
                    .accounts
                    .into_iter()
                    .take(number_of_accounts)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let units_consumed = timings
            .details
            .per_program_timings
            .iter()
            .fold(0, |acc: u64, (_, program_timing)| {
                acc.saturating_add(program_timing.accumulated_units)
            });

        debug!("simulate_transaction: {:?}", timings);

        let execution_result = execution_results.pop().unwrap();
        let flattened_result = execution_result.flattened_result();
        let (logs, return_data) = match execution_result {
            TransactionExecutionResult::Executed { details, .. } => {
                (details.log_messages, details.return_data)
            }
            TransactionExecutionResult::NotExecuted(_) => (None, None),
        };
        let logs = logs.unwrap_or_default();

        TransactionSimulationResult {
            result: flattened_result,
            logs,
            post_simulation_accounts,
            units_consumed,
            return_data,
        }
    }

    fn get_account_overrides_for_simulation(&self, account_keys: &AccountKeys) -> AccountOverrides {
        let mut account_overrides = AccountOverrides::default();
        let slot_history_id = sysvar::slot_history::id();
        if account_keys.iter().any(|pubkey| *pubkey == slot_history_id) {
            let current_account = self.get_account_with_fixed_root(&slot_history_id);
            let slot_history = current_account
                .as_ref()
                .map(|account| from_account::<SlotHistory, _>(account).unwrap())
                .unwrap_or_default();
            if slot_history.check(self.slot()) == Check::Found {
                let ancestors = Ancestors::from(self.proper_ancestors().collect::<Vec<_>>());
                if let Some((account, _)) =
                    self.load_slow_with_fixed_root(&ancestors, &slot_history_id)
                {
                    account_overrides.set_slot_history(Some(account));
                }
            }
        }
        account_overrides
    }

    pub fn unlock_accounts(&self, batch: &mut TransactionBatch) {
        if batch.needs_unlock() {
            batch.set_needs_unlock(false);
            self.rc
                .accounts
                .unlock_accounts(batch.sanitized_transactions().iter(), batch.lock_results())
        }
    }

    pub fn remove_unrooted_slots(&self, slots: &[(Slot, BankId)]) {
        self.rc.accounts.accounts_db.remove_unrooted_slots(slots)
    }

    pub fn set_shrink_paths(&self, paths: Vec<PathBuf>) {
        self.rc.accounts.accounts_db.set_shrink_paths(paths);
    }

    fn check_age<'a>(
        &self,
        txs: impl Iterator<Item = &'a SanitizedTransaction>,
        lock_results: &[Result<()>],
        max_age: usize,
        error_counters: &mut TransactionErrorMetrics,
    ) -> Vec<TransactionCheckResult> {
        let hash_queue = self.blockhash_queue.read().unwrap();
        let last_blockhash = hash_queue.last_hash();
        let next_durable_nonce = DurableNonce::from_blockhash(&last_blockhash);

        txs.zip(lock_results)
            .map(|(tx, lock_res)| match lock_res {
                Ok(()) => self.check_transaction_age(
                    tx,
                    max_age,
                    &next_durable_nonce,
                    &hash_queue,
                    error_counters,
                ),
                Err(e) => (Err(e.clone()), None),
            })
            .collect()
    }

    fn check_transaction_age(
        &self,
        tx: &SanitizedTransaction,
        max_age: usize,
        next_durable_nonce: &DurableNonce,
        hash_queue: &BlockhashQueue,
        error_counters: &mut TransactionErrorMetrics,
    ) -> TransactionCheckResult {
        let recent_blockhash = tx.message().recent_blockhash();
        if hash_queue.is_hash_valid_for_age(recent_blockhash, max_age) {
            (Ok(()), None)
        } else if let Some((address, account)) =
            self.check_transaction_for_nonce(tx, next_durable_nonce)
        {
            (Ok(()), Some(NoncePartial::new(address, account)))
        } else {
            error_counters.blockhash_not_found += 1;
            (Err(TransactionError::BlockhashNotFound), None)
        }
    }

    fn is_transaction_already_processed(
        &self,
        sanitized_tx: &SanitizedTransaction,
        status_cache: &BankStatusCache,
    ) -> bool {
        let key = sanitized_tx.message_hash();
        let transaction_blockhash = sanitized_tx.message().recent_blockhash();
        status_cache
            .get_status(key, transaction_blockhash, &self.ancestors)
            .is_some()
    }

    fn check_status_cache(
        &self,
        sanitized_txs: &[SanitizedTransaction],
        lock_results: Vec<TransactionCheckResult>,
        error_counters: &mut TransactionErrorMetrics,
    ) -> Vec<TransactionCheckResult> {
        let rcache = self.status_cache.read().unwrap();
        sanitized_txs
            .iter()
            .zip(lock_results)
            .map(|(sanitized_tx, (lock_result, nonce))| {
                if lock_result.is_ok()
                    && self.is_transaction_already_processed(sanitized_tx, &rcache)
                {
                    error_counters.already_processed += 1;
                    return (Err(TransactionError::AlreadyProcessed), None);
                }

                (lock_result, nonce)
            })
            .collect()
    }

    pub fn get_hash_age(&self, hash: &Hash) -> Option<u64> {
        self.blockhash_queue.read().unwrap().get_hash_age(hash)
    }

    pub fn is_hash_valid_for_age(&self, hash: &Hash, max_age: usize) -> bool {
        self.blockhash_queue
            .read()
            .unwrap()
            .is_hash_valid_for_age(hash, max_age)
    }

    fn check_message_for_nonce(&self, message: &SanitizedMessage) -> Option<TransactionAccount> {
        let nonce_address = message.get_durable_nonce()?;
        let nonce_account = self.get_account_with_fixed_root(nonce_address)?;
        let nonce_data =
            nonce_account::verify_nonce_account(&nonce_account, message.recent_blockhash())?;

        let nonce_is_authorized = message
            .get_ix_signers(NONCED_TX_MARKER_IX_INDEX as usize)
            .any(|signer| signer == &nonce_data.authority);
        if !nonce_is_authorized {
            return None;
        }

        Some((*nonce_address, nonce_account))
    }

    fn check_transaction_for_nonce(
        &self,
        tx: &SanitizedTransaction,
        next_durable_nonce: &DurableNonce,
    ) -> Option<TransactionAccount> {
        let nonce_is_advanceable = tx.message().recent_blockhash() != next_durable_nonce.as_hash();
        if nonce_is_advanceable {
            self.check_message_for_nonce(tx.message())
        } else {
            None
        }
    }

    pub fn check_transactions(
        &self,
        sanitized_txs: &[SanitizedTransaction],
        lock_results: &[Result<()>],
        max_age: usize,
        error_counters: &mut TransactionErrorMetrics,
    ) -> Vec<TransactionCheckResult> {
        let age_results =
            self.check_age(sanitized_txs.iter(), lock_results, max_age, error_counters);
        self.check_status_cache(sanitized_txs, age_results, error_counters)
    }

    pub fn collect_balances(&self, batch: &TransactionBatch) -> TransactionBalances {
        let mut balances: TransactionBalances = vec![];
        for transaction in batch.sanitized_transactions() {
            let mut transaction_balances: Vec<u64> = vec![];
            for account_key in transaction.message().account_keys().iter() {
                transaction_balances.push(self.get_balance(account_key));
            }
            balances.push(transaction_balances);
        }
        balances
    }

    fn program_modification_slot(&self, pubkey: &Pubkey) -> Result<Slot> {
        let program = self
            .get_account_with_fixed_root(pubkey)
            .ok_or(TransactionError::ProgramAccountNotFound)?;
        if bpf_loader_upgradeable::check_id(program.owner()) {
            if let Ok(UpgradeableLoaderState::Program {
                programdata_address,
            }) = program.state()
            {
                let programdata = self
                    .get_account_with_fixed_root(&programdata_address)
                    .ok_or(TransactionError::ProgramAccountNotFound)?;
                if let Ok(UpgradeableLoaderState::ProgramData {
                    slot,
                    upgrade_authority_address: _,
                }) = programdata.state()
                {
                    return Ok(slot);
                }
            }
            Err(TransactionError::ProgramAccountNotFound)
        } else if loader_v4::check_id(program.owner()) {
            let state = solana_loader_v4_program::get_state(program.data())
                .map_err(|_| TransactionError::ProgramAccountNotFound)?;
            Ok(state.slot)
        } else {
            Ok(0)
        }
    }

    fn load_program_accounts(&self, pubkey: &Pubkey) -> ProgramAccountLoadResult {
        let program_account = match self.get_account_with_fixed_root(pubkey) {
            None => return ProgramAccountLoadResult::AccountNotFound,
            Some(account) => account,
        };

        debug_assert!(solana_bpf_loader_program::check_loader_id(
            program_account.owner()
        ));

        if loader_v4::check_id(program_account.owner()) {
            return solana_loader_v4_program::get_state(program_account.data())
                .ok()
                .and_then(|state| {
                    (!matches!(state.status, LoaderV4Status::Retracted)).then_some(state.slot)
                })
                .map(|slot| ProgramAccountLoadResult::ProgramOfLoaderV4(program_account, slot))
                .unwrap_or(ProgramAccountLoadResult::InvalidV4Program);
        }

        if !bpf_loader_upgradeable::check_id(program_account.owner()) {
            return ProgramAccountLoadResult::ProgramOfLoaderV1orV2(program_account);
        }

        if let Ok(UpgradeableLoaderState::Program {
            programdata_address,
        }) = program_account.state()
        {
            let programdata_account = match self.get_account_with_fixed_root(&programdata_address) {
                None => return ProgramAccountLoadResult::AccountNotFound,
                Some(account) => account,
            };

            if let Ok(UpgradeableLoaderState::ProgramData {
                slot,
                upgrade_authority_address: _,
            }) = programdata_account.state()
            {
                return ProgramAccountLoadResult::ProgramOfLoaderV3(
                    program_account,
                    programdata_account,
                    slot,
                );
            }
        }
        ProgramAccountLoadResult::InvalidAccountData
    }

    pub fn load_program(
        &self,
        pubkey: &Pubkey,
        reload: bool,
        recompile: Option<Arc<LoadedProgram>>,
    ) -> Arc<LoadedProgram> {
        let loaded_programs_cache = self.loaded_programs_cache.read().unwrap();
        let effective_epoch = if recompile.is_some() {
            loaded_programs_cache.latest_root_epoch.saturating_add(1)
        } else {
            self.epoch
        };
        let environments = loaded_programs_cache.get_environments_for_epoch(effective_epoch);
        let mut load_program_metrics = LoadProgramMetrics {
            program_id: pubkey.to_string(),
            ..LoadProgramMetrics::default()
        };

        let mut loaded_program = match self.load_program_accounts(pubkey) {
            ProgramAccountLoadResult::AccountNotFound => Ok(LoadedProgram::new_tombstone(
                self.slot,
                LoadedProgramType::Closed,
            )),

            ProgramAccountLoadResult::InvalidAccountData => {
                Err(InstructionError::InvalidAccountData)
            }

            ProgramAccountLoadResult::ProgramOfLoaderV1orV2(program_account) => {
                solana_bpf_loader_program::load_program_from_bytes(
                    self.feature_set
                        .is_active(&feature_set::delay_visibility_of_program_deployment::id()),
                    None,
                    &mut load_program_metrics,
                    program_account.data(),
                    program_account.owner(),
                    program_account.data().len(),
                    0,
                    environments.program_runtime_v1.clone(),
                    reload,
                )
            }

            ProgramAccountLoadResult::ProgramOfLoaderV3(
                program_account,
                programdata_account,
                slot,
            ) => programdata_account
                .data()
                .get(UpgradeableLoaderState::size_of_programdata_metadata()..)
                .ok_or(InstructionError::InvalidAccountData)
                .and_then(|programdata| {
                    solana_bpf_loader_program::load_program_from_bytes(
                        self.feature_set
                            .is_active(&feature_set::delay_visibility_of_program_deployment::id()),
                        None,
                        &mut load_program_metrics,
                        programdata,
                        program_account.owner(),
                        program_account
                            .data()
                            .len()
                            .saturating_add(programdata_account.data().len()),
                        slot,
                        environments.program_runtime_v1.clone(),
                        reload,
                    )
                }),

            ProgramAccountLoadResult::ProgramOfLoaderV4(program_account, slot) => {
                let loaded_program = program_account
                    .data()
                    .get(LoaderV4State::program_data_offset()..)
                    .and_then(|elf_bytes| {
                        if reload {
                            // Safety: this is safe because the program is being reloaded in the cache.
                            unsafe {
                                LoadedProgram::reload(
                                    &loader_v4::id(),
                                    environments.program_runtime_v2.clone(),
                                    slot,
                                    slot.saturating_add(DELAY_VISIBILITY_SLOT_OFFSET),
                                    None,
                                    elf_bytes,
                                    program_account.data().len(),
                                    &mut load_program_metrics,
                                )
                            }
                        } else {
                            LoadedProgram::new(
                                &loader_v4::id(),
                                environments.program_runtime_v2.clone(),
                                slot,
                                slot.saturating_add(DELAY_VISIBILITY_SLOT_OFFSET),
                                None,
                                elf_bytes,
                                program_account.data().len(),
                                &mut load_program_metrics,
                            )
                        }
                        .ok()
                    })
                    .unwrap_or(LoadedProgram::new_tombstone(
                        self.slot,
                        LoadedProgramType::FailedVerification(
                            environments.program_runtime_v2.clone(),
                        ),
                    ));
                Ok(loaded_program)
            }

            ProgramAccountLoadResult::InvalidV4Program => Ok(LoadedProgram::new_tombstone(
                self.slot,
                LoadedProgramType::FailedVerification(environments.program_runtime_v2.clone()),
            )),
        }
        .unwrap_or_else(|_| {
            LoadedProgram::new_tombstone(
                self.slot,
                LoadedProgramType::FailedVerification(environments.program_runtime_v1.clone()),
            )
        });

        let mut timings = ExecuteDetailsTimings::default();
        load_program_metrics.submit_datapoint(&mut timings);
        if let Some(recompile) = recompile {
            loaded_program.effective_slot = loaded_program.effective_slot.max(
                self.epoch_schedule()
                    .get_first_slot_in_epoch(effective_epoch),
            );
            loaded_program.tx_usage_counter =
                AtomicU64::new(recompile.tx_usage_counter.load(Ordering::Relaxed));
            loaded_program.ix_usage_counter =
                AtomicU64::new(recompile.ix_usage_counter.load(Ordering::Relaxed));
        }
        Arc::new(loaded_program)
    }

    pub fn clear_program_cache(&self) {
        self.loaded_programs_cache
            .write()
            .unwrap()
            .unload_all_programs();
    }

    /// Execute a transaction using the provided loaded accounts and update
    /// the executors cache if the transaction was successful.
    #[allow(clippy::too_many_arguments)]
    fn execute_loaded_transaction(
        &self,
        tx: &SanitizedTransaction,
        loaded_transaction: &mut LoadedTransaction,
        compute_budget: ComputeBudget,
        durable_nonce_fee: Option<DurableNonceFee>,
        enable_cpi_recording: bool,
        enable_log_recording: bool,
        enable_return_data_recording: bool,
        timings: &mut ExecuteTimings,
        error_counters: &mut TransactionErrorMetrics,
        log_messages_bytes_limit: Option<usize>,
        programs_loaded_for_tx_batch: &LoadedProgramsForTxBatch,
    ) -> TransactionExecutionResult {
        let prev_accounts_data_len = self.load_accounts_data_size();
        let transaction_accounts = std::mem::take(&mut loaded_transaction.accounts);

        fn transaction_accounts_lamports_sum(
            accounts: &[(Pubkey, AccountSharedData)],
            message: &SanitizedMessage,
        ) -> Option<u128> {
            let mut lamports_sum = 0u128;
            for i in 0..message.account_keys().len() {
                let Some((_, account)) = accounts.get(i) else {
                    return None;
                };
                lamports_sum = lamports_sum.checked_add(u128::from(account.lamports()))?;
            }
            Some(lamports_sum)
        }

        let lamports_before_tx =
            transaction_accounts_lamports_sum(&transaction_accounts, tx.message()).unwrap_or(0);

        let mut transaction_context = TransactionContext::new(
            transaction_accounts,
            if self
                .feature_set
                .is_active(&enable_early_verification_of_account_modifications::id())
            {
                Some(self.rent_collector.rent)
            } else {
                None
            },
            compute_budget.max_invoke_stack_height,
            if self
                .feature_set
                .is_active(&feature_set::limit_max_instruction_trace_length::id())
            {
                compute_budget.max_instruction_trace_length
            } else {
                std::usize::MAX
            },
        );
        if self
            .feature_set
            .is_active(&feature_set::cap_accounts_data_allocations_per_transaction::id())
        {
            transaction_context.enable_cap_accounts_data_allocations_per_transaction();
        }
        #[cfg(debug_assertions)]
        transaction_context.set_signature(tx.signature());

        let pre_account_state_info =
            self.get_transaction_account_state_info(&transaction_context, tx.message());

        let log_collector = if enable_log_recording {
            match log_messages_bytes_limit {
                None => Some(LogCollector::new_ref()),
                Some(log_messages_bytes_limit) => Some(LogCollector::new_ref_with_limit(Some(
                    log_messages_bytes_limit,
                ))),
            }
        } else {
            None
        };

        let (blockhash, lamports_per_signature) = self.last_blockhash_and_lamports_per_signature();

        let mut executed_units = 0u64;
        let mut programs_modified_by_tx = LoadedProgramsForTxBatch::new(
            self.slot,
            programs_loaded_for_tx_batch.environments.clone(),
        );
        let mut programs_updated_only_for_global_cache = LoadedProgramsForTxBatch::new(
            self.slot,
            programs_loaded_for_tx_batch.environments.clone(),
        );
        let mut process_message_time = Measure::start("process_message_time");
        let process_result = MessageProcessor::process_message(
            tx.message(),
            &loaded_transaction.program_indices,
            &mut transaction_context,
            self.rent_collector.rent,
            log_collector.clone(),
            programs_loaded_for_tx_batch,
            &mut programs_modified_by_tx,
            &mut programs_updated_only_for_global_cache,
            self.feature_set.clone(),
            compute_budget,
            timings,
            &self.sysvar_cache.read().unwrap(),
            blockhash,
            lamports_per_signature,
            prev_accounts_data_len,
            &mut executed_units,
        );
        process_message_time.stop();

        saturating_add_assign!(
            timings.execute_accessories.process_message_us,
            process_message_time.as_us()
        );

        let mut status = process_result
            .and_then(|info| {
                let post_account_state_info =
                    self.get_transaction_account_state_info(&transaction_context, tx.message());
                self.verify_transaction_account_state_changes(
                    &pre_account_state_info,
                    &post_account_state_info,
                    &transaction_context,
                )
                .map(|_| info)
            })
            .map_err(|err| {
                match err {
                    TransactionError::InvalidRentPayingAccount
                    | TransactionError::InsufficientFundsForRent { .. } => {
                        error_counters.invalid_rent_paying_account += 1;
                    }
                    TransactionError::InvalidAccountIndex => {
                        error_counters.invalid_account_index += 1;
                    }
                    _ => {
                        error_counters.instruction_error += 1;
                    }
                }
                err
            });

        let log_messages: Option<TransactionLogMessages> =
            log_collector.and_then(|log_collector| {
                Rc::try_unwrap(log_collector)
                    .map(|log_collector| log_collector.into_inner().into_messages())
                    .ok()
            });

        let inner_instructions = if enable_cpi_recording {
            Some(inner_instructions_list_from_instruction_trace(
                &transaction_context,
            ))
        } else {
            None
        };

        let ExecutionRecord {
            accounts,
            return_data,
            touched_account_count,
            accounts_resize_delta,
        } = transaction_context.into();

        if status.is_ok()
            && transaction_accounts_lamports_sum(&accounts, tx.message())
                .filter(|lamports_after_tx| lamports_before_tx == *lamports_after_tx)
                .is_none()
        {
            status = Err(TransactionError::UnbalancedTransaction);
        }
        let mut accounts_data_len_delta = status
            .as_ref()
            .map_or(0, |info| info.accounts_data_len_delta);
        let status = status.map(|_| ());

        loaded_transaction.accounts = accounts;
        if self
            .feature_set
            .is_active(&enable_early_verification_of_account_modifications::id())
        {
            saturating_add_assign!(
                timings.details.total_account_count,
                loaded_transaction.accounts.len() as u64
            );
            saturating_add_assign!(timings.details.changed_account_count, touched_account_count);
            accounts_data_len_delta = status.as_ref().map_or(0, |_| accounts_resize_delta);
        }

        let return_data = if enable_return_data_recording && !return_data.data.is_empty() {
            Some(return_data)
        } else {
            None
        };

        TransactionExecutionResult::Executed {
            details: TransactionExecutionDetails {
                status,
                log_messages,
                inner_instructions,
                durable_nonce_fee,
                return_data,
                executed_units,
                accounts_data_len_delta,
            },
            programs_modified_by_tx: Box::new(programs_modified_by_tx),
            programs_updated_only_for_global_cache: Box::new(
                programs_updated_only_for_global_cache,
            ),
        }
    }

    fn replenish_program_cache(
        &self,
        program_accounts_map: &HashMap<Pubkey, (&Pubkey, u64)>,
    ) -> LoadedProgramsForTxBatch {
        let mut missing_programs: Vec<(Pubkey, (LoadedProgramMatchCriteria, u64))> =
            if self.check_program_modification_slot {
                program_accounts_map
                    .iter()
                    .map(|(pubkey, (_, count))| {
                        (
                            *pubkey,
                            (
                                self.program_modification_slot(pubkey)
                                    .map_or(LoadedProgramMatchCriteria::Tombstone, |slot| {
                                        LoadedProgramMatchCriteria::DeployedOnOrAfterSlot(slot)
                                    }),
                                *count,
                            ),
                        )
                    })
                    .collect()
            } else {
                program_accounts_map
                    .iter()
                    .map(|(pubkey, (_, count))| {
                        (*pubkey, (LoadedProgramMatchCriteria::NoCriteria, *count))
                    })
                    .collect()
            };

        let mut loaded_programs_for_txs = None;
        let mut program_to_store = None;
        loop {
            let (program_to_load, task_cookie, task_waiter) = {
                // Lock the global cache.
                let mut loaded_programs_cache = self.loaded_programs_cache.write().unwrap();
                // Initialize our local cache.
                if loaded_programs_for_txs.is_none() {
                    loaded_programs_for_txs = Some(LoadedProgramsForTxBatch::new(
                        self.slot,
                        loaded_programs_cache
                            .get_environments_for_epoch(self.epoch)
                            .clone(),
                    ));
                }
                // Submit our last completed loading task.
                if let Some((key, program)) = program_to_store.take() {
                    loaded_programs_cache.finish_cooperative_loading_task(
                        self.slot(),
                        key,
                        program,
                    );
                }
                // Figure out which program needs to be loaded next.
                let program_to_load = loaded_programs_cache.extract(
                    self,
                    &mut missing_programs,
                    loaded_programs_for_txs.as_mut().unwrap(),
                );
                let task_waiter = Arc::clone(&loaded_programs_cache.loading_task_waiter);
                (program_to_load, task_waiter.cookie(), task_waiter)
                // Unlock the global cache again.
            };

            if let Some((key, count)) = program_to_load {
                // Load, verify and compile one program.
                let program = self.load_program(&key, false, None);
                program.tx_usage_counter.store(count, Ordering::Relaxed);
                program_to_store = Some((key, program));
            } else if missing_programs.is_empty() {
                break;
            } else {
                // Sleep until the next finish_cooperative_loading_task() call.
                // Once a task completes we'll wake up and try to load the
                // missing programs inside the tx batch again.
                let _new_cookie = task_waiter.wait(task_cookie);
            }
        }

        loaded_programs_for_txs.unwrap()
    }

    #[allow(clippy::type_complexity)]
    pub fn load_and_execute_transactions(
        &self,
        batch: &TransactionBatch,
        max_age: usize,
        enable_cpi_recording: bool,
        enable_log_recording: bool,
        enable_return_data_recording: bool,
        timings: &mut ExecuteTimings,
        account_overrides: Option<&AccountOverrides>,
        log_messages_bytes_limit: Option<usize>,
    ) -> LoadAndExecuteTransactionsOutput {
        let sanitized_txs = batch.sanitized_transactions();
        debug!("processing transactions: {}", sanitized_txs.len());
        let mut error_counters = TransactionErrorMetrics::default();

        let retryable_transaction_indexes: Vec<_> = batch
            .lock_results()
            .iter()
            .enumerate()
            .filter_map(|(index, res)| match res {
                // following are retryable errors
                Err(TransactionError::AccountInUse) => {
                    error_counters.account_in_use += 1;
                    Some(index)
                }
                Err(TransactionError::WouldExceedMaxBlockCostLimit) => {
                    error_counters.would_exceed_max_block_cost_limit += 1;
                    Some(index)
                }
                Err(TransactionError::WouldExceedMaxVoteCostLimit) => {
                    error_counters.would_exceed_max_vote_cost_limit += 1;
                    Some(index)
                }
                Err(TransactionError::WouldExceedMaxAccountCostLimit) => {
                    error_counters.would_exceed_max_account_cost_limit += 1;
                    Some(index)
                }
                Err(TransactionError::WouldExceedAccountDataBlockLimit) => {
                    error_counters.would_exceed_account_data_block_limit += 1;
                    Some(index)
                }
                // following are non-retryable errors
                Err(TransactionError::TooManyAccountLocks) => {
                    error_counters.too_many_account_locks += 1;
                    None
                }
                Err(_) => None,
                Ok(_) => None,
            })
            .collect();

        let mut check_time = Measure::start("check_transactions");
        let mut check_results = self.check_transactions(
            sanitized_txs,
            batch.lock_results(),
            max_age,
            &mut error_counters,
        );
        check_time.stop();

        const PROGRAM_OWNERS: &[Pubkey] = &[
            bpf_loader_upgradeable::id(),
            bpf_loader::id(),
            bpf_loader_deprecated::id(),
            loader_v4::id(),
        ];
        let mut program_accounts_map = self.rc.accounts.filter_executable_program_accounts(
            &self.ancestors,
            sanitized_txs,
            &mut check_results,
            PROGRAM_OWNERS,
            &self.blockhash_queue.read().unwrap(),
        );
        let native_loader = native_loader::id();
        for builtin_program in self.builtin_programs.iter() {
            program_accounts_map.insert(*builtin_program, (&native_loader, 0));
        }

        let programs_loaded_for_tx_batch = Rc::new(RefCell::new(
            self.replenish_program_cache(&program_accounts_map),
        ));

        let mut load_time = Measure::start("accounts_load");
        let mut loaded_transactions = self.rc.accounts.load_accounts(
            &self.ancestors,
            sanitized_txs,
            check_results,
            &self.blockhash_queue.read().unwrap(),
            &mut error_counters,
            &self.rent_collector,
            &self.feature_set,
            &self.fee_structure,
            account_overrides,
            self.get_reward_interval(),
            &program_accounts_map,
            &programs_loaded_for_tx_batch.borrow(),
        );
        load_time.stop();

        let mut execution_time = Measure::start("execution_time");
        let mut signature_count: u64 = 0;

        let execution_results: Vec<TransactionExecutionResult> = loaded_transactions
            .iter_mut()
            .zip(sanitized_txs.iter())
            .map(|(accs, tx)| match accs {
                (Err(e), _nonce) => TransactionExecutionResult::NotExecuted(e.clone()),
                (Ok(loaded_transaction), nonce) => {
                    let compute_budget = if let Some(compute_budget) =
                        self.runtime_config.compute_budget
                    {
                        compute_budget
                    } else {
                        let mut compute_budget =
                            ComputeBudget::new(compute_budget::MAX_COMPUTE_UNIT_LIMIT as u64);

                        let mut compute_budget_process_transaction_time =
                            Measure::start("compute_budget_process_transaction_time");
                        let process_transaction_result = compute_budget.process_instructions(
                            tx.message().program_instructions_iter(),
                            !self
                                .feature_set
                                .is_active(&remove_deprecated_request_unit_ix::id()),
                            self.feature_set
                                .is_active(&add_set_tx_loaded_accounts_data_size_instruction::id()),
                        );
                        compute_budget_process_transaction_time.stop();
                        saturating_add_assign!(
                            timings
                                .execute_accessories
                                .compute_budget_process_transaction_us,
                            compute_budget_process_transaction_time.as_us()
                        );
                        if let Err(err) = process_transaction_result {
                            return TransactionExecutionResult::NotExecuted(err);
                        }
                        compute_budget
                    };

                    let result = self.execute_loaded_transaction(
                        tx,
                        loaded_transaction,
                        compute_budget,
                        nonce.as_ref().map(DurableNonceFee::from),
                        enable_cpi_recording,
                        enable_log_recording,
                        enable_return_data_recording,
                        timings,
                        &mut error_counters,
                        log_messages_bytes_limit,
                        &programs_loaded_for_tx_batch.borrow(),
                    );

                    if let TransactionExecutionResult::Executed {
                        details,
                        programs_modified_by_tx,
                        programs_updated_only_for_global_cache: _,
                    } = &result
                    {
                        // Update batch specific cache of the loaded programs with the modifications
                        // made by the transaction, if it executed successfully.
                        if details.status.is_ok() {
                            programs_loaded_for_tx_batch
                                .borrow_mut()
                                .merge(programs_modified_by_tx);
                        }
                    }

                    result
                }
            })
            .collect();

        execution_time.stop();

        const SHRINK_LOADED_PROGRAMS_TO_PERCENTAGE: u8 = 90;
        self.loaded_programs_cache
            .write()
            .unwrap()
            .sort_and_unload(Percentage::from(SHRINK_LOADED_PROGRAMS_TO_PERCENTAGE));

        debug!(
            "check: {}us load: {}us execute: {}us txs_len={}",
            check_time.as_us(),
            load_time.as_us(),
            execution_time.as_us(),
            sanitized_txs.len(),
        );

        timings.saturating_add_in_place(ExecuteTimingType::CheckUs, check_time.as_us());
        timings.saturating_add_in_place(ExecuteTimingType::LoadUs, load_time.as_us());
        timings.saturating_add_in_place(ExecuteTimingType::ExecuteUs, execution_time.as_us());

        let mut executed_transactions_count: usize = 0;
        let mut executed_non_vote_transactions_count: usize = 0;
        let mut executed_with_successful_result_count: usize = 0;
        let err_count = &mut error_counters.total;
        let transaction_log_collector_config =
            self.transaction_log_collector_config.read().unwrap();

        let mut collect_logs_time = Measure::start("collect_logs_time");
        for (execution_result, tx) in execution_results.iter().zip(sanitized_txs) {
            if let Some(debug_keys) = &self.transaction_debug_keys {
                for key in tx.message().account_keys().iter() {
                    if debug_keys.contains(key) {
                        let result = execution_result.flattened_result();
                        info!("slot: {} result: {:?} tx: {:?}", self.slot, result, tx);
                        break;
                    }
                }
            }

            let is_vote = tx.is_simple_vote_transaction();

            if execution_result.was_executed() // Skip log collection for unprocessed transactions
                && transaction_log_collector_config.filter != TransactionLogCollectorFilter::None
            {
                let mut filtered_mentioned_addresses = Vec::new();
                if !transaction_log_collector_config
                    .mentioned_addresses
                    .is_empty()
                {
                    for key in tx.message().account_keys().iter() {
                        if transaction_log_collector_config
                            .mentioned_addresses
                            .contains(key)
                        {
                            filtered_mentioned_addresses.push(*key);
                        }
                    }
                }

                let store = match transaction_log_collector_config.filter {
                    TransactionLogCollectorFilter::All => {
                        !is_vote || !filtered_mentioned_addresses.is_empty()
                    }
                    TransactionLogCollectorFilter::AllWithVotes => true,
                    TransactionLogCollectorFilter::None => false,
                    TransactionLogCollectorFilter::OnlyMentionedAddresses => {
                        !filtered_mentioned_addresses.is_empty()
                    }
                };

                if store {
                    if let Some(TransactionExecutionDetails {
                        status,
                        log_messages: Some(log_messages),
                        ..
                    }) = execution_result.details()
                    {
                        let mut transaction_log_collector =
                            self.transaction_log_collector.write().unwrap();
                        let transaction_log_index = transaction_log_collector.logs.len();

                        transaction_log_collector.logs.push(TransactionLogInfo {
                            signature: *tx.signature(),
                            result: status.clone(),
                            is_vote,
                            log_messages: log_messages.clone(),
                        });
                        for key in filtered_mentioned_addresses.into_iter() {
                            transaction_log_collector
                                .mentioned_address_map
                                .entry(key)
                                .or_default()
                                .push(transaction_log_index);
                        }
                    }
                }
            }

            if execution_result.was_executed() {
                // Signature count must be accumulated only if the transaction
                // is executed, otherwise a mismatched count between banking and
                // replay could occur
                signature_count += u64::from(tx.message().header().num_required_signatures);
                executed_transactions_count += 1;
            }

            match execution_result.flattened_result() {
                Ok(()) => {
                    if !is_vote {
                        executed_non_vote_transactions_count += 1;
                    }
                    executed_with_successful_result_count += 1;
                }
                Err(err) => {
                    if *err_count == 0 {
                        debug!("tx error: {:?} {:?}", err, tx);
                    }
                    *err_count += 1;
                }
            }
        }
        collect_logs_time.stop();
        timings
            .saturating_add_in_place(ExecuteTimingType::CollectLogsUs, collect_logs_time.as_us());

        if *err_count > 0 {
            debug!(
                "{} errors of {} txs",
                *err_count,
                *err_count + executed_with_successful_result_count
            );
        }
        LoadAndExecuteTransactionsOutput {
            loaded_transactions,
            execution_results,
            retryable_transaction_indexes,
            executed_transactions_count,
            executed_non_vote_transactions_count,
            executed_with_successful_result_count,
            signature_count,
            error_counters,
        }
    }

    /// The maximum allowed size, in bytes, of the accounts data
    pub fn accounts_data_size_limit(&self) -> u64 {
        MAX_ACCOUNTS_DATA_LEN
    }

    /// Load the accounts data size, in bytes
    pub fn load_accounts_data_size(&self) -> u64 {
        self.accounts_data_size_initial
            .saturating_add_signed(self.load_accounts_data_size_delta())
    }

    /// Load the change in accounts data size in this Bank, in bytes
    pub fn load_accounts_data_size_delta(&self) -> i64 {
        let delta_on_chain = self.load_accounts_data_size_delta_on_chain();
        let delta_off_chain = self.load_accounts_data_size_delta_off_chain();
        delta_on_chain.saturating_add(delta_off_chain)
    }

    /// Load the change in accounts data size in this Bank, in bytes, from on-chain events
    /// i.e. transactions
    pub fn load_accounts_data_size_delta_on_chain(&self) -> i64 {
        self.accounts_data_size_delta_on_chain.load(Acquire)
    }

    /// Load the change in accounts data size in this Bank, in bytes, from off-chain events
    /// i.e. rent collection
    pub fn load_accounts_data_size_delta_off_chain(&self) -> i64 {
        self.accounts_data_size_delta_off_chain.load(Acquire)
    }

    /// Update the accounts data size delta from on-chain events by adding `amount`.
    /// The arithmetic saturates.
    fn update_accounts_data_size_delta_on_chain(&self, amount: i64) {
        if amount == 0 {
            return;
        }

        self.accounts_data_size_delta_on_chain
            .fetch_update(AcqRel, Acquire, |accounts_data_size_delta_on_chain| {
                Some(accounts_data_size_delta_on_chain.saturating_add(amount))
            })
            // SAFETY: unwrap() is safe since our update fn always returns `Some`
            .unwrap();
    }

    /// Update the accounts data size delta from off-chain events by adding `amount`.
    /// The arithmetic saturates.
    fn update_accounts_data_size_delta_off_chain(&self, amount: i64) {
        if amount == 0 {
            return;
        }

        self.accounts_data_size_delta_off_chain
            .fetch_update(AcqRel, Acquire, |accounts_data_size_delta_off_chain| {
                Some(accounts_data_size_delta_off_chain.saturating_add(amount))
            })
            // SAFETY: unwrap() is safe since our update fn always returns `Some`
            .unwrap();
    }

    /// Calculate the data size delta and update the off-chain accounts data size delta
    fn calculate_and_update_accounts_data_size_delta_off_chain(
        &self,
        old_data_size: usize,
        new_data_size: usize,
    ) {
        let data_size_delta = calculate_data_size_delta(old_data_size, new_data_size);
        self.update_accounts_data_size_delta_off_chain(data_size_delta);
    }

    /// Set the initial accounts data size
    /// NOTE: This fn is *ONLY FOR TESTS*
    pub fn set_accounts_data_size_initial_for_tests(&mut self, amount: u64) {
        self.accounts_data_size_initial = amount;
    }

    /// Update the accounts data size off-chain delta
    /// NOTE: This fn is *ONLY FOR TESTS*
    pub fn update_accounts_data_size_delta_off_chain_for_tests(&self, amount: i64) {
        self.update_accounts_data_size_delta_off_chain(amount)
    }

    fn filter_program_errors_and_collect_fee(
        &self,
        txs: &[SanitizedTransaction],
        execution_results: &[TransactionExecutionResult],
    ) -> Vec<Result<()>> {
        let hash_queue = self.blockhash_queue.read().unwrap();
        let mut fees = 0;

        let results = txs
            .iter()
            .zip(execution_results)
            .map(|(tx, execution_result)| {
                let (execution_status, durable_nonce_fee) = match &execution_result {
                    TransactionExecutionResult::Executed { details, .. } => {
                        Ok((&details.status, details.durable_nonce_fee.as_ref()))
                    }
                    TransactionExecutionResult::NotExecuted(err) => Err(err.clone()),
                }?;

                let (lamports_per_signature, is_nonce) = durable_nonce_fee
                    .map(|durable_nonce_fee| durable_nonce_fee.lamports_per_signature())
                    .map(|maybe_lamports_per_signature| (maybe_lamports_per_signature, true))
                    .unwrap_or_else(|| {
                        (
                            hash_queue.get_lamports_per_signature(tx.message().recent_blockhash()),
                            false,
                        )
                    });

                let lamports_per_signature =
                    lamports_per_signature.ok_or(TransactionError::BlockhashNotFound)?;
                let fee = self.get_fee_for_message_with_lamports_per_signature(
                    tx.message(),
                    lamports_per_signature,
                );

                // In case of instruction error, even though no accounts
                // were stored we still need to charge the payer the
                // fee.
                //
                //...except nonce accounts, which already have their
                // post-load, fee deducted, pre-execute account state
                // stored
                if execution_status.is_err() && !is_nonce {
                    self.withdraw(tx.message().fee_payer(), fee)?;
                }

                fees += fee;
                Ok(())
            })
            .collect();

        self.collector_fees.fetch_add(fees, Relaxed);
        results
    }

    /// `committed_transactions_count` is the number of transactions out of `sanitized_txs`
    /// that was executed. Of those, `committed_transactions_count`,
    /// `committed_with_failure_result_count` is the number of executed transactions that returned
    /// a failure result.
    pub fn commit_transactions(
        &self,
        sanitized_txs: &[SanitizedTransaction],
        loaded_txs: &mut [TransactionLoadResult],
        execution_results: Vec<TransactionExecutionResult>,
        last_blockhash: Hash,
        lamports_per_signature: u64,
        counts: CommitTransactionCounts,
        timings: &mut ExecuteTimings,
    ) -> TransactionResults {
        assert!(
            !self.freeze_started(),
            "commit_transactions() working on a bank that is already frozen or is undergoing freezing!"
        );

        let CommitTransactionCounts {
            committed_transactions_count,
            committed_non_vote_transactions_count,
            committed_with_failure_result_count,
            signature_count,
        } = counts;

        self.increment_transaction_count(committed_transactions_count);
        self.increment_non_vote_transaction_count_since_restart(
            committed_non_vote_transactions_count,
        );
        self.increment_signature_count(signature_count);

        if committed_with_failure_result_count > 0 {
            self.transaction_error_count
                .fetch_add(committed_with_failure_result_count, Relaxed);
        }

        // Should be equivalent to checking `committed_transactions_count > 0`
        if execution_results.iter().any(|result| result.was_executed()) {
            self.is_delta.store(true, Relaxed);
            self.transaction_entries_count.fetch_add(1, Relaxed);
            self.transactions_per_entry_max
                .fetch_max(committed_transactions_count, Relaxed);
        }

        let mut write_time = Measure::start("write_time");
        let durable_nonce = DurableNonce::from_blockhash(&last_blockhash);
        self.rc.accounts.store_cached(
            self.slot(),
            sanitized_txs,
            &execution_results,
            loaded_txs,
            &self.rent_collector,
            &durable_nonce,
            lamports_per_signature,
            self.include_slot_in_hash(),
        );
        let rent_debits = self.collect_rent(&execution_results, loaded_txs);

        // Cached vote and stake accounts are synchronized with accounts-db
        // after each transaction.
        let mut update_stakes_cache_time = Measure::start("update_stakes_cache_time");
        self.update_stakes_cache(sanitized_txs, &execution_results, loaded_txs);
        update_stakes_cache_time.stop();

        // once committed there is no way to unroll
        write_time.stop();
        debug!(
            "store: {}us txs_len={}",
            write_time.as_us(),
            sanitized_txs.len()
        );

        let mut store_executors_which_were_deployed_time =
            Measure::start("store_executors_which_were_deployed_time");
        for execution_result in &execution_results {
            if let TransactionExecutionResult::Executed {
                details,
                programs_modified_by_tx,
                programs_updated_only_for_global_cache,
            } = execution_result
            {
                if details.status.is_ok() {
                    let mut cache = self.loaded_programs_cache.write().unwrap();
                    cache.merge(programs_modified_by_tx);
                    cache.merge(programs_updated_only_for_global_cache);
                }
            }
        }
        store_executors_which_were_deployed_time.stop();
        saturating_add_assign!(
            timings.execute_accessories.update_executors_us,
            store_executors_which_were_deployed_time.as_us()
        );

        let accounts_data_len_delta = execution_results
            .iter()
            .filter_map(|execution_result| {
                execution_result
                    .details()
                    .map(|details| details.accounts_data_len_delta)
            })
            .sum();
        self.update_accounts_data_size_delta_on_chain(accounts_data_len_delta);

        timings.saturating_add_in_place(ExecuteTimingType::StoreUs, write_time.as_us());
        timings.saturating_add_in_place(
            ExecuteTimingType::UpdateStakesCacheUs,
            update_stakes_cache_time.as_us(),
        );

        let mut update_transaction_statuses_time = Measure::start("update_transaction_statuses");
        self.update_transaction_statuses(sanitized_txs, &execution_results);
        let fee_collection_results =
            self.filter_program_errors_and_collect_fee(sanitized_txs, &execution_results);
        update_transaction_statuses_time.stop();
        timings.saturating_add_in_place(
            ExecuteTimingType::UpdateTransactionStatuses,
            update_transaction_statuses_time.as_us(),
        );

        TransactionResults {
            fee_collection_results,
            execution_results,
            rent_debits,
        }
    }

    fn collect_rent(
        &self,
        execution_results: &[TransactionExecutionResult],
        loaded_txs: &mut [TransactionLoadResult],
    ) -> Vec<RentDebits> {
        let mut collected_rent: u64 = 0;
        let rent_debits: Vec<_> = loaded_txs
            .iter_mut()
            .zip(execution_results)
            .map(|((load_result, _nonce), execution_result)| {
                if let (Ok(loaded_transaction), true) =
                    (load_result, execution_result.was_executed_successfully())
                {
                    collected_rent += loaded_transaction.rent;
                    mem::take(&mut loaded_transaction.rent_debits)
                } else {
                    RentDebits::default()
                }
            })
            .collect();
        self.collected_rent.fetch_add(collected_rent, Relaxed);
        rent_debits
    }

    fn run_incinerator(&self) {
        if let Some((account, _)) =
            self.get_account_modified_since_parent_with_fixed_root(&incinerator::id())
        {
            self.capitalization.fetch_sub(account.lamports(), Relaxed);
            self.store_account(&incinerator::id(), &AccountSharedData::default());
        }
    }

    /// Get stake and stake node accounts
    pub(crate) fn get_stake_accounts(&self, minimized_account_set: &DashSet<Pubkey>) {
        self.stakes_cache
            .stakes()
            .stake_delegations()
            .iter()
            .for_each(|(pubkey, _)| {
                minimized_account_set.insert(*pubkey);
            });

        self.stakes_cache
            .stakes()
            .staked_nodes()
            .par_iter()
            .for_each(|(pubkey, _)| {
                minimized_account_set.insert(*pubkey);
            });
    }

    fn collect_rent_eagerly(&self) {
        if self.lazy_rent_collection.load(Relaxed) {
            return;
        }

        let mut measure = Measure::start("collect_rent_eagerly-ms");
        let partitions = self.rent_collection_partitions();
        let count = partitions.len();
        let rent_metrics = RentMetrics::default();
        // partitions will usually be 1, but could be more if we skip slots
        let mut parallel = count > 1;
        if parallel {
            let ranges = partitions
                .iter()
                .map(|partition| {
                    (
                        *partition,
                        accounts_partition::pubkey_range_from_partition(*partition),
                    )
                })
                .collect::<Vec<_>>();
            // test every range to make sure ranges are not overlapping
            // some tests collect rent from overlapping ranges
            // example: [(0, 31, 32), (0, 0, 128), (0, 27, 128)]
            // read-modify-write of an account for rent collection cannot be done in parallel
            'outer: for i in 0..ranges.len() {
                for j in 0..ranges.len() {
                    if i == j {
                        continue;
                    }

                    let i = &ranges[i].1;
                    let j = &ranges[j].1;
                    // make sure i doesn't contain j
                    if i.contains(j.start()) || i.contains(j.end()) {
                        parallel = false;
                        break 'outer;
                    }
                }
            }

            if parallel {
                let thread_pool = &self.rc.accounts.accounts_db.thread_pool;
                thread_pool.install(|| {
                    ranges.into_par_iter().for_each(|range| {
                        self.collect_rent_in_range(range.0, range.1, &rent_metrics)
                    });
                });
            }
        }
        if !parallel {
            // collect serially
            partitions
                .into_iter()
                .for_each(|partition| self.collect_rent_in_partition(partition, &rent_metrics));
        }
        measure.stop();
        datapoint_info!(
            "collect_rent_eagerly",
            ("accounts", rent_metrics.count.load(Relaxed), i64),
            ("partitions", count, i64),
            ("total_time_us", measure.as_us(), i64),
            (
                "hold_range_us",
                rent_metrics.hold_range_us.load(Relaxed),
                i64
            ),
            ("load_us", rent_metrics.load_us.load(Relaxed), i64),
            ("collect_us", rent_metrics.collect_us.load(Relaxed), i64),
            ("hash_us", rent_metrics.hash_us.load(Relaxed), i64),
            ("store_us", rent_metrics.store_us.load(Relaxed), i64),
        );
    }

    #[cfg(test)]
    fn restore_old_behavior_for_fragile_tests(&self) {
        self.lazy_rent_collection.store(true, Relaxed);
    }

    fn rent_collection_partitions(&self) -> Vec<Partition> {
        if !self.use_fixed_collection_cycle() {
            // This mode is for production/development/testing.
            // In this mode, we iterate over the whole pubkey value range for each epochs
            // including warm-up epochs.
            // The only exception is the situation where normal epochs are relatively short
            // (currently less than 2 day). In that case, we arrange a single collection
            // cycle to be multiple of epochs so that a cycle could be greater than the 2 day.
            self.variable_cycle_partitions()
        } else {
            // This mode is mainly for benchmarking only.
            // In this mode, we always iterate over the whole pubkey value range with
            // <slot_count_in_two_day> slots as a collection cycle, regardless warm-up or
            // alignment between collection cycles and epochs.
            // Thus, we can simulate stable processing load of eager rent collection,
            // strictly proportional to the number of pubkeys since genesis.
            self.fixed_cycle_partitions()
        }
    }

    /// true if rent collection does NOT rewrite accounts whose pubkey indicates
    ///  it is time for rent collection, but the account is rent exempt.
    /// false if rent collection DOES rewrite accounts if the account is rent exempt
    /// This is the default behavior historically.
    fn bank_hash_skips_rent_rewrites(&self) -> bool {
        self.feature_set
            .is_active(&feature_set::skip_rent_rewrites::id())
    }

    /// Collect rent from `accounts`
    ///
    /// This fn is called inside a parallel loop from `collect_rent_in_partition()`.  Avoid adding
    /// any code that causes contention on shared memory/data (i.e. do not update atomic metrics).
    ///
    /// The return value is a struct of computed values that `collect_rent_in_partition()` will
    /// reduce at the end of its parallel loop.  If possible, place data/computation that cause
    /// contention/take locks in the return struct and process them in
    /// `collect_rent_from_partition()` after reducing the parallel loop.
    fn collect_rent_from_accounts(
        &self,
        mut accounts: Vec<(Pubkey, AccountSharedData, Slot)>,
        rent_paying_pubkeys: Option<&HashSet<Pubkey>>,
        partition_index: PartitionIndex,
    ) -> CollectRentFromAccountsInfo {
        let mut rent_debits = RentDebits::default();
        let mut total_rent_collected_info = CollectedInfo::default();
        let mut accounts_to_store =
            Vec::<(&Pubkey, &AccountSharedData)>::with_capacity(accounts.len());
        let mut time_collecting_rent_us = 0;
        let mut time_storing_accounts_us = 0;
        let can_skip_rewrites = self.bank_hash_skips_rent_rewrites();
        let set_exempt_rent_epoch_max: bool = self
            .feature_set
            .is_active(&solana_sdk::feature_set::set_exempt_rent_epoch_max::id());
        for (pubkey, account, _loaded_slot) in accounts.iter_mut() {
            let (rent_collected_info, measure) =
                measure!(self.rent_collector.collect_from_existing_account(
                    pubkey,
                    account,
                    self.rc.accounts.accounts_db.filler_account_suffix.as_ref(),
                    set_exempt_rent_epoch_max,
                ));
            time_collecting_rent_us += measure.as_us();

            // only store accounts where we collected rent
            // but get the hash for all these accounts even if collected rent is 0 (= not updated).
            // Also, there's another subtle side-effect from rewrites: this
            // ensures we verify the whole on-chain state (= all accounts)
            // via the bank delta hash slowly once per an epoch.
            if !can_skip_rewrites || !Self::skip_rewrite(rent_collected_info.rent_amount, account) {
                if rent_collected_info.rent_amount > 0 {
                    if let Some(rent_paying_pubkeys) = rent_paying_pubkeys {
                        if !rent_paying_pubkeys.contains(pubkey) {
                            let partition_from_pubkey = accounts_partition::partition_from_pubkey(
                                pubkey,
                                self.epoch_schedule.slots_per_epoch,
                            );
                            // Submit datapoint instead of assert while we verify this is correct
                            datapoint_warn!(
                                "bank-unexpected_rent_paying_pubkey",
                                ("slot", self.slot(), i64),
                                ("pubkey", pubkey.to_string(), String),
                                ("partition_index", partition_index, i64),
                                ("partition_from_pubkey", partition_from_pubkey, i64)
                            );
                            warn!(
                                "Collecting rent from unexpected pubkey: {}, slot: {}, parent_slot: {:?}, \
                                partition_index: {}, partition_from_pubkey: {}",
                                pubkey,
                                self.slot(),
                                self.parent().map(|bank| bank.slot()),
                                partition_index,
                                partition_from_pubkey,
                            );
                        }
                    }
                }
                total_rent_collected_info += rent_collected_info;
                accounts_to_store.push((pubkey, account));
            }
            rent_debits.insert(pubkey, rent_collected_info.rent_amount, account.lamports());
        }

        if !accounts_to_store.is_empty() {
            // TODO: Maybe do not call `store_accounts()` here.  Instead return `accounts_to_store`
            // and have `collect_rent_in_partition()` perform all the stores.
            let (_, measure) = measure!(self.store_accounts((
                self.slot(),
                &accounts_to_store[..],
                self.include_slot_in_hash()
            )));
            time_storing_accounts_us += measure.as_us();
        }

        CollectRentFromAccountsInfo {
            rent_collected_info: total_rent_collected_info,
            rent_rewards: rent_debits.into_unordered_rewards_iter().collect(),
            time_collecting_rent_us,
            time_storing_accounts_us,
            num_accounts: accounts.len(),
        }
    }

    /// true if we should include the slot in account hash
    /// This is governed by a feature.
    pub(crate) fn include_slot_in_hash(&self) -> IncludeSlotInHash {
        if self
            .feature_set
            .is_active(&feature_set::account_hash_ignore_slot::id())
        {
            IncludeSlotInHash::RemoveSlot
        } else {
            IncludeSlotInHash::IncludeSlot
        }
    }

    /// convert 'partition' to a pubkey range and 'collect_rent_in_range'
    fn collect_rent_in_partition(&self, partition: Partition, metrics: &RentMetrics) {
        let subrange_full = accounts_partition::pubkey_range_from_partition(partition);
        self.collect_rent_in_range(partition, subrange_full, metrics)
    }

    /// get all pubkeys that we expect to be rent-paying or None, if this was not initialized at load time (that should only exist in test cases)
    fn get_rent_paying_pubkeys(&self, partition: &Partition) -> Option<HashSet<Pubkey>> {
        self.rc
            .accounts
            .accounts_db
            .accounts_index
            .rent_paying_accounts_by_partition
            .get()
            .and_then(|rent_paying_accounts| {
                rent_paying_accounts.is_initialized().then(|| {
                    accounts_partition::get_partition_end_indexes(partition)
                        .into_iter()
                        .flat_map(|end_index| {
                            rent_paying_accounts.get_pubkeys_in_partition_index(end_index)
                        })
                        .cloned()
                        .collect::<HashSet<_>>()
                })
            })
    }

    /// load accounts with pubkeys in 'subrange_full'
    /// collect rent and update 'account.rent_epoch' as necessary
    /// store accounts, whether rent was collected or not (depending on whether we skipping rewrites is enabled)
    /// update bank's rewrites set for all rewrites that were skipped
    /// if 'just_rewrites', function will only update bank's rewrites set and not actually store any accounts.
    ///  This flag is used when restoring from a snapshot to calculate and verify the initial bank's delta hash.
    fn collect_rent_in_range(
        &self,
        partition: Partition,
        subrange_full: RangeInclusive<Pubkey>,
        metrics: &RentMetrics,
    ) {
        let mut hold_range = Measure::start("hold_range");
        let thread_pool = &self.rc.accounts.accounts_db.thread_pool;
        thread_pool.install(|| {
            self.rc
                .accounts
                .hold_range_in_memory(&subrange_full, true, thread_pool);
            hold_range.stop();
            metrics.hold_range_us.fetch_add(hold_range.as_us(), Relaxed);

            let rent_paying_pubkeys_ = self.get_rent_paying_pubkeys(&partition);
            let rent_paying_pubkeys = rent_paying_pubkeys_.as_ref();

            // divide the range into num_threads smaller ranges and process in parallel
            // Note that 'pubkey_range_from_partition' cannot easily be re-used here to break the range smaller.
            // It has special handling of 0..0 and partition_count changes affect all ranges unevenly.
            let num_threads = solana_accounts_db::accounts_db::quarter_thread_count() as u64;
            let sz = std::mem::size_of::<u64>();
            let start_prefix = accounts_partition::prefix_from_pubkey(subrange_full.start());
            let end_prefix_inclusive = accounts_partition::prefix_from_pubkey(subrange_full.end());
            let range = end_prefix_inclusive - start_prefix;
            let increment = range / num_threads;
            let mut results = (0..num_threads)
                .into_par_iter()
                .map(|chunk| {
                    let offset = |chunk| start_prefix + chunk * increment;
                    let start = offset(chunk);
                    let last = chunk == num_threads - 1;
                    let merge_prefix = |prefix: u64, mut bound: Pubkey| {
                        bound.as_mut()[0..sz].copy_from_slice(&prefix.to_be_bytes());
                        bound
                    };
                    let start = merge_prefix(start, *subrange_full.start());
                    let (accounts, measure_load_accounts) = measure!(if last {
                        let end = *subrange_full.end();
                        let subrange = start..=end; // IN-clusive
                        self.rc
                            .accounts
                            .load_to_collect_rent_eagerly(&self.ancestors, subrange)
                    } else {
                        let end = merge_prefix(offset(chunk + 1), *subrange_full.start());
                        let subrange = start..end; // EX-clusive, the next 'start' will be this same value
                        self.rc
                            .accounts
                            .load_to_collect_rent_eagerly(&self.ancestors, subrange)
                    });
                    CollectRentInPartitionInfo::new(
                        self.collect_rent_from_accounts(accounts, rent_paying_pubkeys, partition.1),
                        Duration::from_nanos(measure_load_accounts.as_ns()),
                    )
                })
                .reduce(
                    CollectRentInPartitionInfo::default,
                    CollectRentInPartitionInfo::reduce,
                );

            // We cannot assert here that we collected from all expected keys.
            // Some accounts may have been topped off or may have had all funds removed and gone to 0 lamports.

            self.rc
                .accounts
                .hold_range_in_memory(&subrange_full, false, thread_pool);

            self.collected_rent
                .fetch_add(results.rent_collected, Relaxed);
            self.update_accounts_data_size_delta_off_chain(
                -(results.accounts_data_size_reclaimed as i64),
            );
            self.rewards
                .write()
                .unwrap()
                .append(&mut results.rent_rewards);

            metrics
                .load_us
                .fetch_add(results.time_loading_accounts_us, Relaxed);
            metrics
                .collect_us
                .fetch_add(results.time_collecting_rent_us, Relaxed);
            metrics
                .store_us
                .fetch_add(results.time_storing_accounts_us, Relaxed);
            metrics.count.fetch_add(results.num_accounts, Relaxed);
        });
    }

    /// return true iff storing this account is just a rewrite and can be skipped
    fn skip_rewrite(rent_amount: u64, account: &AccountSharedData) -> bool {
        // if rent was != 0
        // or special case for default rent value
        // these cannot be skipped and must be written
        rent_amount == 0 && account.rent_epoch() != 0
    }

    pub(crate) fn fixed_cycle_partitions_between_slots(
        &self,
        starting_slot: Slot,
        ending_slot: Slot,
    ) -> Vec<Partition> {
        let slot_count_in_two_day = self.slot_count_in_two_day();
        accounts_partition::get_partitions(ending_slot, starting_slot, slot_count_in_two_day)
    }

    fn fixed_cycle_partitions(&self) -> Vec<Partition> {
        self.fixed_cycle_partitions_between_slots(self.parent_slot(), self.slot())
    }

    pub(crate) fn variable_cycle_partitions_between_slots(
        &self,
        starting_slot: Slot,
        ending_slot: Slot,
    ) -> Vec<Partition> {
        let (starting_epoch, mut starting_slot_index) =
            self.get_epoch_and_slot_index(starting_slot);
        let (ending_epoch, ending_slot_index) = self.get_epoch_and_slot_index(ending_slot);

        let mut partitions = vec![];
        if starting_epoch < ending_epoch {
            let slot_skipped = (ending_slot - starting_slot) > 1;
            if slot_skipped {
                // Generate special partitions because there are skipped slots
                // exactly at the epoch transition.

                let parent_last_slot_index = self.get_slots_in_epoch(starting_epoch) - 1;

                // ... for parent epoch
                partitions.push(self.partition_from_slot_indexes_with_gapped_epochs(
                    starting_slot_index,
                    parent_last_slot_index,
                    starting_epoch,
                ));

                if ending_slot_index > 0 {
                    // ... for current epoch
                    partitions.push(self.partition_from_slot_indexes_with_gapped_epochs(
                        0,
                        0,
                        ending_epoch,
                    ));
                }
            }
            starting_slot_index = 0;
        }

        partitions.push(self.partition_from_normal_slot_indexes(
            starting_slot_index,
            ending_slot_index,
            ending_epoch,
        ));

        partitions
    }

    fn variable_cycle_partitions(&self) -> Vec<Partition> {
        self.variable_cycle_partitions_between_slots(self.parent_slot(), self.slot())
    }

    fn do_partition_from_slot_indexes(
        &self,
        start_slot_index: SlotIndex,
        end_slot_index: SlotIndex,
        epoch: Epoch,
        generated_for_gapped_epochs: bool,
    ) -> Partition {
        let slot_count_per_epoch = self.get_slots_in_epoch(epoch);

        let cycle_params = if !self.use_multi_epoch_collection_cycle(epoch) {
            // mnb should always go through this code path
            accounts_partition::rent_single_epoch_collection_cycle_params(
                epoch,
                slot_count_per_epoch,
            )
        } else {
            accounts_partition::rent_multi_epoch_collection_cycle_params(
                epoch,
                slot_count_per_epoch,
                self.first_normal_epoch(),
                self.slot_count_in_two_day() / slot_count_per_epoch,
            )
        };
        accounts_partition::get_partition_from_slot_indexes(
            cycle_params,
            start_slot_index,
            end_slot_index,
            generated_for_gapped_epochs,
        )
    }

    fn partition_from_normal_slot_indexes(
        &self,
        start_slot_index: SlotIndex,
        end_slot_index: SlotIndex,
        epoch: Epoch,
    ) -> Partition {
        self.do_partition_from_slot_indexes(start_slot_index, end_slot_index, epoch, false)
    }

    fn partition_from_slot_indexes_with_gapped_epochs(
        &self,
        start_slot_index: SlotIndex,
        end_slot_index: SlotIndex,
        epoch: Epoch,
    ) -> Partition {
        self.do_partition_from_slot_indexes(start_slot_index, end_slot_index, epoch, true)
    }

    // Given short epochs, it's too costly to collect rent eagerly
    // within an epoch, so lower the frequency of it.
    // These logic isn't strictly eager anymore and should only be used
    // for development/performance purpose.
    // Absolutely not under ClusterType::MainnetBeta!!!!
    fn use_multi_epoch_collection_cycle(&self, epoch: Epoch) -> bool {
        // Force normal behavior, disabling multi epoch collection cycle for manual local testing
        #[cfg(not(test))]
        if self.slot_count_per_normal_epoch() == solana_sdk::epoch_schedule::MINIMUM_SLOTS_PER_EPOCH
        {
            return false;
        }

        epoch >= self.first_normal_epoch()
            && self.slot_count_per_normal_epoch() < self.slot_count_in_two_day()
    }

    pub(crate) fn use_fixed_collection_cycle(&self) -> bool {
        // Force normal behavior, disabling fixed collection cycle for manual local testing
        #[cfg(not(test))]
        if self.slot_count_per_normal_epoch() == solana_sdk::epoch_schedule::MINIMUM_SLOTS_PER_EPOCH
        {
            return false;
        }

        self.cluster_type() != ClusterType::MainnetBeta
            && self.slot_count_per_normal_epoch() < self.slot_count_in_two_day()
    }

    fn slot_count_in_two_day(&self) -> SlotCount {
        Self::slot_count_in_two_day_helper(self.ticks_per_slot)
    }

    // This value is specially chosen to align with slots per epoch in mainnet-beta and testnet
    // Also, assume 500GB account data set as the extreme, then for 2 day (=48 hours) to collect
    // rent eagerly, we'll consume 5.7 MB/s IO bandwidth, bidirectionally.
    pub fn slot_count_in_two_day_helper(ticks_per_slot: SlotCount) -> SlotCount {
        2 * DEFAULT_TICKS_PER_SECOND * SECONDS_PER_DAY / ticks_per_slot
    }

    fn slot_count_per_normal_epoch(&self) -> SlotCount {
        self.get_slots_in_epoch(self.first_normal_epoch())
    }

    pub fn cluster_type(&self) -> ClusterType {
        // unwrap is safe; self.cluster_type is ensured to be Some() always...
        // we only using Option here for ABI compatibility...
        self.cluster_type.unwrap()
    }

    /// Process a batch of transactions.
    #[must_use]
    pub fn load_execute_and_commit_transactions(
        &self,
        batch: &TransactionBatch,
        max_age: usize,
        collect_balances: bool,
        enable_cpi_recording: bool,
        enable_log_recording: bool,
        enable_return_data_recording: bool,
        timings: &mut ExecuteTimings,
        log_messages_bytes_limit: Option<usize>,
    ) -> (TransactionResults, TransactionBalancesSet) {
        let pre_balances = if collect_balances {
            self.collect_balances(batch)
        } else {
            vec![]
        };

        let LoadAndExecuteTransactionsOutput {
            mut loaded_transactions,
            execution_results,
            executed_transactions_count,
            executed_non_vote_transactions_count,
            executed_with_successful_result_count,
            signature_count,
            ..
        } = self.load_and_execute_transactions(
            batch,
            max_age,
            enable_cpi_recording,
            enable_log_recording,
            enable_return_data_recording,
            timings,
            None,
            log_messages_bytes_limit,
        );

        let (last_blockhash, lamports_per_signature) =
            self.last_blockhash_and_lamports_per_signature();
        let results = self.commit_transactions(
            batch.sanitized_transactions(),
            &mut loaded_transactions,
            execution_results,
            last_blockhash,
            lamports_per_signature,
            CommitTransactionCounts {
                committed_transactions_count: executed_transactions_count as u64,
                committed_non_vote_transactions_count: executed_non_vote_transactions_count as u64,
                committed_with_failure_result_count: executed_transactions_count
                    .saturating_sub(executed_with_successful_result_count)
                    as u64,
                signature_count,
            },
            timings,
        );
        let post_balances = if collect_balances {
            self.collect_balances(batch)
        } else {
            vec![]
        };
        (
            results,
            TransactionBalancesSet::new(pre_balances, post_balances),
        )
    }

    /// Process a Transaction. This is used for unit tests and simply calls the vector
    /// Bank::process_transactions method.
    pub fn process_transaction(&self, tx: &Transaction) -> Result<()> {
        self.try_process_transactions(std::iter::once(tx))?[0].clone()?;
        tx.signatures
            .get(0)
            .map_or(Ok(()), |sig| self.get_signature_status(sig).unwrap())
    }

    /// Process a Transaction and store metadata. This is used for tests and the banks services. It
    /// replicates the vector Bank::process_transaction method with metadata recording enabled.
    #[must_use]
    pub fn process_transaction_with_metadata(
        &self,
        tx: impl Into<VersionedTransaction>,
    ) -> TransactionExecutionResult {
        let txs = vec![tx.into()];
        let batch = match self.prepare_entry_batch(txs) {
            Ok(batch) => batch,
            Err(err) => return TransactionExecutionResult::NotExecuted(err),
        };

        let (
            TransactionResults {
                mut execution_results,
                ..
            },
            ..,
        ) = self.load_execute_and_commit_transactions(
            &batch,
            MAX_PROCESSING_AGE,
            false, // collect_balances
            false, // enable_cpi_recording
            true,  // enable_log_recording
            true,  // enable_return_data_recording
            &mut ExecuteTimings::default(),
            Some(1000 * 1000),
        );

        execution_results.remove(0)
    }

    /// Process multiple transaction in a single batch. This is used for benches and unit tests.
    ///
    /// # Panics
    ///
    /// Panics if any of the transactions do not pass sanitization checks.
    #[must_use]
    pub fn process_transactions<'a>(
        &self,
        txs: impl Iterator<Item = &'a Transaction>,
    ) -> Vec<Result<()>> {
        self.try_process_transactions(txs).unwrap()
    }

    /// Process multiple transaction in a single batch. This is used for benches and unit tests.
    /// Short circuits if any of the transactions do not pass sanitization checks.
    pub fn try_process_transactions<'a>(
        &self,
        txs: impl Iterator<Item = &'a Transaction>,
    ) -> Result<Vec<Result<()>>> {
        let txs = txs
            .map(|tx| VersionedTransaction::from(tx.clone()))
            .collect();
        self.try_process_entry_transactions(txs)
    }

    /// Process entry transactions in a single batch. This is used for benches and unit tests.
    ///
    /// # Panics
    ///
    /// Panics if any of the transactions do not pass sanitization checks.
    #[must_use]
    pub fn process_entry_transactions(&self, txs: Vec<VersionedTransaction>) -> Vec<Result<()>> {
        self.try_process_entry_transactions(txs).unwrap()
    }

    /// Process multiple transaction in a single batch. This is used for benches and unit tests.
    /// Short circuits if any of the transactions do not pass sanitization checks.
    pub fn try_process_entry_transactions(
        &self,
        txs: Vec<VersionedTransaction>,
    ) -> Result<Vec<Result<()>>> {
        let batch = self.prepare_entry_batch(txs)?;
        Ok(self.process_transaction_batch(&batch))
    }

    #[must_use]
    fn process_transaction_batch(&self, batch: &TransactionBatch) -> Vec<Result<()>> {
        self.load_execute_and_commit_transactions(
            batch,
            MAX_PROCESSING_AGE,
            false,
            false,
            false,
            false,
            &mut ExecuteTimings::default(),
            None,
        )
        .0
        .fee_collection_results
    }

    /// Create, sign, and process a Transaction from `keypair` to `to` of
    /// `n` lamports where `blockhash` is the last Entry ID observed by the client.
    pub fn transfer(&self, n: u64, keypair: &Keypair, to: &Pubkey) -> Result<Signature> {
        let blockhash = self.last_blockhash();
        let tx = system_transaction::transfer(keypair, to, n, blockhash);
        let signature = tx.signatures[0];
        self.process_transaction(&tx).map(|_| signature)
    }

    pub fn read_balance(account: &AccountSharedData) -> u64 {
        account.lamports()
    }
    /// Each program would need to be able to introspect its own state
    /// this is hard-coded to the Budget language
    pub fn get_balance(&self, pubkey: &Pubkey) -> u64 {
        self.get_account(pubkey)
            .map(|x| Self::read_balance(&x))
            .unwrap_or(0)
    }

    /// Compute all the parents of the bank in order
    pub fn parents(&self) -> Vec<Arc<Bank>> {
        let mut parents = vec![];
        let mut bank = self.parent();
        while let Some(parent) = bank {
            parents.push(parent.clone());
            bank = parent.parent();
        }
        parents
    }

    /// Compute all the parents of the bank including this bank itself
    pub fn parents_inclusive(self: Arc<Self>) -> Vec<Arc<Bank>> {
        let mut parents = self.parents();
        parents.insert(0, self);
        parents
    }

    /// fn store the single `account` with `pubkey`.
    /// Uses `store_accounts`, which works on a vector of accounts.
    pub fn store_account<T: ReadableAccount + Sync + ZeroLamport>(
        &self,
        pubkey: &Pubkey,
        account: &T,
    ) {
        self.store_accounts((
            self.slot(),
            &[(pubkey, account)][..],
            self.include_slot_in_hash(),
        ))
    }

    pub fn store_accounts<'a, T: ReadableAccount + Sync + ZeroLamport + 'a>(
        &self,
        accounts: impl StorableAccounts<'a, T>,
    ) {
        assert!(!self.freeze_started());
        let mut m = Measure::start("stakes_cache.check_and_store");
        let new_warmup_cooldown_rate_epoch = self.new_warmup_cooldown_rate_epoch();
        (0..accounts.len()).for_each(|i| {
            self.stakes_cache.check_and_store(
                accounts.pubkey(i),
                accounts.account(i),
                new_warmup_cooldown_rate_epoch,
            )
        });
        self.rc.accounts.store_accounts_cached(accounts);
        m.stop();
        self.rc
            .accounts
            .accounts_db
            .stats
            .stakes_cache_check_and_store_us
            .fetch_add(m.as_us(), Relaxed);
    }

    pub fn force_flush_accounts_cache(&self) {
        self.rc
            .accounts
            .accounts_db
            .flush_accounts_cache(true, Some(self.slot()))
    }

    pub fn flush_accounts_cache_if_needed(&self) {
        self.rc
            .accounts
            .accounts_db
            .flush_accounts_cache(false, Some(self.slot()))
    }

    #[cfg(test)]
    pub fn flush_accounts_cache_slot_for_tests(&self) {
        self.rc
            .accounts
            .accounts_db
            .flush_accounts_cache_slot_for_tests(self.slot())
    }

    pub fn expire_old_recycle_stores(&self) {
        self.rc.accounts.accounts_db.expire_old_recycle_stores()
    }

    /// Technically this issues (or even burns!) new lamports,
    /// so be extra careful for its usage
    fn store_account_and_update_capitalization(
        &self,
        pubkey: &Pubkey,
        new_account: &AccountSharedData,
    ) {
        let old_account_data_size =
            if let Some(old_account) = self.get_account_with_fixed_root(pubkey) {
                match new_account.lamports().cmp(&old_account.lamports()) {
                    std::cmp::Ordering::Greater => {
                        let increased = new_account.lamports() - old_account.lamports();
                        trace!(
                            "store_account_and_update_capitalization: increased: {} {}",
                            pubkey,
                            increased
                        );
                        self.capitalization.fetch_add(increased, Relaxed);
                    }
                    std::cmp::Ordering::Less => {
                        let decreased = old_account.lamports() - new_account.lamports();
                        trace!(
                            "store_account_and_update_capitalization: decreased: {} {}",
                            pubkey,
                            decreased
                        );
                        self.capitalization.fetch_sub(decreased, Relaxed);
                    }
                    std::cmp::Ordering::Equal => {}
                }
                old_account.data().len()
            } else {
                trace!(
                    "store_account_and_update_capitalization: created: {} {}",
                    pubkey,
                    new_account.lamports()
                );
                self.capitalization
                    .fetch_add(new_account.lamports(), Relaxed);
                0
            };

        self.store_account(pubkey, new_account);
        self.calculate_and_update_accounts_data_size_delta_off_chain(
            old_account_data_size,
            new_account.data().len(),
        );
    }

    fn withdraw(&self, pubkey: &Pubkey, lamports: u64) -> Result<()> {
        match self.get_account_with_fixed_root(pubkey) {
            Some(mut account) => {
                let min_balance = match get_system_account_kind(&account) {
                    Some(SystemAccountKind::Nonce) => self
                        .rent_collector
                        .rent
                        .minimum_balance(nonce::State::size()),
                    _ => 0,
                };

                lamports
                    .checked_add(min_balance)
                    .filter(|required_balance| *required_balance <= account.lamports())
                    .ok_or(TransactionError::InsufficientFundsForFee)?;
                account
                    .checked_sub_lamports(lamports)
                    .map_err(|_| TransactionError::InsufficientFundsForFee)?;
                self.store_account(pubkey, &account);

                Ok(())
            }
            None => Err(TransactionError::AccountNotFound),
        }
    }

    pub fn accounts(&self) -> Arc<Accounts> {
        self.rc.accounts.clone()
    }

    fn finish_init(
        &mut self,
        genesis_config: &GenesisConfig,
        additional_builtins: Option<&[BuiltinPrototype]>,
        debug_do_not_add_builtins: bool,
    ) {
        self.rewards_pool_pubkeys =
            Arc::new(genesis_config.rewards_pools.keys().cloned().collect());

        self.apply_feature_activations(
            ApplyFeatureActivationsCaller::FinishInit,
            debug_do_not_add_builtins,
        );

        if !debug_do_not_add_builtins {
            for builtin in BUILTINS
                .iter()
                .chain(additional_builtins.unwrap_or(&[]).iter())
            {
                if builtin.feature_id.is_none() {
                    self.add_builtin(
                        builtin.program_id,
                        builtin.name.to_string(),
                        LoadedProgram::new_builtin(0, builtin.name.len(), builtin.entrypoint),
                    );
                }
            }
            for precompile in get_precompiles() {
                if precompile.feature.is_none() {
                    self.add_precompile(&precompile.program_id);
                }
            }
        }

        let mut loaded_programs_cache = self.loaded_programs_cache.write().unwrap();
        loaded_programs_cache.latest_root_slot = self.slot();
        loaded_programs_cache.latest_root_epoch = self.epoch();
        loaded_programs_cache.environments.program_runtime_v1 = Arc::new(
            create_program_runtime_environment_v1(
                &self.feature_set,
                &self.runtime_config.compute_budget.unwrap_or_default(),
                false, /* deployment */
                false, /* debugging_features */
            )
            .unwrap(),
        );
        loaded_programs_cache.environments.program_runtime_v2 =
            Arc::new(create_program_runtime_environment_v2(
                &self.runtime_config.compute_budget.unwrap_or_default(),
                false, /* debugging_features */
            ));

        if self
            .feature_set
            .is_active(&feature_set::cap_accounts_data_len::id())
        {
            self.cost_tracker = RwLock::new(CostTracker::new_with_account_data_size_limit(Some(
                self.accounts_data_size_limit()
                    .saturating_sub(self.accounts_data_size_initial),
            )));
        }
    }

    pub fn set_inflation(&self, inflation: Inflation) {
        *self.inflation.write().unwrap() = inflation;
    }

    /// Get a snapshot of the current set of hard forks
    pub fn hard_forks(&self) -> HardForks {
        self.hard_forks.read().unwrap().clone()
    }

    pub fn register_hard_fork(&self, new_hard_fork_slot: Slot) {
        let bank_slot = self.slot();

        let lock = self.freeze_lock();
        let bank_frozen = *lock != Hash::default();
        if new_hard_fork_slot < bank_slot {
            warn!(
                "Hard fork at slot {new_hard_fork_slot} ignored, the hard fork is older \
                than the bank at slot {bank_slot} that attempted to register it."
            );
        } else if (new_hard_fork_slot == bank_slot) && bank_frozen {
            warn!(
                "Hard fork at slot {new_hard_fork_slot} ignored, the hard fork is the same \
                slot as the bank at slot {bank_slot} that attempted to register it, but that \
                bank is already frozen."
            );
        } else {
            self.hard_forks
                .write()
                .unwrap()
                .register(new_hard_fork_slot);
        }
    }

    // Hi! leaky abstraction here....
    // try to use get_account_with_fixed_root() if it's called ONLY from on-chain runtime account
    // processing. That alternative fn provides more safety.
    pub fn get_account(&self, pubkey: &Pubkey) -> Option<AccountSharedData> {
        self.get_account_modified_slot(pubkey)
            .map(|(acc, _slot)| acc)
    }

    // Hi! leaky abstraction here....
    // use this over get_account() if it's called ONLY from on-chain runtime account
    // processing (i.e. from in-band replay/banking stage; that ensures root is *fixed* while
    // running).
    // pro: safer assertion can be enabled inside AccountsDb
    // con: panics!() if called from off-chain processing
    pub fn get_account_with_fixed_root(&self, pubkey: &Pubkey) -> Option<AccountSharedData> {
        self.load_slow_with_fixed_root(&self.ancestors, pubkey)
            .map(|(acc, _slot)| acc)
    }

    pub fn get_account_modified_slot(&self, pubkey: &Pubkey) -> Option<(AccountSharedData, Slot)> {
        self.load_slow(&self.ancestors, pubkey)
    }

    fn load_slow(
        &self,
        ancestors: &Ancestors,
        pubkey: &Pubkey,
    ) -> Option<(AccountSharedData, Slot)> {
        // get_account (= primary this fn caller) may be called from on-chain Bank code even if we
        // try hard to use get_account_with_fixed_root for that purpose...
        // so pass safer LoadHint:Unspecified here as a fallback
        self.rc.accounts.load_without_fixed_root(ancestors, pubkey)
    }

    fn load_slow_with_fixed_root(
        &self,
        ancestors: &Ancestors,
        pubkey: &Pubkey,
    ) -> Option<(AccountSharedData, Slot)> {
        self.rc.accounts.load_with_fixed_root(ancestors, pubkey)
    }

    pub fn get_program_accounts(
        &self,
        program_id: &Pubkey,
        config: &ScanConfig,
    ) -> ScanResult<Vec<TransactionAccount>> {
        self.rc
            .accounts
            .load_by_program(&self.ancestors, self.bank_id, program_id, config)
    }

    pub fn get_filtered_program_accounts<F: Fn(&AccountSharedData) -> bool>(
        &self,
        program_id: &Pubkey,
        filter: F,
        config: &ScanConfig,
    ) -> ScanResult<Vec<TransactionAccount>> {
        self.rc.accounts.load_by_program_with_filter(
            &self.ancestors,
            self.bank_id,
            program_id,
            filter,
            config,
        )
    }

    pub fn get_filtered_indexed_accounts<F: Fn(&AccountSharedData) -> bool>(
        &self,
        index_key: &IndexKey,
        filter: F,
        config: &ScanConfig,
        byte_limit_for_scan: Option<usize>,
    ) -> ScanResult<Vec<TransactionAccount>> {
        self.rc.accounts.load_by_index_key_with_filter(
            &self.ancestors,
            self.bank_id,
            index_key,
            filter,
            config,
            byte_limit_for_scan,
        )
    }

    pub fn account_indexes_include_key(&self, key: &Pubkey) -> bool {
        self.rc.accounts.account_indexes_include_key(key)
    }

    /// Returns all the accounts this bank can load
    pub fn get_all_accounts(&self) -> ScanResult<Vec<PubkeyAccountSlot>> {
        self.rc.accounts.load_all(&self.ancestors, self.bank_id)
    }

    // Scans all the accounts this bank can load, applying `scan_func`
    pub fn scan_all_accounts<F>(&self, scan_func: F) -> ScanResult<()>
    where
        F: FnMut(Option<(&Pubkey, AccountSharedData, Slot)>),
    {
        self.rc
            .accounts
            .scan_all(&self.ancestors, self.bank_id, scan_func)
    }

    pub fn get_program_accounts_modified_since_parent(
        &self,
        program_id: &Pubkey,
    ) -> Vec<TransactionAccount> {
        self.rc
            .accounts
            .load_by_program_slot(self.slot(), Some(program_id))
    }

    pub fn get_transaction_logs(
        &self,
        address: Option<&Pubkey>,
    ) -> Option<Vec<TransactionLogInfo>> {
        self.transaction_log_collector
            .read()
            .unwrap()
            .get_logs_for_address(address)
    }

    /// Returns all the accounts stored in this slot
    pub fn get_all_accounts_modified_since_parent(&self) -> Vec<TransactionAccount> {
        self.rc.accounts.load_by_program_slot(self.slot(), None)
    }

    // if you want get_account_modified_since_parent without fixed_root, please define so...
    fn get_account_modified_since_parent_with_fixed_root(
        &self,
        pubkey: &Pubkey,
    ) -> Option<(AccountSharedData, Slot)> {
        let just_self: Ancestors = Ancestors::from(vec![self.slot()]);
        if let Some((account, slot)) = self.load_slow_with_fixed_root(&just_self, pubkey) {
            if slot == self.slot() {
                return Some((account, slot));
            }
        }
        None
    }

    pub fn get_largest_accounts(
        &self,
        num: usize,
        filter_by_address: &HashSet<Pubkey>,
        filter: AccountAddressFilter,
    ) -> ScanResult<Vec<(Pubkey, u64)>> {
        self.rc.accounts.load_largest_accounts(
            &self.ancestors,
            self.bank_id,
            num,
            filter_by_address,
            filter,
        )
    }

    /// Return the accumulated executed transaction count
    pub fn transaction_count(&self) -> u64 {
        self.transaction_count.load(Relaxed)
    }

    /// Returns the number of non-vote transactions processed without error
    /// since the most recent boot from snapshot or genesis.
    /// This value is not shared though the network, nor retained
    /// within snapshots, but is preserved in `Bank::new_from_parent`.
    pub fn non_vote_transaction_count_since_restart(&self) -> u64 {
        self.non_vote_transaction_count_since_restart.load(Relaxed)
    }

    /// Return the transaction count executed only in this bank
    pub fn executed_transaction_count(&self) -> u64 {
        self.transaction_count()
            .saturating_sub(self.parent().map_or(0, |parent| parent.transaction_count()))
    }

    pub fn transaction_error_count(&self) -> u64 {
        self.transaction_error_count.load(Relaxed)
    }

    pub fn transaction_entries_count(&self) -> u64 {
        self.transaction_entries_count.load(Relaxed)
    }

    pub fn transactions_per_entry_max(&self) -> u64 {
        self.transactions_per_entry_max.load(Relaxed)
    }

    fn increment_transaction_count(&self, tx_count: u64) {
        self.transaction_count.fetch_add(tx_count, Relaxed);
    }

    fn increment_non_vote_transaction_count_since_restart(&self, tx_count: u64) {
        self.non_vote_transaction_count_since_restart
            .fetch_add(tx_count, Relaxed);
    }

    pub fn signature_count(&self) -> u64 {
        self.signature_count.load(Relaxed)
    }

    fn increment_signature_count(&self, signature_count: u64) {
        self.signature_count.fetch_add(signature_count, Relaxed);
    }

    pub fn get_signature_status_processed_since_parent(
        &self,
        signature: &Signature,
    ) -> Option<Result<()>> {
        if let Some((slot, status)) = self.get_signature_status_slot(signature) {
            if slot <= self.slot() {
                return Some(status);
            }
        }
        None
    }

    pub fn get_signature_status_with_blockhash(
        &self,
        signature: &Signature,
        blockhash: &Hash,
    ) -> Option<Result<()>> {
        let rcache = self.status_cache.read().unwrap();
        rcache
            .get_status(signature, blockhash, &self.ancestors)
            .map(|v| v.1)
    }

    pub fn get_signature_status_slot(&self, signature: &Signature) -> Option<(Slot, Result<()>)> {
        let rcache = self.status_cache.read().unwrap();
        rcache.get_status_any_blockhash(signature, &self.ancestors)
    }

    pub fn get_signature_status(&self, signature: &Signature) -> Option<Result<()>> {
        self.get_signature_status_slot(signature).map(|v| v.1)
    }

    pub fn has_signature(&self, signature: &Signature) -> bool {
        self.get_signature_status_slot(signature).is_some()
    }

    /// Hash the `accounts` HashMap. This represents a validator's interpretation
    ///  of the delta of the ledger since the last vote and up to now
    fn hash_internal_state(&self) -> Hash {
        let slot = self.slot();
        let ignore = (!self.is_partitioned_rewards_feature_enabled()
            && (self
                .partitioned_epoch_rewards_config()
                .test_enable_partitioned_rewards
                && self.get_reward_calculation_num_blocks() == 0
                && self.partitioned_rewards_stake_account_stores_per_block() == u64::MAX))
            .then_some(sysvar::epoch_rewards::id());
        let accounts_delta_hash = self
            .rc
            .accounts
            .accounts_db
            .calculate_accounts_delta_hash_internal(slot, ignore);

        let mut signature_count_buf = [0u8; 8];
        LittleEndian::write_u64(&mut signature_count_buf[..], self.signature_count());

        let mut hash = hashv(&[
            self.parent_hash.as_ref(),
            accounts_delta_hash.0.as_ref(),
            &signature_count_buf,
            self.last_blockhash().as_ref(),
        ]);

        let epoch_accounts_hash = self.should_include_epoch_accounts_hash().then(|| {
            let epoch_accounts_hash = self.wait_get_epoch_accounts_hash();
            hash = hashv(&[hash.as_ref(), epoch_accounts_hash.as_ref().as_ref()]);
            epoch_accounts_hash
        });

        let buf = self
            .hard_forks
            .read()
            .unwrap()
            .get_hash_data(slot, self.parent_slot());
        if let Some(buf) = buf {
            let hard_forked_hash = extend_and_hash(&hash, &buf);
            warn!("hard fork at slot {slot} by hashing {buf:?}: {hash} => {hard_forked_hash}");
            hash = hard_forked_hash;
        }

        let bank_hash_stats = self
            .rc
            .accounts
            .accounts_db
            .get_bank_hash_stats(slot)
            .expect("No bank hash stats were found for this bank, that should not be possible");
        info!(
            "bank frozen: {slot} hash: {hash} accounts_delta: {} signature_count: {} last_blockhash: {} capitalization: {}{}, stats: {bank_hash_stats:?}",
            accounts_delta_hash.0,
            self.signature_count(),
            self.last_blockhash(),
            self.capitalization(),
            if let Some(epoch_accounts_hash) = epoch_accounts_hash {
                format!(", epoch_accounts_hash: {:?}", epoch_accounts_hash.as_ref())
            } else {
                "".to_string()
            }
        );
        hash
    }

    /// The epoch accounts hash is hashed into the bank's hash once per epoch at a predefined slot.
    /// Should it be included in *this* bank?
    fn should_include_epoch_accounts_hash(&self) -> bool {
        if !self
            .feature_set
            .is_active(&feature_set::epoch_accounts_hash::id())
        {
            return false;
        }

        if !epoch_accounts_hash_utils::is_enabled_this_epoch(self) {
            return false;
        }

        let stop_slot = epoch_accounts_hash_utils::calculation_stop(self);
        self.parent_slot() < stop_slot && self.slot() >= stop_slot
    }

    /// If the epoch accounts hash should be included in this Bank, then fetch it.  If the EAH
    /// calculation has not completed yet, this fn will block until it does complete.
    fn wait_get_epoch_accounts_hash(&self) -> EpochAccountsHash {
        let (epoch_accounts_hash, measure) = measure!(self
            .rc
            .accounts
            .accounts_db
            .epoch_accounts_hash_manager
            .wait_get_epoch_accounts_hash());

        datapoint_info!(
            "bank-wait_get_epoch_accounts_hash",
            ("slot", self.slot() as i64, i64),
            ("waiting-time-us", measure.as_us() as i64, i64),
        );
        epoch_accounts_hash
    }

    /// Used by ledger tool to run a final hash calculation once all ledger replay has completed.
    /// This should not be called by validator code.
    pub fn run_final_hash_calc(&self, on_halt_store_hash_raw_data_for_debug: bool) {
        self.force_flush_accounts_cache();
        // note that this slot may not be a root
        _ = self.verify_accounts_hash(
            None,
            VerifyAccountsHashConfig {
                test_hash_calculation: false,
                ignore_mismatch: true,
                require_rooted_bank: false,
                run_in_background: false,
                store_hash_raw_data_for_debug: on_halt_store_hash_raw_data_for_debug,
            },
        );
    }

    /// Recalculate the hash_internal_state from the account stores. Would be used to verify a
    /// snapshot.
    /// return true if all is good
    /// Only called from startup or test code.
    #[must_use]
    fn verify_accounts_hash(
        &self,
        base: Option<(Slot, /*capitalization*/ u64)>,
        config: VerifyAccountsHashConfig,
    ) -> bool {
        let accounts = &self.rc.accounts;
        // Wait until initial hash calc is complete before starting a new hash calc.
        // This should only occur when we halt at a slot in ledger-tool.
        accounts
            .accounts_db
            .verify_accounts_hash_in_bg
            .wait_for_complete();

        if config.require_rooted_bank
            && !accounts
                .accounts_db
                .accounts_index
                .is_alive_root(self.slot())
        {
            if let Some(parent) = self.parent() {
                info!("{} is not a root, so attempting to verify bank hash on parent bank at slot: {}", self.slot(), parent.slot());
                return parent.verify_accounts_hash(base, config);
            } else {
                // this will result in mismatch errors
                // accounts hash calc doesn't include unrooted slots
                panic!("cannot verify bank hash when bank is not a root");
            }
        }
        let slot = self.slot();
        let ancestors = &self.ancestors;
        let cap = self.capitalization();
        let epoch_schedule = self.epoch_schedule();
        let rent_collector = self.rent_collector();
        let include_slot_in_hash = self.include_slot_in_hash();
        if config.run_in_background {
            let ancestors = ancestors.clone();
            let accounts = Arc::clone(accounts);
            let epoch_schedule = *epoch_schedule;
            let rent_collector = rent_collector.clone();
            let accounts_ = Arc::clone(&accounts);
            accounts.accounts_db.verify_accounts_hash_in_bg.start(|| {
                Builder::new()
                    .name("solBgHashVerify".into())
                    .spawn(move || {
                        info!("Initial background accounts hash verification has started");
                        let result = accounts_.verify_accounts_hash_and_lamports(
                            slot,
                            cap,
                            base,
                            VerifyAccountsHashAndLamportsConfig {
                                ancestors: &ancestors,
                                test_hash_calculation: config.test_hash_calculation,
                                epoch_schedule: &epoch_schedule,
                                rent_collector: &rent_collector,
                                ignore_mismatch: config.ignore_mismatch,
                                store_detailed_debug_info: config.store_hash_raw_data_for_debug,
                                use_bg_thread_pool: true,
                                include_slot_in_hash,
                            },
                        );
                        accounts_
                            .accounts_db
                            .verify_accounts_hash_in_bg
                            .background_finished();
                        info!("Initial background accounts hash verification has stopped");
                        result
                    })
                    .unwrap()
            });
            true // initial result is true. We haven't failed yet. If verification fails, we'll panic from bg thread.
        } else {
            let result = accounts.verify_accounts_hash_and_lamports(
                slot,
                cap,
                base,
                VerifyAccountsHashAndLamportsConfig {
                    ancestors,
                    test_hash_calculation: config.test_hash_calculation,
                    epoch_schedule,
                    rent_collector,
                    ignore_mismatch: config.ignore_mismatch,
                    store_detailed_debug_info: config.store_hash_raw_data_for_debug,
                    use_bg_thread_pool: false, // fg is waiting for this to run, so we can use the fg thread pool
                    include_slot_in_hash,
                },
            );
            self.set_initial_accounts_hash_verification_completed();
            result
        }
    }

    /// Specify that initial verification has completed.
    /// Called internally when verification runs in the foreground thread.
    /// Also has to be called by some tests which don't do verification on startup.
    pub fn set_initial_accounts_hash_verification_completed(&self) {
        self.rc
            .accounts
            .accounts_db
            .verify_accounts_hash_in_bg
            .verification_complete();
    }

    /// return true if bg hash verification is complete
    /// return false if bg hash verification has not completed yet
    /// if hash verification failed, a panic will occur
    pub fn has_initial_accounts_hash_verification_completed(&self) -> bool {
        self.rc
            .accounts
            .accounts_db
            .verify_accounts_hash_in_bg
            .check_complete()
    }

    /// This is only valid to call from tests.
    /// block until initial accounts hash verification has completed
    pub fn wait_for_initial_accounts_hash_verification_completed_for_tests(&self) {
        self.rc
            .accounts
            .accounts_db
            .verify_accounts_hash_in_bg
            .wait_for_complete()
    }

    /// Get this bank's storages to use for snapshots.
    ///
    /// If a base slot is provided, return only the storages that are *higher* than this slot.
    pub fn get_snapshot_storages(&self, base_slot: Option<Slot>) -> Vec<Arc<AccountStorageEntry>> {
        // if a base slot is provided, request storages starting at the slot *after*
        let start_slot = base_slot.map_or(0, |slot| slot.saturating_add(1));
        // we want to *include* the storage at our slot
        let requested_slots = start_slot..=self.slot();

        self.rc
            .accounts
            .accounts_db
            .get_snapshot_storages(requested_slots)
            .0
    }

    #[must_use]
    fn verify_hash(&self) -> bool {
        assert!(self.is_frozen());
        let calculated_hash = self.hash_internal_state();
        let expected_hash = self.hash();

        if calculated_hash == expected_hash {
            true
        } else {
            warn!(
                "verify failed: slot: {}, {} (calculated) != {} (expected)",
                self.slot(),
                calculated_hash,
                expected_hash
            );
            false
        }
    }

    pub fn verify_transaction(
        &self,
        tx: VersionedTransaction,
        verification_mode: TransactionVerificationMode,
    ) -> Result<SanitizedTransaction> {
        let sanitized_tx = {
            let size =
                bincode::serialized_size(&tx).map_err(|_| TransactionError::SanitizeFailure)?;
            if size > PACKET_DATA_SIZE as u64 {
                return Err(TransactionError::SanitizeFailure);
            }
            let message_hash = if verification_mode == TransactionVerificationMode::FullVerification
            {
                tx.verify_and_hash_message()?
            } else {
                tx.message.hash()
            };

            SanitizedTransaction::try_create(tx, message_hash, None, self)
        }?;

        if verification_mode == TransactionVerificationMode::HashAndVerifyPrecompiles
            || verification_mode == TransactionVerificationMode::FullVerification
        {
            sanitized_tx.verify_precompiles(&self.feature_set)?;
        }

        Ok(sanitized_tx)
    }

    pub fn fully_verify_transaction(
        &self,
        tx: VersionedTransaction,
    ) -> Result<SanitizedTransaction> {
        self.verify_transaction(tx, TransactionVerificationMode::FullVerification)
    }

    /// only called from ledger-tool or tests
    fn calculate_capitalization(&self, debug_verify: bool) -> u64 {
        let is_startup = true;
        self.rc
            .accounts
            .accounts_db
            .verify_accounts_hash_in_bg
            .wait_for_complete();
        self.rc
            .accounts
            .accounts_db
            .update_accounts_hash_with_verify(
                // we have to use the index since the slot could be in the write cache still
                CalcAccountsHashDataSource::IndexForTests,
                debug_verify,
                self.slot(),
                &self.ancestors,
                None,
                self.epoch_schedule(),
                &self.rent_collector,
                is_startup,
                self.include_slot_in_hash(),
            )
            .1
    }

    /// only called from tests or ledger tool
    pub fn calculate_and_verify_capitalization(&self, debug_verify: bool) -> bool {
        let calculated = self.calculate_capitalization(debug_verify);
        let expected = self.capitalization();
        if calculated == expected {
            true
        } else {
            warn!(
                "Capitalization mismatch: calculated: {} != expected: {}",
                calculated, expected
            );
            false
        }
    }

    /// Forcibly overwrites current capitalization by actually recalculating accounts' balances.
    /// This should only be used for developing purposes.
    pub fn set_capitalization(&self) -> u64 {
        let old = self.capitalization();
        // We cannot debug verify the hash calculation here becuase calculate_capitalization will use the index calculation due to callers using the write cache.
        // debug_verify only exists as an extra debugging step under the assumption that this code path is only used for tests. But, this is used by ledger-tool create-snapshot
        // for example.
        let debug_verify = false;
        self.capitalization
            .store(self.calculate_capitalization(debug_verify), Relaxed);
        old
    }

    /// Returns the `AccountsHash` that was calculated for this bank's slot
    ///
    /// This fn is used when creating a snapshot with ledger-tool, or when
    /// packaging a snapshot into an archive (used to get the `SnapshotHash`).
    pub fn get_accounts_hash(&self) -> Option<AccountsHash> {
        self.rc
            .accounts
            .accounts_db
            .get_accounts_hash(self.slot())
            .map(|(accounts_hash, _)| accounts_hash)
    }

    /// Returns the `IncrementalAccountsHash` that was calculated for this bank's slot
    ///
    /// This fn is used when creating an incremental snapshot with ledger-tool, or when
    /// packaging a snapshot into an archive (used to get the `SnapshotHash`).
    pub fn get_incremental_accounts_hash(&self) -> Option<IncrementalAccountsHash> {
        self.rc
            .accounts
            .accounts_db
            .get_incremental_accounts_hash(self.slot())
            .map(|(incremental_accounts_hash, _)| incremental_accounts_hash)
    }

    /// Returns the `SnapshotHash` for this bank's slot
    ///
    /// This fn is used at startup to verify the bank was rebuilt correctly.
    ///
    /// # Panics
    ///
    /// Panics if there is both-or-neither of an `AccountsHash` and an `IncrementalAccountsHash`
    /// for this bank's slot.  There may only be one or the other.
    pub fn get_snapshot_hash(&self) -> SnapshotHash {
        let accounts_hash = self.get_accounts_hash();
        let incremental_accounts_hash = self.get_incremental_accounts_hash();

        let accounts_hash = match (accounts_hash, incremental_accounts_hash) {
            (Some(_), Some(_)) => panic!("Both full and incremental accounts hashes are present for slot {}; it is ambiguous which one to use for the snapshot hash!", self.slot()),
            (Some(accounts_hash), None) => accounts_hash.into(),
            (None, Some(incremental_accounts_hash)) => incremental_accounts_hash.into(),
            (None, None) => panic!("accounts hash is required to get snapshot hash"),
        };
        let epoch_accounts_hash = self.get_epoch_accounts_hash_to_serialize();
        SnapshotHash::new(&accounts_hash, epoch_accounts_hash.as_ref())
    }

    pub fn get_thread_pool(&self) -> &ThreadPool {
        &self.rc.accounts.accounts_db.thread_pool_clean
    }

    pub fn load_account_into_read_cache(&self, key: &Pubkey) {
        self.rc
            .accounts
            .accounts_db
            .load_account_into_read_cache(&self.ancestors, key);
    }

    pub fn update_accounts_hash(
        &self,
        data_source: CalcAccountsHashDataSource,
        mut debug_verify: bool,
        is_startup: bool,
    ) -> AccountsHash {
        let (accounts_hash, total_lamports) = self
            .rc
            .accounts
            .accounts_db
            .update_accounts_hash_with_verify(
                data_source,
                debug_verify,
                self.slot(),
                &self.ancestors,
                Some(self.capitalization()),
                self.epoch_schedule(),
                &self.rent_collector,
                is_startup,
                self.include_slot_in_hash(),
            );
        if total_lamports != self.capitalization() {
            datapoint_info!(
                "capitalization_mismatch",
                ("slot", self.slot(), i64),
                ("calculated_lamports", total_lamports, i64),
                ("capitalization", self.capitalization(), i64),
            );

            if !debug_verify {
                // cap mismatch detected. It has been logged to metrics above.
                // Run both versions of the calculation to attempt to get more info.
                debug_verify = true;
                self.rc
                    .accounts
                    .accounts_db
                    .update_accounts_hash_with_verify(
                        data_source,
                        debug_verify,
                        self.slot(),
                        &self.ancestors,
                        Some(self.capitalization()),
                        self.epoch_schedule(),
                        &self.rent_collector,
                        is_startup,
                        self.include_slot_in_hash(),
                    );
            }

            panic!(
                "capitalization_mismatch. slot: {}, calculated_lamports: {}, capitalization: {}",
                self.slot(),
                total_lamports,
                self.capitalization()
            );
        }
        accounts_hash
    }

    pub fn update_accounts_hash_for_tests(&self) -> AccountsHash {
        self.update_accounts_hash(CalcAccountsHashDataSource::IndexForTests, false, false)
    }

    /// Calculate the incremental accounts hash from `base_slot` to `self`
    pub fn update_incremental_accounts_hash(&self, base_slot: Slot) -> IncrementalAccountsHash {
        let config = CalcAccountsHashConfig {
            use_bg_thread_pool: true,
            check_hash: false,
            ancestors: None, // does not matter, will not be used
            epoch_schedule: &self.epoch_schedule,
            rent_collector: &self.rent_collector,
            store_detailed_debug_info_on_failure: false,
            include_slot_in_hash: self.include_slot_in_hash(),
        };
        let storages = self.get_snapshot_storages(Some(base_slot));
        let sorted_storages = SortedStorages::new(&storages);
        self.rc
            .accounts
            .accounts_db
            .update_incremental_accounts_hash(
                &config,
                &sorted_storages,
                self.slot(),
                HashStats::default(),
            )
            .unwrap() // unwrap here will never fail since check_hash = false
            .0
    }

    /// A snapshot bank should be purged of 0 lamport accounts which are not part of the hash
    /// calculation and could shield other real accounts.
    pub fn verify_snapshot_bank(
        &self,
        test_hash_calculation: bool,
        skip_shrink: bool,
        force_clean: bool,
        last_full_snapshot_slot: Slot,
        base: Option<(Slot, /*capitalization*/ u64)>,
    ) -> bool {
        let (_, clean_time_us) = measure_us!({
            let should_clean = force_clean || (!skip_shrink && self.slot() > 0);
            if should_clean {
                info!("Cleaning...");
                // We cannot clean past the last full snapshot's slot because we are about to
                // perform an accounts hash calculation *up to that slot*.  If we cleaned *past*
                // that slot, then accounts could be removed from older storages, which would
                // change the accounts hash.
                self.rc.accounts.accounts_db.clean_accounts(
                    Some(last_full_snapshot_slot),
                    true,
                    Some(last_full_snapshot_slot),
                    self.epoch_schedule(),
                );
                info!("Cleaning... Done.");
            } else {
                info!("Cleaning... Skipped.");
            }
        });

        let (_, shrink_time_us) = measure_us!({
            let should_shrink = !skip_shrink && self.slot() > 0;
            if should_shrink {
                info!("Shrinking...");
                self.rc.accounts.accounts_db.shrink_all_slots(
                    true,
                    Some(last_full_snapshot_slot),
                    self.epoch_schedule(),
                );
                info!("Shrinking... Done.");
            } else {
                info!("Shrinking... Skipped.");
            }
        });

        let (verified_accounts, verify_accounts_time_us) = measure_us!({
            let should_verify_accounts = !self.rc.accounts.accounts_db.skip_initial_hash_calc;
            if should_verify_accounts {
                info!("Verifying accounts...");
                let verified = self.verify_accounts_hash(
                    base,
                    VerifyAccountsHashConfig {
                        test_hash_calculation,
                        ignore_mismatch: false,
                        require_rooted_bank: false,
                        run_in_background: true,
                        store_hash_raw_data_for_debug: false,
                    },
                );
                info!("Verifying accounts... In background.");
                verified
            } else {
                info!("Verifying accounts... Skipped.");
                self.rc
                    .accounts
                    .accounts_db
                    .verify_accounts_hash_in_bg
                    .verification_complete();
                true
            }
        });

        let (verified_bank, verify_bank_time_us) = measure_us!({
            info!("Verifying bank...");
            let verified = self.verify_hash();
            info!("Verifying bank... Done.");
            verified
        });

        datapoint_info!(
            "verify_snapshot_bank",
            ("clean_us", clean_time_us, i64),
            ("shrink_us", shrink_time_us, i64),
            ("verify_accounts_us", verify_accounts_time_us, i64),
            ("verify_bank_us", verify_bank_time_us, i64),
        );

        verified_accounts && verified_bank
    }

    /// Return the number of hashes per tick
    pub fn hashes_per_tick(&self) -> &Option<u64> {
        &self.hashes_per_tick
    }

    /// Return the number of ticks per slot
    pub fn ticks_per_slot(&self) -> u64 {
        self.ticks_per_slot
    }

    /// Return the number of slots per year
    pub fn slots_per_year(&self) -> f64 {
        self.slots_per_year
    }

    /// Return the number of ticks since genesis.
    pub fn tick_height(&self) -> u64 {
        self.tick_height.load(Relaxed)
    }

    /// Return the inflation parameters of the Bank
    pub fn inflation(&self) -> Inflation {
        *self.inflation.read().unwrap()
    }

    /// Return the rent collector for this Bank
    pub fn rent_collector(&self) -> &RentCollector {
        &self.rent_collector
    }

    /// Return the total capitalization of the Bank
    pub fn capitalization(&self) -> u64 {
        self.capitalization.load(Relaxed)
    }

    /// Return this bank's max_tick_height
    pub fn max_tick_height(&self) -> u64 {
        self.max_tick_height
    }

    /// Return the block_height of this bank
    pub fn block_height(&self) -> u64 {
        self.block_height
    }

    /// Return the number of slots per epoch for the given epoch
    pub fn get_slots_in_epoch(&self, epoch: Epoch) -> u64 {
        self.epoch_schedule().get_slots_in_epoch(epoch)
    }

    /// returns the epoch for which this bank's leader_schedule_slot_offset and slot would
    ///  need to cache leader_schedule
    pub fn get_leader_schedule_epoch(&self, slot: Slot) -> Epoch {
        self.epoch_schedule().get_leader_schedule_epoch(slot)
    }

    /// a bank-level cache of vote accounts and stake delegation info
    fn update_stakes_cache(
        &self,
        txs: &[SanitizedTransaction],
        execution_results: &[TransactionExecutionResult],
        loaded_txs: &[TransactionLoadResult],
    ) {
        debug_assert_eq!(txs.len(), execution_results.len());
        debug_assert_eq!(txs.len(), loaded_txs.len());
        let new_warmup_cooldown_rate_epoch = self.new_warmup_cooldown_rate_epoch();
        izip!(txs, execution_results, loaded_txs)
            .filter(|(_, execution_result, _)| execution_result.was_executed_successfully())
            .flat_map(|(tx, _, (load_result, _))| {
                load_result.iter().flat_map(|loaded_transaction| {
                    let num_account_keys = tx.message().account_keys().len();
                    loaded_transaction.accounts.iter().take(num_account_keys)
                })
            })
            .for_each(|(pubkey, account)| {
                // note that this could get timed to: self.rc.accounts.accounts_db.stats.stakes_cache_check_and_store_us,
                //  but this code path is captured separately in ExecuteTimingType::UpdateStakesCacheUs
                self.stakes_cache
                    .check_and_store(pubkey, account, new_warmup_cooldown_rate_epoch);
            });
    }

    pub fn staked_nodes(&self) -> Arc<HashMap<Pubkey, u64>> {
        self.stakes_cache.stakes().staked_nodes()
    }

    /// current vote accounts for this bank along with the stake
    ///   attributed to each account
    pub fn vote_accounts(&self) -> Arc<VoteAccountsHashMap> {
        let stakes = self.stakes_cache.stakes();
        Arc::from(stakes.vote_accounts())
    }

    /// Vote account for the given vote account pubkey.
    pub fn get_vote_account(&self, vote_account: &Pubkey) -> Option<VoteAccount> {
        let stakes = self.stakes_cache.stakes();
        let vote_account = stakes.vote_accounts().get(vote_account)?;
        Some(vote_account.clone())
    }

    /// Get the EpochStakes for a given epoch
    pub fn epoch_stakes(&self, epoch: Epoch) -> Option<&EpochStakes> {
        self.epoch_stakes.get(&epoch)
    }

    pub fn epoch_stakes_map(&self) -> &HashMap<Epoch, EpochStakes> {
        &self.epoch_stakes
    }

    pub fn epoch_staked_nodes(&self, epoch: Epoch) -> Option<Arc<HashMap<Pubkey, u64>>> {
        Some(self.epoch_stakes.get(&epoch)?.stakes().staked_nodes())
    }

    /// vote accounts for the specific epoch along with the stake
    ///   attributed to each account
    pub fn epoch_vote_accounts(&self, epoch: Epoch) -> Option<&VoteAccountsHashMap> {
        let epoch_stakes = self.epoch_stakes.get(&epoch)?.stakes();
        Some(epoch_stakes.vote_accounts().as_ref())
    }

    /// Get the fixed authorized voter for the given vote account for the
    /// current epoch
    pub fn epoch_authorized_voter(&self, vote_account: &Pubkey) -> Option<&Pubkey> {
        self.epoch_stakes
            .get(&self.epoch)
            .expect("Epoch stakes for bank's own epoch must exist")
            .epoch_authorized_voters()
            .get(vote_account)
    }

    /// Get the fixed set of vote accounts for the given node id for the
    /// current epoch
    pub fn epoch_vote_accounts_for_node_id(&self, node_id: &Pubkey) -> Option<&NodeVoteAccounts> {
        self.epoch_stakes
            .get(&self.epoch)
            .expect("Epoch stakes for bank's own epoch must exist")
            .node_id_to_vote_accounts()
            .get(node_id)
    }

    /// Get the fixed total stake of all vote accounts for current epoch
    pub fn total_epoch_stake(&self) -> u64 {
        self.epoch_stakes
            .get(&self.epoch)
            .expect("Epoch stakes for bank's own epoch must exist")
            .total_stake()
    }

    /// Get the fixed stake of the given vote account for the current epoch
    pub fn epoch_vote_account_stake(&self, vote_account: &Pubkey) -> u64 {
        *self
            .epoch_vote_accounts(self.epoch())
            .expect("Bank epoch vote accounts must contain entry for the bank's own epoch")
            .get(vote_account)
            .map(|(stake, _)| stake)
            .unwrap_or(&0)
    }

    /// given a slot, return the epoch and offset into the epoch this slot falls
    /// e.g. with a fixed number for slots_per_epoch, the calculation is simply:
    ///
    ///  ( slot/slots_per_epoch, slot % slots_per_epoch )
    ///
    pub fn get_epoch_and_slot_index(&self, slot: Slot) -> (Epoch, SlotIndex) {
        self.epoch_schedule().get_epoch_and_slot_index(slot)
    }

    pub fn get_epoch_info(&self) -> EpochInfo {
        let absolute_slot = self.slot();
        let block_height = self.block_height();
        let (epoch, slot_index) = self.get_epoch_and_slot_index(absolute_slot);
        let slots_in_epoch = self.get_slots_in_epoch(epoch);
        let transaction_count = Some(self.transaction_count());
        EpochInfo {
            epoch,
            slot_index,
            slots_in_epoch,
            absolute_slot,
            block_height,
            transaction_count,
        }
    }

    pub fn is_empty(&self) -> bool {
        !self.is_delta.load(Relaxed)
    }

    pub fn add_mockup_builtin(
        &mut self,
        program_id: Pubkey,
        builtin_function: BuiltinFunctionWithContext,
    ) {
        self.add_builtin(
            program_id,
            "mockup".to_string(),
            LoadedProgram::new_builtin(self.slot, 0, builtin_function),
        );
    }

    /// Add a built-in program
    pub fn add_builtin(&mut self, program_id: Pubkey, name: String, builtin: LoadedProgram) {
        debug!("Adding program {} under {:?}", name, program_id);
        self.add_builtin_account(name.as_str(), &program_id, false);
        self.builtin_programs.insert(program_id);
        self.loaded_programs_cache
            .write()
            .unwrap()
            .replenish(program_id, Arc::new(builtin));
        debug!("Added program {} under {:?}", name, program_id);
    }

    /// Remove a built-in instruction processor
    pub fn remove_builtin(&mut self, program_id: Pubkey, name: String) {
        debug!("Removing program {}", program_id);
        // Don't remove the account since the bank expects the account state to
        // be idempotent
        self.add_builtin(
            program_id,
            name,
            LoadedProgram::new_tombstone(self.slot, LoadedProgramType::Closed),
        );
        debug!("Removed program {}", program_id);
    }

    pub fn add_precompile(&mut self, program_id: &Pubkey) {
        debug!("Adding precompiled program {}", program_id);
        self.add_precompiled_account(program_id);
        debug!("Added precompiled program {:?}", program_id);
    }

    // Call AccountsDb::clean_accounts()
    //
    // This fn is meant to be called by the snapshot handler in Accounts Background Service.  If
    // calling from elsewhere, ensure the same invariants hold/expectations are met.
    pub(crate) fn clean_accounts(&self, last_full_snapshot_slot: Option<Slot>) {
        // Don't clean the slot we're snapshotting because it may have zero-lamport
        // accounts that were included in the bank delta hash when the bank was frozen,
        // and if we clean them here, any newly created snapshot's hash for this bank
        // may not match the frozen hash.
        //
        // So when we're snapshotting, the highest slot to clean is lowered by one.
        let highest_slot_to_clean = self.slot().saturating_sub(1);

        self.rc.accounts.accounts_db.clean_accounts(
            Some(highest_slot_to_clean),
            false,
            last_full_snapshot_slot,
            self.epoch_schedule(),
        );
    }

    pub fn print_accounts_stats(&self) {
        self.rc.accounts.accounts_db.print_accounts_stats("");
    }

    pub fn shrink_candidate_slots(&self) -> usize {
        self.rc
            .accounts
            .accounts_db
            .shrink_candidate_slots(self.epoch_schedule())
    }

    pub(crate) fn shrink_ancient_slots(&self) {
        self.rc
            .accounts
            .accounts_db
            .shrink_ancient_slots(self.epoch_schedule())
    }

    pub fn no_overflow_rent_distribution_enabled(&self) -> bool {
        self.feature_set
            .is_active(&feature_set::no_overflow_rent_distribution::id())
    }

    pub fn prevent_rent_paying_rent_recipients(&self) -> bool {
        self.feature_set
            .is_active(&feature_set::prevent_rent_paying_rent_recipients::id())
    }

    pub fn validate_fee_collector_account(&self) -> bool {
        self.feature_set
            .is_active(&feature_set::validate_fee_collector_account::id())
    }

    pub fn read_cost_tracker(&self) -> LockResult<RwLockReadGuard<CostTracker>> {
        self.cost_tracker.read()
    }

    pub fn write_cost_tracker(&self) -> LockResult<RwLockWriteGuard<CostTracker>> {
        self.cost_tracker.write()
    }

    // Check if the wallclock time from bank creation to now has exceeded the allotted
    // time for transaction processing
    pub fn should_bank_still_be_processing_txs(
        bank_creation_time: &Instant,
        max_tx_ingestion_nanos: u128,
    ) -> bool {
        // Do this check outside of the PoH lock, hence not a method on PohRecorder
        bank_creation_time.elapsed().as_nanos() <= max_tx_ingestion_nanos
    }

    pub fn deactivate_feature(&mut self, id: &Pubkey) {
        let mut feature_set = Arc::make_mut(&mut self.feature_set).clone();
        feature_set.active.remove(id);
        feature_set.inactive.insert(*id);
        self.feature_set = Arc::new(feature_set);
    }

    pub fn activate_feature(&mut self, id: &Pubkey) {
        let mut feature_set = Arc::make_mut(&mut self.feature_set).clone();
        feature_set.inactive.remove(id);
        feature_set.active.insert(*id, 0);
        self.feature_set = Arc::new(feature_set);
    }

    pub fn fill_bank_with_ticks_for_tests(&self) {
        if self.tick_height.load(Relaxed) < self.max_tick_height {
            let last_blockhash = self.last_blockhash();
            while self.last_blockhash() == last_blockhash {
                self.register_tick(&Hash::new_unique())
            }
        } else {
            warn!("Bank already reached max tick height, cannot fill it with more ticks");
        }
    }

    // This is called from snapshot restore AND for each epoch boundary
    // The entire code path herein must be idempotent
    fn apply_feature_activations(
        &mut self,
        caller: ApplyFeatureActivationsCaller,
        debug_do_not_add_builtins: bool,
    ) {
        use ApplyFeatureActivationsCaller::*;
        let allow_new_activations = match caller {
            FinishInit => false,
            NewFromParent => true,
            WarpFromParent => false,
        };
        let (feature_set, new_feature_activations) =
            self.compute_active_feature_set(allow_new_activations);
        self.feature_set = Arc::new(feature_set);

        // Update activation slot of features in `new_feature_activations`
        for feature_id in new_feature_activations.iter() {
            if let Some(mut account) = self.get_account_with_fixed_root(feature_id) {
                if let Some(mut feature) = feature::from_account(&account) {
                    feature.activated_at = Some(self.slot());
                    if feature::to_account(&feature, &mut account).is_some() {
                        self.store_account(feature_id, &account);
                    }
                    info!("Feature {} activated at slot {}", feature_id, self.slot());
                }
            }
        }

        if new_feature_activations.contains(&feature_set::pico_inflation::id()) {
            *self.inflation.write().unwrap() = Inflation::pico();
            self.fee_rate_governor.burn_percent = 50; // 50% fee burn
            self.rent_collector.rent.burn_percent = 50; // 50% rent burn
        }

        if !new_feature_activations.is_disjoint(&self.feature_set.full_inflation_features_enabled())
        {
            *self.inflation.write().unwrap() = Inflation::full();
            self.fee_rate_governor.burn_percent = 50; // 50% fee burn
            self.rent_collector.rent.burn_percent = 50; // 50% rent burn
        }

        if !debug_do_not_add_builtins {
            self.apply_builtin_program_feature_transitions(
                allow_new_activations,
                &new_feature_activations,
            );
        }

        if new_feature_activations.contains(&feature_set::cap_accounts_data_len::id()) {
            const ACCOUNTS_DATA_LEN: u64 = 50_000_000_000;
            self.accounts_data_size_initial = ACCOUNTS_DATA_LEN;
        }

        if new_feature_activations.contains(&feature_set::update_hashes_per_tick::id()) {
            self.apply_updated_hashes_per_tick(DEFAULT_HASHES_PER_TICK);
        }

        if new_feature_activations.contains(&feature_set::update_hashes_per_tick2::id()) {
            self.apply_updated_hashes_per_tick(UPDATED_HASHES_PER_TICK2);
        }

        if new_feature_activations.contains(&feature_set::update_hashes_per_tick3::id()) {
            self.apply_updated_hashes_per_tick(UPDATED_HASHES_PER_TICK3);
        }

        if new_feature_activations.contains(&feature_set::update_hashes_per_tick4::id()) {
            self.apply_updated_hashes_per_tick(UPDATED_HASHES_PER_TICK4);
        }

        if new_feature_activations.contains(&feature_set::update_hashes_per_tick5::id()) {
            self.apply_updated_hashes_per_tick(UPDATED_HASHES_PER_TICK5);
        }

        if new_feature_activations.contains(&feature_set::update_hashes_per_tick6::id()) {
            self.apply_updated_hashes_per_tick(UPDATED_HASHES_PER_TICK6);
        }
    }

    fn apply_updated_hashes_per_tick(&mut self, hashes_per_tick: u64) {
        info!(
            "Activating update_hashes_per_tick {} at slot {}",
            hashes_per_tick,
            self.slot(),
        );
        self.hashes_per_tick = Some(hashes_per_tick);
    }

    fn adjust_sysvar_balance_for_rent(&self, account: &mut AccountSharedData) {
        account.set_lamports(
            self.get_minimum_balance_for_rent_exemption(account.data().len())
                .max(account.lamports()),
        );
    }

    /// Compute the active feature set based on the current bank state,
    /// and return it together with the set of newly activated features.
    fn compute_active_feature_set(&self, include_pending: bool) -> (FeatureSet, HashSet<Pubkey>) {
        let mut active = self.feature_set.active.clone();
        let mut inactive = HashSet::new();
        let mut pending = HashSet::new();
        let slot = self.slot();

        for feature_id in &self.feature_set.inactive {
            let mut activated = None;
            if let Some(account) = self.get_account_with_fixed_root(feature_id) {
                if let Some(feature) = feature::from_account(&account) {
                    match feature.activated_at {
                        None if include_pending => {
                            // Feature activation is pending
                            pending.insert(*feature_id);
                            activated = Some(slot);
                        }
                        Some(activation_slot) if slot >= activation_slot => {
                            // Feature has been activated already
                            activated = Some(activation_slot);
                        }
                        _ => {}
                    }
                }
            }
            if let Some(slot) = activated {
                active.insert(*feature_id, slot);
            } else {
                inactive.insert(*feature_id);
            }
        }

        (FeatureSet { active, inactive }, pending)
    }

    fn apply_builtin_program_feature_transitions(
        &mut self,
        only_apply_transitions_for_new_features: bool,
        new_feature_activations: &HashSet<Pubkey>,
    ) {
        for builtin in BUILTINS.iter() {
            if let Some(feature_id) = builtin.feature_id {
                let should_apply_action_for_feature_transition =
                    if only_apply_transitions_for_new_features {
                        new_feature_activations.contains(&feature_id)
                    } else {
                        self.feature_set.is_active(&feature_id)
                    };
                if should_apply_action_for_feature_transition {
                    self.add_builtin(
                        builtin.program_id,
                        builtin.name.to_string(),
                        LoadedProgram::new_builtin(
                            self.feature_set.activated_slot(&feature_id).unwrap_or(0),
                            builtin.name.len(),
                            builtin.entrypoint,
                        ),
                    );
                }
            }
        }
        for precompile in get_precompiles() {
            #[allow(clippy::blocks_in_if_conditions)]
            if precompile.feature.map_or(false, |ref feature_id| {
                self.feature_set.is_active(feature_id)
            }) {
                self.add_precompile(&precompile.program_id);
            }
        }
    }

    /// Use to replace programs by feature activation
    #[allow(dead_code)]
    fn replace_program_account(
        &mut self,
        old_address: &Pubkey,
        new_address: &Pubkey,
        datapoint_name: &'static str,
    ) {
        if let Some(old_account) = self.get_account_with_fixed_root(old_address) {
            if let Some(new_account) = self.get_account_with_fixed_root(new_address) {
                datapoint_info!(datapoint_name, ("slot", self.slot, i64));

                // Burn lamports in the old account
                self.capitalization
                    .fetch_sub(old_account.lamports(), Relaxed);

                // Transfer new account to old account
                self.store_account(old_address, &new_account);

                // Clear new account
                self.store_account(new_address, &AccountSharedData::default());

                // Unload a program from the bank's cache
                self.loaded_programs_cache
                    .write()
                    .unwrap()
                    .remove_programs([*old_address].into_iter());

                self.calculate_and_update_accounts_data_size_delta_off_chain(
                    old_account.data().len(),
                    new_account.data().len(),
                );
            }
        }
    }

    /// Get all the accounts for this bank and calculate stats
    pub fn get_total_accounts_stats(&self) -> ScanResult<TotalAccountsStats> {
        let accounts = self.get_all_accounts()?;
        Ok(self.calculate_total_accounts_stats(
            accounts
                .iter()
                .map(|(pubkey, account, _slot)| (pubkey, account)),
        ))
    }

    /// Given all the accounts for a bank, calculate stats
    pub fn calculate_total_accounts_stats<'a>(
        &self,
        accounts: impl Iterator<Item = (&'a Pubkey, &'a AccountSharedData)>,
    ) -> TotalAccountsStats {
        let rent_collector = self.rent_collector();
        let mut total_accounts_stats = TotalAccountsStats::default();
        accounts.for_each(|(pubkey, account)| {
            total_accounts_stats.accumulate_account(pubkey, account, rent_collector);
        });

        total_accounts_stats
    }

    /// Get the EAH that will be used by snapshots
    ///
    /// Since snapshots are taken on roots, if the bank is in the EAH calculation window then an
    /// EAH *must* be included.  This means if an EAH calculation is currently in-flight we will
    /// wait for it to complete.
    pub fn get_epoch_accounts_hash_to_serialize(&self) -> Option<EpochAccountsHash> {
        let should_get_epoch_accounts_hash = self
            .feature_set
            .is_active(&feature_set::epoch_accounts_hash::id())
            && epoch_accounts_hash_utils::is_enabled_this_epoch(self)
            && epoch_accounts_hash_utils::is_in_calculation_window(self);
        if !should_get_epoch_accounts_hash {
            return None;
        }

        let (epoch_accounts_hash, measure) = measure!(self
            .rc
            .accounts
            .accounts_db
            .epoch_accounts_hash_manager
            .wait_get_epoch_accounts_hash());

        datapoint_info!(
            "bank-get_epoch_accounts_hash_to_serialize",
            ("slot", self.slot(), i64),
            ("waiting-time-us", measure.as_us(), i64),
        );
        Some(epoch_accounts_hash)
    }

    /// Return the epoch_reward_status field on the bank to serialize
    /// Returns none if we are NOT in the reward interval.
    pub(crate) fn get_epoch_reward_status_to_serialize(&self) -> Option<&EpochRewardStatus> {
        matches!(self.epoch_reward_status, EpochRewardStatus::Active(_))
            .then_some(&self.epoch_reward_status)
    }

    /// Convenience fn to get the Epoch Accounts Hash
    pub fn epoch_accounts_hash(&self) -> Option<EpochAccountsHash> {
        self.rc
            .accounts
            .accounts_db
            .epoch_accounts_hash_manager
            .try_get_epoch_accounts_hash()
    }

    /// Checks a batch of sanitized transactions again bank for age and status
    pub fn check_transactions_with_forwarding_delay(
        &self,
        transactions: &[SanitizedTransaction],
        filter: &[transaction::Result<()>],
        forward_transactions_to_leader_at_slot_offset: u64,
    ) -> Vec<TransactionCheckResult> {
        let mut error_counters = TransactionErrorMetrics::default();
        // The following code also checks if the blockhash for a transaction is too old
        // The check accounts for
        //  1. Transaction forwarding delay
        //  2. The slot at which the next leader will actually process the transaction
        // Drop the transaction if it will expire by the time the next node receives and processes it
        let api = perf_libs::api();
        let max_tx_fwd_delay = if api.is_none() {
            MAX_TRANSACTION_FORWARDING_DELAY
        } else {
            MAX_TRANSACTION_FORWARDING_DELAY_GPU
        };

        self.check_transactions(
            transactions,
            filter,
            (MAX_PROCESSING_AGE)
                .saturating_sub(max_tx_fwd_delay)
                .saturating_sub(forward_transactions_to_leader_at_slot_offset as usize),
            &mut error_counters,
        )
    }

    pub fn is_in_slot_hashes_history(&self, slot: &Slot) -> bool {
        if slot < &self.slot {
            if let Ok(sysvar_cache) = self.sysvar_cache.read() {
                if let Ok(slot_hashes) = sysvar_cache.get_slot_hashes() {
                    return slot_hashes.get(slot).is_some();
                }
            }
        }
        false
    }
}

/// Compute how much an account has changed size.  This function is useful when the data size delta
/// needs to be computed and passed to an `update_accounts_data_size_delta` function.
fn calculate_data_size_delta(old_data_size: usize, new_data_size: usize) -> i64 {
    assert!(old_data_size <= i64::MAX as usize);
    assert!(new_data_size <= i64::MAX as usize);
    let old_data_size = old_data_size as i64;
    let new_data_size = new_data_size as i64;

    new_data_size.saturating_sub(old_data_size)
}

/// Since `apply_feature_activations()` has different behavior depending on its caller, enumerate
/// those callers explicitly.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum ApplyFeatureActivationsCaller {
    FinishInit,
    NewFromParent,
    WarpFromParent,
}

/// Return the computed values from `collect_rent_from_accounts()`
///
/// Since `collect_rent_from_accounts()` is running in parallel, instead of updating the
/// atomics/shared data inside this function, return those values in this struct for the caller to
/// process later.
#[derive(Debug, Default)]
struct CollectRentFromAccountsInfo {
    rent_collected_info: CollectedInfo,
    rent_rewards: Vec<(Pubkey, RewardInfo)>,
    time_collecting_rent_us: u64,
    time_storing_accounts_us: u64,
    num_accounts: usize,
}

/// Return the computed values—of each iteration in the parallel loop inside
/// `collect_rent_in_partition()`—and then perform a reduce on all of them.
#[derive(Debug, Default)]
struct CollectRentInPartitionInfo {
    rent_collected: u64,
    accounts_data_size_reclaimed: u64,
    rent_rewards: Vec<(Pubkey, RewardInfo)>,
    time_loading_accounts_us: u64,
    time_collecting_rent_us: u64,
    time_storing_accounts_us: u64,
    num_accounts: usize,
}

impl CollectRentInPartitionInfo {
    /// Create a new `CollectRentInPartitionInfo` from the results of loading accounts and
    /// collecting rent on them.
    #[must_use]
    fn new(info: CollectRentFromAccountsInfo, time_loading_accounts: Duration) -> Self {
        Self {
            rent_collected: info.rent_collected_info.rent_amount,
            accounts_data_size_reclaimed: info.rent_collected_info.account_data_len_reclaimed,
            rent_rewards: info.rent_rewards,
            time_loading_accounts_us: time_loading_accounts.as_micros() as u64,
            time_collecting_rent_us: info.time_collecting_rent_us,
            time_storing_accounts_us: info.time_storing_accounts_us,
            num_accounts: info.num_accounts,
        }
    }

    /// Reduce (i.e. 'combine') two `CollectRentInPartitionInfo`s into one.
    ///
    /// This fn is used by `collect_rent_in_partition()` as the reduce step (of map-reduce) in its
    /// parallel loop of rent collection.
    #[must_use]
    fn reduce(lhs: Self, rhs: Self) -> Self {
        Self {
            rent_collected: lhs.rent_collected.saturating_add(rhs.rent_collected),
            accounts_data_size_reclaimed: lhs
                .accounts_data_size_reclaimed
                .saturating_add(rhs.accounts_data_size_reclaimed),
            rent_rewards: [lhs.rent_rewards, rhs.rent_rewards].concat(),
            time_loading_accounts_us: lhs
                .time_loading_accounts_us
                .saturating_add(rhs.time_loading_accounts_us),
            time_collecting_rent_us: lhs
                .time_collecting_rent_us
                .saturating_add(rhs.time_collecting_rent_us),
            time_storing_accounts_us: lhs
                .time_storing_accounts_us
                .saturating_add(rhs.time_storing_accounts_us),
            num_accounts: lhs.num_accounts.saturating_add(rhs.num_accounts),
        }
    }
}

/// Struct to collect stats when scanning all accounts in `get_total_accounts_stats()`
#[derive(Debug, Default, Copy, Clone)]
pub struct TotalAccountsStats {
    /// Total number of accounts
    pub num_accounts: usize,
    /// Total data size of all accounts
    pub data_len: usize,

    /// Total number of executable accounts
    pub num_executable_accounts: usize,
    /// Total data size of executable accounts
    pub executable_data_len: usize,

    /// Total number of rent exempt accounts
    pub num_rent_exempt_accounts: usize,
    /// Total number of rent paying accounts
    pub num_rent_paying_accounts: usize,
    /// Total number of rent paying accounts without data
    pub num_rent_paying_accounts_without_data: usize,
    /// Total amount of lamports in rent paying accounts
    pub lamports_in_rent_paying_accounts: u64,
}

impl TotalAccountsStats {
    pub fn accumulate_account(
        &mut self,
        address: &Pubkey,
        account: &AccountSharedData,
        rent_collector: &RentCollector,
    ) {
        let data_len = account.data().len();
        self.num_accounts += 1;
        self.data_len += data_len;

        if account.executable() {
            self.num_executable_accounts += 1;
            self.executable_data_len += data_len;
        }

        if !rent_collector.should_collect_rent(address, account)
            || rent_collector.get_rent_due(account).is_exempt()
        {
            self.num_rent_exempt_accounts += 1;
        } else {
            self.num_rent_paying_accounts += 1;
            self.lamports_in_rent_paying_accounts += account.lamports();
            if data_len == 0 {
                self.num_rent_paying_accounts_without_data += 1;
            }
        }
    }
}

impl Drop for Bank {
    fn drop(&mut self) {
        if let Some(drop_callback) = self.drop_callback.read().unwrap().0.as_ref() {
            drop_callback.callback(self);
        } else {
            // Default case for tests
            self.rc
                .accounts
                .accounts_db
                .purge_slot(self.slot(), self.bank_id(), false);
        }
    }
}

/// utility function used for testing and benchmarking.
pub mod test_utils {
    use {
        super::Bank,
        solana_sdk::{
            account::{ReadableAccount, WritableAccount},
            hash::hashv,
            lamports::LamportsError,
            pubkey::Pubkey,
        },
        solana_vote_program::vote_state::{self, BlockTimestamp, VoteStateVersions},
    };
    pub fn goto_end_of_slot(bank: &Bank) {
        let mut tick_hash = bank.last_blockhash();
        loop {
            tick_hash = hashv(&[tick_hash.as_ref(), &[42]]);
            bank.register_tick(&tick_hash);
            if tick_hash == bank.last_blockhash() {
                bank.freeze();
                return;
            }
        }
    }

    pub fn update_vote_account_timestamp(
        timestamp: BlockTimestamp,
        bank: &Bank,
        vote_pubkey: &Pubkey,
    ) {
        let mut vote_account = bank.get_account(vote_pubkey).unwrap_or_default();
        let mut vote_state = vote_state::from(&vote_account).unwrap_or_default();
        vote_state.last_timestamp = timestamp;
        let versioned = VoteStateVersions::new_current(vote_state);
        vote_state::to(&versioned, &mut vote_account).unwrap();
        bank.store_account(vote_pubkey, &vote_account);
    }

    pub fn deposit(
        bank: &Bank,
        pubkey: &Pubkey,
        lamports: u64,
    ) -> std::result::Result<u64, LamportsError> {
        // This doesn't collect rents intentionally.
        // Rents should only be applied to actual TXes
        let mut account = bank.get_account_with_fixed_root(pubkey).unwrap_or_default();
        account.checked_add_lamports(lamports)?;
        bank.store_account(pubkey, &account);
        Ok(account.lamports())
    }
}

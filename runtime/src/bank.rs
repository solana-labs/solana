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
//! It then has apis for retrieving if a transaction has been processed and it's status.
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
        account_overrides::AccountOverrides,
        account_rent_state::RentState,
        accounts::{
            AccountAddressFilter, Accounts, LoadedTransaction, PubkeyAccountSlot,
            TransactionLoadResult,
        },
        accounts_db::{
            AccountShrinkThreshold, AccountStorageEntry, AccountsDbConfig,
            BankHashLamportsVerifyConfig, CalcAccountsHashDataSource, IncludeSlotInHash,
            ACCOUNTS_DB_CONFIG_FOR_BENCHMARKS, ACCOUNTS_DB_CONFIG_FOR_TESTING,
        },
        accounts_hash::AccountsHash,
        accounts_index::{AccountSecondaryIndexes, IndexKey, ScanConfig, ScanResult, ZeroLamport},
        accounts_update_notifier_interface::AccountsUpdateNotifier,
        ancestors::{Ancestors, AncestorsForSerialization},
        bank::metrics::*,
        blockhash_queue::BlockhashQueue,
        builtins::{self, BuiltinAction, BuiltinFeatureTransition, Builtins},
        cost_tracker::CostTracker,
        epoch_accounts_hash::{self, EpochAccountsHash},
        epoch_stakes::{EpochStakes, NodeVoteAccounts},
        inline_spl_associated_token_account, inline_spl_token,
        message_processor::MessageProcessor,
        rent_collector::{CollectedInfo, RentCollector},
        runtime_config::RuntimeConfig,
        snapshot_hash::SnapshotHash,
        stake_account::{self, StakeAccount},
        stake_weighted_timestamp::{
            calculate_stake_weighted_timestamp, MaxAllowableDrift,
            MAX_ALLOWABLE_DRIFT_PERCENTAGE_FAST, MAX_ALLOWABLE_DRIFT_PERCENTAGE_SLOW_V2,
        },
        stakes::{InvalidCacheEntryReason, Stakes, StakesCache, StakesEnum},
        status_cache::{SlotDelta, StatusCache},
        storable_accounts::StorableAccounts,
        system_instruction_processor::{get_system_account_kind, SystemAccountKind},
        transaction_batch::TransactionBatch,
        transaction_error_metrics::TransactionErrorMetrics,
        vote_account::{VoteAccount, VoteAccountsHashMap},
        vote_parser,
    },
    byteorder::{ByteOrder, LittleEndian},
    dashmap::{DashMap, DashSet},
    itertools::Itertools,
    log::*,
    rayon::{
        iter::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator},
        ThreadPool, ThreadPoolBuilder,
    },
    solana_measure::{measure, measure::Measure, measure_us},
    solana_metrics::{inc_new_counter_debug, inc_new_counter_info},
    solana_perf::perf_libs,
    solana_program_runtime::{
        accounts_data_meter::MAX_ACCOUNTS_DATA_LEN,
        compute_budget::{self, ComputeBudget},
        executor_cache::{BankExecutorCache, TransactionExecutorCache, MAX_CACHED_EXECUTORS},
        invoke_context::{BuiltinProgram, ProcessInstructionWithContext},
        loaded_programs::{LoadedProgram, LoadedPrograms, WorkingSlot},
        log_collector::LogCollector,
        sysvar_cache::SysvarCache,
        timings::{ExecuteTimingType, ExecuteTimings},
    },
    solana_sdk::{
        account::{
            create_account_shared_data_with_fields as create_account, from_account, Account,
            AccountSharedData, InheritableAccountFields, ReadableAccount, WritableAccount,
        },
        account_utils::StateMut,
        bpf_loader_upgradeable::{self, UpgradeableLoaderState},
        clock::{
            BankId, Epoch, Slot, SlotCount, SlotIndex, UnixTimestamp, DEFAULT_HASHES_PER_TICK,
            DEFAULT_TICKS_PER_SECOND, INITIAL_RENT_EPOCH, MAX_PROCESSING_AGE,
            MAX_TRANSACTION_FORWARDING_DELAY, MAX_TRANSACTION_FORWARDING_DELAY_GPU,
            SECONDS_PER_DAY,
        },
        ed25519_program,
        epoch_info::EpochInfo,
        epoch_schedule::EpochSchedule,
        feature,
        feature_set::{
            self, disable_fee_calculator, enable_early_verification_of_account_modifications,
            enable_request_heap_frame_ix, remove_congestion_multiplier_from_fee_calculation,
            remove_deprecated_request_unit_ix, use_default_units_in_fee_calculation, FeatureSet,
        },
        fee::FeeStructure,
        fee_calculator::{FeeCalculator, FeeRateGovernor},
        genesis_config::{ClusterType, GenesisConfig},
        hard_forks::HardForks,
        hash::{extend_and_hash, hashv, Hash},
        incinerator,
        inflation::Inflation,
        instruction::{CompiledInstruction, TRANSACTION_LEVEL_STACK_HEIGHT},
        lamports::LamportsError,
        message::{AccountKeys, SanitizedMessage},
        native_loader,
        native_token::{sol_to_lamports, LAMPORTS_PER_SOL},
        nonce::{self, state::DurableNonce, NONCED_TX_MARKER_IX_INDEX},
        nonce_account,
        packet::PACKET_DATA_SIZE,
        precompiles::get_precompiles,
        pubkey::Pubkey,
        saturating_add_assign, secp256k1_program,
        signature::{Keypair, Signature},
        slot_hashes::SlotHashes,
        slot_history::{Check, SlotHistory},
        stake::state::Delegation,
        system_transaction,
        sysvar::{self, Sysvar, SysvarId},
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
        self, InflationPointCalculationEvent, PointValue, StakeState,
    },
    solana_vote_program::vote_state::{VoteState, VoteStateVersions},
    std::{
        borrow::Cow,
        cell::RefCell,
        collections::{HashMap, HashSet},
        convert::{TryFrom, TryInto},
        fmt, mem,
        ops::{Deref, RangeInclusive},
        path::PathBuf,
        rc::Rc,
        sync::{
            atomic::{
                AtomicBool, AtomicI64, AtomicU64, AtomicUsize,
                Ordering::{AcqRel, Acquire, Relaxed},
            },
            Arc, LockResult, RwLock, RwLockReadGuard, RwLockWriteGuard,
        },
        thread::Builder,
        time::{Duration, Instant},
    },
};

/// params to `verify_bank_hash`
pub struct VerifyBankHash {
    pub test_hash_calculation: bool,
    pub ignore_mismatch: bool,
    pub require_rooted_bank: bool,
    pub run_in_background: bool,
    pub store_hash_raw_data_for_debug: bool,
}

mod address_lookup_table;
mod builtin_programs;
mod metrics;
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RentDebit {
    rent_collected: u64,
    post_balance: u64,
}

impl RentDebit {
    fn try_into_reward_info(self) -> Option<RewardInfo> {
        let rent_debit = i64::try_from(self.rent_collected)
            .ok()
            .and_then(|r| r.checked_neg());
        rent_debit.map(|rent_debit| RewardInfo {
            reward_type: RewardType::Rent,
            lamports: rent_debit,
            post_balance: self.post_balance,
            commission: None, // Not applicable
        })
    }
}

/// Incremental snapshots only calculate their accounts hash based on the account changes WITHIN the incremental slot range.
/// So, we need to keep track of the full snapshot expected accounts hash results.
/// We also need to keep track of the hash and capitalization specific to the incremental snapshot slot range.
/// The capitalization we calculate for the incremental slot will NOT be consistent with the bank's capitalization.
/// It is not feasible to calculate a capitalization delta that is correct given just incremental slots account data and the full snapshot's capitalization.
#[derive(Serialize, Deserialize, AbiExample, Clone, Debug, Default, PartialEq, Eq)]
pub struct BankIncrementalSnapshotPersistence {
    /// slot of full snapshot
    pub full_slot: Slot,
    /// accounts hash from the full snapshot
    pub full_hash: Hash,
    /// capitalization from the full snapshot
    pub full_capitalization: u64,
    /// hash of the accounts in the incremental snapshot slot range, including zero-lamport accounts
    pub incremental_hash: Hash,
    /// capitalization of the accounts in the incremental snapshot slot range
    pub incremental_capitalization: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RentDebits(HashMap<Pubkey, RentDebit>);
impl RentDebits {
    fn get_account_rent_debit(&self, address: &Pubkey) -> u64 {
        self.0
            .get(address)
            .map(|r| r.rent_collected)
            .unwrap_or_default()
    }

    pub fn insert(&mut self, address: &Pubkey, rent_collected: u64, post_balance: u64) {
        if rent_collected != 0 {
            self.0.insert(
                *address,
                RentDebit {
                    rent_collected,
                    post_balance,
                },
            );
        }
    }

    pub fn into_unordered_rewards_iter(self) -> impl Iterator<Item = (Pubkey, RewardInfo)> {
        self.0
            .into_iter()
            .filter_map(|(address, rent_debit)| Some((address, rent_debit.try_into_reward_info()?)))
    }
}

pub type BankStatusCache = StatusCache<Result<()>>;
#[frozen_abi(digest = "AwRx17Bgesvvu9NZVwAoqofq7MU52gkwvjLvjVQpW64S")]
pub type BankSlotDelta = SlotDelta<Result<()>>;

// Eager rent collection repeats in cyclic manner.
// Each cycle is composed of <partition_count> number of tiny pubkey subranges
// to scan, which is always multiple of the number of slots in epoch.
pub(crate) type PartitionIndex = u64;
pub type PartitionsPerCycle = u64;
type Partition = (PartitionIndex, PartitionIndex, PartitionsPerCycle);
type RentCollectionCycleParams = (
    Epoch,
    SlotCount,
    bool,
    Epoch,
    EpochCount,
    PartitionsPerCycle,
);

pub struct SquashTiming {
    pub squash_accounts_ms: u64,
    pub squash_accounts_cache_ms: u64,
    pub squash_accounts_index_ms: u64,
    pub squash_accounts_store_ms: u64,

    pub squash_cache_ms: u64,
}

type EpochCount = u64;

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

pub type TransactionCheckResult = (Result<()>, Option<NoncePartial>);

pub struct TransactionResults {
    pub fee_collection_results: Vec<Result<()>>,
    pub execution_results: Vec<TransactionExecutionResult>,
    pub rent_debits: Vec<RentDebits>,
}

#[derive(Debug, Clone)]
pub struct TransactionExecutionDetails {
    pub status: Result<()>,
    pub log_messages: Option<Vec<String>>,
    pub inner_instructions: Option<InnerInstructionsList>,
    pub durable_nonce_fee: Option<DurableNonceFee>,
    pub return_data: Option<TransactionReturnData>,
    pub executed_units: u64,
    /// The change in accounts data len for this transaction.
    /// NOTE: This value is valid IFF `status` is `Ok`.
    pub accounts_data_len_delta: i64,
}

/// Type safe representation of a transaction execution attempt which
/// differentiates between a transaction that was executed (will be
/// committed to the ledger) and a transaction which wasn't executed
/// and will be dropped.
///
/// Note: `Result<TransactionExecutionDetails, TransactionError>` is not
/// used because it's easy to forget that the inner `details.status` field
/// is what should be checked to detect a successful transaction. This
/// enum provides a convenience method `Self::was_executed_successfully` to
/// make such checks hard to do incorrectly.
#[derive(Debug, Clone)]
pub enum TransactionExecutionResult {
    Executed {
        details: TransactionExecutionDetails,
        tx_executor_cache: Rc<RefCell<TransactionExecutorCache>>,
    },
    NotExecuted(TransactionError),
}

impl TransactionExecutionResult {
    pub fn was_executed_successfully(&self) -> bool {
        match self {
            Self::Executed { details, .. } => details.status.is_ok(),
            Self::NotExecuted { .. } => false,
        }
    }

    pub fn was_executed(&self) -> bool {
        match self {
            Self::Executed { .. } => true,
            Self::NotExecuted(_) => false,
        }
    }

    pub fn details(&self) -> Option<&TransactionExecutionDetails> {
        match self {
            Self::Executed { details, .. } => Some(details),
            Self::NotExecuted(_) => None,
        }
    }

    pub fn flattened_result(&self) -> Result<()> {
        match self {
            Self::Executed { details, .. } => details.status.clone(),
            Self::NotExecuted(err) => Err(err.clone()),
        }
    }
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

#[derive(Debug, Clone)]
pub enum DurableNonceFee {
    Valid(u64),
    Invalid,
}

impl From<&NonceFull> for DurableNonceFee {
    fn from(nonce: &NonceFull) -> Self {
        match nonce.lamports_per_signature() {
            Some(lamports_per_signature) => Self::Valid(lamports_per_signature),
            None => Self::Invalid,
        }
    }
}

impl DurableNonceFee {
    pub fn lamports_per_signature(&self) -> Option<u64> {
        match self {
            Self::Valid(lamports_per_signature) => Some(*lamports_per_signature),
            Self::Invalid => None,
        }
    }
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

/// An ordered list of compiled instructions that were invoked during a
/// transaction instruction
pub type InnerInstructions = Vec<InnerInstruction>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InnerInstruction {
    pub instruction: CompiledInstruction,
    /// Invocation stack height of this instruction. Instruction stack height
    /// starts at 1 for transaction instructions.
    pub stack_height: u8,
}

/// A list of compiled instructions that were invoked during each instruction of
/// a transaction
pub type InnerInstructionsList = Vec<InnerInstructions>;

/// Extract the InnerInstructionsList from a TransactionContext
pub fn inner_instructions_list_from_instruction_trace(
    transaction_context: &TransactionContext,
) -> InnerInstructionsList {
    debug_assert!(transaction_context
        .get_instruction_context_at_index_in_trace(0)
        .map(|instruction_context| instruction_context.get_stack_height()
            == TRANSACTION_LEVEL_STACK_HEIGHT)
        .unwrap_or(true));
    let mut outer_instructions = Vec::new();
    for index_in_trace in 0..transaction_context.get_instruction_trace_length() {
        if let Ok(instruction_context) =
            transaction_context.get_instruction_context_at_index_in_trace(index_in_trace)
        {
            let stack_height = instruction_context.get_stack_height();
            if stack_height == TRANSACTION_LEVEL_STACK_HEIGHT {
                outer_instructions.push(Vec::new());
            } else if let Some(inner_instructions) = outer_instructions.last_mut() {
                let stack_height = u8::try_from(stack_height).unwrap_or(u8::MAX);
                let instruction = CompiledInstruction::new_from_raw_parts(
                    instruction_context
                        .get_index_of_program_account_in_transaction(
                            instruction_context
                                .get_number_of_program_accounts()
                                .saturating_sub(1),
                        )
                        .unwrap_or_default() as u8,
                    instruction_context.get_instruction_data().to_vec(),
                    (0..instruction_context.get_number_of_instruction_accounts())
                        .map(|instruction_account_index| {
                            instruction_context
                                .get_index_of_instruction_account_in_transaction(
                                    instruction_account_index,
                                )
                                .unwrap_or_default() as u8
                        })
                        .collect(),
                );
                inner_instructions.push(InnerInstruction {
                    instruction,
                    stack_height,
                });
            } else {
                debug_assert!(false);
            }
        } else {
            debug_assert!(false);
        }
    }
    outer_instructions
}

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

pub trait NonceInfo {
    fn address(&self) -> &Pubkey;
    fn account(&self) -> &AccountSharedData;
    fn lamports_per_signature(&self) -> Option<u64>;
    fn fee_payer_account(&self) -> Option<&AccountSharedData>;
}

/// Holds limited nonce info available during transaction checks
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NoncePartial {
    address: Pubkey,
    account: AccountSharedData,
}
impl NoncePartial {
    pub fn new(address: Pubkey, account: AccountSharedData) -> Self {
        Self { address, account }
    }
}
impl NonceInfo for NoncePartial {
    fn address(&self) -> &Pubkey {
        &self.address
    }
    fn account(&self) -> &AccountSharedData {
        &self.account
    }
    fn lamports_per_signature(&self) -> Option<u64> {
        nonce_account::lamports_per_signature_of(&self.account)
    }
    fn fee_payer_account(&self) -> Option<&AccountSharedData> {
        None
    }
}

/// Holds fee subtracted nonce info
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NonceFull {
    address: Pubkey,
    account: AccountSharedData,
    fee_payer_account: Option<AccountSharedData>,
}
impl NonceFull {
    pub fn new(
        address: Pubkey,
        account: AccountSharedData,
        fee_payer_account: Option<AccountSharedData>,
    ) -> Self {
        Self {
            address,
            account,
            fee_payer_account,
        }
    }
    pub fn from_partial(
        partial: NoncePartial,
        message: &SanitizedMessage,
        accounts: &[TransactionAccount],
        rent_debits: &RentDebits,
    ) -> Result<Self> {
        let fee_payer = (0..message.account_keys().len()).find_map(|i| {
            if let Some((k, a)) = &accounts.get(i) {
                if message.is_non_loader_key(i) {
                    return Some((k, a));
                }
            }
            None
        });

        if let Some((fee_payer_address, fee_payer_account)) = fee_payer {
            let mut fee_payer_account = fee_payer_account.clone();
            let rent_debit = rent_debits.get_account_rent_debit(fee_payer_address);
            fee_payer_account.set_lamports(fee_payer_account.lamports().saturating_add(rent_debit));

            let nonce_address = *partial.address();
            if *fee_payer_address == nonce_address {
                Ok(Self::new(nonce_address, fee_payer_account, None))
            } else {
                Ok(Self::new(
                    nonce_address,
                    partial.account().clone(),
                    Some(fee_payer_account),
                ))
            }
        } else {
            Err(TransactionError::AccountNotFound)
        }
    }
}
impl NonceInfo for NonceFull {
    fn address(&self) -> &Pubkey {
        &self.address
    }
    fn account(&self) -> &AccountSharedData {
        &self.account
    }
    fn lamports_per_signature(&self) -> Option<u64> {
        nonce_account::lamports_per_signature_of(&self.account)
    }
    fn fee_payer_account(&self) -> Option<&AccountSharedData> {
        self.fee_payer_account.as_ref()
    }
}

// Bank's common fields shared by all supported snapshot versions for deserialization.
// Sync fields with BankFieldsToSerialize! This is paired with it.
// All members are made public to remain Bank's members private and to make versioned deserializer workable on this
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
}

// Bank's common fields shared by all supported snapshot versions for serialization.
// This is separated from BankFieldsToDeserialize to avoid cloning by using refs.
// So, sync fields with BankFieldsToDeserialize!
// all members are made public to keep Bank private and to make versioned serializer workable on this
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
            fee_calculator,
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
            builtin_feature_transitions: _,
            rewards: _,
            cluster_type: _,
            lazy_rent_collection: _,
            rewards_pool_pubkeys: _,
            executor_cache: _,
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
            && fee_calculator == &other.fee_calculator
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

fn null_tracer() -> Option<impl Fn(&RewardCalculationEvent) + Send + Sync> {
    None::<fn(&RewardCalculationEvent)>
}

pub trait DropCallback: fmt::Debug {
    fn callback(&self, b: &Bank);
    fn clone_box(&self) -> Box<dyn DropCallback + Send + Sync>;
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, AbiExample, Clone, Copy)]
pub struct RewardInfo {
    pub reward_type: RewardType,
    pub lamports: i64,          // Reward amount
    pub post_balance: u64,      // Account balance in lamports after `lamports` was applied
    pub commission: Option<u8>, // Vote account commission when the reward was credited, only present for voting and staking rewards
}

#[derive(Debug, Default)]
pub struct OptionalDropCallback(Option<Box<dyn DropCallback + Send + Sync>>);

#[cfg(RUSTC_WITH_SPECIALIZATION)]
impl AbiExample for OptionalDropCallback {
    fn example() -> Self {
        Self(None)
    }
}

#[derive(Debug, Clone, Default)]
pub struct BuiltinPrograms {
    pub vec: Vec<BuiltinProgram>,
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
impl AbiExample for BuiltinPrograms {
    fn example() -> Self {
        Self::default()
    }
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

    /// Deprecated, do not use
    /// Latest transaction fees for transactions processed by this bank
    pub(crate) fee_calculator: FeeCalculator,

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

    /// The builtin programs
    builtin_programs: BuiltinPrograms,

    /// Optional config parameters that can override runtime behavior
    runtime_config: Arc<RuntimeConfig>,

    /// Dynamic feature transitions for builtin programs
    #[allow(clippy::rc_buffer)]
    builtin_feature_transitions: Arc<Vec<BuiltinFeatureTransition>>,

    /// Protocol-level rewards that were distributed by this bank
    pub rewards: RwLock<Vec<(Pubkey, RewardInfo)>>,

    pub cluster_type: Option<ClusterType>,

    pub lazy_rent_collection: AtomicBool,

    // this is temporary field only to remove rewards_pool entirely
    pub rewards_pool_pubkeys: Arc<HashSet<Pubkey>>,

    /// Cached executors
    executor_cache: RwLock<BankExecutorCache>,

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

    pub loaded_programs_cache: Arc<RwLock<LoadedPrograms>>,
}

struct VoteWithStakeDelegations {
    vote_state: Arc<VoteState>,
    vote_account: AccountSharedData,
    // TODO: use StakeAccount<Delegation> once the old code is deleted.
    delegations: Vec<(Pubkey, StakeAccount<()>)>,
}

struct LoadVoteAndStakeAccountsResult {
    vote_with_stake_delegations_map: DashMap<Pubkey, VoteWithStakeDelegations>,
    invalid_stake_keys: DashMap<Pubkey, InvalidCacheEntryReason>,
    invalid_vote_keys: DashMap<Pubkey, InvalidCacheEntryReason>,
    invalid_cached_vote_accounts: usize,
    invalid_cached_stake_accounts: usize,
    invalid_cached_stake_accounts_rent_epoch: usize,
    vote_accounts_cache_miss_count: usize,
}

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

struct StakeReward {
    stake_pubkey: Pubkey,
    stake_reward_info: RewardInfo,
    stake_account: AccountSharedData,
}

impl StakeReward {
    pub fn get_stake_reward(&self) -> i64 {
        self.stake_reward_info.lamports
    }
}

/// allow [StakeReward] to be passed to `StoreAccounts` directly without copies or vec construction
impl<'a> StorableAccounts<'a, AccountSharedData> for (Slot, &'a [StakeReward], IncludeSlotInHash) {
    fn pubkey(&self, index: usize) -> &Pubkey {
        &self.1[index].stake_pubkey
    }
    fn account(&self, index: usize) -> &AccountSharedData {
        &self.1[index].stake_account
    }
    fn slot(&self, _index: usize) -> Slot {
        // per-index slot is not unique per slot when per-account slot is not included in the source data
        self.target_slot()
    }
    fn target_slot(&self) -> Slot {
        self.0
    }
    fn len(&self) -> usize {
        self.1.len()
    }
    fn include_slot_in_hash(&self) -> IncludeSlotInHash {
        self.2
    }
}

impl WorkingSlot for Bank {
    fn current_slot(&self) -> Slot {
        self.slot
    }

    fn is_ancestor(&self, other: Slot) -> bool {
        self.ancestors.contains_key(&other)
    }
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
            Arc::<RuntimeConfig>::default(),
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
            fee_calculator: FeeCalculator::default(),
            fee_rate_governor: FeeRateGovernor::default(),
            collected_rent: AtomicU64::default(),
            rent_collector: RentCollector::default(),
            epoch_schedule: EpochSchedule::default(),
            inflation: Arc::<RwLock<Inflation>>::default(),
            stakes_cache: StakesCache::default(),
            epoch_stakes: HashMap::<Epoch, EpochStakes>::default(),
            is_delta: AtomicBool::default(),
            builtin_programs: BuiltinPrograms::default(),
            runtime_config: Arc::<RuntimeConfig>::default(),
            builtin_feature_transitions: Arc::<Vec<BuiltinFeatureTransition>>::default(),
            rewards: RwLock::<Vec<(Pubkey, RewardInfo)>>::default(),
            cluster_type: Option::<ClusterType>::default(),
            lazy_rent_collection: AtomicBool::default(),
            rewards_pool_pubkeys: Arc::<HashSet<Pubkey>>::default(),
            executor_cache: RwLock::<BankExecutorCache>::default(),
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
            loaded_programs_cache: Arc::<RwLock<LoadedPrograms>>::default(),
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
            &Arc::default(),
        )
    }

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
            &Arc::default(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_paths(
        genesis_config: &GenesisConfig,
        runtime_config: Arc<RuntimeConfig>,
        paths: Vec<PathBuf>,
        debug_keys: Option<Arc<HashSet<Pubkey>>>,
        additional_builtins: Option<&Builtins>,
        account_indexes: AccountSecondaryIndexes,
        shrink_ratio: AccountShrinkThreshold,
        debug_do_not_add_builtins: bool,
        accounts_db_config: Option<AccountsDbConfig>,
        accounts_update_notifier: Option<AccountsUpdateNotifier>,
        exit: &Arc<AtomicBool>,
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
        bank.fill_missing_sysvar_cache_entries();
        bank
    }

    /// Create a new bank that points to an immutable checkpoint of another bank.
    pub fn new_from_parent(parent: &Arc<Bank>, collector_id: &Pubkey, slot: Slot) -> Self {
        Self::_new_from_parent(
            parent,
            collector_id,
            slot,
            null_tracer(),
            NewBankOptions::default(),
        )
    }

    pub fn new_from_parent_with_options(
        parent: &Arc<Bank>,
        collector_id: &Pubkey,
        slot: Slot,
        new_bank_options: NewBankOptions,
    ) -> Self {
        Self::_new_from_parent(parent, collector_id, slot, null_tracer(), new_bank_options)
    }

    pub fn new_from_parent_with_tracer(
        parent: &Arc<Bank>,
        collector_id: &Pubkey,
        slot: Slot,
        reward_calc_tracer: impl Fn(&RewardCalculationEvent) + Send + Sync,
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

    fn _new_from_parent(
        parent: &Arc<Bank>,
        collector_id: &Pubkey,
        slot: Slot,
        reward_calc_tracer: Option<impl Fn(&RewardCalculationEvent) + Send + Sync>,
        new_bank_options: NewBankOptions,
    ) -> Self {
        let mut time = Measure::start("bank::new_from_parent");
        let NewBankOptions { vote_only_bank } = new_bank_options;

        parent.freeze();
        assert_ne!(slot, parent.slot());

        let epoch_schedule = parent.epoch_schedule;
        let epoch = epoch_schedule.get_epoch(slot);

        let (rc, bank_rc_creation_time_us) = measure_us!(BankRc {
            accounts: Arc::new(Accounts::new_from_parent(
                &parent.rc.accounts,
                slot,
                parent.slot(),
            )),
            parent: RwLock::new(Some(parent.clone())),
            slot,
            bank_id_generator: parent.rc.bank_id_generator.clone(),
        });

        let (status_cache, status_cache_time_us) = measure_us!(Arc::clone(&parent.status_cache));

        let ((fee_rate_governor, fee_calculator), fee_components_time_us) = measure_us!({
            let fee_rate_governor =
                FeeRateGovernor::new_derived(&parent.fee_rate_governor, parent.signature_count());

            let fee_calculator = if parent.feature_set.is_active(&disable_fee_calculator::id()) {
                FeeCalculator::default()
            } else {
                fee_rate_governor.create_fee_calculator()
            };
            (fee_rate_governor, fee_calculator)
        });

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

        let (executor_cache, executor_cache_time_us) = measure_us!({
            let parent_bank_executors = parent.executor_cache.read().unwrap();
            RwLock::new(BankExecutorCache::new_from_parent_bank_executors(
                &parent_bank_executors,
                epoch,
            ))
        });

        let (transaction_debug_keys, transaction_debug_keys_time_us) =
            measure_us!(parent.transaction_debug_keys.clone());

        let (transaction_log_collector_config, transaction_log_collector_config_time_us) =
            measure_us!(parent.transaction_log_collector_config.clone());

        let (feature_set, feature_set_time_us) = measure_us!(parent.feature_set.clone());

        let accounts_data_size_initial = parent.load_accounts_data_size();
        let mut new = Bank {
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
            fee_calculator,
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
            tick_height: AtomicU64::new(parent.tick_height.load(Relaxed)),
            signature_count: AtomicU64::new(0),
            builtin_programs,
            runtime_config: parent.runtime_config.clone(),
            builtin_feature_transitions: parent.builtin_feature_transitions.clone(),
            hard_forks: parent.hard_forks.clone(),
            rewards: RwLock::new(vec![]),
            cluster_type: parent.cluster_type,
            lazy_rent_collection: AtomicBool::new(parent.lazy_rent_collection.load(Relaxed)),
            rewards_pool_pubkeys,
            executor_cache,
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
        let parent_epoch = parent.epoch();
        let (_, update_epoch_time_us) = measure_us!({
            if parent_epoch < new.epoch() {
                let (thread_pool, thread_pool_time) = measure!(
                    ThreadPoolBuilder::new().build().unwrap(),
                    "thread_pool_creation",
                );

                let (_, apply_feature_activations_time) = measure!(
                    new.apply_feature_activations(
                        ApplyFeatureActivationsCaller::NewFromParent,
                        false
                    ),
                    "apply_feature_activation",
                );

                // Add new entry to stakes.stake_history, set appropriate epoch and
                // update vote accounts with warmed up stakes before saving a
                // snapshot of stakes in epoch stakes
                let (_, activate_epoch_time) = measure!(
                    new.stakes_cache.activate_epoch(epoch, &thread_pool),
                    "activate_epoch",
                );

                // Save a snapshot of stakes for use in consensus and stake weighted networking
                let leader_schedule_epoch = epoch_schedule.get_leader_schedule_epoch(slot);
                let (_, update_epoch_stakes_time) = measure!(
                    new.update_epoch_stakes(leader_schedule_epoch),
                    "update_epoch_stakes",
                );

                let mut rewards_metrics = RewardsMetrics::default();
                // After saving a snapshot of stakes, apply stake rewards and commission
                let (_, update_rewards_with_thread_pool_time) = measure!(
                    {
                        new.update_rewards_with_thread_pool(
                            parent_epoch,
                            reward_calc_tracer,
                            &thread_pool,
                            &mut rewards_metrics,
                        )
                    },
                    "update_rewards_with_thread_pool",
                );

                report_new_epoch_metrics(
                    new.epoch(),
                    slot,
                    parent.slot(),
                    NewEpochTimings {
                        thread_pool_time_us: thread_pool_time.as_us(),
                        apply_feature_activations_time_us: apply_feature_activations_time.as_us(),
                        activate_epoch_time_us: activate_epoch_time.as_us(),
                        update_epoch_stakes_time_us: update_epoch_stakes_time.as_us(),
                        update_rewards_with_thread_pool_time_us:
                            update_rewards_with_thread_pool_time.as_us(),
                    },
                    rewards_metrics,
                );
            } else {
                // Save a snapshot of stakes for use in consensus and stake weighted networking
                let leader_schedule_epoch = epoch_schedule.get_leader_schedule_epoch(slot);
                new.update_epoch_stakes(leader_schedule_epoch);
            }
        });

        // Update sysvars before processing transactions
        let (_, update_sysvars_time_us) = measure_us!({
            new.update_slot_hashes();
            new.update_stake_history(Some(parent_epoch));
            new.update_clock(Some(parent_epoch));
            new.update_fees();
        });

        let (_, fill_sysvar_cache_time_us) = measure_us!(new.fill_missing_sysvar_cache_entries());
        time.stop();

        report_new_bank_metrics(
            slot,
            new.block_height,
            parent.slot(),
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
                executor_cache_time_us,
                transaction_debug_keys_time_us,
                transaction_log_collector_config_time_us,
                feature_set_time_us,
                ancestors_time_us,
                update_epoch_time_us,
                update_sysvars_time_us,
                fill_sysvar_cache_time_us,
            },
        );

        parent
            .executor_cache
            .read()
            .unwrap()
            .stats
            .submit(parent.slot());

        new
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
        parent: &Arc<Bank>,
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
        additional_builtins: Option<&Builtins>,
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
            fee_calculator: fields.fee_calculator,
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
            builtin_feature_transitions: new(),
            rewards: new(),
            cluster_type: Some(genesis_config.cluster_type),
            lazy_rent_collection: new(),
            rewards_pool_pubkeys: new(),
            executor_cache: RwLock::new(BankExecutorCache::new(MAX_CACHED_EXECUTORS, fields.epoch)),
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
            loaded_programs_cache: Arc::<RwLock<LoadedPrograms>>::default(),
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
        if !bank.feature_set.is_active(&disable_fee_calculator::id()) {
            bank.fee_rate_governor.lamports_per_signature =
                bank.fee_calculator.lamports_per_signature;
            assert_eq!(
                bank.fee_rate_governor.create_fee_calculator(),
                bank.fee_calculator
            );
        }

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
            fee_calculator: self.fee_calculator,
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
            {
                let vote_stakes: HashMap<_, _> = self
                    .stakes_cache
                    .stakes()
                    .vote_accounts()
                    .delegated_stakes()
                    .map(|(pubkey, stake)| (*pubkey, stake))
                    .collect();
                info!(
                    "new epoch stakes, epoch: {}, stakes: {:#?}, total_stake: {}",
                    leader_schedule_epoch,
                    vote_stakes,
                    new_epoch_stakes.total_stake(),
                );
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
        let update_rewards_from_cached_accounts = self
            .feature_set
            .is_active(&feature_set::update_rewards_from_cached_accounts::id());

        self.pay_validator_rewards_with_thread_pool(
            prev_epoch,
            validator_rewards,
            reward_calc_tracer,
            self.credits_auto_rewind(),
            thread_pool,
            metrics,
            update_rewards_from_cached_accounts,
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
            ("num_stake_accounts", num_stake_accounts as i64, i64),
            ("num_vote_accounts", num_vote_accounts as i64, i64),
        );
    }

    /// map stake delegations into resolved (pubkey, account) pairs
    ///  returns a map (has to be copied) of loaded
    ///   ( Vec<(staker info)> (voter account) ) keyed by voter pubkey
    ///
    /// Filters out invalid pairs
    fn load_vote_and_stake_accounts_with_thread_pool(
        &self,
        thread_pool: &ThreadPool,
        reward_calc_tracer: Option<impl Fn(&RewardCalculationEvent) + Send + Sync>,
    ) -> LoadVoteAndStakeAccountsResult {
        let stakes = self.stakes_cache.stakes();
        let cached_vote_accounts = stakes.vote_accounts();
        let vote_with_stake_delegations_map = DashMap::with_capacity(cached_vote_accounts.len());
        let invalid_stake_keys: DashMap<Pubkey, InvalidCacheEntryReason> = DashMap::new();
        let invalid_vote_keys: DashMap<Pubkey, InvalidCacheEntryReason> = DashMap::new();
        let invalid_cached_stake_accounts = AtomicUsize::default();
        let invalid_cached_vote_accounts = AtomicUsize::default();
        let invalid_cached_stake_accounts_rent_epoch = AtomicUsize::default();

        let stake_delegations = self.filter_stake_delegations(&stakes);
        thread_pool.install(|| {
            stake_delegations
                .into_par_iter()
                .for_each(|(stake_pubkey, cached_stake_account)| {
                    let delegation = cached_stake_account.delegation();
                    let vote_pubkey = &delegation.voter_pubkey;
                    if invalid_vote_keys.contains_key(vote_pubkey) {
                        return;
                    }
                    let stake_account = match self.get_account_with_fixed_root(stake_pubkey) {
                        Some(stake_account) => stake_account,
                        None => {
                            invalid_stake_keys
                                .insert(*stake_pubkey, InvalidCacheEntryReason::Missing);
                            invalid_cached_stake_accounts.fetch_add(1, Relaxed);
                            return;
                        }
                    };
                    if cached_stake_account.account() != &stake_account {
                        if self.rc.accounts.accounts_db.assert_stakes_cache_consistency {
                            panic!(
                                "stakes cache accounts mismatch {cached_stake_account:?} {stake_account:?}"
                            );
                        }
                        invalid_cached_stake_accounts.fetch_add(1, Relaxed);
                        let cached_stake_account = cached_stake_account.account();
                        if cached_stake_account.lamports() == stake_account.lamports()
                            && cached_stake_account.data() == stake_account.data()
                            && cached_stake_account.owner() == stake_account.owner()
                            && cached_stake_account.executable() == stake_account.executable()
                        {
                            invalid_cached_stake_accounts_rent_epoch.fetch_add(1, Relaxed);
                        } else {
                            debug!(
                                "cached stake account mismatch: {}: {:?}, {:?}",
                                stake_pubkey, stake_account, cached_stake_account
                            );
                        }
                    }
                    let stake_account = match StakeAccount::<()>::try_from(stake_account) {
                        Ok(stake_account) => stake_account,
                        Err(stake_account::Error::InvalidOwner { .. }) => {
                            invalid_stake_keys
                                .insert(*stake_pubkey, InvalidCacheEntryReason::WrongOwner);
                            return;
                        }
                        Err(stake_account::Error::InstructionError(_)) => {
                            invalid_stake_keys
                                .insert(*stake_pubkey, InvalidCacheEntryReason::BadState);
                            return;
                        }
                        Err(stake_account::Error::InvalidDelegation(_)) => {
                            // This should not happen.
                            error!(
                                "Unexpected code path! StakeAccount<()> \
                                should not check if stake-state is a \
                                Delegation."
                            );
                            return;
                        }
                    };
                    let stake_delegation = (*stake_pubkey, stake_account);
                    let mut vote_delegations = if let Some(vote_delegations) =
                        vote_with_stake_delegations_map.get_mut(vote_pubkey)
                    {
                        vote_delegations
                    } else {
                        let cached_vote_account = cached_vote_accounts.get(vote_pubkey);
                        let vote_account = match self.get_account_with_fixed_root(vote_pubkey) {
                            Some(vote_account) => {
                                if vote_account.owner() != &solana_vote_program::id() {
                                    invalid_vote_keys
                                        .insert(*vote_pubkey, InvalidCacheEntryReason::WrongOwner);
                                    if cached_vote_account.is_some() {
                                        invalid_cached_vote_accounts.fetch_add(1, Relaxed);
                                    }
                                    return;
                                }
                                vote_account
                            }
                            None => {
                                if cached_vote_account.is_some() {
                                    invalid_cached_vote_accounts.fetch_add(1, Relaxed);
                                }
                                invalid_vote_keys
                                    .insert(*vote_pubkey, InvalidCacheEntryReason::Missing);
                                return;
                            }
                        };

                        let vote_state = if let Ok(vote_state) =
                            StateMut::<VoteStateVersions>::state(&vote_account)
                        {
                            vote_state.convert_to_current()
                        } else {
                            invalid_vote_keys
                                .insert(*vote_pubkey, InvalidCacheEntryReason::BadState);
                            if cached_vote_account.is_some() {
                                invalid_cached_vote_accounts.fetch_add(1, Relaxed);
                            }
                            return;
                        };
                        match cached_vote_account {
                            Some(cached_vote_account)
                                if cached_vote_account.account() == &vote_account => {}
                            _ => {
                                invalid_cached_vote_accounts.fetch_add(1, Relaxed);
                            }
                        };
                        vote_with_stake_delegations_map
                            .entry(*vote_pubkey)
                            .or_insert_with(|| VoteWithStakeDelegations {
                                vote_state: Arc::new(vote_state),
                                vote_account,
                                delegations: vec![],
                            })
                    };

                    if let Some(reward_calc_tracer) = reward_calc_tracer.as_ref() {
                        reward_calc_tracer(&RewardCalculationEvent::Staking(
                            stake_pubkey,
                            &InflationPointCalculationEvent::Delegation(
                                delegation,
                                solana_vote_program::id(),
                            ),
                        ));
                    }

                    vote_delegations.delegations.push(stake_delegation);
                });
        });
        invalid_cached_stake_accounts.fetch_add(invalid_stake_keys.len(), Relaxed);
        LoadVoteAndStakeAccountsResult {
            vote_with_stake_delegations_map,
            invalid_vote_keys,
            invalid_stake_keys,
            invalid_cached_vote_accounts: invalid_cached_vote_accounts.into_inner(),
            invalid_cached_stake_accounts: invalid_cached_stake_accounts.into_inner(),
            invalid_cached_stake_accounts_rent_epoch: invalid_cached_stake_accounts_rent_epoch
                .into_inner(),
            vote_accounts_cache_miss_count: 0,
        }
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

    fn load_vote_and_stake_accounts<F>(
        &self,
        thread_pool: &ThreadPool,
        reward_calc_tracer: Option<F>,
    ) -> LoadVoteAndStakeAccountsResult
    where
        F: Fn(&RewardCalculationEvent) + Send + Sync,
    {
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
            let vote_account = match get_vote_account(&vote_pubkey) {
                Some(vote_account) => vote_account,
                None => {
                    invalid_vote_keys.insert(vote_pubkey, InvalidCacheEntryReason::Missing);
                    return None;
                }
            };
            if vote_account.owner() != &solana_vote_program {
                invalid_vote_keys.insert(vote_pubkey, InvalidCacheEntryReason::WrongOwner);
                return None;
            }
            let vote_state = match vote_account.vote_state().deref() {
                Ok(vote_state) => vote_state.clone(),
                Err(_) => {
                    invalid_vote_keys.insert(vote_pubkey, InvalidCacheEntryReason::BadState);
                    return None;
                }
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
            let mut vote_delegations =
                match vote_with_stake_delegations_map.get_mut(&delegation.voter_pubkey) {
                    Some(vote_delegations) => vote_delegations,
                    None => return,
                };
            if let Some(reward_calc_tracer) = reward_calc_tracer.as_ref() {
                let delegation =
                    InflationPointCalculationEvent::Delegation(delegation, solana_vote_program);
                let event = RewardCalculationEvent::Staking(stake_pubkey, &delegation);
                reward_calc_tracer(&event);
            }
            let stake_account = StakeAccount::from(stake_account.clone());
            let stake_delegation = (*stake_pubkey, stake_account);
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
            invalid_stake_keys: DashMap::default(),
            invalid_cached_vote_accounts: 0,
            invalid_cached_stake_accounts: 0,
            invalid_cached_stake_accounts_rent_epoch: 0,
            vote_accounts_cache_miss_count: vote_accounts_cache_miss_count.into_inner(),
        }
    }

    /// iterate over all stakes, redeem vote credits for each stake we can
    /// successfully load and parse, return the lamport value of one point
    fn pay_validator_rewards_with_thread_pool(
        &mut self,
        rewarded_epoch: Epoch,
        rewards: u64,
        reward_calc_tracer: Option<impl Fn(&RewardCalculationEvent) + Send + Sync>,
        credits_auto_rewind: bool,
        thread_pool: &ThreadPool,
        metrics: &mut RewardsMetrics,
        update_rewards_from_cached_accounts: bool,
    ) {
        let stake_history = self.stakes_cache.stakes().history().clone();
        let vote_with_stake_delegations_map = {
            let mut m = Measure::start("load_vote_and_stake_accounts_us");
            let LoadVoteAndStakeAccountsResult {
                vote_with_stake_delegations_map,
                invalid_stake_keys,
                invalid_vote_keys,
                invalid_cached_vote_accounts,
                invalid_cached_stake_accounts,
                invalid_cached_stake_accounts_rent_epoch,
                vote_accounts_cache_miss_count,
            } = if update_rewards_from_cached_accounts {
                self.load_vote_and_stake_accounts(thread_pool, reward_calc_tracer.as_ref())
            } else {
                self.load_vote_and_stake_accounts_with_thread_pool(
                    thread_pool,
                    reward_calc_tracer.as_ref(),
                )
            };
            m.stop();
            metrics
                .load_vote_and_stake_accounts_us
                .fetch_add(m.as_us(), Relaxed);
            metrics.invalid_cached_vote_accounts += invalid_cached_vote_accounts;
            metrics.invalid_cached_stake_accounts += invalid_cached_stake_accounts;
            metrics.invalid_cached_stake_accounts_rent_epoch +=
                invalid_cached_stake_accounts_rent_epoch;
            metrics.vote_accounts_cache_miss_count += vote_accounts_cache_miss_count;
            self.stakes_cache.handle_invalid_keys(
                invalid_stake_keys,
                invalid_vote_keys,
                self.slot(),
            );
            vote_with_stake_delegations_map
        };

        let mut m = Measure::start("calculate_points");
        let points: u128 = thread_pool.install(|| {
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
                                Some(&stake_history),
                            )
                            .unwrap_or(0)
                        })
                        .sum::<u128>()
                })
                .sum()
        });
        m.stop();
        metrics.calculate_points_us.fetch_add(m.as_us(), Relaxed);

        if points == 0 {
            return;
        }

        // pay according to point value
        let point_value = PointValue { rewards, points };
        let vote_account_rewards: DashMap<Pubkey, (AccountSharedData, u8, u64, bool)> =
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
                vote_account_rewards
                    .insert(vote_pubkey, (vote_account, vote_state.commission, 0, false));
                delegations
                    .into_par_iter()
                    .map(move |delegation| (vote_pubkey, Arc::clone(&vote_state), delegation))
            },
        );

        let mut m = Measure::start("redeem_rewards");
        let stake_rewards: Vec<StakeReward> = thread_pool.install(|| {
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
                        <(AccountSharedData, StakeState)>::from(stake_account);
                    let redeemed = stake_state::redeem_rewards(
                        rewarded_epoch,
                        stake_state,
                        &mut stake_account,
                        &vote_state,
                        &point_value,
                        Some(&stake_history),
                        reward_calc_tracer.as_ref(),
                        credits_auto_rewind,
                    );
                    if let Ok((stakers_reward, voters_reward)) = redeemed {
                        // track voter rewards
                        if let Some((
                            _vote_account,
                            _commission,
                            vote_rewards_sum,
                            vote_needs_store,
                        )) = vote_account_rewards.get_mut(&vote_pubkey).as_deref_mut()
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
        });
        m.stop();
        metrics.redeem_rewards_us += m.as_us();

        self.store_stake_accounts(&stake_rewards, metrics);
        let vote_rewards = self.store_vote_accounts(vote_account_rewards, metrics);
        self.update_reward_history(stake_rewards, vote_rewards);
    }

    fn store_stake_accounts(&self, stake_rewards: &[StakeReward], metrics: &mut RewardsMetrics) {
        // store stake account even if stakers_reward is 0
        // because credits observed has changed
        let mut m = Measure::start("store_stake_account");
        self.store_accounts((self.slot(), stake_rewards, self.include_slot_in_hash()));
        m.stop();
        metrics
            .store_stake_accounts_us
            .fetch_add(m.as_us(), Relaxed);
    }

    fn store_vote_accounts(
        &self,
        vote_account_rewards: DashMap<Pubkey, (AccountSharedData, u8, u64, bool)>,
        metrics: &mut RewardsMetrics,
    ) -> Vec<(Pubkey, RewardInfo)> {
        let mut m = Measure::start("store_vote_accounts");
        let vote_rewards = vote_account_rewards
            .into_iter()
            .filter_map(
                |(vote_pubkey, (mut vote_account, commission, vote_rewards, vote_needs_store))| {
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
            .collect::<Vec<_>>();

        m.stop();
        metrics.store_vote_accounts_us.fetch_add(m.as_us(), Relaxed);
        vote_rewards
    }

    fn update_reward_history(
        &self,
        stake_rewards: Vec<StakeReward>,
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

    // Distribute collected transaction fees for this slot to collector_id (= current leader).
    //
    // Each validator is incentivized to process more transactions to earn more transaction fees.
    // Transaction fees are rewarded for the computing resource utilization cost, directly
    // proportional to their actual processing power.
    //
    // collector_id is rotated according to stake-weighted leader schedule. So the opportunity of
    // earning transaction fees are fairly distributed by stake. And missing the opportunity
    // (not producing a block as a leader) earns nothing. So, being online is incentivized as a
    // form of transaction fees as well.
    //
    // On the other hand, rent fees are distributed under slightly different philosophy, while
    // still being stake-weighted.
    // Ref: distribute_rent_to_validators
    fn collect_fees(&self) {
        let collector_fees = self.collector_fees.load(Relaxed);

        if collector_fees != 0 {
            let (deposit, mut burn) = self.fee_rate_governor.burn(collector_fees);
            // burn a portion of fees
            debug!(
                "distributed fee: {} (rounded from: {}, burned: {})",
                deposit, collector_fees, burn
            );

            match self.deposit(&self.collector_id, deposit) {
                Ok(post_balance) => {
                    if deposit != 0 {
                        self.rewards.write().unwrap().push((
                            self.collector_id,
                            RewardInfo {
                                reward_type: RewardType::Fee,
                                lamports: deposit as i64,
                                post_balance,
                                commission: None,
                            },
                        ));
                    }
                }
                Err(_) => {
                    error!(
                        "Burning {} fee instead of crediting {}",
                        deposit, self.collector_id
                    );
                    inc_new_counter_error!("bank-burned_fee_lamports", deposit as usize);
                    burn += deposit;
                }
            }
            self.capitalization.fetch_sub(burn, Relaxed);
        }
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
        // `process_and_record_transactions_locked()` from coming
        // in after the last tick is observed. This is because in
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
            self.collect_fees();
            self.distribute_rent();
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
        self.fee_calculator = self.fee_rate_governor.create_fee_calculator();

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

        // highest staked node is the first collector
        self.collector_id = self
            .stakes_cache
            .stakes()
            .highest_staked_node()
            .unwrap_or_default();

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
        Some(Self::calculate_fee(
            message,
            lamports_per_signature,
            &self.fee_structure,
            self.feature_set
                .is_active(&use_default_units_in_fee_calculation::id()),
            !self
                .feature_set
                .is_active(&remove_deprecated_request_unit_ix::id()),
            self.feature_set
                .is_active(&remove_congestion_multiplier_from_fee_calculation::id()),
            self.enable_request_heap_frame_ix(),
        ))
    }

    pub fn get_startup_verification_complete(&self) -> &Arc<AtomicBool> {
        &self
            .rc
            .accounts
            .accounts_db
            .verify_accounts_hash_in_bg
            .verified
    }

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
        Self::calculate_fee(
            message,
            lamports_per_signature,
            &self.fee_structure,
            self.feature_set
                .is_active(&use_default_units_in_fee_calculation::id()),
            !self
                .feature_set
                .is_active(&remove_deprecated_request_unit_ix::id()),
            self.feature_set
                .is_active(&remove_congestion_multiplier_from_fee_calculation::id()),
            self.enable_request_heap_frame_ix(),
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

        inc_new_counter_debug!("bank-register_tick-registered", 1);
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
        if self
            .feature_set
            .is_active(&feature_set::fix_recent_blockhashes::id())
        {
            tick_height == self.max_tick_height
        } else {
            tick_height % self.ticks_per_slot == 0
        }
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
            .map(|tx| {
                SanitizedTransaction::try_create(
                    tx,
                    MessageHash::Compute,
                    None,
                    self,
                    self.feature_set
                        .is_active(&feature_set::require_static_program_ids_in_transaction::ID),
                )
            })
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
        transaction_results: impl Iterator<Item = &'b Result<()>>,
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

    /// Prepare a transaction batch without locking accounts for transaction simulation.
    pub(crate) fn prepare_simulation_batch(
        &self,
        transaction: SanitizedTransaction,
    ) -> TransactionBatch<'_, '_> {
        let tx_account_lock_limit = self.get_transaction_account_lock_limit();
        let lock_result = transaction
            .get_account_locks(tx_account_lock_limit)
            .map(|_| ());
        let mut batch =
            TransactionBatch::new(vec![lock_result], self, Cow::Owned(vec![transaction]));
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
        let batch = self.prepare_simulation_batch(transaction);
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
                Ok(()) => {
                    let recent_blockhash = tx.message().recent_blockhash();
                    if hash_queue.is_hash_valid_for_age(recent_blockhash, max_age) {
                        (Ok(()), None)
                    } else if let Some((address, account)) =
                        self.check_transaction_for_nonce(tx, &next_durable_nonce)
                    {
                        (Ok(()), Some(NoncePartial::new(address, account)))
                    } else {
                        error_counters.blockhash_not_found += 1;
                        (Err(TransactionError::BlockhashNotFound), None)
                    }
                }
                Err(e) => (Err(e.clone()), None),
            })
            .collect()
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

    /// Get any cached executors needed by the transaction
    fn get_tx_executor_cache(
        &self,
        accounts: &[TransactionAccount],
    ) -> Rc<RefCell<TransactionExecutorCache>> {
        let executable_keys: Vec<_> = accounts
            .iter()
            .filter_map(|(key, account)| {
                if account.executable() && !native_loader::check_id(account.owner()) {
                    Some(key)
                } else {
                    None
                }
            })
            .collect();

        if executable_keys.is_empty() {
            return Rc::new(RefCell::new(TransactionExecutorCache::default()));
        }

        let tx_executor_cache = {
            let cache = self.executor_cache.read().unwrap();
            TransactionExecutorCache::new(
                executable_keys
                    .into_iter()
                    .filter_map(|key| cache.get(key).map(|executor| (*key, executor))),
            )
        };

        Rc::new(RefCell::new(tx_executor_cache))
    }

    /// Add executors back to the bank's cache if they were missing and not re-/deployed
    fn store_executors_which_added_to_the_cache(
        &self,
        tx_executor_cache: &RefCell<TransactionExecutorCache>,
    ) {
        let executors = tx_executor_cache
            .borrow_mut()
            .get_executors_added_to_the_cache();
        if !executors.is_empty() {
            self.executor_cache
                .write()
                .unwrap()
                .put(executors.into_iter());
        }
    }

    /// Add re-/deployed executors to the bank's cache
    fn store_executors_which_were_deployed(
        &self,
        tx_executor_cache: &RefCell<TransactionExecutorCache>,
    ) {
        let executors = tx_executor_cache
            .borrow_mut()
            .get_executors_which_were_deployed();
        if !executors.is_empty() {
            self.executor_cache
                .write()
                .unwrap()
                .put(executors.into_iter());
        }
    }

    /// Remove an executor from the bank's cache
    fn remove_executor(&self, pubkey: &Pubkey) {
        let _ = self.executor_cache.write().unwrap().remove(pubkey);
    }

    #[allow(dead_code)] // Preparation for BankExecutorCache rework
    fn load_program(&self, pubkey: &Pubkey) -> Result<Arc<LoadedProgram>> {
        let program = if let Some(program) = self.get_account_with_fixed_root(pubkey) {
            program
        } else {
            return Err(TransactionError::ProgramAccountNotFound);
        };
        let mut transaction_accounts = vec![(*pubkey, program)];
        let is_upgradeable_loader =
            bpf_loader_upgradeable::check_id(transaction_accounts[0].1.owner());
        if is_upgradeable_loader {
            if let Ok(UpgradeableLoaderState::Program {
                programdata_address,
            }) = transaction_accounts[0].1.state()
            {
                if let Some(programdata_account) =
                    self.get_account_with_fixed_root(&programdata_address)
                {
                    transaction_accounts.push((programdata_address, programdata_account));
                } else {
                    return Err(TransactionError::ProgramAccountNotFound);
                }
            } else {
                return Err(TransactionError::ProgramAccountNotFound);
            }
        }
        let mut transaction_context = TransactionContext::new(
            transaction_accounts,
            Some(sysvar::rent::Rent::default()),
            1,
            1,
        );
        let instruction_context = transaction_context
            .get_next_instruction_context()
            .map_err(|err| TransactionError::InstructionError(0, err))?;
        instruction_context.configure(if is_upgradeable_loader { &[0, 1] } else { &[0] }, &[], &[]);
        transaction_context
            .push()
            .map_err(|err| TransactionError::InstructionError(0, err))?;
        let instruction_context = transaction_context
            .get_current_instruction_context()
            .map_err(|err| TransactionError::InstructionError(0, err))?;
        let program = instruction_context
            .try_borrow_program_account(&transaction_context, 0)
            .map_err(|err| TransactionError::InstructionError(0, err))?;
        let programdata = if is_upgradeable_loader {
            Some(
                instruction_context
                    .try_borrow_program_account(&transaction_context, 1)
                    .map_err(|err| TransactionError::InstructionError(0, err))?,
            )
        } else {
            None
        };
        solana_bpf_loader_program::load_program_from_account(
            &self.feature_set,
            &self.runtime_config.compute_budget.unwrap_or_default(),
            None, // log_collector
            None,
            &program,
            programdata.as_ref().unwrap_or(&program),
            self.runtime_config.bpf_jit,
        )
        .map(|(loaded_program, _create_executor_metrics)| loaded_program)
        .map_err(|err| TransactionError::InstructionError(0, err))
    }

    pub fn clear_executors(&self) {
        self.executor_cache.write().unwrap().clear();
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
    ) -> TransactionExecutionResult {
        let mut get_tx_executor_cache_time = Measure::start("get_tx_executor_cache_time");
        let tx_executor_cache = self.get_tx_executor_cache(&loaded_transaction.accounts);
        get_tx_executor_cache_time.stop();
        saturating_add_assign!(
            timings.execute_accessories.get_executors_us,
            get_tx_executor_cache_time.as_us()
        );

        let prev_accounts_data_len = self.load_accounts_data_size();
        let transaction_accounts = std::mem::take(&mut loaded_transaction.accounts);
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

        let mut process_message_time = Measure::start("process_message_time");
        let process_result = MessageProcessor::process_message(
            &self.builtin_programs.vec,
            tx.message(),
            &loaded_transaction.program_indices,
            &mut transaction_context,
            self.rent_collector.rent,
            log_collector.clone(),
            tx_executor_cache.clone(),
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

        let mut store_executors_which_added_to_the_cache_time =
            Measure::start("store_executors_which_added_to_the_cache_time");
        self.store_executors_which_added_to_the_cache(&tx_executor_cache);
        store_executors_which_added_to_the_cache_time.stop();
        saturating_add_assign!(
            timings.execute_accessories.update_executors_us,
            store_executors_which_added_to_the_cache_time.as_us()
        );

        let status = process_result
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
        let mut accounts_data_len_delta = status
            .as_ref()
            .map_or(0, |info| info.accounts_data_len_delta);
        let status = status.map(|_| ());

        let log_messages: Option<TransactionLogMessages> =
            log_collector.and_then(|log_collector| {
                Rc::try_unwrap(log_collector)
                    .map(|log_collector| log_collector.into_inner().into())
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
            mut return_data,
            touched_account_count,
            accounts_resize_delta,
        } = transaction_context.into();
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

        let return_data = if enable_return_data_recording {
            if let Some(end_index) = return_data.data.iter().rposition(|&x| x != 0) {
                let end_index = end_index.saturating_add(1);
                return_data.data.truncate(end_index);
                Some(return_data)
            } else {
                None
            }
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
            tx_executor_cache,
        }
    }

    // A cluster specific feature gate, when not activated it keeps v1.13 behavior in mainnet-beta;
    // once activated for v1.14+, it allows compute_budget::request_heap_frame and
    // compute_budget::set_compute_unit_price co-exist in same transaction.
    fn enable_request_heap_frame_ix(&self) -> bool {
        self.feature_set
            .is_active(&enable_request_heap_frame_ix::id())
            || self.cluster_type() != ClusterType::MainnetBeta
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
        inc_new_counter_info!("bank-process_transactions", sanitized_txs.len());
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
        let check_results = self.check_transactions(
            sanitized_txs,
            batch.lock_results(),
            max_age,
            &mut error_counters,
        );
        check_time.stop();

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
                    let compute_budget =
                        if let Some(compute_budget) = self.runtime_config.compute_budget {
                            compute_budget
                        } else {
                            let mut compute_budget =
                                ComputeBudget::new(compute_budget::MAX_COMPUTE_UNIT_LIMIT as u64);

                            let mut compute_budget_process_transaction_time =
                                Measure::start("compute_budget_process_transaction_time");
                            let process_transaction_result = compute_budget.process_instructions(
                                tx.message().program_instructions_iter(),
                                true,
                                !self
                                    .feature_set
                                    .is_active(&remove_deprecated_request_unit_ix::id()),
                                true, // don't reject txs that use request heap size ix
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

                    self.execute_loaded_transaction(
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
                    )
                }
            })
            .collect();

        execution_time.stop();

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

            let is_vote = vote_parser::is_simple_vote_transaction(tx);

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

    fn get_num_signatures_in_message(message: &SanitizedMessage) -> u64 {
        let mut num_signatures = u64::from(message.header().num_required_signatures);
        // This next part is really calculating the number of pre-processor
        // operations being done and treating them like a signature
        for (program_id, instruction) in message.program_instructions_iter() {
            if secp256k1_program::check_id(program_id) || ed25519_program::check_id(program_id) {
                if let Some(num_verifies) = instruction.data.first() {
                    num_signatures = num_signatures.saturating_add(u64::from(*num_verifies));
                }
            }
        }
        num_signatures
    }

    fn get_num_write_locks_in_message(message: &SanitizedMessage) -> u64 {
        message
            .account_keys()
            .len()
            .saturating_sub(message.num_readonly_accounts()) as u64
    }

    /// Calculate fee for `SanitizedMessage`
    pub fn calculate_fee(
        message: &SanitizedMessage,
        lamports_per_signature: u64,
        fee_structure: &FeeStructure,
        use_default_units_per_instruction: bool,
        support_request_units_deprecated: bool,
        remove_congestion_multiplier: bool,
        enable_request_heap_frame_ix: bool,
    ) -> u64 {
        // Fee based on compute units and signatures
        let congestion_multiplier = if lamports_per_signature == 0 {
            0.0 // test only
        } else if remove_congestion_multiplier {
            1.0 // multiplier that has no effect
        } else {
            const BASE_CONGESTION: f64 = 5_000.0;
            let current_congestion = BASE_CONGESTION.max(lamports_per_signature as f64);
            BASE_CONGESTION / current_congestion
        };

        let mut compute_budget = ComputeBudget::default();
        let prioritization_fee_details = compute_budget
            .process_instructions(
                message.program_instructions_iter(),
                use_default_units_per_instruction,
                support_request_units_deprecated,
                enable_request_heap_frame_ix,
            )
            .unwrap_or_default();
        let prioritization_fee = prioritization_fee_details.get_fee();
        let signature_fee = Self::get_num_signatures_in_message(message)
            .saturating_mul(fee_structure.lamports_per_signature);
        let write_lock_fee = Self::get_num_write_locks_in_message(message)
            .saturating_mul(fee_structure.lamports_per_write_lock);
        let compute_fee = fee_structure
            .compute_fee_bins
            .iter()
            .find(|bin| compute_budget.compute_unit_limit <= bin.limit)
            .map(|bin| bin.fee)
            .unwrap_or_else(|| {
                fee_structure
                    .compute_fee_bins
                    .last()
                    .map(|bin| bin.fee)
                    .unwrap_or_default()
            });

        ((prioritization_fee
            .saturating_add(signature_fee)
            .saturating_add(write_lock_fee)
            .saturating_add(compute_fee) as f64)
            * congestion_multiplier)
            .round() as u64
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
                let fee = Self::calculate_fee(
                    tx.message(),
                    lamports_per_signature,
                    &self.fee_structure,
                    self.feature_set
                        .is_active(&use_default_units_in_fee_calculation::id()),
                    !self
                        .feature_set
                        .is_active(&remove_deprecated_request_unit_ix::id()),
                    self.feature_set
                        .is_active(&remove_congestion_multiplier_from_fee_calculation::id()),
                    self.enable_request_heap_frame_ix(),
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

        inc_new_counter_info!(
            "bank-process_transactions-txs",
            committed_transactions_count as usize
        );
        inc_new_counter_info!(
            "bank-process_non_vote_transactions-txs",
            committed_non_vote_transactions_count as usize
        );
        inc_new_counter_info!("bank-process_transactions-sigs", signature_count as usize);

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
                tx_executor_cache,
            } = execution_result
            {
                if details.status.is_ok() {
                    self.store_executors_which_were_deployed(tx_executor_cache);
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

    // Distribute collected rent fees for this slot to staked validators (excluding stakers)
    // according to stake.
    //
    // The nature of rent fee is the cost of doing business, every validator has to hold (or have
    // access to) the same list of accounts, so we pay according to stake, which is a rough proxy for
    // value to the network.
    //
    // Currently, rent distribution doesn't consider given validator's uptime at all (this might
    // change). That's because rent should be rewarded for the storage resource utilization cost.
    // It's treated differently from transaction fees, which is for the computing resource
    // utilization cost.
    //
    // We can't use collector_id (which is rotated according to stake-weighted leader schedule)
    // as an approximation to the ideal rent distribution to simplify and avoid this per-slot
    // computation for the distribution (time: N log N, space: N acct. stores; N = # of
    // validators).
    // The reason is that rent fee doesn't need to be incentivized for throughput unlike transaction
    // fees
    //
    // Ref: collect_fees
    #[allow(clippy::needless_collect)]
    fn distribute_rent_to_validators(
        &self,
        vote_accounts: &VoteAccountsHashMap,
        rent_to_be_distributed: u64,
    ) {
        let mut total_staked = 0;

        // Collect the stake associated with each validator.
        // Note that a validator may be present in this vector multiple times if it happens to have
        // more than one staked vote account somehow
        let mut validator_stakes = vote_accounts
            .iter()
            .filter_map(|(_vote_pubkey, (staked, account))| {
                if *staked == 0 {
                    None
                } else {
                    total_staked += *staked;
                    Some((account.node_pubkey()?, *staked))
                }
            })
            .collect::<Vec<(Pubkey, u64)>>();

        #[cfg(test)]
        if validator_stakes.is_empty() {
            // some tests bank.freezes() with bad staking state
            self.capitalization
                .fetch_sub(rent_to_be_distributed, Relaxed);
            return;
        }
        #[cfg(not(test))]
        assert!(!validator_stakes.is_empty());

        // Sort first by stake and then by validator identity pubkey for determinism.
        // If two items are still equal, their relative order does not matter since
        // both refer to the same validator.
        validator_stakes.sort_unstable_by(|(pubkey1, staked1), (pubkey2, staked2)| {
            (staked1, pubkey1).cmp(&(staked2, pubkey2)).reverse()
        });

        let enforce_fix = self.no_overflow_rent_distribution_enabled();

        let mut rent_distributed_in_initial_round = 0;
        let validator_rent_shares = validator_stakes
            .into_iter()
            .map(|(pubkey, staked)| {
                let rent_share = if !enforce_fix {
                    (((staked * rent_to_be_distributed) as f64) / (total_staked as f64)) as u64
                } else {
                    (((staked as u128) * (rent_to_be_distributed as u128)) / (total_staked as u128))
                        .try_into()
                        .unwrap()
                };
                rent_distributed_in_initial_round += rent_share;
                (pubkey, rent_share)
            })
            .collect::<Vec<(Pubkey, u64)>>();

        // Leftover lamports after fraction calculation, will be paid to validators starting from highest stake
        // holder
        let mut leftover_lamports = rent_to_be_distributed - rent_distributed_in_initial_round;

        let mut rewards = vec![];
        validator_rent_shares
            .into_iter()
            .for_each(|(pubkey, rent_share)| {
                let rent_to_be_paid = if leftover_lamports > 0 {
                    leftover_lamports -= 1;
                    rent_share + 1
                } else {
                    rent_share
                };
                if !enforce_fix || rent_to_be_paid > 0 {
                    let mut account = self
                        .get_account_with_fixed_root(&pubkey)
                        .unwrap_or_default();
                    let rent = self.rent_collector().rent;
                    let recipient_pre_rent_state = RentState::from_account(&account, &rent);
                    let distribution = account.checked_add_lamports(rent_to_be_paid);
                    let recipient_post_rent_state = RentState::from_account(&account, &rent);
                    let rent_state_transition_allowed = recipient_post_rent_state
                        .transition_allowed_from(&recipient_pre_rent_state);
                    if !rent_state_transition_allowed {
                        warn!(
                            "Rent distribution of {rent_to_be_paid} to {pubkey} results in \
                            invalid RentState: {recipient_post_rent_state:?}"
                        );
                        inc_new_counter_warn!(
                            "rent-distribution-rent-paying",
                            rent_to_be_paid as usize
                        );
                    }
                    if distribution.is_err()
                        || (self.prevent_rent_paying_rent_recipients()
                            && !rent_state_transition_allowed)
                    {
                        // overflow adding lamports or resulting account is not rent-exempt
                        self.capitalization.fetch_sub(rent_to_be_paid, Relaxed);
                        error!(
                            "Burned {} rent lamports instead of sending to {}",
                            rent_to_be_paid, pubkey
                        );
                        inc_new_counter_error!(
                            "bank-burned_rent_lamports",
                            rent_to_be_paid as usize
                        );
                    } else {
                        self.store_account(&pubkey, &account);
                        rewards.push((
                            pubkey,
                            RewardInfo {
                                reward_type: RewardType::Rent,
                                lamports: rent_to_be_paid as i64,
                                post_balance: account.lamports(),
                                commission: None,
                            },
                        ));
                    }
                }
            });
        self.rewards.write().unwrap().append(&mut rewards);

        if enforce_fix {
            assert_eq!(leftover_lamports, 0);
        } else if leftover_lamports != 0 {
            warn!(
                "There was leftover from rent distribution: {}",
                leftover_lamports
            );
            self.capitalization.fetch_sub(leftover_lamports, Relaxed);
        }
    }

    fn distribute_rent(&self) {
        let total_rent_collected = self.collected_rent.load(Relaxed);

        let (burned_portion, rent_to_be_distributed) = self
            .rent_collector
            .rent
            .calculate_burn(total_rent_collected);

        debug!(
            "distributed rent: {} (rounded from: {}, burned: {})",
            rent_to_be_distributed, total_rent_collected, burned_portion
        );
        self.capitalization.fetch_sub(burned_portion, Relaxed);

        if rent_to_be_distributed == 0 {
            return;
        }

        self.distribute_rent_to_validators(&self.vote_accounts(), rent_to_be_distributed);
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

    /// return all end partition indexes for the given partition
    /// partition could be (0, 1, N). In this case we only return [1]
    ///  the single 'end_index' that covers this partition.
    /// partition could be (0, 2, N). In this case, we return [1, 2], which are all
    /// the 'end_index' values contained in that range.
    /// (0, 0, N) returns [0] as a special case.
    /// There is a relationship between
    /// 1. 'pubkey_range_from_partition'
    /// 2. 'partition_from_pubkey'
    /// 3. this function
    fn get_partition_end_indexes(partition: &Partition) -> Vec<PartitionIndex> {
        if partition.0 == partition.1 && partition.0 == 0 {
            // special case for start=end=0. ie. (0, 0, N). This returns [0]
            vec![0]
        } else {
            // normal case of (start, end, N)
            // so, we want [start+1, start+2, ..=end]
            // if start == end, then return []
            (partition.0..partition.1).map(|index| index + 1).collect()
        }
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
                .map(|partition| (*partition, Self::pubkey_range_from_partition(*partition)))
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
                            // inc counter instead of assert while we verify this is correct
                            inc_new_counter_info!("unexpected-rent-paying-pubkey", 1);
                            warn!(
                                "Collecting rent from unexpected pubkey: {}, slot: {}, parent_slot: {:?}, partition_index: {}, partition_from_pubkey: {}",
                                pubkey,
                                self.slot(),
                                self.parent().map(|bank| bank.slot()),
                                partition_index,
                                Bank::partition_from_pubkey(pubkey, self.epoch_schedule.slots_per_epoch),
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
    fn include_slot_in_hash(&self) -> IncludeSlotInHash {
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
        let subrange_full = Self::pubkey_range_from_partition(partition);
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
                    Self::get_partition_end_indexes(partition)
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
            let num_threads = crate::accounts_db::quarter_thread_count() as u64;
            let sz = std::mem::size_of::<u64>();
            let start_prefix = Self::prefix_from_pubkey(subrange_full.start());
            let end_prefix_inclusive = Self::prefix_from_pubkey(subrange_full.end());
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

    fn prefix_from_pubkey(pubkey: &Pubkey) -> u64 {
        const PREFIX_SIZE: usize = mem::size_of::<u64>();
        u64::from_be_bytes(pubkey.as_ref()[0..PREFIX_SIZE].try_into().unwrap())
    }

    /// This is the inverse of pubkey_range_from_partition.
    /// return the lowest end_index which would contain this pubkey
    pub fn partition_from_pubkey(
        pubkey: &Pubkey,
        partition_count: PartitionsPerCycle,
    ) -> PartitionIndex {
        type Prefix = u64;
        const PREFIX_MAX: Prefix = Prefix::max_value();

        if partition_count == 1 {
            return 0;
        }

        // not-overflowing way of `(Prefix::max_value() + 1) / partition_count`
        let partition_width = (PREFIX_MAX - partition_count + 1) / partition_count + 1;

        let prefix = Self::prefix_from_pubkey(pubkey);
        if prefix == 0 {
            return 0;
        }

        if prefix == PREFIX_MAX {
            return partition_count - 1;
        }

        let mut result = (prefix + 1) / partition_width;
        if (prefix + 1) % partition_width == 0 {
            // adjust for integer divide
            result = result.saturating_sub(1);
        }
        result
    }

    // Mostly, the pair (start_index & end_index) is equivalent to this range:
    // start_index..=end_index. But it has some exceptional cases, including
    // this important and valid one:
    //   0..=0: the first partition in the new epoch when crossing epochs
    pub fn pubkey_range_from_partition(
        (start_index, end_index, partition_count): Partition,
    ) -> RangeInclusive<Pubkey> {
        assert!(start_index <= end_index);
        assert!(start_index < partition_count);
        assert!(end_index < partition_count);
        assert!(0 < partition_count);

        type Prefix = u64;
        const PREFIX_SIZE: usize = mem::size_of::<Prefix>();
        const PREFIX_MAX: Prefix = Prefix::max_value();

        let mut start_pubkey = [0x00u8; 32];
        let mut end_pubkey = [0xffu8; 32];

        if partition_count == 1 {
            assert_eq!(start_index, 0);
            assert_eq!(end_index, 0);
            return Pubkey::new_from_array(start_pubkey)..=Pubkey::new_from_array(end_pubkey);
        }

        // not-overflowing way of `(Prefix::max_value() + 1) / partition_count`
        let partition_width = (PREFIX_MAX - partition_count + 1) / partition_count + 1;
        let mut start_key_prefix = if start_index == 0 && end_index == 0 {
            0
        } else if start_index + 1 == partition_count {
            PREFIX_MAX
        } else {
            (start_index + 1) * partition_width
        };

        let mut end_key_prefix = if end_index + 1 == partition_count {
            PREFIX_MAX
        } else {
            (end_index + 1) * partition_width - 1
        };

        if start_index != 0 && start_index == end_index {
            // n..=n (n != 0): a noop pair across epochs without a gap under
            // multi_epoch_cycle, just nullify it.
            if end_key_prefix == PREFIX_MAX {
                start_key_prefix = end_key_prefix;
                start_pubkey = end_pubkey;
            } else {
                end_key_prefix = start_key_prefix;
                end_pubkey = start_pubkey;
            }
        }

        start_pubkey[0..PREFIX_SIZE].copy_from_slice(&start_key_prefix.to_be_bytes());
        end_pubkey[0..PREFIX_SIZE].copy_from_slice(&end_key_prefix.to_be_bytes());
        let start_pubkey_final = Pubkey::new_from_array(start_pubkey);
        let end_pubkey_final = Pubkey::new_from_array(end_pubkey);
        trace!(
            "pubkey_range_from_partition: ({}-{})/{} [{}]: {}-{}",
            start_index,
            end_index,
            partition_count,
            (end_key_prefix - start_key_prefix),
            start_pubkey.iter().map(|x| format!("{x:02x}")).join(""),
            end_pubkey.iter().map(|x| format!("{x:02x}")).join(""),
        );
        #[cfg(test)]
        if start_index != end_index {
            assert_eq!(
                if start_index == 0 && end_index == 0 {
                    0
                } else {
                    start_index + 1
                },
                Self::partition_from_pubkey(&start_pubkey_final, partition_count),
                "{start_index}, {end_index}, start_key_prefix: {start_key_prefix}, {start_pubkey_final}, {partition_count}"
            );
            assert_eq!(
                end_index,
                Self::partition_from_pubkey(&end_pubkey_final, partition_count),
                "{start_index}, {end_index}, {end_pubkey_final}, {partition_count}"
            );
            if start_index != 0 {
                start_pubkey[0..PREFIX_SIZE]
                    .copy_from_slice(&start_key_prefix.saturating_sub(1).to_be_bytes());
                let pubkey_test = Pubkey::new_from_array(start_pubkey);
                assert_eq!(
                    start_index,
                    Self::partition_from_pubkey(&pubkey_test, partition_count),
                    "{}, {}, start_key_prefix-1: {}, {}, {}",
                    start_index,
                    end_index,
                    start_key_prefix.saturating_sub(1),
                    pubkey_test,
                    partition_count
                );
            }
            if end_index != partition_count - 1 && end_index != 0 {
                end_pubkey[0..PREFIX_SIZE]
                    .copy_from_slice(&end_key_prefix.saturating_add(1).to_be_bytes());
                let pubkey_test = Pubkey::new_from_array(end_pubkey);
                assert_eq!(
                    end_index.saturating_add(1),
                    Self::partition_from_pubkey(&pubkey_test, partition_count),
                    "start: {}, end: {}, pubkey: {}, partition_count: {}, prefix_before_addition: {}, prefix after: {}",
                    start_index,
                    end_index,
                    pubkey_test,
                    partition_count,
                    end_key_prefix,
                    end_key_prefix.saturating_add(1),
                );
            }
        }
        // should be an inclusive range (a closed interval) like this:
        // [0xgg00-0xhhff], [0xii00-0xjjff], ... (where 0xii00 == 0xhhff + 1)
        start_pubkey_final..=end_pubkey_final
    }

    pub fn get_partitions(
        slot: Slot,
        parent_slot: Slot,
        slot_count_in_two_day: SlotCount,
    ) -> Vec<Partition> {
        let parent_cycle = parent_slot / slot_count_in_two_day;
        let current_cycle = slot / slot_count_in_two_day;
        let mut parent_cycle_index = parent_slot % slot_count_in_two_day;
        let current_cycle_index = slot % slot_count_in_two_day;
        let mut partitions = vec![];
        if parent_cycle < current_cycle {
            if current_cycle_index > 0 {
                // generate and push gapped partitions because some slots are skipped
                let parent_last_cycle_index = slot_count_in_two_day - 1;

                // ... for parent cycle
                partitions.push((
                    parent_cycle_index,
                    parent_last_cycle_index,
                    slot_count_in_two_day,
                ));

                // ... for current cycle
                partitions.push((0, 0, slot_count_in_two_day));
            }
            parent_cycle_index = 0;
        }

        partitions.push((
            parent_cycle_index,
            current_cycle_index,
            slot_count_in_two_day,
        ));

        partitions
    }

    pub(crate) fn fixed_cycle_partitions_between_slots(
        &self,
        starting_slot: Slot,
        ending_slot: Slot,
    ) -> Vec<Partition> {
        let slot_count_in_two_day = self.slot_count_in_two_day();
        Self::get_partitions(ending_slot, starting_slot, slot_count_in_two_day)
    }

    fn fixed_cycle_partitions(&self) -> Vec<Partition> {
        self.fixed_cycle_partitions_between_slots(self.parent_slot(), self.slot())
    }

    /// used only by filler accounts in debug path
    /// previous means slot - 1, not parent
    pub fn variable_cycle_partition_from_previous_slot(
        epoch_schedule: &EpochSchedule,
        slot: Slot,
    ) -> Partition {
        // similar code to Bank::variable_cycle_partitions
        let (current_epoch, current_slot_index) = epoch_schedule.get_epoch_and_slot_index(slot);
        let (parent_epoch, mut parent_slot_index) =
            epoch_schedule.get_epoch_and_slot_index(slot.saturating_sub(1));
        let cycle_params = Self::rent_single_epoch_collection_cycle_params(
            current_epoch,
            epoch_schedule.get_slots_in_epoch(current_epoch),
        );

        if parent_epoch < current_epoch {
            parent_slot_index = 0;
        }

        let generated_for_gapped_epochs = false;
        Self::get_partition_from_slot_indexes(
            cycle_params,
            parent_slot_index,
            current_slot_index,
            generated_for_gapped_epochs,
        )
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
        let cycle_params = self.determine_collection_cycle_params(epoch);
        Self::get_partition_from_slot_indexes(
            cycle_params,
            start_slot_index,
            end_slot_index,
            generated_for_gapped_epochs,
        )
    }

    fn get_partition_from_slot_indexes(
        cycle_params: RentCollectionCycleParams,
        start_slot_index: SlotIndex,
        end_slot_index: SlotIndex,
        generated_for_gapped_epochs: bool,
    ) -> Partition {
        let (_, _, in_multi_epoch_cycle, _, _, partition_count) = cycle_params;

        // use common codepath for both very likely and very unlikely for the sake of minimized
        // risk of any miscalculation instead of negligibly faster computation per slot for the
        // likely case.
        let mut start_partition_index =
            Self::partition_index_from_slot_index(start_slot_index, cycle_params);
        let mut end_partition_index =
            Self::partition_index_from_slot_index(end_slot_index, cycle_params);

        // Adjust partition index for some edge cases
        let is_special_new_epoch = start_slot_index == 0 && end_slot_index != 1;
        let in_middle_of_cycle = start_partition_index > 0;
        if in_multi_epoch_cycle && is_special_new_epoch && in_middle_of_cycle {
            // Adjust slot indexes so that the final partition ranges are continuous!
            // This is need because the caller gives us off-by-one indexes when
            // an epoch boundary is crossed.
            // Usually there is no need for this adjustment because cycles are aligned
            // with epochs. But for multi-epoch cycles, adjust the indexes if it
            // happens in the middle of a cycle for both gapped and not-gapped cases:
            //
            // epoch (slot range)|slot idx.*1|raw part. idx.|adj. part. idx.|epoch boundary
            // ------------------+-----------+--------------+---------------+--------------
            // 3 (20..30)        | [7..8]    |   7.. 8      |   7.. 8
            //                   | [8..9]    |   8.. 9      |   8.. 9
            // 4 (30..40)        | [0..0]    |<10>..10      | <9>..10      <--- not gapped
            //                   | [0..1]    |  10..11      |  10..12
            //                   | [1..2]    |  11..12      |  11..12
            //                   | [2..9   *2|  12..19      |  12..19      <-+
            // 5 (40..50)        |  0..0   *2|<20>..<20>    |<19>..<19> *3 <-+- gapped
            //                   |  0..4]    |<20>..24      |<19>..24      <-+
            //                   | [4..5]    |  24..25      |  24..25
            //                   | [5..6]    |  25..26      |  25..26
            //
            // NOTE: <..> means the adjusted slots
            //
            // *1: The range of parent_bank.slot() and current_bank.slot() is firstly
            //     split by the epoch boundaries and then the split ones are given to us.
            //     The original ranges are denoted as [...]
            // *2: These are marked with generated_for_gapped_epochs = true.
            // *3: This becomes no-op partition
            start_partition_index -= 1;
            if generated_for_gapped_epochs {
                assert_eq!(start_slot_index, end_slot_index);
                end_partition_index -= 1;
            }
        }

        (start_partition_index, end_partition_index, partition_count)
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

    fn rent_single_epoch_collection_cycle_params(
        epoch: Epoch,
        slot_count_per_epoch: SlotCount,
    ) -> RentCollectionCycleParams {
        (
            epoch,
            slot_count_per_epoch,
            false,
            0,
            1,
            slot_count_per_epoch,
        )
    }

    fn determine_collection_cycle_params(&self, epoch: Epoch) -> RentCollectionCycleParams {
        let slot_count_per_epoch = self.get_slots_in_epoch(epoch);

        if !self.use_multi_epoch_collection_cycle(epoch) {
            // mnb should always go through this code path
            Self::rent_single_epoch_collection_cycle_params(epoch, slot_count_per_epoch)
        } else {
            let epoch_count_in_cycle = self.slot_count_in_two_day() / slot_count_per_epoch;
            let partition_count = slot_count_per_epoch * epoch_count_in_cycle;

            (
                epoch,
                slot_count_per_epoch,
                true,
                self.first_normal_epoch(),
                epoch_count_in_cycle,
                partition_count,
            )
        }
    }

    fn partition_index_from_slot_index(
        slot_index_in_epoch: SlotIndex,
        (
            epoch,
            slot_count_per_epoch,
            _,
            base_epoch,
            epoch_count_per_cycle,
            _,
        ): RentCollectionCycleParams,
    ) -> PartitionIndex {
        let epoch_offset = epoch - base_epoch;
        let epoch_index_in_cycle = epoch_offset % epoch_count_per_cycle;
        slot_index_in_epoch + epoch_index_in_cycle * slot_count_per_epoch
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
        (0..accounts.len()).for_each(|i| {
            self.stakes_cache
                .check_and_store(accounts.pubkey(i), accounts.account(i))
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

    pub fn deposit(
        &self,
        pubkey: &Pubkey,
        lamports: u64,
    ) -> std::result::Result<u64, LamportsError> {
        // This doesn't collect rents intentionally.
        // Rents should only be applied to actual TXes
        let mut account = self.get_account_with_fixed_root(pubkey).unwrap_or_default();
        account.checked_add_lamports(lamports)?;
        self.store_account(pubkey, &account);
        Ok(account.lamports())
    }

    pub fn accounts(&self) -> Arc<Accounts> {
        self.rc.accounts.clone()
    }

    fn finish_init(
        &mut self,
        genesis_config: &GenesisConfig,
        additional_builtins: Option<&Builtins>,
        debug_do_not_add_builtins: bool,
    ) {
        self.rewards_pool_pubkeys =
            Arc::new(genesis_config.rewards_pools.keys().cloned().collect());

        let mut builtins = builtins::get();
        if let Some(additional_builtins) = additional_builtins {
            builtins
                .genesis_builtins
                .extend_from_slice(&additional_builtins.genesis_builtins);
            builtins
                .feature_transitions
                .extend_from_slice(&additional_builtins.feature_transitions);
        }
        if !debug_do_not_add_builtins {
            for builtin in builtins.genesis_builtins {
                self.add_builtin(
                    &builtin.name,
                    &builtin.id,
                    builtin.process_instruction_with_context,
                );
            }
            for precompile in get_precompiles() {
                if precompile.feature.is_none() {
                    self.add_precompile(&precompile.program_id);
                }
            }
        }
        self.builtin_feature_transitions = Arc::new(builtins.feature_transitions);

        self.apply_feature_activations(
            ApplyFeatureActivationsCaller::FinishInit,
            debug_do_not_add_builtins,
        );

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

    pub fn hard_forks(&self) -> Arc<RwLock<HardForks>> {
        self.hard_forks.clone()
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

    pub fn get_all_accounts_with_modified_slots(&self) -> ScanResult<Vec<PubkeyAccountSlot>> {
        self.rc.accounts.load_all(&self.ancestors, self.bank_id)
    }

    pub fn scan_all_accounts_with_modified_slots<F>(&self, scan_func: F) -> ScanResult<()>
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
        let accounts_delta_hash = self
            .rc
            .accounts
            .accounts_db
            .calculate_accounts_delta_hash(slot);

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

        if !epoch_accounts_hash::is_enabled_this_epoch(self) {
            return false;
        }

        let stop_slot = epoch_accounts_hash::calculation_stop(self);
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

    /// Recalculate the hash_internal_state from the account stores. Would be used to verify a
    /// snapshot.
    /// return true if all is good
    /// Only called from startup or test code.
    #[must_use]
    pub fn verify_bank_hash(&self, config: VerifyBankHash) -> bool {
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
                return parent.verify_bank_hash(config);
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
                        info!(
                            "running initial verification accounts hash calculation in background"
                        );
                        let result = accounts_.verify_bank_hash_and_lamports(
                            slot,
                            cap,
                            BankHashLamportsVerifyConfig {
                                ancestors: &ancestors,
                                test_hash_calculation: config.test_hash_calculation,
                                epoch_schedule: &epoch_schedule,
                                rent_collector: &rent_collector,
                                ignore_mismatch: config.ignore_mismatch,
                                store_detailed_debug_info: config.store_hash_raw_data_for_debug,
                                use_bg_thread_pool: true,
                            },
                        );
                        accounts_
                            .accounts_db
                            .verify_accounts_hash_in_bg
                            .background_finished();
                        result
                    })
                    .unwrap()
            });
            true // initial result is true. We haven't failed yet. If verification fails, we'll panic from bg thread.
        } else {
            let result = accounts.verify_bank_hash_and_lamports(
                slot,
                cap,
                BankHashLamportsVerifyConfig {
                    ancestors,
                    test_hash_calculation: config.test_hash_calculation,
                    epoch_schedule,
                    rent_collector,
                    ignore_mismatch: config.ignore_mismatch,
                    store_detailed_debug_info: config.store_hash_raw_data_for_debug,
                    use_bg_thread_pool: false, // fg is waiting for this to run, so we can use the fg thread pool
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
            .get_snapshot_storages(requested_slots, None)
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

            SanitizedTransaction::try_create(
                tx,
                message_hash,
                None,
                self,
                self.feature_set
                    .is_active(&feature_set::require_static_program_ids_in_transaction::ID),
            )
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
            .update_accounts_hash(
                // we have to use the index since the slot could be in the write cache still
                CalcAccountsHashDataSource::IndexForTests,
                debug_verify,
                self.slot(),
                &self.ancestors,
                None,
                self.epoch_schedule(),
                &self.rent_collector,
                is_startup,
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

    pub fn get_accounts_hash(&self) -> Option<AccountsHash> {
        self.rc.accounts.accounts_db.get_accounts_hash(self.slot())
    }

    pub fn get_snapshot_hash(&self) -> SnapshotHash {
        let accounts_hash = self
            .get_accounts_hash()
            .expect("accounts hash is required to get snapshot hash");
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
        let (accounts_hash, total_lamports) = self.rc.accounts.accounts_db.update_accounts_hash(
            data_source,
            debug_verify,
            self.slot(),
            &self.ancestors,
            Some(self.capitalization()),
            self.epoch_schedule(),
            &self.rent_collector,
            is_startup,
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
                self.rc.accounts.accounts_db.update_accounts_hash(
                    data_source,
                    debug_verify,
                    self.slot(),
                    &self.ancestors,
                    Some(self.capitalization()),
                    self.epoch_schedule(),
                    &self.rent_collector,
                    is_startup,
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

    /// A snapshot bank should be purged of 0 lamport accounts which are not part of the hash
    /// calculation and could shield other real accounts.
    pub fn verify_snapshot_bank(
        &self,
        test_hash_calculation: bool,
        accounts_db_skip_shrink: bool,
        last_full_snapshot_slot: Slot,
    ) -> bool {
        let mut clean_time = Measure::start("clean");
        if !accounts_db_skip_shrink && self.slot() > 0 {
            info!("cleaning..");
            self.rc
                .accounts
                .accounts_db
                .clean_accounts(None, true, Some(last_full_snapshot_slot));
        }
        clean_time.stop();

        let mut shrink_all_slots_time = Measure::start("shrink_all_slots");
        if !accounts_db_skip_shrink && self.slot() > 0 {
            info!("shrinking..");
            self.rc
                .accounts
                .accounts_db
                .shrink_all_slots(true, Some(last_full_snapshot_slot));
        }
        shrink_all_slots_time.stop();

        let (mut verify, verify_time_us) = if !self.rc.accounts.accounts_db.skip_initial_hash_calc {
            info!("verify_bank_hash..");
            let mut verify_time = Measure::start("verify_bank_hash");
            let verify = self.verify_bank_hash(VerifyBankHash {
                test_hash_calculation,
                ignore_mismatch: false,
                require_rooted_bank: false,
                run_in_background: true,
                store_hash_raw_data_for_debug: false,
            });
            verify_time.stop();
            (verify, verify_time.as_us())
        } else {
            self.rc
                .accounts
                .accounts_db
                .verify_accounts_hash_in_bg
                .verification_complete();
            (true, 0)
        };

        info!("verify_hash..");
        let mut verify2_time = Measure::start("verify_hash");
        // Order and short-circuiting is significant; verify_hash requires a valid bank hash
        verify = verify && self.verify_hash();
        verify2_time.stop();

        datapoint_info!(
            "verify_snapshot_bank",
            ("clean_us", clean_time.as_us(), i64),
            ("shrink_all_slots_us", shrink_all_slots_time.as_us(), i64),
            ("verify_bank_hash_us", verify_time_us, i64),
            ("verify_hash_us", verify2_time.as_us(), i64),
        );

        verify
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
        for (i, ((load_result, _load_nonce), tx)) in loaded_txs.iter().zip(txs).enumerate() {
            if let (Ok(loaded_transaction), true) = (
                load_result,
                execution_results[i].was_executed_successfully(),
            ) {
                // note that this could get timed to: self.rc.accounts.accounts_db.stats.stakes_cache_check_and_store_us,
                //  but this code path is captured separately in ExecuteTimingType::UpdateStakesCacheUs
                let message = tx.message();
                for (_i, (pubkey, account)) in
                    (0..message.account_keys().len()).zip(loaded_transaction.accounts.iter())
                {
                    self.stakes_cache.check_and_store(pubkey, account);
                }
            }
        }
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

    /// Add an instruction processor to intercept instructions before the dynamic loader.
    pub fn add_builtin(
        &mut self,
        name: &str,
        program_id: &Pubkey,
        process_instruction: ProcessInstructionWithContext,
    ) {
        debug!("Adding program {} under {:?}", name, program_id);
        self.add_builtin_account(name, program_id, false);
        if let Some(entry) = self
            .builtin_programs
            .vec
            .iter_mut()
            .find(|entry| entry.program_id == *program_id)
        {
            entry.process_instruction = process_instruction;
        } else {
            self.builtin_programs.vec.push(BuiltinProgram {
                program_id: *program_id,
                process_instruction,
            });
        }
        debug!("Added program {} under {:?}", name, program_id);
    }

    /// Remove a builtin instruction processor if it already exists
    pub fn remove_builtin(&mut self, program_id: &Pubkey) {
        debug!("Removing program {}", program_id);
        // Don't remove the account since the bank expects the account state to
        // be idempotent
        if let Some(position) = self
            .builtin_programs
            .vec
            .iter()
            .position(|entry| entry.program_id == *program_id)
        {
            self.builtin_programs.vec.remove(position);
        }
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
        );
    }

    pub fn print_accounts_stats(&self) {
        self.rc.accounts.accounts_db.print_accounts_stats("");
    }

    pub fn shrink_candidate_slots(&self) -> usize {
        self.rc.accounts.accounts_db.shrink_candidate_slots()
    }

    pub fn no_overflow_rent_distribution_enabled(&self) -> bool {
        self.feature_set
            .is_active(&feature_set::no_overflow_rent_distribution::id())
    }

    pub fn prevent_rent_paying_rent_recipients(&self) -> bool {
        self.feature_set
            .is_active(&feature_set::prevent_rent_paying_rent_recipients::id())
    }

    pub fn versioned_tx_message_enabled(&self) -> bool {
        self.feature_set
            .is_active(&feature_set::versioned_tx_message_enabled::id())
    }

    pub fn credits_auto_rewind(&self) -> bool {
        self.feature_set
            .is_active(&feature_set::credits_auto_rewind::id())
    }

    pub fn send_to_tpu_vote_port_enabled(&self) -> bool {
        self.feature_set
            .is_active(&feature_set::send_to_tpu_vote_port::id())
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
        // Do this check outside of the poh lock, hence not a method on PohRecorder
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
        let new_feature_activations = self.compute_active_feature_set(allow_new_activations);

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

        if new_feature_activations.contains(&feature_set::spl_token_v3_4_0::id()) {
            self.replace_program_account(
                &inline_spl_token::id(),
                &inline_spl_token::program_v3_4_0::id(),
                "bank-apply_spl_token_v3_4_0",
            );
        }

        if new_feature_activations.contains(&feature_set::spl_associated_token_account_v1_1_0::id())
        {
            self.replace_program_account(
                &inline_spl_associated_token_account::id(),
                &inline_spl_associated_token_account::program_v1_1_0::id(),
                "bank-apply_spl_associated_token_account_v1_1_0",
            );
        }

        if !debug_do_not_add_builtins {
            self.apply_builtin_program_feature_transitions(
                allow_new_activations,
                &new_feature_activations,
            );
            self.reconfigure_token2_native_mint();
        }

        if new_feature_activations.contains(&feature_set::cap_accounts_data_len::id()) {
            const ACCOUNTS_DATA_LEN: u64 = 50_000_000_000;
            self.accounts_data_size_initial = ACCOUNTS_DATA_LEN;
        }

        if new_feature_activations.contains(&feature_set::update_hashes_per_tick::id()) {
            self.apply_updated_hashes_per_tick(DEFAULT_HASHES_PER_TICK);
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

    // Compute the active feature set based on the current bank state, and return the set of newly activated features
    fn compute_active_feature_set(&mut self, allow_new_activations: bool) -> HashSet<Pubkey> {
        let mut active = self.feature_set.active.clone();
        let mut inactive = HashSet::new();
        let mut newly_activated = HashSet::new();
        let slot = self.slot();

        for feature_id in &self.feature_set.inactive {
            let mut activated = None;
            if let Some(mut account) = self.get_account_with_fixed_root(feature_id) {
                if let Some(mut feature) = feature::from_account(&account) {
                    match feature.activated_at {
                        None => {
                            if allow_new_activations {
                                // Feature has been requested, activate it now
                                feature.activated_at = Some(slot);
                                if feature::to_account(&feature, &mut account).is_some() {
                                    self.store_account(feature_id, &account);
                                }
                                newly_activated.insert(*feature_id);
                                activated = Some(slot);
                                info!("Feature {} activated at slot {}", feature_id, slot);
                            }
                        }
                        Some(activation_slot) => {
                            if slot >= activation_slot {
                                // Feature is already active
                                activated = Some(activation_slot);
                            }
                        }
                    }
                }
            }
            if let Some(slot) = activated {
                active.insert(*feature_id, slot);
            } else {
                inactive.insert(*feature_id);
            }
        }

        self.feature_set = Arc::new(FeatureSet { active, inactive });
        newly_activated
    }

    fn apply_builtin_program_feature_transitions(
        &mut self,
        only_apply_transitions_for_new_features: bool,
        new_feature_activations: &HashSet<Pubkey>,
    ) {
        let feature_set = self.feature_set.clone();
        let should_apply_action_for_feature_transition = |feature_id: &Pubkey| -> bool {
            if only_apply_transitions_for_new_features {
                new_feature_activations.contains(feature_id)
            } else {
                feature_set.is_active(feature_id)
            }
        };

        let builtin_feature_transitions = self.builtin_feature_transitions.clone();
        for transition in builtin_feature_transitions.iter() {
            if let Some(builtin_action) =
                transition.to_action(&should_apply_action_for_feature_transition)
            {
                match builtin_action {
                    BuiltinAction::Add(builtin) => self.add_builtin(
                        &builtin.name,
                        &builtin.id,
                        builtin.process_instruction_with_context,
                    ),
                    BuiltinAction::Remove(program_id) => self.remove_builtin(&program_id),
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

                self.remove_executor(old_address);

                self.calculate_and_update_accounts_data_size_delta_off_chain(
                    old_account.data().len(),
                    new_account.data().len(),
                );
            }
        }
    }

    fn reconfigure_token2_native_mint(&mut self) {
        let reconfigure_token2_native_mint = match self.cluster_type() {
            ClusterType::Development => true,
            ClusterType::Devnet => true,
            ClusterType::Testnet => self.epoch() == 93,
            ClusterType::MainnetBeta => self.epoch() == 75,
        };

        if reconfigure_token2_native_mint {
            let mut native_mint_account = solana_sdk::account::AccountSharedData::from(Account {
                owner: inline_spl_token::id(),
                data: inline_spl_token::native_mint::ACCOUNT_DATA.to_vec(),
                lamports: sol_to_lamports(1.),
                executable: false,
                rent_epoch: self.epoch() + 1,
            });

            // As a workaround for
            // https://github.com/solana-labs/solana-program-library/issues/374, ensure that the
            // spl-token 2 native mint account is owned by the spl-token 2 program.
            let old_account_data_size;
            let store = if let Some(existing_native_mint_account) =
                self.get_account_with_fixed_root(&inline_spl_token::native_mint::id())
            {
                old_account_data_size = existing_native_mint_account.data().len();
                if existing_native_mint_account.owner() == &solana_sdk::system_program::id() {
                    native_mint_account.set_lamports(existing_native_mint_account.lamports());
                    true
                } else {
                    false
                }
            } else {
                old_account_data_size = 0;
                self.capitalization
                    .fetch_add(native_mint_account.lamports(), Relaxed);
                true
            };

            if store {
                self.store_account(&inline_spl_token::native_mint::id(), &native_mint_account);
                self.calculate_and_update_accounts_data_size_delta_off_chain(
                    old_account_data_size,
                    native_mint_account.data().len(),
                );
            }
        }
    }

    /// Get all the accounts for this bank and calculate stats
    pub fn get_total_accounts_stats(&self) -> ScanResult<TotalAccountsStats> {
        let accounts = self.get_all_accounts_with_modified_slots()?;
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
            && epoch_accounts_hash::is_enabled_this_epoch(self)
            && epoch_accounts_hash::is_in_calculation_window(self);
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
        solana_sdk::{hash::hashv, pubkey::Pubkey},
        solana_vote_program::vote_state::{self, BlockTimestamp, VoteStateVersions},
    };
    pub fn goto_end_of_slot(bank: &mut Bank) {
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
}

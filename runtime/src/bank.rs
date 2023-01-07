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
        accounts::{
            AccountAddressFilter, Accounts, LoadedTransaction, PubkeyAccountSlot,
            TransactionLoadResult,
        },
        accounts_db::{
            AccountShrinkThreshold, AccountsDbConfig, CalcAccountsHashDataSource,
            IncludeSlotInHash, SnapshotStorages, ACCOUNTS_DB_CONFIG_FOR_BENCHMARKS,
            ACCOUNTS_DB_CONFIG_FOR_TESTING,
        },
        accounts_hash::AccountsHash,
        accounts_index::{AccountSecondaryIndexes, IndexKey, ScanConfig, ScanResult, ZeroLamport},
        accounts_update_notifier_interface::AccountsUpdateNotifier,
        ancestors::{Ancestors, AncestorsForSerialization},
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
    solana_measure::{measure, measure::Measure},
    solana_metrics::{inc_new_counter_debug, inc_new_counter_info},
    solana_perf::perf_libs,
    solana_program_runtime::{
        accounts_data_meter::MAX_ACCOUNTS_DATA_LEN,
        compute_budget::{self, ComputeBudget},
        executor::Executor,
        executor_cache::{
            BankExecutorCache, TransactionExecutorCache, TxBankExecutorCacheDiff,
            MAX_CACHED_EXECUTORS,
        },
        invoke_context::{BuiltinProgram, ProcessInstructionWithContext},
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
            BankId, Epoch, Slot, SlotCount, SlotIndex, UnixTimestamp, DEFAULT_TICKS_PER_SECOND,
            INITIAL_RENT_EPOCH, MAX_PROCESSING_AGE, MAX_TRANSACTION_FORWARDING_DELAY,
            MAX_TRANSACTION_FORWARDING_DELAY_GPU, SECONDS_PER_DAY,
        },
        ed25519_program,
        epoch_info::EpochInfo,
        epoch_schedule::EpochSchedule,
        feature,
        feature_set::{
            self, cap_transaction_accounts_data_size, disable_fee_calculator,
            enable_early_verification_of_account_modifications, remove_deprecated_request_unit_ix,
            use_default_units_in_fee_calculation, FeatureSet,
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
        native_token::sol_to_lamports,
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

#[derive(Debug, Default)]
struct RewardsMetrics {
    load_vote_and_stake_accounts_us: AtomicU64,
    calculate_points_us: AtomicU64,
    redeem_rewards_us: u64,
    store_stake_accounts_us: AtomicU64,
    store_vote_accounts_us: AtomicU64,
    invalid_cached_vote_accounts: usize,
    invalid_cached_stake_accounts: usize,
    invalid_cached_stake_accounts_rent_epoch: usize,
    vote_accounts_cache_miss_count: usize,
}

mod address_lookup_table;
mod builtin_programs;
mod sysvar_cache;
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
#[frozen_abi(digest = "3qia1Zm8X66bzFaBuC8ahz3hADRRATyUPRV36ZzrSois")]
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
    fn contains_multiple_slots(&self) -> bool {
        false
    }
    fn include_slot_in_hash(&self) -> IncludeSlotInHash {
        self.2
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

        let (rc, bank_rc_time) = measure!(
            BankRc {
                accounts: Arc::new(Accounts::new_from_parent(
                    &parent.rc.accounts,
                    slot,
                    parent.slot(),
                )),
                parent: RwLock::new(Some(parent.clone())),
                slot,
                bank_id_generator: parent.rc.bank_id_generator.clone(),
            },
            "bank_rc_creation",
        );

        let (status_cache, status_cache_time) =
            measure!(Arc::clone(&parent.status_cache), "status_cache_creation",);

        let ((fee_rate_governor, fee_calculator), fee_components_time) = measure!(
            {
                let fee_rate_governor = FeeRateGovernor::new_derived(
                    &parent.fee_rate_governor,
                    parent.signature_count(),
                );

                let fee_calculator = if parent.feature_set.is_active(&disable_fee_calculator::id())
                {
                    FeeCalculator::default()
                } else {
                    fee_rate_governor.create_fee_calculator()
                };
                (fee_rate_governor, fee_calculator)
            },
            "fee_components_creation",
        );

        let bank_id = rc.bank_id_generator.fetch_add(1, Relaxed) + 1;
        let (blockhash_queue, blockhash_queue_time) = measure!(
            RwLock::new(parent.blockhash_queue.read().unwrap().clone()),
            "blockhash_queue_creation",
        );

        let (stakes_cache, stakes_cache_time) = measure!(
            StakesCache::new(parent.stakes_cache.stakes().clone()),
            "stakes_cache_creation",
        );

        let (epoch_stakes, epoch_stakes_time) =
            measure!(parent.epoch_stakes.clone(), "epoch_stakes_creation");

        let (builtin_programs, builtin_programs_time) =
            measure!(parent.builtin_programs.clone(), "builtin_programs_creation");

        let (rewards_pool_pubkeys, rewards_pool_pubkeys_time) = measure!(
            parent.rewards_pool_pubkeys.clone(),
            "rewards_pool_pubkeys_creation",
        );

        let (executor_cache, executor_cache_time) = measure!(
            {
                let parent_bank_executors = parent.executor_cache.read().unwrap();
                RwLock::new(BankExecutorCache::new_from_parent_bank_executors(
                    &parent_bank_executors,
                    epoch,
                ))
            },
            "executor_cache_creation",
        );

        let (transaction_debug_keys, transaction_debug_keys_time) = measure!(
            parent.transaction_debug_keys.clone(),
            "transaction_debug_keys_creation",
        );

        let (transaction_log_collector_config, transaction_log_collector_config_time) = measure!(
            parent.transaction_log_collector_config.clone(),
            "transaction_log_collector_config_creation",
        );

        let (feature_set, feature_set_time) =
            measure!(parent.feature_set.clone(), "feature_set_creation");

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
        };

        let (_, ancestors_time) = measure!(
            {
                let mut ancestors = Vec::with_capacity(1 + new.parents().len());
                ancestors.push(new.slot());
                new.parents().iter().for_each(|p| {
                    ancestors.push(p.slot());
                });
                new.ancestors = Ancestors::from(ancestors);
            },
            "ancestors_creation",
        );

        // Following code may touch AccountsDb, requiring proper ancestors
        let parent_epoch = parent.epoch();
        let (_, update_epoch_time) = measure!(
            {
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

                    let mut metrics = RewardsMetrics::default();
                    // After saving a snapshot of stakes, apply stake rewards and commission
                    let (_, update_rewards_with_thread_pool_time) = measure!(
                        {
                            new.update_rewards_with_thread_pool(
                                parent_epoch,
                                reward_calc_tracer,
                                &thread_pool,
                                &mut metrics,
                            )
                        },
                        "update_rewards_with_thread_pool",
                    );

                    datapoint_info!(
                        "bank-new_from_parent-new_epoch_timings",
                        ("epoch", new.epoch(), i64),
                        ("slot", slot, i64),
                        ("parent_slot", parent.slot(), i64),
                        ("thread_pool_creation_us", thread_pool_time.as_us(), i64),
                        (
                            "apply_feature_activations",
                            apply_feature_activations_time.as_us(),
                            i64
                        ),
                        ("activate_epoch_Us", activate_epoch_time.as_us(), i64),
                        (
                            "update_epoch_stakes_us",
                            update_epoch_stakes_time.as_us(),
                            i64
                        ),
                        (
                            "update_rewards_with_thread_pool_us",
                            update_rewards_with_thread_pool_time.as_us(),
                            i64
                        ),
                        (
                            "load_vote_and_stake_accounts_us",
                            metrics.load_vote_and_stake_accounts_us.load(Relaxed),
                            i64
                        ),
                        (
                            "calculate_points_us",
                            metrics.calculate_points_us.load(Relaxed),
                            i64
                        ),
                        ("redeem_rewards_us", metrics.redeem_rewards_us, i64),
                        (
                            "store_stake_accounts_us",
                            metrics.store_stake_accounts_us.load(Relaxed),
                            i64
                        ),
                        (
                            "store_vote_accounts_us",
                            metrics.store_vote_accounts_us.load(Relaxed),
                            i64
                        ),
                        (
                            "invalid_cached_vote_accounts",
                            metrics.invalid_cached_vote_accounts,
                            i64
                        ),
                        (
                            "invalid_cached_stake_accounts",
                            metrics.invalid_cached_stake_accounts,
                            i64
                        ),
                        (
                            "invalid_cached_stake_accounts_rent_epoch",
                            metrics.invalid_cached_stake_accounts_rent_epoch,
                            i64
                        ),
                        (
                            "vote_accounts_cache_miss_count",
                            metrics.vote_accounts_cache_miss_count,
                            i64
                        ),
                    );
                } else {
                    // Save a snapshot of stakes for use in consensus and stake weighted networking
                    let leader_schedule_epoch = epoch_schedule.get_leader_schedule_epoch(slot);
                    new.update_epoch_stakes(leader_schedule_epoch);
                }
            },
            "update_epoch",
        );

        // Update sysvars before processing transactions
        let (_, update_sysvars_time) = measure!(
            {
                new.update_slot_hashes();
                new.update_stake_history(Some(parent_epoch));
                new.update_clock(Some(parent_epoch));
                new.update_fees();
            },
            "update_sysvars",
        );

        let (_, fill_sysvar_cache_time) =
            measure!(new.fill_missing_sysvar_cache_entries(), "fill_sysvar_cache");

        time.stop();

        datapoint_info!(
            "bank-new_from_parent-heights",
            ("slot", slot, i64),
            ("block_height", new.block_height, i64),
            ("parent_slot", parent.slot(), i64),
            ("bank_rc_creation_us", bank_rc_time.as_us(), i64),
            ("total_elapsed_us", time.as_us(), i64),
            ("status_cache_us", status_cache_time.as_us(), i64),
            ("fee_components_us", fee_components_time.as_us(), i64),
            ("blockhash_queue_us", blockhash_queue_time.as_us(), i64),
            ("stakes_cache_us", stakes_cache_time.as_us(), i64),
            ("epoch_stakes_time_us", epoch_stakes_time.as_us(), i64),
            ("builtin_programs_us", builtin_programs_time.as_us(), i64),
            (
                "rewards_pool_pubkeys_us",
                rewards_pool_pubkeys_time.as_us(),
                i64
            ),
            ("executor_cache_us", executor_cache_time.as_us(), i64),
            (
                "transaction_debug_keys_us",
                transaction_debug_keys_time.as_us(),
                i64
            ),
            (
                "transaction_log_collector_config_us",
                transaction_log_collector_config_time.as_us(),
                i64
            ),
            ("feature_set_us", feature_set_time.as_us(), i64),
            ("ancestors_us", ancestors_time.as_us(), i64),
            ("update_epoch_us", update_epoch_time.as_us(), i64),
            ("update_sysvars_us", update_sysvars_time.as_us(), i64),
            ("fill_sysvar_cache_us", fill_sysvar_cache_time.as_us(), i64),
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
        };
        bank.finish_init(
            genesis_config,
            additional_builtins,
            debug_do_not_add_builtins,
        );

        // Sanity assertions between bank snapshot and genesis config
        // Consider removing from serializable bank state
        // (BankFieldsToSerialize/BankFieldsToDeserialize) and initializing
        // from the passed in genesis_config instead (as new()/new_with_paths() already do)
        assert_eq!(
            bank.genesis_creation_time, genesis_config.creation_time,
            "Bank snapshot genesis creation time does not match genesis.bin creation time.\
             The snapshot and genesis.bin might pertain to different clusters"
        );
        assert_eq!(
            bank.hashes_per_tick,
            genesis_config.poh_config.hashes_per_tick
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

        let stake_delegations: Vec<_> = stakes.stake_delegations().iter().collect();
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

    fn load_vote_and_stake_accounts<F>(
        &self,
        thread_pool: &ThreadPool,
        reward_calc_tracer: Option<F>,
    ) -> LoadVoteAndStakeAccountsResult
    where
        F: Fn(&RewardCalculationEvent) + Send + Sync,
    {
        let stakes = self.stakes_cache.stakes();
        let stake_delegations: Vec<_> = stakes.stake_delegations().iter().collect();
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
                .is_active(&cap_transaction_accounts_data_size::id()),
            Self::get_loaded_accounts_data_limit_type(&self.feature_set),
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
                .is_active(&cap_transaction_accounts_data_size::id()),
            Self::get_loaded_accounts_data_limit_type(&self.feature_set),
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

    /// Add executors back to the bank's cache if they were missing and not updated
    fn store_missing_executors(&self, tx_executor_cache: &RefCell<TransactionExecutorCache>) {
        tx_executor_cache
            .borrow()
            .update_global_cache(&self.executor_cache, |difference| {
                difference == TxBankExecutorCacheDiff::Inserted
            });
    }

    /// Add updated executors back to the bank's cache
    fn store_updated_executors(&self, tx_executor_cache: &RefCell<TransactionExecutorCache>) {
        tx_executor_cache
            .borrow()
            .update_global_cache(&self.executor_cache, |difference| {
                difference == TxBankExecutorCacheDiff::Updated
            });
    }

    #[allow(dead_code)] // Preparation for BankExecutorCache rework
    fn create_executor(&self, pubkey: &Pubkey) -> Result<Arc<dyn Executor>> {
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
        solana_bpf_loader_program::create_executor_from_account(
            &self.feature_set,
            &self.runtime_config.compute_budget.unwrap_or_default(),
            None, // log_collector
            None, // tx_executor_cache
            &program,
            programdata.as_ref().unwrap_or(&program),
            self.runtime_config.bpf_jit,
        )
        .map(|(executor, _create_executor_metrics)| executor)
        .map_err(|err| TransactionError::InstructionError(0, err))
    }

    /// Remove an executor from the bank's cache
    fn remove_executor(&self, pubkey: &Pubkey) {
        let _ = self.executor_cache.write().unwrap().remove(pubkey);
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

        let mut store_missing_executors_time = Measure::start("store_missing_executors_time");
        self.store_missing_executors(&tx_executor_cache);
        store_missing_executors_time.stop();
        saturating_add_assign!(
            timings.execute_accessories.update_executors_us,
            store_missing_executors_time.as_us()
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
                                self.feature_set
                                    .is_active(&cap_transaction_accounts_data_size::id()),
                                Self::get_loaded_accounts_data_limit_type(&self.feature_set),
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
        // Mixed integer ops currently not stable, so copying the impl.
        // Copied from: https://github.com/a1phyr/rust/blob/47edde1086412b36e9efd6098b191ec15a2a760a/library/core/src/num/uint_macros.rs#L1039-L1048
        fn saturating_add_signed(lhs: u64, rhs: i64) -> u64 {
            let (res, overflow) = lhs.overflowing_add(rhs as u64);
            if overflow == (rhs < 0) {
                res
            } else if overflow {
                u64::MAX
            } else {
                u64::MIN
            }
        }
        saturating_add_signed(
            self.accounts_data_size_initial,
            self.load_accounts_data_size_delta(),
        )
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
        cap_transaction_accounts_data_size: bool,
        loaded_accounts_data_limit_type: compute_budget::LoadedAccountsDataLimitType,
    ) -> u64 {
        // Fee based on compute units and signatures
        const BASE_CONGESTION: f64 = 5_000.0;
        let current_congestion = BASE_CONGESTION.max(lamports_per_signature as f64);
        let congestion_multiplier = if lamports_per_signature == 0 {
            0.0 // test only
        } else {
            BASE_CONGESTION / current_congestion
        };

        let mut compute_budget = ComputeBudget::default();
        let prioritization_fee_details = compute_budget
            .process_instructions(
                message.program_instructions_iter(),
                use_default_units_per_instruction,
                support_request_units_deprecated,
                cap_transaction_accounts_data_size,
                loaded_accounts_data_limit_type,
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
                        .is_active(&cap_transaction_accounts_data_size::id()),
                    Self::get_loaded_accounts_data_limit_type(&self.feature_set),
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

        let tx_count = if self.bank_transaction_count_fix_enabled() {
            committed_transactions_count
        } else {
            committed_transactions_count.saturating_sub(committed_with_failure_result_count)
        };

        self.increment_transaction_count(tx_count);
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

        let mut store_updated_executors_time = Measure::start("store_updated_executors_time");
        for execution_result in &execution_results {
            if let TransactionExecutionResult::Executed {
                details,
                tx_executor_cache,
            } = execution_result
            {
                if details.status.is_ok() {
                    self.store_updated_executors(tx_executor_cache);
                }
            }
        }
        store_updated_executors_time.stop();
        saturating_add_assign!(
            timings.execute_accessories.update_executors_us,
            store_updated_executors_time.as_us()
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

        // Sort first by stake and then by validator identity pubkey for determinism
        validator_stakes.sort_by(|(pubkey1, staked1), (pubkey2, staked2)| {
            match staked2.cmp(staked1) {
                std::cmp::Ordering::Equal => pubkey2.cmp(pubkey1),
                other => other,
            }
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
                    if account.checked_add_lamports(rent_to_be_paid).is_err() {
                        // overflow adding lamports
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
        let can_skip_rewrites = false; // this will be goverened by a feature soon
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
        let mut executed_transaction_count = self
            .transaction_count()
            .saturating_sub(self.parent().map_or(0, |parent| parent.transaction_count()));
        if !self.bank_transaction_count_fix_enabled() {
            // When the feature bank_tranaction_count_fix is enabled, transaction_count() excludes
            // the transactions which were executed but landed in error, we add it here.
            executed_transaction_count =
                executed_transaction_count.saturating_add(self.transaction_error_count());
        }
        executed_transaction_count
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
        // If there are no accounts, return the hash of the previous state and the latest blockhash
        let bank_hash_info = self.rc.accounts.bank_hash_info_at(self.slot());
        let mut signature_count_buf = [0u8; 8];
        LittleEndian::write_u64(&mut signature_count_buf[..], self.signature_count());

        let mut hash = hashv(&[
            self.parent_hash.as_ref(),
            bank_hash_info.accounts_delta_hash.as_ref(),
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
            .get_hash_data(self.slot(), self.parent_slot());
        if let Some(buf) = buf {
            let hard_forked_hash = extend_and_hash(&hash, &buf);
            warn!(
                "hard fork at slot {} by hashing {:?}: {} => {}",
                self.slot(),
                buf,
                hash,
                hard_forked_hash
            );
            hash = hard_forked_hash;
        }

        info!(
            "bank frozen: {} hash: {} accounts_delta: {} signature_count: {} last_blockhash: {} capitalization: {}{}",
            self.slot(),
            hash,
            bank_hash_info.accounts_delta_hash,
            self.signature_count(),
            self.last_blockhash(),
            self.capitalization(),
            if let Some(epoch_accounts_hash) = epoch_accounts_hash {
                format!(", epoch_accounts_hash: {:?}", epoch_accounts_hash.as_ref())
            } else {
                "".to_string()
            }
        );

        info!(
            "accounts hash slot: {} stats: {:?}",
            self.slot(),
            bank_hash_info.stats,
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
                            &ancestors,
                            cap,
                            config.test_hash_calculation,
                            &epoch_schedule,
                            &rent_collector,
                            config.ignore_mismatch,
                            config.store_hash_raw_data_for_debug,
                            // true to run using bg thread pool
                            true,
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
                &self.ancestors,
                cap,
                config.test_hash_calculation,
                epoch_schedule,
                rent_collector,
                config.ignore_mismatch,
                config.store_hash_raw_data_for_debug,
                // fg is waiting for this to run, so we can use the fg thread pool
                false,
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

    /// Get this bank's storages to use for snapshots.
    ///
    /// If a base slot is provided, return only the storages that are *higher* than this slot.
    pub fn get_snapshot_storages(&self, base_slot: Option<Slot>) -> SnapshotStorages {
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

    /// only called from ledger-tool or tests
    fn calculate_capitalization(&self, debug_verify: bool) -> u64 {
        self.rc.accounts.calculate_capitalization(
            &self.ancestors,
            self.slot(),
            debug_verify,
            self.epoch_schedule(),
            &self.rent_collector,
            // we have to use the index since the slot could be in the write cache still
            CalcAccountsHashDataSource::IndexForTests,
        )
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

    pub fn get_accounts_hash(&self) -> AccountsHash {
        self.rc.accounts.accounts_db.get_accounts_hash(self.slot)
    }

    pub fn get_snapshot_hash(&self) -> SnapshotHash {
        let accounts_hash = self.get_accounts_hash();
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

    pub fn bank_transaction_count_fix_enabled(&self) -> bool {
        self.feature_set
            .is_active(&feature_set::bank_transaction_count_fix::id())
    }

    pub fn shrink_candidate_slots(&self) -> usize {
        self.rc.accounts.accounts_db.shrink_candidate_slots()
    }

    pub fn no_overflow_rent_distribution_enabled(&self) -> bool {
        self.feature_set
            .is_active(&feature_set::no_overflow_rent_distribution::id())
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

    /// if the `default` and/or `max` value for ComputeBudget:::Accounts_data_size_limit changes,
    /// the change needs to be gated by feature gate with corresponding new enum value.
    /// should use this function to get correct loaded_accounts_data_limit_type based on
    /// feature_set.
    pub fn get_loaded_accounts_data_limit_type(
        _feature_set: &FeatureSet,
    ) -> compute_budget::LoadedAccountsDataLimitType {
        compute_budget::LoadedAccountsDataLimitType::V0
        // In the future, use feature_set to determine correct LoadedAccountsDataLimitType here.
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
    use {super::Bank, solana_sdk::hash::hashv};
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
}

#[cfg(test)]
pub(crate) mod tests {
    #[allow(deprecated)]
    use solana_sdk::sysvar::fees::Fees;
    use {
        super::*,
        crate::{
            accounts_background_service::{PrunedBanksRequestHandler, SendDroppedBankCallback},
            accounts_db::DEFAULT_ACCOUNTS_SHRINK_RATIO,
            accounts_index::{AccountIndex, AccountSecondaryIndexes, ScanError, ITER_BATCH_SIZE},
            ancestors::Ancestors,
            bank_client::BankClient,
            genesis_utils::{
                self, activate_all_features, activate_feature, bootstrap_validator_stake_lamports,
                create_genesis_config_with_leader, create_genesis_config_with_vote_accounts,
                genesis_sysvar_and_builtin_program_lamports, GenesisConfigInfo,
                ValidatorVoteKeypairs,
            },
            rent_paying_accounts_by_partition::RentPayingAccountsByPartition,
            status_cache::MAX_CACHE_ENTRIES,
        },
        crossbeam_channel::{bounded, unbounded},
        rand::Rng,
        solana_program_runtime::{
            compute_budget::MAX_COMPUTE_UNIT_LIMIT,
            invoke_context::{mock_process_instruction, InvokeContext},
            prioritization_fee::{PrioritizationFeeDetails, PrioritizationFeeType},
        },
        solana_sdk::{
            account::Account,
            bpf_loader, bpf_loader_deprecated,
            bpf_loader_upgradeable::{self, UpgradeableLoaderState},
            client::SyncClient,
            clock::{DEFAULT_SLOTS_PER_EPOCH, DEFAULT_TICKS_PER_SLOT, MAX_RECENT_BLOCKHASHES},
            compute_budget::ComputeBudgetInstruction,
            entrypoint::MAX_PERMITTED_DATA_INCREASE,
            epoch_schedule::MINIMUM_SLOTS_PER_EPOCH,
            feature::Feature,
            genesis_config::create_genesis_config,
            hash,
            instruction::{AccountMeta, CompiledInstruction, Instruction, InstructionError},
            loader_upgradeable_instruction::UpgradeableLoaderInstruction,
            message::{Message, MessageHeader},
            native_token::LAMPORTS_PER_SOL,
            nonce,
            poh_config::PohConfig,
            program::MAX_RETURN_DATA,
            rent::Rent,
            signature::{keypair_from_seed, Keypair, Signer},
            stake::{
                instruction as stake_instruction,
                state::{Authorized, Delegation, Lockup, Stake},
            },
            system_instruction::{
                self, SystemError, MAX_PERMITTED_ACCOUNTS_DATA_ALLOCATIONS_PER_TRANSACTION,
                MAX_PERMITTED_DATA_LENGTH,
            },
            system_program,
            timing::duration_as_s,
            transaction_context::IndexOfAccount,
        },
        solana_vote_program::{
            vote_instruction,
            vote_state::{
                self, BlockTimestamp, Vote, VoteInit, VoteState, VoteStateVersions,
                MAX_LOCKOUT_HISTORY,
            },
        },
        std::{
            fs::File, io::Read, result, str::FromStr, sync::atomic::Ordering::Release,
            thread::Builder, time::Duration,
        },
        test_utils::goto_end_of_slot,
    };

    fn new_sanitized_message(
        instructions: &[Instruction],
        payer: Option<&Pubkey>,
    ) -> SanitizedMessage {
        Message::new(instructions, payer).try_into().unwrap()
    }

    fn new_execution_result(
        status: Result<()>,
        nonce: Option<&NonceFull>,
    ) -> TransactionExecutionResult {
        TransactionExecutionResult::Executed {
            details: TransactionExecutionDetails {
                status,
                log_messages: None,
                inner_instructions: None,
                durable_nonce_fee: nonce.map(DurableNonceFee::from),
                return_data: None,
                executed_units: 0,
                accounts_data_len_delta: 0,
            },
            tx_executor_cache: Rc::new(RefCell::new(TransactionExecutorCache::default())),
        }
    }

    impl Bank {
        fn clean_accounts_for_tests(&self) {
            self.rc.accounts.accounts_db.clean_accounts_for_tests()
        }
    }

    #[test]
    fn test_nonce_info() {
        let lamports_per_signature = 42;

        let nonce_authority = keypair_from_seed(&[0; 32]).unwrap();
        let nonce_address = nonce_authority.pubkey();
        let from = keypair_from_seed(&[1; 32]).unwrap();
        let from_address = from.pubkey();
        let to_address = Pubkey::new_unique();

        let durable_nonce = DurableNonce::from_blockhash(&Hash::new_unique());
        let nonce_account = AccountSharedData::new_data(
            43,
            &nonce::state::Versions::new(nonce::State::Initialized(nonce::state::Data::new(
                Pubkey::default(),
                durable_nonce,
                lamports_per_signature,
            ))),
            &system_program::id(),
        )
        .unwrap();
        let from_account = AccountSharedData::new(44, 0, &Pubkey::default());
        let to_account = AccountSharedData::new(45, 0, &Pubkey::default());
        let recent_blockhashes_sysvar_account = AccountSharedData::new(4, 0, &Pubkey::default());

        const TEST_RENT_DEBIT: u64 = 1;
        let rent_collected_nonce_account = {
            let mut account = nonce_account.clone();
            account.set_lamports(nonce_account.lamports() - TEST_RENT_DEBIT);
            account
        };
        let rent_collected_from_account = {
            let mut account = from_account.clone();
            account.set_lamports(from_account.lamports() - TEST_RENT_DEBIT);
            account
        };

        let instructions = vec![
            system_instruction::advance_nonce_account(&nonce_address, &nonce_authority.pubkey()),
            system_instruction::transfer(&from_address, &to_address, 42),
        ];

        // NoncePartial create + NonceInfo impl
        let partial = NoncePartial::new(nonce_address, rent_collected_nonce_account.clone());
        assert_eq!(*partial.address(), nonce_address);
        assert_eq!(*partial.account(), rent_collected_nonce_account);
        assert_eq!(
            partial.lamports_per_signature(),
            Some(lamports_per_signature)
        );
        assert_eq!(partial.fee_payer_account(), None);

        // Add rent debits to ensure the rollback captures accounts without rent fees
        let mut rent_debits = RentDebits::default();
        rent_debits.insert(
            &from_address,
            TEST_RENT_DEBIT,
            rent_collected_from_account.lamports(),
        );
        rent_debits.insert(
            &nonce_address,
            TEST_RENT_DEBIT,
            rent_collected_nonce_account.lamports(),
        );

        // NonceFull create + NonceInfo impl
        {
            let message = new_sanitized_message(&instructions, Some(&from_address));
            let accounts = [
                (
                    *message.account_keys().get(0).unwrap(),
                    rent_collected_from_account.clone(),
                ),
                (
                    *message.account_keys().get(1).unwrap(),
                    rent_collected_nonce_account.clone(),
                ),
                (*message.account_keys().get(2).unwrap(), to_account.clone()),
                (
                    *message.account_keys().get(3).unwrap(),
                    recent_blockhashes_sysvar_account.clone(),
                ),
            ];

            let full = NonceFull::from_partial(partial.clone(), &message, &accounts, &rent_debits)
                .unwrap();
            assert_eq!(*full.address(), nonce_address);
            assert_eq!(*full.account(), rent_collected_nonce_account);
            assert_eq!(full.lamports_per_signature(), Some(lamports_per_signature));
            assert_eq!(
                full.fee_payer_account(),
                Some(&from_account),
                "rent debit should be refunded in captured fee account"
            );
        }

        // Nonce account is fee-payer
        {
            let message = new_sanitized_message(&instructions, Some(&nonce_address));
            let accounts = [
                (
                    *message.account_keys().get(0).unwrap(),
                    rent_collected_nonce_account,
                ),
                (
                    *message.account_keys().get(1).unwrap(),
                    rent_collected_from_account,
                ),
                (*message.account_keys().get(2).unwrap(), to_account),
                (
                    *message.account_keys().get(3).unwrap(),
                    recent_blockhashes_sysvar_account,
                ),
            ];

            let full = NonceFull::from_partial(partial.clone(), &message, &accounts, &rent_debits)
                .unwrap();
            assert_eq!(*full.address(), nonce_address);
            assert_eq!(*full.account(), nonce_account);
            assert_eq!(full.lamports_per_signature(), Some(lamports_per_signature));
            assert_eq!(full.fee_payer_account(), None);
        }

        // NonceFull create, fee-payer not in account_keys fails
        {
            let message = new_sanitized_message(&instructions, Some(&nonce_address));
            assert_eq!(
                NonceFull::from_partial(partial, &message, &[], &RentDebits::default())
                    .unwrap_err(),
                TransactionError::AccountNotFound,
            );
        }
    }

    #[test]
    fn test_bank_unix_timestamp_from_genesis() {
        let (genesis_config, _mint_keypair) = create_genesis_config(1);
        let mut bank = Arc::new(Bank::new_for_tests(&genesis_config));

        assert_eq!(
            genesis_config.creation_time,
            bank.unix_timestamp_from_genesis()
        );
        let slots_per_sec = 1.0
            / (duration_as_s(&genesis_config.poh_config.target_tick_duration)
                * genesis_config.ticks_per_slot as f32);

        for _i in 0..slots_per_sec as usize + 1 {
            bank = Arc::new(new_from_parent(&bank));
        }

        assert!(bank.unix_timestamp_from_genesis() - genesis_config.creation_time >= 1);
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_bank_new() {
        let dummy_leader_pubkey = solana_sdk::pubkey::new_rand();
        let dummy_leader_stake_lamports = bootstrap_validator_stake_lamports();
        let mint_lamports = 10_000;
        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair,
            voting_keypair,
            ..
        } = create_genesis_config_with_leader(
            mint_lamports,
            &dummy_leader_pubkey,
            dummy_leader_stake_lamports,
        );

        genesis_config.rent = Rent {
            lamports_per_byte_year: 5,
            exemption_threshold: 1.2,
            burn_percent: 5,
        };

        let bank = Bank::new_for_tests(&genesis_config);
        assert_eq!(bank.get_balance(&mint_keypair.pubkey()), mint_lamports);
        assert_eq!(
            bank.get_balance(&voting_keypair.pubkey()),
            dummy_leader_stake_lamports /* 1 token goes to the vote account associated with dummy_leader_lamports */
        );

        let rent_account = bank.get_account(&sysvar::rent::id()).unwrap();
        let rent = from_account::<sysvar::rent::Rent, _>(&rent_account).unwrap();

        assert_eq!(rent.burn_percent, 5);
        assert_eq!(rent.exemption_threshold, 1.2);
        assert_eq!(rent.lamports_per_byte_year, 5);
    }

    fn create_simple_test_bank(lamports: u64) -> Bank {
        let (genesis_config, _mint_keypair) = create_genesis_config(lamports);
        Bank::new_for_tests(&genesis_config)
    }

    fn create_simple_test_arc_bank(lamports: u64) -> Arc<Bank> {
        Arc::new(create_simple_test_bank(lamports))
    }

    #[test]
    fn test_bank_block_height() {
        let bank0 = create_simple_test_arc_bank(1);
        assert_eq!(bank0.block_height(), 0);
        let bank1 = Arc::new(new_from_parent(&bank0));
        assert_eq!(bank1.block_height(), 1);
    }

    #[test]
    fn test_bank_update_epoch_stakes() {
        impl Bank {
            fn epoch_stake_keys(&self) -> Vec<Epoch> {
                let mut keys: Vec<Epoch> = self.epoch_stakes.keys().copied().collect();
                keys.sort_unstable();
                keys
            }

            fn epoch_stake_key_info(&self) -> (Epoch, Epoch, usize) {
                let mut keys: Vec<Epoch> = self.epoch_stakes.keys().copied().collect();
                keys.sort_unstable();
                (*keys.first().unwrap(), *keys.last().unwrap(), keys.len())
            }
        }

        let mut bank = create_simple_test_bank(100_000);

        let initial_epochs = bank.epoch_stake_keys();
        assert_eq!(initial_epochs, vec![0, 1]);

        for existing_epoch in &initial_epochs {
            bank.update_epoch_stakes(*existing_epoch);
            assert_eq!(bank.epoch_stake_keys(), initial_epochs);
        }

        for epoch in (initial_epochs.len() as Epoch)..MAX_LEADER_SCHEDULE_STAKES {
            bank.update_epoch_stakes(epoch);
            assert_eq!(bank.epoch_stakes.len() as Epoch, epoch + 1);
        }

        assert_eq!(
            bank.epoch_stake_key_info(),
            (
                0,
                MAX_LEADER_SCHEDULE_STAKES - 1,
                MAX_LEADER_SCHEDULE_STAKES as usize
            )
        );

        bank.update_epoch_stakes(MAX_LEADER_SCHEDULE_STAKES);
        assert_eq!(
            bank.epoch_stake_key_info(),
            (
                0,
                MAX_LEADER_SCHEDULE_STAKES,
                MAX_LEADER_SCHEDULE_STAKES as usize + 1
            )
        );

        bank.update_epoch_stakes(MAX_LEADER_SCHEDULE_STAKES + 1);
        assert_eq!(
            bank.epoch_stake_key_info(),
            (
                1,
                MAX_LEADER_SCHEDULE_STAKES + 1,
                MAX_LEADER_SCHEDULE_STAKES as usize + 1
            )
        );
    }

    fn bank0_sysvar_delta() -> u64 {
        const SLOT_HISTORY_SYSVAR_MIN_BALANCE: u64 = 913_326_000;
        SLOT_HISTORY_SYSVAR_MIN_BALANCE
    }

    fn bank1_sysvar_delta() -> u64 {
        const SLOT_HASHES_SYSVAR_MIN_BALANCE: u64 = 143_487_360;
        SLOT_HASHES_SYSVAR_MIN_BALANCE
    }

    #[test]
    fn test_bank_capitalization() {
        let bank0 = Arc::new(Bank::new_for_tests(&GenesisConfig {
            accounts: (0..42)
                .map(|_| {
                    (
                        solana_sdk::pubkey::new_rand(),
                        Account::new(42, 0, &Pubkey::default()),
                    )
                })
                .collect(),
            cluster_type: ClusterType::MainnetBeta,
            ..GenesisConfig::default()
        }));

        assert_eq!(
            bank0.capitalization(),
            42 * 42 + genesis_sysvar_and_builtin_program_lamports(),
        );

        bank0.freeze();

        assert_eq!(
            bank0.capitalization(),
            42 * 42 + genesis_sysvar_and_builtin_program_lamports() + bank0_sysvar_delta(),
        );

        let bank1 = Bank::new_from_parent(&bank0, &Pubkey::default(), 1);
        assert_eq!(
            bank1.capitalization(),
            42 * 42
                + genesis_sysvar_and_builtin_program_lamports()
                + bank0_sysvar_delta()
                + bank1_sysvar_delta(),
        );
    }

    fn rent_with_exemption_threshold(exemption_threshold: f64) -> Rent {
        Rent {
            lamports_per_byte_year: 1,
            exemption_threshold,
            burn_percent: 10,
        }
    }

    #[test]
    /// one thing being tested here is that a failed tx (due to rent collection using up all lamports) followed by rent collection
    /// results in the same state as if just rent collection ran (and emptied the accounts that have too few lamports)
    fn test_credit_debit_rent_no_side_effect_on_hash() {
        for set_exempt_rent_epoch_max in [false, true] {
            solana_logger::setup();

            let (mut genesis_config, _mint_keypair) = create_genesis_config(10);

            genesis_config.rent = rent_with_exemption_threshold(21.0);

            let slot = years_as_slots(
                2.0,
                &genesis_config.poh_config.target_tick_duration,
                genesis_config.ticks_per_slot,
            ) as u64;
            let root_bank = Arc::new(Bank::new_for_tests(&genesis_config));
            let bank = Bank::new_from_parent(&root_bank, &Pubkey::default(), slot);

            let root_bank_2 = Arc::new(Bank::new_for_tests(&genesis_config));
            let bank_with_success_txs =
                Bank::new_from_parent(&root_bank_2, &Pubkey::default(), slot);

            assert_eq!(bank.last_blockhash(), genesis_config.hash());

            let plenty_of_lamports = 264;
            let too_few_lamports = 10;
            // Initialize credit-debit and credit only accounts
            let accounts = [
                AccountSharedData::new(plenty_of_lamports, 0, &Pubkey::default()),
                AccountSharedData::new(plenty_of_lamports, 1, &Pubkey::default()),
                AccountSharedData::new(plenty_of_lamports, 0, &Pubkey::default()),
                AccountSharedData::new(plenty_of_lamports, 1, &Pubkey::default()),
                // Transaction between these two accounts will fail
                AccountSharedData::new(too_few_lamports, 0, &Pubkey::default()),
                AccountSharedData::new(too_few_lamports, 1, &Pubkey::default()),
            ];

            let keypairs = accounts.iter().map(|_| Keypair::new()).collect::<Vec<_>>();
            {
                // make sure rent and epoch change are such that we collect all lamports in accounts 4 & 5
                let mut account_copy = accounts[4].clone();
                let expected_rent = bank.rent_collector().collect_from_existing_account(
                    &keypairs[4].pubkey(),
                    &mut account_copy,
                    None,
                    set_exempt_rent_epoch_max,
                );
                assert_eq!(expected_rent.rent_amount, too_few_lamports);
                assert_eq!(account_copy.lamports(), 0);
            }

            for i in 0..accounts.len() {
                let account = &accounts[i];
                bank.store_account(&keypairs[i].pubkey(), account);
                bank_with_success_txs.store_account(&keypairs[i].pubkey(), account);
            }

            // Make builtin instruction loader rent exempt
            let system_program_id = system_program::id();
            let mut system_program_account = bank.get_account(&system_program_id).unwrap();
            system_program_account.set_lamports(
                bank.get_minimum_balance_for_rent_exemption(system_program_account.data().len()),
            );
            bank.store_account(&system_program_id, &system_program_account);
            bank_with_success_txs.store_account(&system_program_id, &system_program_account);

            let t1 = system_transaction::transfer(
                &keypairs[0],
                &keypairs[1].pubkey(),
                1,
                genesis_config.hash(),
            );
            let t2 = system_transaction::transfer(
                &keypairs[2],
                &keypairs[3].pubkey(),
                1,
                genesis_config.hash(),
            );
            // the idea is this transaction will result in both accounts being drained of all lamports due to rent collection
            let t3 = system_transaction::transfer(
                &keypairs[4],
                &keypairs[5].pubkey(),
                1,
                genesis_config.hash(),
            );

            let txs = vec![t1.clone(), t2.clone(), t3];
            let res = bank.process_transactions(txs.iter());

            assert_eq!(res.len(), 3);
            assert_eq!(res[0], Ok(()));
            assert_eq!(res[1], Ok(()));
            assert_eq!(res[2], Err(TransactionError::AccountNotFound));

            bank.freeze();

            let rwlockguard_bank_hash = bank.hash.read().unwrap();
            let bank_hash = rwlockguard_bank_hash.as_ref();

            let txs = vec![t2, t1];
            let res = bank_with_success_txs.process_transactions(txs.iter());

            assert_eq!(res.len(), 2);
            assert_eq!(res[0], Ok(()));
            assert_eq!(res[1], Ok(()));

            bank_with_success_txs.freeze();

            let rwlockguard_bank_with_success_txs_hash = bank_with_success_txs.hash.read().unwrap();
            let bank_with_success_txs_hash = rwlockguard_bank_with_success_txs_hash.as_ref();

            assert_eq!(bank_with_success_txs_hash, bank_hash);
        }
    }

    fn store_accounts_for_rent_test(
        bank: &Bank,
        keypairs: &mut [Keypair],
        mock_program_id: Pubkey,
        generic_rent_due_for_system_account: u64,
    ) {
        let mut account_pairs: Vec<TransactionAccount> = Vec::with_capacity(keypairs.len() - 1);
        account_pairs.push((
            keypairs[0].pubkey(),
            AccountSharedData::new(
                generic_rent_due_for_system_account + 2,
                0,
                &Pubkey::default(),
            ),
        ));
        account_pairs.push((
            keypairs[1].pubkey(),
            AccountSharedData::new(
                generic_rent_due_for_system_account + 2,
                0,
                &Pubkey::default(),
            ),
        ));
        account_pairs.push((
            keypairs[2].pubkey(),
            AccountSharedData::new(
                generic_rent_due_for_system_account + 2,
                0,
                &Pubkey::default(),
            ),
        ));
        account_pairs.push((
            keypairs[3].pubkey(),
            AccountSharedData::new(
                generic_rent_due_for_system_account + 2,
                0,
                &Pubkey::default(),
            ),
        ));
        account_pairs.push((
            keypairs[4].pubkey(),
            AccountSharedData::new(10, 0, &Pubkey::default()),
        ));
        account_pairs.push((
            keypairs[5].pubkey(),
            AccountSharedData::new(10, 0, &Pubkey::default()),
        ));
        account_pairs.push((
            keypairs[6].pubkey(),
            AccountSharedData::new(
                (2 * generic_rent_due_for_system_account) + 24,
                0,
                &Pubkey::default(),
            ),
        ));

        account_pairs.push((
            keypairs[8].pubkey(),
            AccountSharedData::new(
                generic_rent_due_for_system_account + 2 + 929,
                0,
                &Pubkey::default(),
            ),
        ));
        account_pairs.push((
            keypairs[9].pubkey(),
            AccountSharedData::new(10, 0, &Pubkey::default()),
        ));

        // Feeding to MockProgram to test read only rent behaviour
        account_pairs.push((
            keypairs[10].pubkey(),
            AccountSharedData::new(
                generic_rent_due_for_system_account + 3,
                0,
                &Pubkey::default(),
            ),
        ));
        account_pairs.push((
            keypairs[11].pubkey(),
            AccountSharedData::new(generic_rent_due_for_system_account + 3, 0, &mock_program_id),
        ));
        account_pairs.push((
            keypairs[12].pubkey(),
            AccountSharedData::new(generic_rent_due_for_system_account + 3, 0, &mock_program_id),
        ));
        account_pairs.push((
            keypairs[13].pubkey(),
            AccountSharedData::new(14, 22, &mock_program_id),
        ));

        for account_pair in account_pairs.iter() {
            bank.store_account(&account_pair.0, &account_pair.1);
        }
    }

    fn create_child_bank_for_rent_test(
        root_bank: &Arc<Bank>,
        genesis_config: &GenesisConfig,
    ) -> Bank {
        let mut bank = Bank::new_from_parent(
            root_bank,
            &Pubkey::default(),
            years_as_slots(
                2.0,
                &genesis_config.poh_config.target_tick_duration,
                genesis_config.ticks_per_slot,
            ) as u64,
        );
        bank.rent_collector.slots_per_year = 421_812.0;
        bank
    }

    /// if asserter returns true, check the capitalization
    /// Checking the capitalization requires that the bank be a root and the slot be flushed.
    /// All tests are getting converted to use the write cache, so over time, each caller will be visited to throttle this input.
    /// Flushing the cache has a side effects on the test, so often the test has to be restarted to continue to function correctly.
    fn assert_capitalization_diff(
        bank: &Bank,
        updater: impl Fn(),
        asserter: impl Fn(u64, u64) -> bool,
    ) {
        let old = bank.capitalization();
        updater();
        let new = bank.capitalization();
        if asserter(old, new) {
            add_root_and_flush_write_cache(bank);
            assert_eq!(bank.capitalization(), bank.calculate_capitalization(true));
        }
    }

    #[test]
    fn test_store_account_and_update_capitalization_missing() {
        let bank = create_simple_test_bank(0);
        let pubkey = solana_sdk::pubkey::new_rand();

        let some_lamports = 400;
        let account = AccountSharedData::new(some_lamports, 0, &system_program::id());

        assert_capitalization_diff(
            &bank,
            || bank.store_account_and_update_capitalization(&pubkey, &account),
            |old, new| {
                assert_eq!(old + some_lamports, new);
                true
            },
        );
        assert_eq!(account, bank.get_account(&pubkey).unwrap());
    }

    #[test]
    fn test_store_account_and_update_capitalization_increased() {
        let old_lamports = 400;
        let (genesis_config, mint_keypair) = create_genesis_config(old_lamports);
        let bank = Bank::new_for_tests(&genesis_config);
        let pubkey = mint_keypair.pubkey();

        let new_lamports = 500;
        let account = AccountSharedData::new(new_lamports, 0, &system_program::id());

        assert_capitalization_diff(
            &bank,
            || bank.store_account_and_update_capitalization(&pubkey, &account),
            |old, new| {
                assert_eq!(old + 100, new);
                true
            },
        );
        assert_eq!(account, bank.get_account(&pubkey).unwrap());
    }

    #[test]
    fn test_store_account_and_update_capitalization_decreased() {
        let old_lamports = 400;
        let (genesis_config, mint_keypair) = create_genesis_config(old_lamports);
        let bank = Bank::new_for_tests(&genesis_config);
        let pubkey = mint_keypair.pubkey();

        let new_lamports = 100;
        let account = AccountSharedData::new(new_lamports, 0, &system_program::id());

        assert_capitalization_diff(
            &bank,
            || bank.store_account_and_update_capitalization(&pubkey, &account),
            |old, new| {
                assert_eq!(old - 300, new);
                true
            },
        );
        assert_eq!(account, bank.get_account(&pubkey).unwrap());
    }

    #[test]
    fn test_store_account_and_update_capitalization_unchanged() {
        let lamports = 400;
        let (genesis_config, mint_keypair) = create_genesis_config(lamports);
        let bank = Bank::new_for_tests(&genesis_config);
        let pubkey = mint_keypair.pubkey();

        let account = AccountSharedData::new(lamports, 1, &system_program::id());

        assert_capitalization_diff(
            &bank,
            || bank.store_account_and_update_capitalization(&pubkey, &account),
            |old, new| {
                assert_eq!(old, new);
                true
            },
        );
        assert_eq!(account, bank.get_account(&pubkey).unwrap());
    }

    #[test]
    #[ignore]
    fn test_rent_distribution() {
        solana_logger::setup();

        let bootstrap_validator_pubkey = solana_sdk::pubkey::new_rand();
        let bootstrap_validator_stake_lamports = 30;
        let mut genesis_config = create_genesis_config_with_leader(
            10,
            &bootstrap_validator_pubkey,
            bootstrap_validator_stake_lamports,
        )
        .genesis_config;
        // While we are preventing new accounts left in a rent-paying state, not quite ready to rip
        // out all the rent assessment tests. Just deactivate the feature for now.
        genesis_config
            .accounts
            .remove(&feature_set::require_rent_exempt_accounts::id())
            .unwrap();

        genesis_config.epoch_schedule = EpochSchedule::custom(
            MINIMUM_SLOTS_PER_EPOCH,
            genesis_config.epoch_schedule.leader_schedule_slot_offset,
            false,
        );

        genesis_config.rent = rent_with_exemption_threshold(2.0);

        let rent = Rent::free();

        let validator_1_pubkey = solana_sdk::pubkey::new_rand();
        let validator_1_stake_lamports = 20;
        let validator_1_staking_keypair = Keypair::new();
        let validator_1_voting_keypair = Keypair::new();

        let validator_1_vote_account = vote_state::create_account(
            &validator_1_voting_keypair.pubkey(),
            &validator_1_pubkey,
            0,
            validator_1_stake_lamports,
        );

        let validator_1_stake_account = stake_state::create_account(
            &validator_1_staking_keypair.pubkey(),
            &validator_1_voting_keypair.pubkey(),
            &validator_1_vote_account,
            &rent,
            validator_1_stake_lamports,
        );

        genesis_config.accounts.insert(
            validator_1_pubkey,
            Account::new(42, 0, &system_program::id()),
        );
        genesis_config.accounts.insert(
            validator_1_staking_keypair.pubkey(),
            Account::from(validator_1_stake_account),
        );
        genesis_config.accounts.insert(
            validator_1_voting_keypair.pubkey(),
            Account::from(validator_1_vote_account),
        );

        let validator_2_pubkey = solana_sdk::pubkey::new_rand();
        let validator_2_stake_lamports = 20;
        let validator_2_staking_keypair = Keypair::new();
        let validator_2_voting_keypair = Keypair::new();

        let validator_2_vote_account = vote_state::create_account(
            &validator_2_voting_keypair.pubkey(),
            &validator_2_pubkey,
            0,
            validator_2_stake_lamports,
        );

        let validator_2_stake_account = stake_state::create_account(
            &validator_2_staking_keypair.pubkey(),
            &validator_2_voting_keypair.pubkey(),
            &validator_2_vote_account,
            &rent,
            validator_2_stake_lamports,
        );

        genesis_config.accounts.insert(
            validator_2_pubkey,
            Account::new(42, 0, &system_program::id()),
        );
        genesis_config.accounts.insert(
            validator_2_staking_keypair.pubkey(),
            Account::from(validator_2_stake_account),
        );
        genesis_config.accounts.insert(
            validator_2_voting_keypair.pubkey(),
            Account::from(validator_2_vote_account),
        );

        let validator_3_pubkey = solana_sdk::pubkey::new_rand();
        let validator_3_stake_lamports = 30;
        let validator_3_staking_keypair = Keypair::new();
        let validator_3_voting_keypair = Keypair::new();

        let validator_3_vote_account = vote_state::create_account(
            &validator_3_voting_keypair.pubkey(),
            &validator_3_pubkey,
            0,
            validator_3_stake_lamports,
        );

        let validator_3_stake_account = stake_state::create_account(
            &validator_3_staking_keypair.pubkey(),
            &validator_3_voting_keypair.pubkey(),
            &validator_3_vote_account,
            &rent,
            validator_3_stake_lamports,
        );

        genesis_config.accounts.insert(
            validator_3_pubkey,
            Account::new(42, 0, &system_program::id()),
        );
        genesis_config.accounts.insert(
            validator_3_staking_keypair.pubkey(),
            Account::from(validator_3_stake_account),
        );
        genesis_config.accounts.insert(
            validator_3_voting_keypair.pubkey(),
            Account::from(validator_3_vote_account),
        );

        genesis_config.rent = rent_with_exemption_threshold(10.0);

        let mut bank = Bank::new_for_tests(&genesis_config);
        // Enable rent collection
        bank.rent_collector.epoch = 5;
        bank.rent_collector.slots_per_year = 192.0;

        let payer = Keypair::new();
        let payer_account = AccountSharedData::new(400, 0, &system_program::id());
        bank.store_account_and_update_capitalization(&payer.pubkey(), &payer_account);

        let payee = Keypair::new();
        let payee_account = AccountSharedData::new(70, 1, &system_program::id());
        bank.store_account_and_update_capitalization(&payee.pubkey(), &payee_account);

        let bootstrap_validator_initial_balance = bank.get_balance(&bootstrap_validator_pubkey);

        let tx = system_transaction::transfer(&payer, &payee.pubkey(), 180, genesis_config.hash());

        let result = bank.process_transaction(&tx);
        assert_eq!(result, Ok(()));

        let mut total_rent_deducted = 0;

        // 400 - 128(Rent) - 180(Transfer)
        assert_eq!(bank.get_balance(&payer.pubkey()), 92);
        total_rent_deducted += 128;

        // 70 - 70(Rent) + 180(Transfer) - 21(Rent)
        assert_eq!(bank.get_balance(&payee.pubkey()), 159);
        total_rent_deducted += 70 + 21;

        let previous_capitalization = bank.capitalization.load(Relaxed);

        bank.freeze();

        assert_eq!(bank.collected_rent.load(Relaxed), total_rent_deducted);

        let burned_portion =
            total_rent_deducted * u64::from(bank.rent_collector.rent.burn_percent) / 100;
        let rent_to_be_distributed = total_rent_deducted - burned_portion;

        let bootstrap_validator_portion =
            ((bootstrap_validator_stake_lamports * rent_to_be_distributed) as f64 / 100.0) as u64
                + 1; // Leftover lamport
        assert_eq!(
            bank.get_balance(&bootstrap_validator_pubkey),
            bootstrap_validator_portion + bootstrap_validator_initial_balance
        );

        // Since, validator 1 and validator 2 has equal smallest stake, it comes down to comparison
        // between their pubkey.
        let tweak_1 = u64::from(validator_1_pubkey > validator_2_pubkey);
        let validator_1_portion =
            ((validator_1_stake_lamports * rent_to_be_distributed) as f64 / 100.0) as u64 + tweak_1;
        assert_eq!(
            bank.get_balance(&validator_1_pubkey),
            validator_1_portion + 42 - tweak_1,
        );

        // Since, validator 1 and validator 2 has equal smallest stake, it comes down to comparison
        // between their pubkey.
        let tweak_2 = u64::from(validator_2_pubkey > validator_1_pubkey);
        let validator_2_portion =
            ((validator_2_stake_lamports * rent_to_be_distributed) as f64 / 100.0) as u64 + tweak_2;
        assert_eq!(
            bank.get_balance(&validator_2_pubkey),
            validator_2_portion + 42 - tweak_2,
        );

        let validator_3_portion =
            ((validator_3_stake_lamports * rent_to_be_distributed) as f64 / 100.0) as u64 + 1;
        assert_eq!(
            bank.get_balance(&validator_3_pubkey),
            validator_3_portion + 42
        );

        let current_capitalization = bank.capitalization.load(Relaxed);

        // only slot history is newly created
        let sysvar_and_builtin_program_delta =
            min_rent_exempt_balance_for_sysvars(&bank, &[sysvar::slot_history::id()]);
        assert_eq!(
            previous_capitalization - (current_capitalization - sysvar_and_builtin_program_delta),
            burned_portion
        );

        assert!(bank.calculate_and_verify_capitalization(true));

        assert_eq!(
            rent_to_be_distributed,
            bank.rewards
                .read()
                .unwrap()
                .iter()
                .map(|(address, reward)| {
                    if reward.lamports > 0 {
                        assert_eq!(reward.reward_type, RewardType::Rent);
                        if *address == validator_2_pubkey {
                            assert_eq!(reward.post_balance, validator_2_portion + 42 - tweak_2);
                        } else if *address == validator_3_pubkey {
                            assert_eq!(reward.post_balance, validator_3_portion + 42);
                        }
                        reward.lamports as u64
                    } else {
                        0
                    }
                })
                .sum::<u64>()
        );
    }

    #[test]
    fn test_distribute_rent_to_validators_overflow() {
        solana_logger::setup();

        // These values are taken from the real cluster (testnet)
        const RENT_TO_BE_DISTRIBUTED: u64 = 120_525;
        const VALIDATOR_STAKE: u64 = 374_999_998_287_840;

        let validator_pubkey = solana_sdk::pubkey::new_rand();
        let mut genesis_config =
            create_genesis_config_with_leader(10, &validator_pubkey, VALIDATOR_STAKE)
                .genesis_config;

        let bank = Bank::new_for_tests(&genesis_config);
        let old_validator_lamports = bank.get_balance(&validator_pubkey);
        bank.distribute_rent_to_validators(&bank.vote_accounts(), RENT_TO_BE_DISTRIBUTED);
        let new_validator_lamports = bank.get_balance(&validator_pubkey);
        assert_eq!(
            new_validator_lamports,
            old_validator_lamports + RENT_TO_BE_DISTRIBUTED
        );

        genesis_config
            .accounts
            .remove(&feature_set::no_overflow_rent_distribution::id())
            .unwrap();
        let bank = std::panic::AssertUnwindSafe(Bank::new_for_tests(&genesis_config));
        let old_validator_lamports = bank.get_balance(&validator_pubkey);
        let new_validator_lamports = std::panic::catch_unwind(|| {
            bank.distribute_rent_to_validators(&bank.vote_accounts(), RENT_TO_BE_DISTRIBUTED);
            bank.get_balance(&validator_pubkey)
        });

        if let Ok(new_validator_lamports) = new_validator_lamports {
            info!("asserting overflowing incorrect rent distribution");
            assert_ne!(
                new_validator_lamports,
                old_validator_lamports + RENT_TO_BE_DISTRIBUTED
            );
        } else {
            info!("NOT-asserting overflowing incorrect rent distribution");
        }
    }

    #[test]
    fn test_rent_exempt_executable_account() {
        let (mut genesis_config, mint_keypair) = create_genesis_config(100_000);
        genesis_config.rent = rent_with_exemption_threshold(1000.0);

        let root_bank = Arc::new(Bank::new_for_tests(&genesis_config));
        let bank = create_child_bank_for_rent_test(&root_bank, &genesis_config);

        let account_pubkey = solana_sdk::pubkey::new_rand();
        let account_balance = 1;
        let mut account =
            AccountSharedData::new(account_balance, 0, &solana_sdk::pubkey::new_rand());
        account.set_executable(true);
        bank.store_account(&account_pubkey, &account);

        let transfer_lamports = 1;
        let tx = system_transaction::transfer(
            &mint_keypair,
            &account_pubkey,
            transfer_lamports,
            genesis_config.hash(),
        );

        assert_eq!(
            bank.process_transaction(&tx),
            Err(TransactionError::InvalidWritableAccount)
        );
        assert_eq!(bank.get_balance(&account_pubkey), account_balance);
    }

    #[test]
    #[ignore]
    #[allow(clippy::cognitive_complexity)]
    fn test_rent_complex() {
        solana_logger::setup();
        let mock_program_id = Pubkey::new(&[2u8; 32]);

        #[derive(Serialize, Deserialize)]
        enum MockInstruction {
            Deduction,
        }

        fn mock_process_instruction(
            _first_instruction_account: IndexOfAccount,
            invoke_context: &mut InvokeContext,
        ) -> result::Result<(), InstructionError> {
            let transaction_context = &invoke_context.transaction_context;
            let instruction_context = transaction_context.get_current_instruction_context()?;
            let instruction_data = instruction_context.get_instruction_data();
            if let Ok(instruction) = bincode::deserialize(instruction_data) {
                match instruction {
                    MockInstruction::Deduction => {
                        instruction_context
                            .try_borrow_instruction_account(transaction_context, 1)?
                            .checked_add_lamports(1)?;
                        instruction_context
                            .try_borrow_instruction_account(transaction_context, 2)?
                            .checked_sub_lamports(1)?;
                        Ok(())
                    }
                }
            } else {
                Err(InstructionError::InvalidInstructionData)
            }
        }

        let (mut genesis_config, _mint_keypair) = create_genesis_config(10);
        let mut keypairs: Vec<Keypair> = Vec::with_capacity(14);
        for _i in 0..14 {
            keypairs.push(Keypair::new());
        }

        genesis_config.rent = rent_with_exemption_threshold(1000.0);

        let root_bank = Bank::new_for_tests(&genesis_config);
        // until we completely transition to the eager rent collection,
        // we must ensure lazy rent collection doens't get broken!
        root_bank.restore_old_behavior_for_fragile_tests();
        let root_bank = Arc::new(root_bank);
        let mut bank = create_child_bank_for_rent_test(&root_bank, &genesis_config);
        bank.add_builtin("mock_program", &mock_program_id, mock_process_instruction);

        assert_eq!(bank.last_blockhash(), genesis_config.hash());

        let slots_elapsed: u64 = (0..=bank.epoch)
            .map(|epoch| {
                bank.rent_collector
                    .epoch_schedule
                    .get_slots_in_epoch(epoch + 1)
            })
            .sum();
        let generic_rent_due_for_system_account = bank
            .rent_collector
            .rent
            .due(
                bank.get_minimum_balance_for_rent_exemption(0) - 1,
                0,
                slots_elapsed as f64 / bank.rent_collector.slots_per_year,
            )
            .lamports();

        store_accounts_for_rent_test(
            &bank,
            &mut keypairs,
            mock_program_id,
            generic_rent_due_for_system_account,
        );

        let magic_rent_number = 131; // yuck, derive this value programmatically one day

        let t1 = system_transaction::transfer(
            &keypairs[0],
            &keypairs[1].pubkey(),
            1,
            genesis_config.hash(),
        );
        let t2 = system_transaction::transfer(
            &keypairs[2],
            &keypairs[3].pubkey(),
            1,
            genesis_config.hash(),
        );
        let t3 = system_transaction::transfer(
            &keypairs[4],
            &keypairs[5].pubkey(),
            1,
            genesis_config.hash(),
        );
        let t4 = system_transaction::transfer(
            &keypairs[6],
            &keypairs[7].pubkey(),
            generic_rent_due_for_system_account + 1,
            genesis_config.hash(),
        );
        let t5 = system_transaction::transfer(
            &keypairs[8],
            &keypairs[9].pubkey(),
            929,
            genesis_config.hash(),
        );

        let account_metas = vec![
            AccountMeta::new(keypairs[10].pubkey(), true),
            AccountMeta::new(keypairs[11].pubkey(), true),
            AccountMeta::new(keypairs[12].pubkey(), true),
            AccountMeta::new_readonly(keypairs[13].pubkey(), false),
        ];
        let deduct_instruction = Instruction::new_with_bincode(
            mock_program_id,
            &MockInstruction::Deduction,
            account_metas,
        );
        let t6 = Transaction::new_signed_with_payer(
            &[deduct_instruction],
            Some(&keypairs[10].pubkey()),
            &[&keypairs[10], &keypairs[11], &keypairs[12]],
            genesis_config.hash(),
        );

        let txs = vec![t6, t5, t1, t2, t3, t4];
        let res = bank.process_transactions(txs.iter());

        assert_eq!(res.len(), 6);
        assert_eq!(res[0], Ok(()));
        assert_eq!(res[1], Ok(()));
        assert_eq!(res[2], Ok(()));
        assert_eq!(res[3], Ok(()));
        assert_eq!(res[4], Err(TransactionError::AccountNotFound));
        assert_eq!(res[5], Ok(()));

        bank.freeze();

        let mut rent_collected = 0;

        // 48992 - generic_rent_due_for_system_account(Rent) - 1(transfer)
        assert_eq!(bank.get_balance(&keypairs[0].pubkey()), 1);
        rent_collected += generic_rent_due_for_system_account;

        // 48992 - generic_rent_due_for_system_account(Rent) + 1(transfer)
        assert_eq!(bank.get_balance(&keypairs[1].pubkey()), 3);
        rent_collected += generic_rent_due_for_system_account;

        // 48992 - generic_rent_due_for_system_account(Rent) - 1(transfer)
        assert_eq!(bank.get_balance(&keypairs[2].pubkey()), 1);
        rent_collected += generic_rent_due_for_system_account;

        // 48992 - generic_rent_due_for_system_account(Rent) + 1(transfer)
        assert_eq!(bank.get_balance(&keypairs[3].pubkey()), 3);
        rent_collected += generic_rent_due_for_system_account;

        // No rent deducted
        assert_eq!(bank.get_balance(&keypairs[4].pubkey()), 10);
        assert_eq!(bank.get_balance(&keypairs[5].pubkey()), 10);

        // 98004 - generic_rent_due_for_system_account(Rent) - 48991(transfer)
        assert_eq!(bank.get_balance(&keypairs[6].pubkey()), 23);
        rent_collected += generic_rent_due_for_system_account;

        // 0 + 48990(transfer) - magic_rent_number(Rent)
        assert_eq!(
            bank.get_balance(&keypairs[7].pubkey()),
            generic_rent_due_for_system_account + 1 - magic_rent_number
        );

        // Epoch should be updated
        // Rent deducted on store side
        let account8 = bank.get_account(&keypairs[7].pubkey()).unwrap();
        // Epoch should be set correctly.
        assert_eq!(account8.rent_epoch(), bank.epoch + 1);
        rent_collected += magic_rent_number;

        // 49921 - generic_rent_due_for_system_account(Rent) - 929(Transfer)
        assert_eq!(bank.get_balance(&keypairs[8].pubkey()), 2);
        rent_collected += generic_rent_due_for_system_account;

        let account10 = bank.get_account(&keypairs[9].pubkey()).unwrap();
        // Account was overwritten at load time, since it didn't have sufficient balance to pay rent
        // Then, at store time we deducted `magic_rent_number` rent for the current epoch, once it has balance
        assert_eq!(account10.rent_epoch(), bank.epoch + 1);
        // account data is blank now
        assert_eq!(account10.data().len(), 0);
        // 10 - 10(Rent) + 929(Transfer) - magic_rent_number(Rent)
        assert_eq!(account10.lamports(), 929 - magic_rent_number);
        rent_collected += magic_rent_number + 10;

        // 48993 - generic_rent_due_for_system_account(Rent)
        assert_eq!(bank.get_balance(&keypairs[10].pubkey()), 3);
        rent_collected += generic_rent_due_for_system_account;

        // 48993 - generic_rent_due_for_system_account(Rent) + 1(Addition by program)
        assert_eq!(bank.get_balance(&keypairs[11].pubkey()), 4);
        rent_collected += generic_rent_due_for_system_account;

        // 48993 - generic_rent_due_for_system_account(Rent) - 1(Deduction by program)
        assert_eq!(bank.get_balance(&keypairs[12].pubkey()), 2);
        rent_collected += generic_rent_due_for_system_account;

        // No rent for read-only account
        assert_eq!(bank.get_balance(&keypairs[13].pubkey()), 14);

        // Bank's collected rent should be sum of rent collected from all accounts
        assert_eq!(bank.collected_rent.load(Relaxed), rent_collected);
    }

    fn test_rent_collection_partitions(bank: &Bank) -> Vec<Partition> {
        let partitions = bank.rent_collection_partitions();
        let slot = bank.slot();
        if slot.saturating_sub(1) == bank.parent_slot() {
            let partition = Bank::variable_cycle_partition_from_previous_slot(
                bank.epoch_schedule(),
                bank.slot(),
            );
            assert_eq!(
                partitions.last().unwrap(),
                &partition,
                "slot: {}, slots per epoch: {}, partitions: {:?}",
                bank.slot(),
                bank.epoch_schedule().slots_per_epoch,
                partitions
            );
        }
        partitions
    }

    #[test]
    fn test_rent_eager_across_epoch_without_gap() {
        let mut bank = create_simple_test_arc_bank(1);
        assert_eq!(bank.rent_collection_partitions(), vec![(0, 0, 32)]);

        bank = Arc::new(new_from_parent(&bank));
        assert_eq!(bank.rent_collection_partitions(), vec![(0, 1, 32)]);
        for _ in 2..32 {
            bank = Arc::new(new_from_parent(&bank));
        }
        assert_eq!(bank.rent_collection_partitions(), vec![(30, 31, 32)]);
        bank = Arc::new(new_from_parent(&bank));
        assert_eq!(bank.rent_collection_partitions(), vec![(0, 0, 64)]);
    }

    #[test]
    fn test_rent_eager_across_epoch_without_gap_mnb() {
        solana_logger::setup();
        let (mut genesis_config, _mint_keypair) = create_genesis_config(1);
        genesis_config.cluster_type = ClusterType::MainnetBeta;

        let mut bank = Arc::new(Bank::new_for_tests(&genesis_config));
        assert_eq!(test_rent_collection_partitions(&bank), vec![(0, 0, 32)]);

        bank = Arc::new(new_from_parent(&bank));
        assert_eq!(test_rent_collection_partitions(&bank), vec![(0, 1, 32)]);
        for _ in 2..32 {
            bank = Arc::new(new_from_parent(&bank));
        }
        assert_eq!(test_rent_collection_partitions(&bank), vec![(30, 31, 32)]);
        bank = Arc::new(new_from_parent(&bank));
        assert_eq!(test_rent_collection_partitions(&bank), vec![(0, 0, 64)]);
    }

    #[test]
    fn test_rent_eager_across_epoch_with_full_gap() {
        let (mut genesis_config, _mint_keypair) = create_genesis_config(1);
        activate_all_features(&mut genesis_config);

        let mut bank = Arc::new(Bank::new_for_tests(&genesis_config));
        assert_eq!(bank.rent_collection_partitions(), vec![(0, 0, 32)]);

        bank = Arc::new(new_from_parent(&bank));
        assert_eq!(bank.rent_collection_partitions(), vec![(0, 1, 32)]);
        for _ in 2..15 {
            bank = Arc::new(new_from_parent(&bank));
        }
        assert_eq!(bank.rent_collection_partitions(), vec![(13, 14, 32)]);
        bank = Arc::new(Bank::new_from_parent(&bank, &Pubkey::default(), 49));
        assert_eq!(
            bank.rent_collection_partitions(),
            vec![(14, 31, 32), (0, 0, 64), (0, 17, 64)]
        );
        bank = Arc::new(new_from_parent(&bank));
        assert_eq!(bank.rent_collection_partitions(), vec![(17, 18, 64)]);
    }

    #[test]
    fn test_rent_eager_across_epoch_with_half_gap() {
        let (mut genesis_config, _mint_keypair) = create_genesis_config(1);
        activate_all_features(&mut genesis_config);

        let mut bank = Arc::new(Bank::new_for_tests(&genesis_config));
        assert_eq!(bank.rent_collection_partitions(), vec![(0, 0, 32)]);

        bank = Arc::new(new_from_parent(&bank));
        assert_eq!(bank.rent_collection_partitions(), vec![(0, 1, 32)]);
        for _ in 2..15 {
            bank = Arc::new(new_from_parent(&bank));
        }
        assert_eq!(bank.rent_collection_partitions(), vec![(13, 14, 32)]);
        bank = Arc::new(Bank::new_from_parent(&bank, &Pubkey::default(), 32));
        assert_eq!(
            bank.rent_collection_partitions(),
            vec![(14, 31, 32), (0, 0, 64)]
        );
        bank = Arc::new(new_from_parent(&bank));
        assert_eq!(bank.rent_collection_partitions(), vec![(0, 1, 64)]);
    }

    #[test]
    #[allow(clippy::cognitive_complexity)]
    fn test_rent_eager_across_epoch_without_gap_under_multi_epoch_cycle() {
        let leader_pubkey = solana_sdk::pubkey::new_rand();
        let leader_lamports = 3;
        let mut genesis_config =
            create_genesis_config_with_leader(5, &leader_pubkey, leader_lamports).genesis_config;
        genesis_config.cluster_type = ClusterType::MainnetBeta;

        const SLOTS_PER_EPOCH: u64 = MINIMUM_SLOTS_PER_EPOCH;
        const LEADER_SCHEDULE_SLOT_OFFSET: u64 = SLOTS_PER_EPOCH * 3 - 3;
        genesis_config.epoch_schedule =
            EpochSchedule::custom(SLOTS_PER_EPOCH, LEADER_SCHEDULE_SLOT_OFFSET, false);

        let mut bank = Arc::new(Bank::new_for_tests(&genesis_config));
        assert_eq!(DEFAULT_SLOTS_PER_EPOCH, 432_000);
        assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 32);
        assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (0, 0));
        assert_eq!(bank.rent_collection_partitions(), vec![(0, 0, 432_000)]);

        bank = Arc::new(new_from_parent(&bank));
        assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 32);
        assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (0, 1));
        assert_eq!(bank.rent_collection_partitions(), vec![(0, 1, 432_000)]);

        for _ in 2..32 {
            bank = Arc::new(new_from_parent(&bank));
        }
        assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 32);
        assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (0, 31));
        assert_eq!(bank.rent_collection_partitions(), vec![(30, 31, 432_000)]);

        bank = Arc::new(new_from_parent(&bank));
        assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 32);
        assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (1, 0));
        assert_eq!(bank.rent_collection_partitions(), vec![(31, 32, 432_000)]);

        bank = Arc::new(new_from_parent(&bank));
        assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 32);
        assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (1, 1));
        assert_eq!(bank.rent_collection_partitions(), vec![(32, 33, 432_000)]);

        bank = Arc::new(Bank::new_from_parent(&bank, &Pubkey::default(), 1000));
        bank = Arc::new(Bank::new_from_parent(&bank, &Pubkey::default(), 1001));
        assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 32);
        assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (31, 9));
        assert_eq!(
            bank.rent_collection_partitions(),
            vec![(1000, 1001, 432_000)]
        );

        bank = Arc::new(Bank::new_from_parent(&bank, &Pubkey::default(), 431_998));
        bank = Arc::new(Bank::new_from_parent(&bank, &Pubkey::default(), 431_999));
        assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 32);
        assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (13499, 31));
        assert_eq!(
            bank.rent_collection_partitions(),
            vec![(431_998, 431_999, 432_000)]
        );

        bank = Arc::new(new_from_parent(&bank));
        assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 32);
        assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (13500, 0));
        assert_eq!(bank.rent_collection_partitions(), vec![(0, 0, 432_000)]);

        bank = Arc::new(new_from_parent(&bank));
        assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 32);
        assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (13500, 1));
        assert_eq!(bank.rent_collection_partitions(), vec![(0, 1, 432_000)]);
    }

    #[test]
    fn test_rent_eager_across_epoch_with_gap_under_multi_epoch_cycle() {
        let leader_pubkey = solana_sdk::pubkey::new_rand();
        let leader_lamports = 3;
        let mut genesis_config =
            create_genesis_config_with_leader(5, &leader_pubkey, leader_lamports).genesis_config;
        genesis_config.cluster_type = ClusterType::MainnetBeta;

        const SLOTS_PER_EPOCH: u64 = MINIMUM_SLOTS_PER_EPOCH;
        const LEADER_SCHEDULE_SLOT_OFFSET: u64 = SLOTS_PER_EPOCH * 3 - 3;
        genesis_config.epoch_schedule =
            EpochSchedule::custom(SLOTS_PER_EPOCH, LEADER_SCHEDULE_SLOT_OFFSET, false);

        let mut bank = Arc::new(Bank::new_for_tests(&genesis_config));
        assert_eq!(DEFAULT_SLOTS_PER_EPOCH, 432_000);
        assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 32);
        assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (0, 0));
        assert_eq!(bank.rent_collection_partitions(), vec![(0, 0, 432_000)]);

        bank = Arc::new(new_from_parent(&bank));
        assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 32);
        assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (0, 1));
        assert_eq!(bank.rent_collection_partitions(), vec![(0, 1, 432_000)]);

        for _ in 2..19 {
            bank = Arc::new(new_from_parent(&bank));
        }
        assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 32);
        assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (0, 18));
        assert_eq!(bank.rent_collection_partitions(), vec![(17, 18, 432_000)]);

        bank = Arc::new(Bank::new_from_parent(&bank, &Pubkey::default(), 44));
        assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 32);
        assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (1, 12));
        assert_eq!(
            bank.rent_collection_partitions(),
            vec![(18, 31, 432_000), (31, 31, 432_000), (31, 44, 432_000)]
        );

        bank = Arc::new(new_from_parent(&bank));
        assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 32);
        assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (1, 13));
        assert_eq!(bank.rent_collection_partitions(), vec![(44, 45, 432_000)]);

        bank = Arc::new(Bank::new_from_parent(&bank, &Pubkey::default(), 431_993));
        bank = Arc::new(Bank::new_from_parent(&bank, &Pubkey::default(), 432_011));
        assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 32);
        assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (13500, 11));
        assert_eq!(
            bank.rent_collection_partitions(),
            vec![
                (431_993, 431_999, 432_000),
                (0, 0, 432_000),
                (0, 11, 432_000)
            ]
        );
    }

    #[test]
    fn test_rent_eager_with_warmup_epochs_under_multi_epoch_cycle() {
        let leader_pubkey = solana_sdk::pubkey::new_rand();
        let leader_lamports = 3;
        let mut genesis_config =
            create_genesis_config_with_leader(5, &leader_pubkey, leader_lamports).genesis_config;
        genesis_config.cluster_type = ClusterType::MainnetBeta;

        const SLOTS_PER_EPOCH: u64 = MINIMUM_SLOTS_PER_EPOCH * 8;
        const LEADER_SCHEDULE_SLOT_OFFSET: u64 = SLOTS_PER_EPOCH * 3 - 3;
        genesis_config.epoch_schedule =
            EpochSchedule::custom(SLOTS_PER_EPOCH, LEADER_SCHEDULE_SLOT_OFFSET, true);

        let mut bank = Arc::new(Bank::new_for_tests(&genesis_config));
        assert_eq!(DEFAULT_SLOTS_PER_EPOCH, 432_000);
        assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 32);
        assert_eq!(bank.first_normal_epoch(), 3);
        assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (0, 0));
        assert_eq!(bank.rent_collection_partitions(), vec![(0, 0, 32)]);

        bank = Arc::new(Bank::new_from_parent(&bank, &Pubkey::default(), 222));
        bank = Arc::new(new_from_parent(&bank));
        assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 128);
        assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (2, 127));
        assert_eq!(bank.rent_collection_partitions(), vec![(126, 127, 128)]);

        bank = Arc::new(new_from_parent(&bank));
        assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 256);
        assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (3, 0));
        assert_eq!(bank.rent_collection_partitions(), vec![(0, 0, 431_872)]);
        assert_eq!(431_872 % bank.get_slots_in_epoch(bank.epoch()), 0);

        bank = Arc::new(new_from_parent(&bank));
        assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 256);
        assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (3, 1));
        assert_eq!(bank.rent_collection_partitions(), vec![(0, 1, 431_872)]);

        bank = Arc::new(Bank::new_from_parent(
            &bank,
            &Pubkey::default(),
            431_872 + 223 - 1,
        ));
        bank = Arc::new(new_from_parent(&bank));
        assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 256);
        assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (1689, 255));
        assert_eq!(
            bank.rent_collection_partitions(),
            vec![(431_870, 431_871, 431_872)]
        );

        bank = Arc::new(new_from_parent(&bank));
        assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 256);
        assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (1690, 0));
        assert_eq!(bank.rent_collection_partitions(), vec![(0, 0, 431_872)]);
    }

    #[test]
    fn test_rent_eager_under_fixed_cycle_for_development() {
        solana_logger::setup();
        let leader_pubkey = solana_sdk::pubkey::new_rand();
        let leader_lamports = 3;
        let mut genesis_config =
            create_genesis_config_with_leader(5, &leader_pubkey, leader_lamports).genesis_config;

        const SLOTS_PER_EPOCH: u64 = MINIMUM_SLOTS_PER_EPOCH * 8;
        const LEADER_SCHEDULE_SLOT_OFFSET: u64 = SLOTS_PER_EPOCH * 3 - 3;
        genesis_config.epoch_schedule =
            EpochSchedule::custom(SLOTS_PER_EPOCH, LEADER_SCHEDULE_SLOT_OFFSET, true);

        let mut bank = Arc::new(Bank::new_for_tests(&genesis_config));
        assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 32);
        assert_eq!(bank.first_normal_epoch(), 3);
        assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (0, 0));
        assert_eq!(bank.rent_collection_partitions(), vec![(0, 0, 432_000)]);

        bank = Arc::new(Bank::new_from_parent(&bank, &Pubkey::default(), 222));
        bank = Arc::new(new_from_parent(&bank));
        assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 128);
        assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (2, 127));
        assert_eq!(bank.rent_collection_partitions(), vec![(222, 223, 432_000)]);

        bank = Arc::new(new_from_parent(&bank));
        assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 256);
        assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (3, 0));
        assert_eq!(bank.rent_collection_partitions(), vec![(223, 224, 432_000)]);

        bank = Arc::new(new_from_parent(&bank));
        assert_eq!(bank.get_slots_in_epoch(bank.epoch()), 256);
        assert_eq!(bank.get_epoch_and_slot_index(bank.slot()), (3, 1));
        assert_eq!(bank.rent_collection_partitions(), vec![(224, 225, 432_000)]);

        bank = Arc::new(Bank::new_from_parent(
            &bank,
            &Pubkey::default(),
            432_000 - 2,
        ));
        bank = Arc::new(new_from_parent(&bank));
        assert_eq!(
            bank.rent_collection_partitions(),
            vec![(431_998, 431_999, 432_000)]
        );
        bank = Arc::new(new_from_parent(&bank));
        assert_eq!(bank.rent_collection_partitions(), vec![(0, 0, 432_000)]);
        bank = Arc::new(new_from_parent(&bank));
        assert_eq!(bank.rent_collection_partitions(), vec![(0, 1, 432_000)]);

        bank = Arc::new(Bank::new_from_parent(
            &bank,
            &Pubkey::default(),
            864_000 - 20,
        ));
        bank = Arc::new(Bank::new_from_parent(
            &bank,
            &Pubkey::default(),
            864_000 + 39,
        ));
        assert_eq!(
            bank.rent_collection_partitions(),
            vec![
                (431_980, 431_999, 432_000),
                (0, 0, 432_000),
                (0, 39, 432_000)
            ]
        );
    }

    #[test]
    fn test_rent_eager_pubkey_range_minimal() {
        let range = Bank::pubkey_range_from_partition((0, 0, 1));
        assert_eq!(
            range,
            Pubkey::new_from_array([0x00; 32])..=Pubkey::new_from_array([0xff; 32])
        );
    }

    #[test]
    fn test_rent_eager_pubkey_range_maximum() {
        let max = !0;

        let range = Bank::pubkey_range_from_partition((0, 0, max));
        assert_eq!(
            range,
            Pubkey::new_from_array([0x00; 32])
                ..=Pubkey::new_from_array([
                    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff
                ])
        );
        let range = Bank::pubkey_range_from_partition((0, 1, max));
        const ONE: u8 = 0x01;
        assert_eq!(
            range,
            Pubkey::new_from_array([
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, ONE, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00,
            ])
                ..=Pubkey::new_from_array([
                    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff
                ])
        );
        let range = Bank::pubkey_range_from_partition((max - 3, max - 2, max));
        const FD: u8 = 0xfd;
        assert_eq!(
            range,
            Pubkey::new_from_array([
                0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xfd, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00,
            ])
                ..=Pubkey::new_from_array([
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, FD, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff
                ])
        );
        let range = Bank::pubkey_range_from_partition((max - 2, max - 1, max));
        assert_eq!(
            range,
            Pubkey::new_from_array([
                0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xfe, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00,
            ])..=pubkey_max_value()
        );

        fn should_cause_overflow(partition_count: u64) -> bool {
            // Check `partition_width = (u64::max_value() + 1) / partition_count` is exact and
            // does not have a remainder.
            // This way, `partition_width * partition_count == (u64::max_value() + 1)`,
            // so the test actually tests for overflow
            (u64::max_value() - partition_count + 1) % partition_count == 0
        }

        let max_exact = 64;
        // Make sure `max_exact` divides evenly when calculating `calculate_partition_width`
        assert!(should_cause_overflow(max_exact));
        // Make sure `max_inexact` doesn't divide evenly when calculating `calculate_partition_width`
        let max_inexact = 10;
        assert!(!should_cause_overflow(max_inexact));

        for max in &[max_exact, max_inexact] {
            let range = Bank::pubkey_range_from_partition((max - 1, max - 1, *max));
            assert_eq!(range, pubkey_max_value()..=pubkey_max_value());
        }
    }

    fn map_to_test_bad_range() -> std::collections::BTreeMap<Pubkey, i8> {
        let mut map = std::collections::BTreeMap::new();
        // when empty, std::collections::BTreeMap doesn't sanitize given range...
        map.insert(solana_sdk::pubkey::new_rand(), 1);
        map
    }

    #[test]
    #[should_panic(expected = "range start is greater than range end in BTreeMap")]
    fn test_rent_eager_bad_range() {
        let test_map = map_to_test_bad_range();
        let _ = test_map.range(
            Pubkey::new_from_array([
                0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x01,
            ])
                ..=Pubkey::new_from_array([
                    0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0x00, 0x00, 0x00, 0x00, 0x00,
                    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                ]),
        );
    }

    #[test]
    fn test_rent_eager_pubkey_range_noop_range() {
        let test_map = map_to_test_bad_range();

        let range = Bank::pubkey_range_from_partition((0, 0, 3));
        assert_eq!(
            range,
            Pubkey::new_from_array([0x00; 32])
                ..=Pubkey::new_from_array([
                    0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x54, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff
                ])
        );
        let _ = test_map.range(range);

        let range = Bank::pubkey_range_from_partition((1, 1, 3));
        let same = Pubkey::new_from_array([
            0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00,
        ]);
        assert_eq!(range, same..=same);
        let _ = test_map.range(range);

        let range = Bank::pubkey_range_from_partition((2, 2, 3));
        assert_eq!(range, pubkey_max_value()..=pubkey_max_value());
        let _ = test_map.range(range);
    }

    fn pubkey_max_value() -> Pubkey {
        let highest = Pubkey::from_str("JEKNVnkbo3jma5nREBBJCDoXFVeKkD56V3xKrvRmWxFG").unwrap();
        let arr = Pubkey::new_from_array([0xff; 32]);
        assert_eq!(highest, arr);
        arr
    }

    #[test]
    fn test_rent_pubkey_range_max() {
        // start==end && start != 0 is curious behavior. Verifying it here.
        solana_logger::setup();
        let range = Bank::pubkey_range_from_partition((1, 1, 3));
        let p = Bank::partition_from_pubkey(range.start(), 3);
        assert_eq!(p, 2);
        let range = Bank::pubkey_range_from_partition((1, 2, 3));
        let p = Bank::partition_from_pubkey(range.start(), 3);
        assert_eq!(p, 2);
        let range = Bank::pubkey_range_from_partition((2, 2, 3));
        let p = Bank::partition_from_pubkey(range.start(), 3);
        assert_eq!(p, 2);
        let range = Bank::pubkey_range_from_partition((1, 1, 16));
        let p = Bank::partition_from_pubkey(range.start(), 16);
        assert_eq!(p, 2);
        let range = Bank::pubkey_range_from_partition((1, 2, 16));
        let p = Bank::partition_from_pubkey(range.start(), 16);
        assert_eq!(p, 2);
        let range = Bank::pubkey_range_from_partition((2, 2, 16));
        let p = Bank::partition_from_pubkey(range.start(), 16);
        assert_eq!(p, 3);
        let range = Bank::pubkey_range_from_partition((15, 15, 16));
        let p = Bank::partition_from_pubkey(range.start(), 16);
        assert_eq!(p, 15);
    }

    #[test]
    fn test_rent_eager_pubkey_range_dividable() {
        let test_map = map_to_test_bad_range();
        let range = Bank::pubkey_range_from_partition((0, 0, 2));

        assert_eq!(
            range,
            Pubkey::new_from_array([0x00; 32])
                ..=Pubkey::new_from_array([
                    0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff
                ])
        );
        let _ = test_map.range(range);

        let range = Bank::pubkey_range_from_partition((0, 1, 2));
        assert_eq!(
            range,
            Pubkey::new_from_array([
                0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00
            ])
                ..=Pubkey::new_from_array([
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff
                ])
        );
        let _ = test_map.range(range);
    }

    #[test]
    fn test_rent_eager_pubkey_range_not_dividable() {
        solana_logger::setup();

        let test_map = map_to_test_bad_range();
        let range = Bank::pubkey_range_from_partition((0, 0, 3));
        assert_eq!(
            range,
            Pubkey::new_from_array([0x00; 32])
                ..=Pubkey::new_from_array([
                    0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x54, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff
                ])
        );
        let _ = test_map.range(range);

        let range = Bank::pubkey_range_from_partition((0, 1, 3));
        assert_eq!(
            range,
            Pubkey::new_from_array([
                0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00
            ])
                ..=Pubkey::new_from_array([
                    0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xa9, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff
                ])
        );
        let _ = test_map.range(range);

        let range = Bank::pubkey_range_from_partition((1, 2, 3));
        assert_eq!(
            range,
            Pubkey::new_from_array([
                0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00
            ])
                ..=Pubkey::new_from_array([
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff
                ])
        );
        let _ = test_map.range(range);
    }

    #[test]
    fn test_rent_eager_pubkey_range_gap() {
        solana_logger::setup();

        let test_map = map_to_test_bad_range();
        let range = Bank::pubkey_range_from_partition((120, 1023, 12345));
        assert_eq!(
            range,
            Pubkey::new_from_array([
                0x02, 0x82, 0x5a, 0x89, 0xd1, 0xac, 0x58, 0x9c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00
            ])
                ..=Pubkey::new_from_array([
                    0x15, 0x3c, 0x1d, 0xf1, 0xc6, 0x39, 0xef, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff
                ])
        );
        let _ = test_map.range(range);
    }

    impl Bank {
        fn slots_by_pubkey(&self, pubkey: &Pubkey, ancestors: &Ancestors) -> Vec<Slot> {
            let (locked_entry, _) = self
                .rc
                .accounts
                .accounts_db
                .accounts_index
                .get(pubkey, Some(ancestors), None)
                .unwrap();
            locked_entry
                .slot_list()
                .iter()
                .map(|(slot, _)| *slot)
                .collect::<Vec<Slot>>()
        }
    }

    #[test]
    fn test_rent_eager_collect_rent_in_partition() {
        solana_logger::setup();

        let (mut genesis_config, _mint_keypair) = create_genesis_config(1_000_000);
        for feature_id in FeatureSet::default().inactive {
            if feature_id != solana_sdk::feature_set::set_exempt_rent_epoch_max::id() {
                activate_feature(&mut genesis_config, feature_id);
            }
        }

        let zero_lamport_pubkey = solana_sdk::pubkey::new_rand();
        let rent_due_pubkey = solana_sdk::pubkey::new_rand();
        let rent_exempt_pubkey = solana_sdk::pubkey::new_rand();
        let mut bank = Arc::new(Bank::new_for_tests(&genesis_config));
        let zero_lamports = 0;
        let little_lamports = 1234;
        let large_lamports = 123_456_789;
        // genesis_config.epoch_schedule.slots_per_epoch == 432_000 and is unsuitable for this test
        let some_slot = MINIMUM_SLOTS_PER_EPOCH; // chosen to cause epoch to be +1
        let rent_collected = 1; // this is a function of 'some_slot'

        bank.store_account(
            &zero_lamport_pubkey,
            &AccountSharedData::new(zero_lamports, 0, &Pubkey::default()),
        );
        bank.store_account(
            &rent_due_pubkey,
            &AccountSharedData::new(little_lamports, 0, &Pubkey::default()),
        );
        bank.store_account(
            &rent_exempt_pubkey,
            &AccountSharedData::new(large_lamports, 0, &Pubkey::default()),
        );

        let genesis_slot = 0;
        let ancestors = vec![(some_slot, 0), (0, 1)].into_iter().collect();

        let previous_epoch = bank.epoch();
        bank = Arc::new(Bank::new_from_parent(&bank, &Pubkey::default(), some_slot));
        let current_epoch = bank.epoch();
        assert_eq!(previous_epoch + 1, current_epoch);

        assert_eq!(bank.collected_rent.load(Relaxed), 0);
        assert_eq!(
            bank.get_account(&rent_due_pubkey).unwrap().lamports(),
            little_lamports
        );
        assert_eq!(bank.get_account(&rent_due_pubkey).unwrap().rent_epoch(), 0);
        assert_eq!(
            bank.slots_by_pubkey(&rent_due_pubkey, &ancestors),
            vec![genesis_slot]
        );
        assert_eq!(
            bank.slots_by_pubkey(&rent_exempt_pubkey, &ancestors),
            vec![genesis_slot]
        );
        assert_eq!(
            bank.slots_by_pubkey(&zero_lamport_pubkey, &ancestors),
            vec![genesis_slot]
        );

        assert_eq!(bank.collected_rent.load(Relaxed), 0);
        bank.collect_rent_in_partition((0, 0, 1), &RentMetrics::default()); // all range

        assert_eq!(bank.collected_rent.load(Relaxed), rent_collected);
        assert_eq!(
            bank.get_account(&rent_due_pubkey).unwrap().lamports(),
            little_lamports - rent_collected
        );
        assert_eq!(
            bank.get_account(&rent_due_pubkey).unwrap().rent_epoch(),
            current_epoch + 1
        );
        assert_eq!(
            bank.get_account(&rent_exempt_pubkey).unwrap().lamports(),
            large_lamports
        );
        // Once preserve_rent_epoch_for_rent_exempt_accounts is activated,
        // rent_epoch of rent-exempt accounts will no longer advance.
        assert_eq!(
            bank.get_account(&rent_exempt_pubkey).unwrap().rent_epoch(),
            0
        );
        assert_eq!(
            bank.slots_by_pubkey(&rent_due_pubkey, &ancestors),
            vec![genesis_slot, some_slot]
        );
        assert_eq!(
            bank.slots_by_pubkey(&rent_exempt_pubkey, &ancestors),
            vec![genesis_slot, some_slot]
        );
        assert_eq!(
            bank.slots_by_pubkey(&zero_lamport_pubkey, &ancestors),
            vec![genesis_slot]
        );
    }

    fn new_from_parent_next_epoch(parent: &Arc<Bank>, epochs: Epoch) -> Bank {
        let mut slot = parent.slot();
        let mut epoch = parent.epoch();
        for _ in 0..epochs {
            slot += parent.epoch_schedule().get_slots_in_epoch(epoch);
            epoch = parent.epoch_schedule().get_epoch(slot);
        }

        Bank::new_from_parent(parent, &Pubkey::default(), slot)
    }

    #[test]
    /// tests that an account which has already had rent collected IN this slot does not skip rewrites
    fn test_collect_rent_from_accounts() {
        solana_logger::setup();

        let zero_lamport_pubkey = Pubkey::new(&[0; 32]);

        let genesis_bank = create_simple_test_arc_bank(100000);
        let first_bank = Arc::new(new_from_parent(&genesis_bank));
        let first_slot = 1;
        assert_eq!(first_slot, first_bank.slot());
        let epoch_delta = 4;
        let later_bank = Arc::new(new_from_parent_next_epoch(&first_bank, epoch_delta)); // a bank a few epochs in the future
        let later_slot = later_bank.slot();
        assert!(later_bank.epoch() == genesis_bank.epoch() + epoch_delta);

        let data_size = 0; // make sure we're rent exempt
        let lamports = later_bank.get_minimum_balance_for_rent_exemption(data_size); // cannot be 0 or we zero out rent_epoch in rent collection and we need to be rent exempt
        let mut account = AccountSharedData::new(lamports, data_size, &Pubkey::default());
        account.set_rent_epoch(later_bank.epoch() - 1); // non-zero, but less than later_bank's epoch

        // loaded from previous slot, so we skip rent collection on it
        let _result = later_bank.collect_rent_from_accounts(
            vec![(zero_lamport_pubkey, account, later_slot - 1)],
            None,
            PartitionIndex::default(),
        );
    }

    #[test]
    fn test_rent_eager_collect_rent_zero_lamport_deterministic() {
        solana_logger::setup();

        let (genesis_config, _mint_keypair) = create_genesis_config(1);

        let zero_lamport_pubkey = solana_sdk::pubkey::new_rand();

        let genesis_bank1 = Arc::new(Bank::new_for_tests(&genesis_config));
        let genesis_bank2 = Arc::new(Bank::new_for_tests(&genesis_config));
        let bank1_with_zero = Arc::new(new_from_parent(&genesis_bank1));
        let bank1_without_zero = Arc::new(new_from_parent(&genesis_bank2));

        let zero_lamports = 0;
        let data_size = 12345; // use non-zero data size to also test accounts_data_size
        let account = AccountSharedData::new(zero_lamports, data_size, &Pubkey::default());
        bank1_with_zero.store_account(&zero_lamport_pubkey, &account);
        bank1_without_zero.store_account(&zero_lamport_pubkey, &account);

        bank1_without_zero
            .rc
            .accounts
            .accounts_db
            .accounts_index
            .add_root(genesis_bank1.slot() + 1);
        bank1_without_zero
            .rc
            .accounts
            .accounts_db
            .accounts_index
            .purge_roots(&zero_lamport_pubkey);

        // genesis_config.epoch_schedule.slots_per_epoch == 432_000 and is unsuitable for this test
        let some_slot = MINIMUM_SLOTS_PER_EPOCH; // 1 epoch
        let bank2_with_zero = Arc::new(Bank::new_from_parent(
            &bank1_with_zero,
            &Pubkey::default(),
            some_slot,
        ));
        assert_eq!(bank1_with_zero.epoch() + 1, bank2_with_zero.epoch());
        let bank2_without_zero = Arc::new(Bank::new_from_parent(
            &bank1_without_zero,
            &Pubkey::default(),
            some_slot,
        ));
        let hash1_with_zero = bank1_with_zero.hash();
        let hash1_without_zero = bank1_without_zero.hash();
        assert_eq!(hash1_with_zero, hash1_without_zero);
        assert_ne!(hash1_with_zero, Hash::default());

        bank2_with_zero.collect_rent_in_partition((0, 0, 1), &RentMetrics::default()); // all
        bank2_without_zero.collect_rent_in_partition((0, 0, 1), &RentMetrics::default()); // all

        bank2_with_zero.freeze();
        let hash2_with_zero = bank2_with_zero.hash();
        bank2_without_zero.freeze();
        let hash2_without_zero = bank2_without_zero.hash();

        assert_eq!(hash2_with_zero, hash2_without_zero);
        assert_ne!(hash2_with_zero, Hash::default());
    }

    #[test]
    fn test_bank_update_vote_stake_rewards() {
        let thread_pool = ThreadPoolBuilder::new().num_threads(1).build().unwrap();
        check_bank_update_vote_stake_rewards(|bank: &Bank| {
            bank.load_vote_and_stake_accounts_with_thread_pool(&thread_pool, null_tracer())
        });
        check_bank_update_vote_stake_rewards(|bank: &Bank| {
            bank.load_vote_and_stake_accounts(&thread_pool, null_tracer())
        });
    }

    fn check_bank_update_vote_stake_rewards<F>(load_vote_and_stake_accounts: F)
    where
        F: Fn(&Bank) -> LoadVoteAndStakeAccountsResult,
    {
        solana_logger::setup();

        // create a bank that ticks really slowly...
        let bank0 = Arc::new(Bank::new_for_tests(&GenesisConfig {
            accounts: (0..42)
                .map(|_| {
                    (
                        solana_sdk::pubkey::new_rand(),
                        Account::new(1_000_000_000, 0, &Pubkey::default()),
                    )
                })
                .collect(),
            // set it up so the first epoch is a full year long
            poh_config: PohConfig {
                target_tick_duration: Duration::from_secs(
                    SECONDS_PER_YEAR as u64 / MINIMUM_SLOTS_PER_EPOCH / DEFAULT_TICKS_PER_SLOT,
                ),
                hashes_per_tick: None,
                target_tick_count: None,
            },
            cluster_type: ClusterType::MainnetBeta,

            ..GenesisConfig::default()
        }));

        // enable lazy rent collection because this test depends on rent-due accounts
        // not being eagerly-collected for exact rewards calculation
        bank0.restore_old_behavior_for_fragile_tests();

        assert_eq!(
            bank0.capitalization(),
            42 * 1_000_000_000 + genesis_sysvar_and_builtin_program_lamports(),
        );

        let ((vote_id, mut vote_account), (stake_id, stake_account)) =
            crate::stakes::tests::create_staked_node_accounts(10_000);
        let starting_vote_and_stake_balance = 10_000 + 1;

        // set up accounts
        bank0.store_account_and_update_capitalization(&stake_id, &stake_account);

        // generate some rewards
        let mut vote_state = Some(vote_state::from(&vote_account).unwrap());
        for i in 0..MAX_LOCKOUT_HISTORY + 42 {
            if let Some(v) = vote_state.as_mut() {
                vote_state::process_slot_vote_unchecked(v, i as u64)
            }
            let versioned = VoteStateVersions::Current(Box::new(vote_state.take().unwrap()));
            vote_state::to(&versioned, &mut vote_account).unwrap();
            bank0.store_account_and_update_capitalization(&vote_id, &vote_account);
            match versioned {
                VoteStateVersions::Current(v) => {
                    vote_state = Some(*v);
                }
                _ => panic!("Has to be of type Current"),
            };
        }
        bank0.store_account_and_update_capitalization(&vote_id, &vote_account);
        bank0.freeze();

        assert_eq!(
            bank0.capitalization(),
            42 * 1_000_000_000
                + genesis_sysvar_and_builtin_program_lamports()
                + starting_vote_and_stake_balance
                + bank0_sysvar_delta(),
        );
        assert!(bank0.rewards.read().unwrap().is_empty());

        load_vote_and_stake_accounts(&bank0);

        // put a child bank in epoch 1, which calls update_rewards()...
        let bank1 = Bank::new_from_parent(
            &bank0,
            &Pubkey::default(),
            bank0.get_slots_in_epoch(bank0.epoch()) + 1,
        );
        // verify that there's inflation
        assert_ne!(bank1.capitalization(), bank0.capitalization());

        // verify the inflation is represented in validator_points
        let paid_rewards = bank1.capitalization() - bank0.capitalization() - bank1_sysvar_delta();

        // this assumes that no new builtins or precompiles were activated in bank1
        let PrevEpochInflationRewards {
            validator_rewards, ..
        } = bank1.calculate_previous_epoch_inflation_rewards(bank0.capitalization(), bank0.epoch());

        // verify the stake and vote accounts are the right size
        assert!(
            ((bank1.get_balance(&stake_id) - stake_account.lamports() + bank1.get_balance(&vote_id)
                - vote_account.lamports()) as f64
                - validator_rewards as f64)
                .abs()
                < 1.0
        );

        // verify the rewards are the right size
        assert!((validator_rewards as f64 - paid_rewards as f64).abs() < 1.0); // rounding, truncating

        // verify validator rewards show up in bank1.rewards vector
        assert_eq!(
            *bank1.rewards.read().unwrap(),
            vec![
                (
                    vote_id,
                    RewardInfo {
                        reward_type: RewardType::Voting,
                        lamports: 0,
                        post_balance: bank1.get_balance(&vote_id),
                        commission: Some(0),
                    }
                ),
                (
                    stake_id,
                    RewardInfo {
                        reward_type: RewardType::Staking,
                        lamports: validator_rewards as i64,
                        post_balance: bank1.get_balance(&stake_id),
                        commission: Some(0),
                    }
                )
            ]
        );
        bank1.freeze();
        add_root_and_flush_write_cache(&bank0);
        add_root_and_flush_write_cache(&bank1);
        assert!(bank1.calculate_and_verify_capitalization(true));
    }

    fn do_test_bank_update_rewards_determinism() -> u64 {
        // create a bank that ticks really slowly...
        let bank = Arc::new(Bank::new_for_tests(&GenesisConfig {
            accounts: (0..42)
                .map(|_| {
                    (
                        solana_sdk::pubkey::new_rand(),
                        Account::new(1_000_000_000, 0, &Pubkey::default()),
                    )
                })
                .collect(),
            // set it up so the first epoch is a full year long
            poh_config: PohConfig {
                target_tick_duration: Duration::from_secs(
                    SECONDS_PER_YEAR as u64 / MINIMUM_SLOTS_PER_EPOCH / DEFAULT_TICKS_PER_SLOT,
                ),
                hashes_per_tick: None,
                target_tick_count: None,
            },
            cluster_type: ClusterType::MainnetBeta,

            ..GenesisConfig::default()
        }));

        // enable lazy rent collection because this test depends on rent-due accounts
        // not being eagerly-collected for exact rewards calculation
        bank.restore_old_behavior_for_fragile_tests();

        assert_eq!(
            bank.capitalization(),
            42 * 1_000_000_000 + genesis_sysvar_and_builtin_program_lamports()
        );

        let vote_id = solana_sdk::pubkey::new_rand();
        let mut vote_account =
            vote_state::create_account(&vote_id, &solana_sdk::pubkey::new_rand(), 0, 100);
        let (stake_id1, stake_account1) = crate::stakes::tests::create_stake_account(123, &vote_id);
        let (stake_id2, stake_account2) = crate::stakes::tests::create_stake_account(456, &vote_id);

        // set up accounts
        bank.store_account_and_update_capitalization(&stake_id1, &stake_account1);
        bank.store_account_and_update_capitalization(&stake_id2, &stake_account2);

        // generate some rewards
        let mut vote_state = Some(vote_state::from(&vote_account).unwrap());
        for i in 0..MAX_LOCKOUT_HISTORY + 42 {
            if let Some(v) = vote_state.as_mut() {
                vote_state::process_slot_vote_unchecked(v, i as u64)
            }
            let versioned = VoteStateVersions::Current(Box::new(vote_state.take().unwrap()));
            vote_state::to(&versioned, &mut vote_account).unwrap();
            bank.store_account_and_update_capitalization(&vote_id, &vote_account);
            match versioned {
                VoteStateVersions::Current(v) => {
                    vote_state = Some(*v);
                }
                _ => panic!("Has to be of type Current"),
            };
        }
        bank.store_account_and_update_capitalization(&vote_id, &vote_account);

        // put a child bank in epoch 1, which calls update_rewards()...
        let bank1 = Bank::new_from_parent(
            &bank,
            &Pubkey::default(),
            bank.get_slots_in_epoch(bank.epoch()) + 1,
        );
        // verify that there's inflation
        assert_ne!(bank1.capitalization(), bank.capitalization());

        bank1.freeze();
        add_root_and_flush_write_cache(&bank);
        add_root_and_flush_write_cache(&bank1);
        assert!(bank1.calculate_and_verify_capitalization(true));

        // verify voting and staking rewards are recorded
        let rewards = bank1.rewards.read().unwrap();
        rewards
            .iter()
            .find(|(_address, reward)| reward.reward_type == RewardType::Voting)
            .unwrap();
        rewards
            .iter()
            .find(|(_address, reward)| reward.reward_type == RewardType::Staking)
            .unwrap();

        bank1.capitalization()
    }

    #[test]
    fn test_bank_update_rewards_determinism() {
        solana_logger::setup();

        // The same reward should be distributed given same credits
        let expected_capitalization = do_test_bank_update_rewards_determinism();
        // Repeat somewhat large number of iterations to expose possible different behavior
        // depending on the randomly-seeded HashMap ordering
        for _ in 0..30 {
            let actual_capitalization = do_test_bank_update_rewards_determinism();
            assert_eq!(actual_capitalization, expected_capitalization);
        }
    }

    impl VerifyBankHash {
        fn default_for_test() -> Self {
            Self {
                test_hash_calculation: true,
                ignore_mismatch: false,
                require_rooted_bank: false,
                run_in_background: false,
                store_hash_raw_data_for_debug: false,
            }
        }
    }

    // Test that purging 0 lamports accounts works.
    #[test]
    fn test_purge_empty_accounts() {
        // When using the write cache, flushing is destructive/cannot be undone
        //  so we have to stop at various points and restart to actively test.
        for pass in 0..3 {
            solana_logger::setup();
            let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.));
            let amount = genesis_config.rent.minimum_balance(0);
            let parent = Arc::new(Bank::new_for_tests_with_config(
                &genesis_config,
                BankTestConfig::default(),
            ));
            let mut bank = parent;
            for _ in 0..10 {
                let blockhash = bank.last_blockhash();
                let pubkey = solana_sdk::pubkey::new_rand();
                let tx = system_transaction::transfer(&mint_keypair, &pubkey, 0, blockhash);
                bank.process_transaction(&tx).unwrap();
                bank.freeze();
                bank.squash();
                bank = Arc::new(new_from_parent(&bank));
            }

            bank.freeze();
            bank.squash();
            bank.force_flush_accounts_cache();
            let hash = bank.update_accounts_hash_for_tests();
            bank.clean_accounts_for_tests();
            assert_eq!(bank.update_accounts_hash_for_tests(), hash);

            let bank0 = Arc::new(new_from_parent(&bank));
            let blockhash = bank.last_blockhash();
            let keypair = Keypair::new();
            let tx =
                system_transaction::transfer(&mint_keypair, &keypair.pubkey(), amount, blockhash);
            bank0.process_transaction(&tx).unwrap();

            let bank1 = Arc::new(new_from_parent(&bank0));
            let pubkey = solana_sdk::pubkey::new_rand();
            let blockhash = bank.last_blockhash();
            let tx = system_transaction::transfer(&keypair, &pubkey, amount, blockhash);
            bank1.process_transaction(&tx).unwrap();

            assert_eq!(
                bank0.get_account(&keypair.pubkey()).unwrap().lamports(),
                amount
            );
            assert_eq!(bank1.get_account(&keypair.pubkey()), None);

            info!("bank0 purge");
            let hash = bank0.update_accounts_hash_for_tests();
            bank0.clean_accounts_for_tests();
            assert_eq!(bank0.update_accounts_hash_for_tests(), hash);

            assert_eq!(
                bank0.get_account(&keypair.pubkey()).unwrap().lamports(),
                amount
            );
            assert_eq!(bank1.get_account(&keypair.pubkey()), None);

            info!("bank1 purge");
            bank1.clean_accounts_for_tests();

            assert_eq!(
                bank0.get_account(&keypair.pubkey()).unwrap().lamports(),
                amount
            );
            assert_eq!(bank1.get_account(&keypair.pubkey()), None);

            if pass == 0 {
                add_root_and_flush_write_cache(&bank0);
                assert!(bank0.verify_bank_hash(VerifyBankHash::default_for_test()));
                continue;
            }

            // Squash and then verify hash_internal value
            bank0.freeze();
            bank0.squash();
            add_root_and_flush_write_cache(&bank0);
            if pass == 1 {
                assert!(bank0.verify_bank_hash(VerifyBankHash::default_for_test()));
                continue;
            }

            bank1.freeze();
            bank1.squash();
            add_root_and_flush_write_cache(&bank1);
            bank1.update_accounts_hash_for_tests();
            assert!(bank1.verify_bank_hash(VerifyBankHash::default_for_test()));

            // keypair should have 0 tokens on both forks
            assert_eq!(bank0.get_account(&keypair.pubkey()), None);
            assert_eq!(bank1.get_account(&keypair.pubkey()), None);

            bank1.clean_accounts_for_tests();

            assert!(bank1.verify_bank_hash(VerifyBankHash::default_for_test()));
        }
    }

    #[test]
    fn test_two_payments_to_one_party() {
        let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.));
        let pubkey = solana_sdk::pubkey::new_rand();
        let bank = Bank::new_for_tests(&genesis_config);
        let amount = genesis_config.rent.minimum_balance(0);
        assert_eq!(bank.last_blockhash(), genesis_config.hash());

        bank.transfer(amount, &mint_keypair, &pubkey).unwrap();
        assert_eq!(bank.get_balance(&pubkey), amount);

        bank.transfer(amount * 2, &mint_keypair, &pubkey).unwrap();
        assert_eq!(bank.get_balance(&pubkey), amount * 3);
        assert_eq!(bank.transaction_count(), 2);
        assert_eq!(bank.non_vote_transaction_count_since_restart(), 2);
    }

    #[test]
    fn test_one_source_two_tx_one_batch() {
        let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.));
        let key1 = solana_sdk::pubkey::new_rand();
        let key2 = solana_sdk::pubkey::new_rand();
        let bank = Bank::new_for_tests(&genesis_config);
        let amount = genesis_config.rent.minimum_balance(0);
        assert_eq!(bank.last_blockhash(), genesis_config.hash());

        let t1 = system_transaction::transfer(&mint_keypair, &key1, amount, genesis_config.hash());
        let t2 = system_transaction::transfer(&mint_keypair, &key2, amount, genesis_config.hash());
        let txs = vec![t1.clone(), t2.clone()];
        let res = bank.process_transactions(txs.iter());

        assert_eq!(res.len(), 2);
        assert_eq!(res[0], Ok(()));
        assert_eq!(res[1], Err(TransactionError::AccountInUse));
        assert_eq!(
            bank.get_balance(&mint_keypair.pubkey()),
            sol_to_lamports(1.) - amount
        );
        assert_eq!(bank.get_balance(&key1), amount);
        assert_eq!(bank.get_balance(&key2), 0);
        assert_eq!(bank.get_signature_status(&t1.signatures[0]), Some(Ok(())));
        // TODO: Transactions that fail to pay a fee could be dropped silently.
        // Non-instruction errors don't get logged in the signature cache
        assert_eq!(bank.get_signature_status(&t2.signatures[0]), None);
    }

    #[test]
    fn test_one_tx_two_out_atomic_fail() {
        let amount = sol_to_lamports(1.);
        let (genesis_config, mint_keypair) = create_genesis_config(amount);
        let key1 = solana_sdk::pubkey::new_rand();
        let key2 = solana_sdk::pubkey::new_rand();
        let bank = Bank::new_for_tests(&genesis_config);
        let instructions = system_instruction::transfer_many(
            &mint_keypair.pubkey(),
            &[(key1, amount), (key2, amount)],
        );
        let message = Message::new(&instructions, Some(&mint_keypair.pubkey()));
        let tx = Transaction::new(&[&mint_keypair], message, genesis_config.hash());
        assert_eq!(
            bank.process_transaction(&tx).unwrap_err(),
            TransactionError::InstructionError(1, SystemError::ResultWithNegativeLamports.into())
        );
        assert_eq!(bank.get_balance(&mint_keypair.pubkey()), amount);
        assert_eq!(bank.get_balance(&key1), 0);
        assert_eq!(bank.get_balance(&key2), 0);
    }

    #[test]
    fn test_one_tx_two_out_atomic_pass() {
        let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.));
        let key1 = solana_sdk::pubkey::new_rand();
        let key2 = solana_sdk::pubkey::new_rand();
        let bank = Bank::new_for_tests(&genesis_config);
        let amount = genesis_config.rent.minimum_balance(0);
        let instructions = system_instruction::transfer_many(
            &mint_keypair.pubkey(),
            &[(key1, amount), (key2, amount)],
        );
        let message = Message::new(&instructions, Some(&mint_keypair.pubkey()));
        let tx = Transaction::new(&[&mint_keypair], message, genesis_config.hash());
        bank.process_transaction(&tx).unwrap();
        assert_eq!(
            bank.get_balance(&mint_keypair.pubkey()),
            sol_to_lamports(1.) - (2 * amount)
        );
        assert_eq!(bank.get_balance(&key1), amount);
        assert_eq!(bank.get_balance(&key2), amount);
    }

    // This test demonstrates that fees are paid even when a program fails.
    #[test]
    fn test_detect_failed_duplicate_transactions() {
        let (mut genesis_config, mint_keypair) = create_genesis_config(10_000);
        genesis_config.fee_rate_governor = FeeRateGovernor::new(5_000, 0);
        let bank = Bank::new_for_tests(&genesis_config);

        let dest = Keypair::new();

        // source with 0 program context
        let tx = system_transaction::transfer(
            &mint_keypair,
            &dest.pubkey(),
            10_000,
            genesis_config.hash(),
        );
        let signature = tx.signatures[0];
        assert!(!bank.has_signature(&signature));

        assert_eq!(
            bank.process_transaction(&tx),
            Err(TransactionError::InstructionError(
                0,
                SystemError::ResultWithNegativeLamports.into(),
            ))
        );

        // The lamports didn't move, but the from address paid the transaction fee.
        assert_eq!(bank.get_balance(&dest.pubkey()), 0);

        // This should be the original balance minus the transaction fee.
        assert_eq!(bank.get_balance(&mint_keypair.pubkey()), 5000);
    }

    #[test]
    fn test_account_not_found() {
        solana_logger::setup();
        let (genesis_config, mint_keypair) = create_genesis_config(0);
        let bank = Bank::new_for_tests(&genesis_config);
        let keypair = Keypair::new();
        assert_eq!(
            bank.transfer(
                genesis_config.rent.minimum_balance(0),
                &keypair,
                &mint_keypair.pubkey()
            ),
            Err(TransactionError::AccountNotFound)
        );
        assert_eq!(bank.transaction_count(), 0);
        assert_eq!(bank.non_vote_transaction_count_since_restart(), 0);
    }

    #[test]
    fn test_insufficient_funds() {
        let mint_amount = sol_to_lamports(1.);
        let (genesis_config, mint_keypair) = create_genesis_config(mint_amount);
        let bank = Bank::new_for_tests(&genesis_config);
        let pubkey = solana_sdk::pubkey::new_rand();
        let amount = genesis_config.rent.minimum_balance(0);
        bank.transfer(amount, &mint_keypair, &pubkey).unwrap();
        assert_eq!(bank.transaction_count(), 1);
        assert_eq!(bank.non_vote_transaction_count_since_restart(), 1);
        assert_eq!(bank.get_balance(&pubkey), amount);
        assert_eq!(
            bank.transfer((mint_amount - amount) + 1, &mint_keypair, &pubkey),
            Err(TransactionError::InstructionError(
                0,
                SystemError::ResultWithNegativeLamports.into(),
            ))
        );
        assert_eq!(bank.transaction_count(), 1);
        assert_eq!(bank.non_vote_transaction_count_since_restart(), 1);

        let mint_pubkey = mint_keypair.pubkey();
        assert_eq!(bank.get_balance(&mint_pubkey), mint_amount - amount);
        assert_eq!(bank.get_balance(&pubkey), amount);
    }

    #[test]
    fn test_executed_transaction_count_pre_bank_transaction_count_fix() {
        let mint_amount = sol_to_lamports(1.);
        let (genesis_config, mint_keypair) = create_genesis_config(mint_amount);
        let mut bank = Bank::new_for_tests(&genesis_config);
        bank.deactivate_feature(&feature_set::bank_transaction_count_fix::id());
        let pubkey = solana_sdk::pubkey::new_rand();
        let amount = genesis_config.rent.minimum_balance(0);
        bank.transfer(amount, &mint_keypair, &pubkey).unwrap();
        assert_eq!(
            bank.transfer((mint_amount - amount) + 1, &mint_keypair, &pubkey),
            Err(TransactionError::InstructionError(
                0,
                SystemError::ResultWithNegativeLamports.into(),
            ))
        );

        // Without bank_transaction_count_fix, transaction_count should include only the successful
        // transactions, but executed_transaction_count include all always
        assert_eq!(bank.transaction_count(), 1);
        assert_eq!(bank.executed_transaction_count(), 2);
        assert_eq!(bank.transaction_error_count(), 1);

        let bank = Arc::new(bank);
        let bank2 = Bank::new_from_parent(
            &bank,
            &Pubkey::default(),
            genesis_config.epoch_schedule.first_normal_slot,
        );

        assert_eq!(
            bank2.transfer((mint_amount - amount) + 2, &mint_keypair, &pubkey),
            Err(TransactionError::InstructionError(
                0,
                SystemError::ResultWithNegativeLamports.into(),
            ))
        );

        // The transaction_count inherited from parent bank is still 1 as it does
        // not include the failed ones!
        assert_eq!(bank2.transaction_count(), 1);
        assert_eq!(bank2.executed_transaction_count(), 1);
        assert_eq!(bank2.transaction_error_count(), 1);
    }

    #[test]
    fn test_executed_transaction_count_post_bank_transaction_count_fix() {
        let mint_amount = sol_to_lamports(1.);
        let (genesis_config, mint_keypair) = create_genesis_config(mint_amount);
        let mut bank = Bank::new_for_tests(&genesis_config);
        bank.activate_feature(&feature_set::bank_transaction_count_fix::id());
        let pubkey = solana_sdk::pubkey::new_rand();
        let amount = genesis_config.rent.minimum_balance(0);
        bank.transfer(amount, &mint_keypair, &pubkey).unwrap();
        assert_eq!(
            bank.transfer((mint_amount - amount) + 1, &mint_keypair, &pubkey),
            Err(TransactionError::InstructionError(
                0,
                SystemError::ResultWithNegativeLamports.into(),
            ))
        );

        // With bank_transaction_count_fix, transaction_count should include both the successful and
        // failed transactions.
        assert_eq!(bank.transaction_count(), 2);
        assert_eq!(bank.executed_transaction_count(), 2);
        assert_eq!(bank.transaction_error_count(), 1);

        let bank = Arc::new(bank);
        let bank2 = Bank::new_from_parent(
            &bank,
            &Pubkey::default(),
            genesis_config.epoch_schedule.first_normal_slot,
        );

        assert_eq!(
            bank2.transfer((mint_amount - amount) + 2, &mint_keypair, &pubkey),
            Err(TransactionError::InstructionError(
                0,
                SystemError::ResultWithNegativeLamports.into(),
            ))
        );

        // The transaction_count inherited from parent bank is 3: 2 from the parent bank and 1 at this bank2
        assert_eq!(bank2.transaction_count(), 3);
        assert_eq!(bank2.executed_transaction_count(), 1);
        assert_eq!(bank2.transaction_error_count(), 1);
    }

    #[test]
    fn test_transfer_to_newb() {
        solana_logger::setup();
        let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.));
        let bank = Bank::new_for_tests(&genesis_config);
        let amount = genesis_config.rent.minimum_balance(0);
        let pubkey = solana_sdk::pubkey::new_rand();
        bank.transfer(amount, &mint_keypair, &pubkey).unwrap();
        assert_eq!(bank.get_balance(&pubkey), amount);
    }

    #[test]
    fn test_transfer_to_sysvar() {
        solana_logger::setup();
        let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.));
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));
        let amount = genesis_config.rent.minimum_balance(0);

        let normal_pubkey = solana_sdk::pubkey::new_rand();
        let sysvar_pubkey = sysvar::clock::id();
        assert_eq!(bank.get_balance(&normal_pubkey), 0);
        assert_eq!(bank.get_balance(&sysvar_pubkey), 1_169_280);

        bank.transfer(amount, &mint_keypair, &normal_pubkey)
            .unwrap();
        bank.transfer(amount, &mint_keypair, &sysvar_pubkey)
            .unwrap_err();
        assert_eq!(bank.get_balance(&normal_pubkey), amount);
        assert_eq!(bank.get_balance(&sysvar_pubkey), 1_169_280);

        let bank = Arc::new(new_from_parent(&bank));
        assert_eq!(bank.get_balance(&normal_pubkey), amount);
        assert_eq!(bank.get_balance(&sysvar_pubkey), 1_169_280);
    }

    #[test]
    fn test_bank_deposit() {
        let bank = create_simple_test_bank(100);

        // Test new account
        let key = solana_sdk::pubkey::new_rand();
        let new_balance = bank.deposit(&key, 10).unwrap();
        assert_eq!(new_balance, 10);
        assert_eq!(bank.get_balance(&key), 10);

        // Existing account
        let new_balance = bank.deposit(&key, 3).unwrap();
        assert_eq!(new_balance, 13);
        assert_eq!(bank.get_balance(&key), 13);
    }

    #[test]
    fn test_bank_withdraw() {
        let bank = create_simple_test_bank(100);

        // Test no account
        let key = solana_sdk::pubkey::new_rand();
        assert_eq!(
            bank.withdraw(&key, 10),
            Err(TransactionError::AccountNotFound)
        );

        bank.deposit(&key, 3).unwrap();
        assert_eq!(bank.get_balance(&key), 3);

        // Low balance
        assert_eq!(
            bank.withdraw(&key, 10),
            Err(TransactionError::InsufficientFundsForFee)
        );

        // Enough balance
        assert_eq!(bank.withdraw(&key, 2), Ok(()));
        assert_eq!(bank.get_balance(&key), 1);
    }

    #[test]
    fn test_bank_withdraw_from_nonce_account() {
        let (mut genesis_config, _mint_keypair) = create_genesis_config(100_000);
        genesis_config.rent.lamports_per_byte_year = 42;
        let bank = Bank::new_for_tests(&genesis_config);

        let min_balance = bank.get_minimum_balance_for_rent_exemption(nonce::State::size());
        let nonce = Keypair::new();
        let nonce_account = AccountSharedData::new_data(
            min_balance + 42,
            &nonce::state::Versions::new(nonce::State::Initialized(nonce::state::Data::default())),
            &system_program::id(),
        )
        .unwrap();
        bank.store_account(&nonce.pubkey(), &nonce_account);
        assert_eq!(bank.get_balance(&nonce.pubkey()), min_balance + 42);

        // Resulting in non-zero, but sub-min_balance balance fails
        assert_eq!(
            bank.withdraw(&nonce.pubkey(), min_balance / 2),
            Err(TransactionError::InsufficientFundsForFee)
        );
        assert_eq!(bank.get_balance(&nonce.pubkey()), min_balance + 42);

        // Resulting in exactly rent-exempt balance succeeds
        bank.withdraw(&nonce.pubkey(), 42).unwrap();
        assert_eq!(bank.get_balance(&nonce.pubkey()), min_balance);

        // Account closure fails
        assert_eq!(
            bank.withdraw(&nonce.pubkey(), min_balance),
            Err(TransactionError::InsufficientFundsForFee),
        );
    }

    #[test]
    fn test_bank_tx_fee() {
        solana_logger::setup();

        let arbitrary_transfer_amount = 42_000;
        let mint = arbitrary_transfer_amount * 100;
        let leader = solana_sdk::pubkey::new_rand();
        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config_with_leader(mint, &leader, 3);
        genesis_config.fee_rate_governor = FeeRateGovernor::new(5000, 0); // something divisible by 2

        let expected_fee_paid = genesis_config
            .fee_rate_governor
            .create_fee_calculator()
            .lamports_per_signature;
        let (expected_fee_collected, expected_fee_burned) =
            genesis_config.fee_rate_governor.burn(expected_fee_paid);

        let mut bank = Bank::new_for_tests(&genesis_config);

        let capitalization = bank.capitalization();

        let key = solana_sdk::pubkey::new_rand();
        let tx = system_transaction::transfer(
            &mint_keypair,
            &key,
            arbitrary_transfer_amount,
            bank.last_blockhash(),
        );

        let initial_balance = bank.get_balance(&leader);
        assert_eq!(bank.process_transaction(&tx), Ok(()));
        assert_eq!(bank.get_balance(&key), arbitrary_transfer_amount);
        assert_eq!(
            bank.get_balance(&mint_keypair.pubkey()),
            mint - arbitrary_transfer_amount - expected_fee_paid
        );

        assert_eq!(bank.get_balance(&leader), initial_balance);
        goto_end_of_slot(&mut bank);
        assert_eq!(bank.signature_count(), 1);
        assert_eq!(
            bank.get_balance(&leader),
            initial_balance + expected_fee_collected
        ); // Leader collects fee after the bank is frozen

        // verify capitalization
        let sysvar_and_builtin_program_delta = 1;
        assert_eq!(
            capitalization - expected_fee_burned + sysvar_and_builtin_program_delta,
            bank.capitalization()
        );

        assert_eq!(
            *bank.rewards.read().unwrap(),
            vec![(
                leader,
                RewardInfo {
                    reward_type: RewardType::Fee,
                    lamports: expected_fee_collected as i64,
                    post_balance: initial_balance + expected_fee_collected,
                    commission: None,
                }
            )]
        );

        // Verify that an InstructionError collects fees, too
        let mut bank = Bank::new_from_parent(&Arc::new(bank), &leader, 1);
        let mut tx = system_transaction::transfer(&mint_keypair, &key, 1, bank.last_blockhash());
        // Create a bogus instruction to system_program to cause an instruction error
        tx.message.instructions[0].data[0] = 40;

        bank.process_transaction(&tx)
            .expect_err("instruction error");
        assert_eq!(bank.get_balance(&key), arbitrary_transfer_amount); // no change
        assert_eq!(
            bank.get_balance(&mint_keypair.pubkey()),
            mint - arbitrary_transfer_amount - 2 * expected_fee_paid
        ); // mint_keypair still pays a fee
        goto_end_of_slot(&mut bank);
        assert_eq!(bank.signature_count(), 1);

        // Profit! 2 transaction signatures processed at 3 lamports each
        assert_eq!(
            bank.get_balance(&leader),
            initial_balance + 2 * expected_fee_collected
        );

        assert_eq!(
            *bank.rewards.read().unwrap(),
            vec![(
                leader,
                RewardInfo {
                    reward_type: RewardType::Fee,
                    lamports: expected_fee_collected as i64,
                    post_balance: initial_balance + 2 * expected_fee_collected,
                    commission: None,
                }
            )]
        );
    }

    #[test]
    fn test_bank_tx_compute_unit_fee() {
        solana_logger::setup();

        let key = solana_sdk::pubkey::new_rand();
        let arbitrary_transfer_amount = 42;
        let mint = arbitrary_transfer_amount * 10_000_000;
        let leader = solana_sdk::pubkey::new_rand();
        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config_with_leader(mint, &leader, 3);
        genesis_config.fee_rate_governor = FeeRateGovernor::new(4, 0); // something divisible by 2

        let expected_fee_paid = Bank::calculate_fee(
            &SanitizedMessage::try_from(Message::new(&[], Some(&Pubkey::new_unique()))).unwrap(),
            genesis_config
                .fee_rate_governor
                .create_fee_calculator()
                .lamports_per_signature,
            &FeeStructure::default(),
            true,
            false,
            true,
            compute_budget::LoadedAccountsDataLimitType::V0,
        );

        let (expected_fee_collected, expected_fee_burned) =
            genesis_config.fee_rate_governor.burn(expected_fee_paid);

        let mut bank = Bank::new_for_tests(&genesis_config);

        let capitalization = bank.capitalization();

        let tx = system_transaction::transfer(
            &mint_keypair,
            &key,
            arbitrary_transfer_amount,
            bank.last_blockhash(),
        );

        let initial_balance = bank.get_balance(&leader);
        assert_eq!(bank.process_transaction(&tx), Ok(()));
        assert_eq!(bank.get_balance(&key), arbitrary_transfer_amount);
        assert_eq!(
            bank.get_balance(&mint_keypair.pubkey()),
            mint - arbitrary_transfer_amount - expected_fee_paid
        );

        assert_eq!(bank.get_balance(&leader), initial_balance);
        goto_end_of_slot(&mut bank);
        assert_eq!(bank.signature_count(), 1);
        assert_eq!(
            bank.get_balance(&leader),
            initial_balance + expected_fee_collected
        ); // Leader collects fee after the bank is frozen

        // verify capitalization
        let sysvar_and_builtin_program_delta = 1;
        assert_eq!(
            capitalization - expected_fee_burned + sysvar_and_builtin_program_delta,
            bank.capitalization()
        );

        assert_eq!(
            *bank.rewards.read().unwrap(),
            vec![(
                leader,
                RewardInfo {
                    reward_type: RewardType::Fee,
                    lamports: expected_fee_collected as i64,
                    post_balance: initial_balance + expected_fee_collected,
                    commission: None,
                }
            )]
        );

        // Verify that an InstructionError collects fees, too
        let mut bank = Bank::new_from_parent(&Arc::new(bank), &leader, 1);
        let mut tx = system_transaction::transfer(&mint_keypair, &key, 1, bank.last_blockhash());
        // Create a bogus instruction to system_program to cause an instruction error
        tx.message.instructions[0].data[0] = 40;

        bank.process_transaction(&tx)
            .expect_err("instruction error");
        assert_eq!(bank.get_balance(&key), arbitrary_transfer_amount); // no change
        assert_eq!(
            bank.get_balance(&mint_keypair.pubkey()),
            mint - arbitrary_transfer_amount - 2 * expected_fee_paid
        ); // mint_keypair still pays a fee
        goto_end_of_slot(&mut bank);
        assert_eq!(bank.signature_count(), 1);

        // Profit! 2 transaction signatures processed at 3 lamports each
        assert_eq!(
            bank.get_balance(&leader),
            initial_balance + 2 * expected_fee_collected
        );

        assert_eq!(
            *bank.rewards.read().unwrap(),
            vec![(
                leader,
                RewardInfo {
                    reward_type: RewardType::Fee,
                    lamports: expected_fee_collected as i64,
                    post_balance: initial_balance + 2 * expected_fee_collected,
                    commission: None,
                }
            )]
        );
    }

    #[test]
    fn test_bank_blockhash_fee_structure() {
        //solana_logger::setup();

        let leader = solana_sdk::pubkey::new_rand();
        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config_with_leader(1_000_000, &leader, 3);
        genesis_config
            .fee_rate_governor
            .target_lamports_per_signature = 5000;
        genesis_config.fee_rate_governor.target_signatures_per_slot = 0;

        let mut bank = Bank::new_for_tests(&genesis_config);
        goto_end_of_slot(&mut bank);
        let cheap_blockhash = bank.last_blockhash();
        let cheap_lamports_per_signature = bank.get_lamports_per_signature();
        assert_eq!(cheap_lamports_per_signature, 0);

        let mut bank = Bank::new_from_parent(&Arc::new(bank), &leader, 1);
        goto_end_of_slot(&mut bank);
        let expensive_blockhash = bank.last_blockhash();
        let expensive_lamports_per_signature = bank.get_lamports_per_signature();
        assert!(cheap_lamports_per_signature < expensive_lamports_per_signature);

        let bank = Bank::new_from_parent(&Arc::new(bank), &leader, 2);

        // Send a transfer using cheap_blockhash
        let key = solana_sdk::pubkey::new_rand();
        let initial_mint_balance = bank.get_balance(&mint_keypair.pubkey());
        let tx = system_transaction::transfer(&mint_keypair, &key, 1, cheap_blockhash);
        assert_eq!(bank.process_transaction(&tx), Ok(()));
        assert_eq!(bank.get_balance(&key), 1);
        assert_eq!(
            bank.get_balance(&mint_keypair.pubkey()),
            initial_mint_balance - 1 - cheap_lamports_per_signature
        );

        // Send a transfer using expensive_blockhash
        let key = solana_sdk::pubkey::new_rand();
        let initial_mint_balance = bank.get_balance(&mint_keypair.pubkey());
        let tx = system_transaction::transfer(&mint_keypair, &key, 1, expensive_blockhash);
        assert_eq!(bank.process_transaction(&tx), Ok(()));
        assert_eq!(bank.get_balance(&key), 1);
        assert_eq!(
            bank.get_balance(&mint_keypair.pubkey()),
            initial_mint_balance - 1 - expensive_lamports_per_signature
        );
    }

    #[test]
    fn test_bank_blockhash_compute_unit_fee_structure() {
        //solana_logger::setup();

        let leader = solana_sdk::pubkey::new_rand();
        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config_with_leader(1_000_000_000, &leader, 3);
        genesis_config
            .fee_rate_governor
            .target_lamports_per_signature = 1000;
        genesis_config.fee_rate_governor.target_signatures_per_slot = 1;

        let mut bank = Bank::new_for_tests(&genesis_config);
        goto_end_of_slot(&mut bank);
        let cheap_blockhash = bank.last_blockhash();
        let cheap_lamports_per_signature = bank.get_lamports_per_signature();
        assert_eq!(cheap_lamports_per_signature, 0);

        let mut bank = Bank::new_from_parent(&Arc::new(bank), &leader, 1);
        goto_end_of_slot(&mut bank);
        let expensive_blockhash = bank.last_blockhash();
        let expensive_lamports_per_signature = bank.get_lamports_per_signature();
        assert!(cheap_lamports_per_signature < expensive_lamports_per_signature);

        let bank = Bank::new_from_parent(&Arc::new(bank), &leader, 2);

        // Send a transfer using cheap_blockhash
        let key = solana_sdk::pubkey::new_rand();
        let initial_mint_balance = bank.get_balance(&mint_keypair.pubkey());
        let tx = system_transaction::transfer(&mint_keypair, &key, 1, cheap_blockhash);
        assert_eq!(bank.process_transaction(&tx), Ok(()));
        assert_eq!(bank.get_balance(&key), 1);
        let cheap_fee = Bank::calculate_fee(
            &SanitizedMessage::try_from(Message::new(&[], Some(&Pubkey::new_unique()))).unwrap(),
            cheap_lamports_per_signature,
            &FeeStructure::default(),
            true,
            false,
            true,
            compute_budget::LoadedAccountsDataLimitType::V0,
        );
        assert_eq!(
            bank.get_balance(&mint_keypair.pubkey()),
            initial_mint_balance - 1 - cheap_fee
        );

        // Send a transfer using expensive_blockhash
        let key = solana_sdk::pubkey::new_rand();
        let initial_mint_balance = bank.get_balance(&mint_keypair.pubkey());
        let tx = system_transaction::transfer(&mint_keypair, &key, 1, expensive_blockhash);
        assert_eq!(bank.process_transaction(&tx), Ok(()));
        assert_eq!(bank.get_balance(&key), 1);
        let expensive_fee = Bank::calculate_fee(
            &SanitizedMessage::try_from(Message::new(&[], Some(&Pubkey::new_unique()))).unwrap(),
            expensive_lamports_per_signature,
            &FeeStructure::default(),
            true,
            false,
            true,
            compute_budget::LoadedAccountsDataLimitType::V0,
        );
        assert_eq!(
            bank.get_balance(&mint_keypair.pubkey()),
            initial_mint_balance - 1 - expensive_fee
        );
    }

    #[test]
    fn test_filter_program_errors_and_collect_fee() {
        let leader = solana_sdk::pubkey::new_rand();
        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config_with_leader(100_000, &leader, 3);
        genesis_config.fee_rate_governor = FeeRateGovernor::new(5000, 0);
        let bank = Bank::new_for_tests(&genesis_config);

        let key = solana_sdk::pubkey::new_rand();
        let tx1 = SanitizedTransaction::from_transaction_for_tests(system_transaction::transfer(
            &mint_keypair,
            &key,
            2,
            genesis_config.hash(),
        ));
        let tx2 = SanitizedTransaction::from_transaction_for_tests(system_transaction::transfer(
            &mint_keypair,
            &key,
            5,
            genesis_config.hash(),
        ));

        let results = vec![
            new_execution_result(Ok(()), None),
            new_execution_result(
                Err(TransactionError::InstructionError(
                    1,
                    SystemError::ResultWithNegativeLamports.into(),
                )),
                None,
            ),
        ];
        let initial_balance = bank.get_balance(&leader);

        let results = bank.filter_program_errors_and_collect_fee(&[tx1, tx2], &results);
        bank.freeze();
        assert_eq!(
            bank.get_balance(&leader),
            initial_balance
                + bank
                    .fee_rate_governor
                    .burn(bank.fee_rate_governor.lamports_per_signature * 2)
                    .0
        );
        assert_eq!(results[0], Ok(()));
        assert_eq!(results[1], Ok(()));
    }

    #[test]
    fn test_filter_program_errors_and_collect_compute_unit_fee() {
        let leader = solana_sdk::pubkey::new_rand();
        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config_with_leader(1000000, &leader, 3);
        genesis_config.fee_rate_governor = FeeRateGovernor::new(2, 0);
        let bank = Bank::new_for_tests(&genesis_config);

        let key = solana_sdk::pubkey::new_rand();
        let tx1 = SanitizedTransaction::from_transaction_for_tests(system_transaction::transfer(
            &mint_keypair,
            &key,
            2,
            genesis_config.hash(),
        ));
        let tx2 = SanitizedTransaction::from_transaction_for_tests(system_transaction::transfer(
            &mint_keypair,
            &key,
            5,
            genesis_config.hash(),
        ));

        let results = vec![
            new_execution_result(Ok(()), None),
            new_execution_result(
                Err(TransactionError::InstructionError(
                    1,
                    SystemError::ResultWithNegativeLamports.into(),
                )),
                None,
            ),
        ];
        let initial_balance = bank.get_balance(&leader);

        let results = bank.filter_program_errors_and_collect_fee(&[tx1, tx2], &results);
        bank.freeze();
        assert_eq!(
            bank.get_balance(&leader),
            initial_balance
                + bank
                    .fee_rate_governor
                    .burn(
                        Bank::calculate_fee(
                            &SanitizedMessage::try_from(Message::new(
                                &[],
                                Some(&Pubkey::new_unique())
                            ))
                            .unwrap(),
                            genesis_config
                                .fee_rate_governor
                                .create_fee_calculator()
                                .lamports_per_signature,
                            &FeeStructure::default(),
                            true,
                            false,
                            true,
                            compute_budget::LoadedAccountsDataLimitType::V0,
                        ) * 2
                    )
                    .0
        );
        assert_eq!(results[0], Ok(()));
        assert_eq!(results[1], Ok(()));
    }

    #[test]
    fn test_debits_before_credits() {
        let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(2.));
        let bank = Bank::new_for_tests(&genesis_config);
        let keypair = Keypair::new();
        let tx0 = system_transaction::transfer(
            &mint_keypair,
            &keypair.pubkey(),
            sol_to_lamports(2.),
            genesis_config.hash(),
        );
        let tx1 = system_transaction::transfer(
            &keypair,
            &mint_keypair.pubkey(),
            sol_to_lamports(1.),
            genesis_config.hash(),
        );
        let txs = vec![tx0, tx1];
        let results = bank.process_transactions(txs.iter());
        assert!(results[1].is_err());

        // Assert bad transactions aren't counted.
        assert_eq!(bank.transaction_count(), 1);
        assert_eq!(bank.non_vote_transaction_count_since_restart(), 1);
    }

    #[test]
    fn test_readonly_accounts() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config_with_leader(500, &solana_sdk::pubkey::new_rand(), 0);
        let bank = Bank::new_for_tests(&genesis_config);

        let vote_pubkey0 = solana_sdk::pubkey::new_rand();
        let vote_pubkey1 = solana_sdk::pubkey::new_rand();
        let vote_pubkey2 = solana_sdk::pubkey::new_rand();
        let authorized_voter = Keypair::new();
        let payer0 = Keypair::new();
        let payer1 = Keypair::new();

        // Create vote accounts
        let vote_account0 =
            vote_state::create_account(&vote_pubkey0, &authorized_voter.pubkey(), 0, 100);
        let vote_account1 =
            vote_state::create_account(&vote_pubkey1, &authorized_voter.pubkey(), 0, 100);
        let vote_account2 =
            vote_state::create_account(&vote_pubkey2, &authorized_voter.pubkey(), 0, 100);
        bank.store_account(&vote_pubkey0, &vote_account0);
        bank.store_account(&vote_pubkey1, &vote_account1);
        bank.store_account(&vote_pubkey2, &vote_account2);

        // Fund payers
        bank.transfer(10, &mint_keypair, &payer0.pubkey()).unwrap();
        bank.transfer(10, &mint_keypair, &payer1.pubkey()).unwrap();
        bank.transfer(1, &mint_keypair, &authorized_voter.pubkey())
            .unwrap();

        let vote = Vote::new(vec![1], Hash::default());
        let ix0 = vote_instruction::vote(&vote_pubkey0, &authorized_voter.pubkey(), vote.clone());
        let tx0 = Transaction::new_signed_with_payer(
            &[ix0],
            Some(&payer0.pubkey()),
            &[&payer0, &authorized_voter],
            bank.last_blockhash(),
        );
        let ix1 = vote_instruction::vote(&vote_pubkey1, &authorized_voter.pubkey(), vote.clone());
        let tx1 = Transaction::new_signed_with_payer(
            &[ix1],
            Some(&payer1.pubkey()),
            &[&payer1, &authorized_voter],
            bank.last_blockhash(),
        );
        let txs = vec![tx0, tx1];
        let results = bank.process_transactions(txs.iter());

        // If multiple transactions attempt to read the same account, they should succeed.
        // Vote authorized_voter and sysvar accounts are given read-only handling
        assert_eq!(results[0], Ok(()));
        assert_eq!(results[1], Ok(()));

        let ix0 = vote_instruction::vote(&vote_pubkey2, &authorized_voter.pubkey(), vote);
        let tx0 = Transaction::new_signed_with_payer(
            &[ix0],
            Some(&payer0.pubkey()),
            &[&payer0, &authorized_voter],
            bank.last_blockhash(),
        );
        let tx1 = system_transaction::transfer(
            &authorized_voter,
            &solana_sdk::pubkey::new_rand(),
            1,
            bank.last_blockhash(),
        );
        let txs = vec![tx0, tx1];
        let results = bank.process_transactions(txs.iter());
        // However, an account may not be locked as read-only and writable at the same time.
        assert_eq!(results[0], Ok(()));
        assert_eq!(results[1], Err(TransactionError::AccountInUse));
    }

    #[test]
    fn test_interleaving_locks() {
        let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.));
        let bank = Bank::new_for_tests(&genesis_config);
        let alice = Keypair::new();
        let bob = Keypair::new();
        let amount = genesis_config.rent.minimum_balance(0);

        let tx1 = system_transaction::transfer(
            &mint_keypair,
            &alice.pubkey(),
            amount,
            genesis_config.hash(),
        );
        let pay_alice = vec![tx1];

        let lock_result = bank.prepare_batch_for_tests(pay_alice);
        let results_alice = bank
            .load_execute_and_commit_transactions(
                &lock_result,
                MAX_PROCESSING_AGE,
                false,
                false,
                false,
                false,
                &mut ExecuteTimings::default(),
                None,
            )
            .0
            .fee_collection_results;
        assert_eq!(results_alice[0], Ok(()));

        // try executing an interleaved transfer twice
        assert_eq!(
            bank.transfer(amount, &mint_keypair, &bob.pubkey()),
            Err(TransactionError::AccountInUse)
        );
        // the second time should fail as well
        // this verifies that `unlock_accounts` doesn't unlock `AccountInUse` accounts
        assert_eq!(
            bank.transfer(amount, &mint_keypair, &bob.pubkey()),
            Err(TransactionError::AccountInUse)
        );

        drop(lock_result);

        assert!(bank
            .transfer(2 * amount, &mint_keypair, &bob.pubkey())
            .is_ok());
    }

    #[test]
    fn test_readonly_relaxed_locks() {
        let (genesis_config, _) = create_genesis_config(3);
        let bank = Bank::new_for_tests(&genesis_config);
        let key0 = Keypair::new();
        let key1 = Keypair::new();
        let key2 = Keypair::new();
        let key3 = solana_sdk::pubkey::new_rand();

        let message = Message {
            header: MessageHeader {
                num_required_signatures: 1,
                num_readonly_signed_accounts: 0,
                num_readonly_unsigned_accounts: 1,
            },
            account_keys: vec![key0.pubkey(), key3],
            recent_blockhash: Hash::default(),
            instructions: vec![],
        };
        let tx = Transaction::new(&[&key0], message, genesis_config.hash());
        let txs = vec![tx];

        let batch0 = bank.prepare_batch_for_tests(txs);
        assert!(batch0.lock_results()[0].is_ok());

        // Try locking accounts, locking a previously read-only account as writable
        // should fail
        let message = Message {
            header: MessageHeader {
                num_required_signatures: 1,
                num_readonly_signed_accounts: 0,
                num_readonly_unsigned_accounts: 0,
            },
            account_keys: vec![key1.pubkey(), key3],
            recent_blockhash: Hash::default(),
            instructions: vec![],
        };
        let tx = Transaction::new(&[&key1], message, genesis_config.hash());
        let txs = vec![tx];

        let batch1 = bank.prepare_batch_for_tests(txs);
        assert!(batch1.lock_results()[0].is_err());

        // Try locking a previously read-only account a 2nd time; should succeed
        let message = Message {
            header: MessageHeader {
                num_required_signatures: 1,
                num_readonly_signed_accounts: 0,
                num_readonly_unsigned_accounts: 1,
            },
            account_keys: vec![key2.pubkey(), key3],
            recent_blockhash: Hash::default(),
            instructions: vec![],
        };
        let tx = Transaction::new(&[&key2], message, genesis_config.hash());
        let txs = vec![tx];

        let batch2 = bank.prepare_batch_for_tests(txs);
        assert!(batch2.lock_results()[0].is_ok());
    }

    #[test]
    fn test_bank_invalid_account_index() {
        let (genesis_config, mint_keypair) = create_genesis_config(1);
        let keypair = Keypair::new();
        let bank = Bank::new_for_tests(&genesis_config);

        let tx = system_transaction::transfer(
            &mint_keypair,
            &keypair.pubkey(),
            1,
            genesis_config.hash(),
        );

        let mut tx_invalid_program_index = tx.clone();
        tx_invalid_program_index.message.instructions[0].program_id_index = 42;
        assert_eq!(
            bank.process_transaction(&tx_invalid_program_index),
            Err(TransactionError::SanitizeFailure)
        );

        let mut tx_invalid_account_index = tx;
        tx_invalid_account_index.message.instructions[0].accounts[0] = 42;
        assert_eq!(
            bank.process_transaction(&tx_invalid_account_index),
            Err(TransactionError::SanitizeFailure)
        );
    }

    #[test]
    fn test_bank_pay_to_self() {
        let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.));
        let key1 = Keypair::new();
        let bank = Bank::new_for_tests(&genesis_config);
        let amount = genesis_config.rent.minimum_balance(0);

        bank.transfer(amount, &mint_keypair, &key1.pubkey())
            .unwrap();
        assert_eq!(bank.get_balance(&key1.pubkey()), amount);
        let tx = system_transaction::transfer(&key1, &key1.pubkey(), amount, genesis_config.hash());
        let _res = bank.process_transaction(&tx);

        assert_eq!(bank.get_balance(&key1.pubkey()), amount);
        bank.get_signature_status(&tx.signatures[0])
            .unwrap()
            .unwrap();
    }

    fn new_from_parent(parent: &Arc<Bank>) -> Bank {
        Bank::new_from_parent(parent, &Pubkey::default(), parent.slot() + 1)
    }

    /// Verify that the parent's vector is computed correctly
    #[test]
    fn test_bank_parents() {
        let (genesis_config, _) = create_genesis_config(1);
        let parent = Arc::new(Bank::new_for_tests(&genesis_config));

        let bank = new_from_parent(&parent);
        assert!(Arc::ptr_eq(&bank.parents()[0], &parent));
    }

    /// Verifies that transactions are dropped if they have already been processed
    #[test]
    fn test_tx_already_processed() {
        let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.));
        let bank = Bank::new_for_tests(&genesis_config);

        let key1 = Keypair::new();
        let mut tx = system_transaction::transfer(
            &mint_keypair,
            &key1.pubkey(),
            genesis_config.rent.minimum_balance(0),
            genesis_config.hash(),
        );

        // First process `tx` so that the status cache is updated
        assert_eq!(bank.process_transaction(&tx), Ok(()));

        // Ensure that signature check works
        assert_eq!(
            bank.process_transaction(&tx),
            Err(TransactionError::AlreadyProcessed)
        );

        // Change transaction signature to simulate processing a transaction with a different signature
        // for the same message.
        tx.signatures[0] = Signature::default();

        // Ensure that message hash check works
        assert_eq!(
            bank.process_transaction(&tx),
            Err(TransactionError::AlreadyProcessed)
        );
    }

    /// Verifies that last ids and status cache are correctly referenced from parent
    #[test]
    fn test_bank_parent_already_processed() {
        let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.));
        let key1 = Keypair::new();
        let parent = Arc::new(Bank::new_for_tests(&genesis_config));
        let amount = genesis_config.rent.minimum_balance(0);

        let tx = system_transaction::transfer(
            &mint_keypair,
            &key1.pubkey(),
            amount,
            genesis_config.hash(),
        );
        assert_eq!(parent.process_transaction(&tx), Ok(()));
        let bank = new_from_parent(&parent);
        assert_eq!(
            bank.process_transaction(&tx),
            Err(TransactionError::AlreadyProcessed)
        );
    }

    /// Verifies that last ids and accounts are correctly referenced from parent
    #[test]
    fn test_bank_parent_account_spend() {
        let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.0));
        let key1 = Keypair::new();
        let key2 = Keypair::new();
        let parent = Arc::new(Bank::new_for_tests(&genesis_config));
        let amount = genesis_config.rent.minimum_balance(0);

        let tx = system_transaction::transfer(
            &mint_keypair,
            &key1.pubkey(),
            amount,
            genesis_config.hash(),
        );
        assert_eq!(parent.process_transaction(&tx), Ok(()));
        let bank = new_from_parent(&parent);
        let tx = system_transaction::transfer(&key1, &key2.pubkey(), amount, genesis_config.hash());
        assert_eq!(bank.process_transaction(&tx), Ok(()));
        assert_eq!(parent.get_signature_status(&tx.signatures[0]), None);
    }

    #[test]
    fn test_bank_hash_internal_state() {
        let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.));
        let bank0 = Bank::new_for_tests(&genesis_config);
        let bank1 = Bank::new_for_tests(&genesis_config);
        let amount = genesis_config.rent.minimum_balance(0);
        let initial_state = bank0.hash_internal_state();
        assert_eq!(bank1.hash_internal_state(), initial_state);

        let pubkey = solana_sdk::pubkey::new_rand();
        bank0.transfer(amount, &mint_keypair, &pubkey).unwrap();
        assert_ne!(bank0.hash_internal_state(), initial_state);
        bank1.transfer(amount, &mint_keypair, &pubkey).unwrap();
        assert_eq!(bank0.hash_internal_state(), bank1.hash_internal_state());

        // Checkpointing should always result in a new state
        let bank1 = Arc::new(bank1);
        let bank2 = new_from_parent(&bank1);
        assert_ne!(bank0.hash_internal_state(), bank2.hash_internal_state());

        let pubkey2 = solana_sdk::pubkey::new_rand();
        info!("transfer 2 {}", pubkey2);
        bank2.transfer(amount, &mint_keypair, &pubkey2).unwrap();
        add_root_and_flush_write_cache(&bank0);
        add_root_and_flush_write_cache(&bank1);
        add_root_and_flush_write_cache(&bank2);
        bank2.update_accounts_hash_for_tests();
        assert!(bank2.verify_bank_hash(VerifyBankHash::default_for_test()));
    }

    #[test]
    fn test_bank_hash_internal_state_verify() {
        for pass in 0..3 {
            solana_logger::setup();
            let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.));
            let bank0 = Bank::new_for_tests(&genesis_config);
            let amount = genesis_config.rent.minimum_balance(0);

            let pubkey = solana_sdk::pubkey::new_rand();
            info!("transfer 0 {} mint: {}", pubkey, mint_keypair.pubkey());
            bank0.transfer(amount, &mint_keypair, &pubkey).unwrap();

            let bank0_state = bank0.hash_internal_state();
            let bank0 = Arc::new(bank0);
            // Checkpointing should result in a new state while freezing the parent
            let bank2 = Bank::new_from_parent(&bank0, &solana_sdk::pubkey::new_rand(), 1);
            assert_ne!(bank0_state, bank2.hash_internal_state());
            // Checkpointing should modify the checkpoint's state when freezed
            assert_ne!(bank0_state, bank0.hash_internal_state());

            // Checkpointing should never modify the checkpoint's state once frozen
            add_root_and_flush_write_cache(&bank0);
            let bank0_state = bank0.hash_internal_state();
            if pass == 0 {
                // we later modify bank 2, so this flush is destructive to the test
                add_root_and_flush_write_cache(&bank2);
                bank2.update_accounts_hash_for_tests();
                assert!(bank2.verify_bank_hash(VerifyBankHash::default_for_test()));
            }
            let bank3 = Bank::new_from_parent(&bank0, &solana_sdk::pubkey::new_rand(), 2);
            assert_eq!(bank0_state, bank0.hash_internal_state());
            if pass == 0 {
                // this relies on us having set the bank hash in the pass==0 if above
                assert!(bank2.verify_bank_hash(VerifyBankHash::default_for_test()));
                continue;
            }
            if pass == 1 {
                // flushing slot 3 here causes us to mark it as a root. Marking it as a root
                // prevents us from marking slot 2 as a root later since slot 2 is < slot 3.
                // Doing so throws an assert. So, we can't flush 3 until 2 is flushed.
                add_root_and_flush_write_cache(&bank3);
                bank3.update_accounts_hash_for_tests();
                assert!(bank3.verify_bank_hash(VerifyBankHash::default_for_test()));
                continue;
            }

            let pubkey2 = solana_sdk::pubkey::new_rand();
            info!("transfer 2 {}", pubkey2);
            bank2.transfer(amount, &mint_keypair, &pubkey2).unwrap();
            add_root_and_flush_write_cache(&bank2);
            bank2.update_accounts_hash_for_tests();
            assert!(bank2.verify_bank_hash(VerifyBankHash::default_for_test()));
            add_root_and_flush_write_cache(&bank3);
            bank3.update_accounts_hash_for_tests();
            assert!(bank3.verify_bank_hash(VerifyBankHash::default_for_test()));
        }
    }

    #[test]
    #[should_panic(expected = "assertion failed: self.is_frozen()")]
    fn test_verify_hash_unfrozen() {
        let bank = create_simple_test_bank(2_000);
        assert!(bank.verify_hash());
    }

    #[test]
    fn test_verify_snapshot_bank() {
        solana_logger::setup();
        let pubkey = solana_sdk::pubkey::new_rand();
        let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.));
        let bank = Bank::new_for_tests(&genesis_config);
        bank.transfer(
            genesis_config.rent.minimum_balance(0),
            &mint_keypair,
            &pubkey,
        )
        .unwrap();
        bank.freeze();
        add_root_and_flush_write_cache(&bank);
        bank.update_accounts_hash_for_tests();
        assert!(bank.verify_snapshot_bank(true, false, bank.slot()));

        // tamper the bank after freeze!
        bank.increment_signature_count(1);
        assert!(!bank.verify_snapshot_bank(true, false, bank.slot()));
    }

    // Test that two bank forks with the same accounts should not hash to the same value.
    #[test]
    fn test_bank_hash_internal_state_same_account_different_fork() {
        solana_logger::setup();
        let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.));
        let amount = genesis_config.rent.minimum_balance(0);
        let bank0 = Arc::new(Bank::new_for_tests(&genesis_config));
        let initial_state = bank0.hash_internal_state();
        let bank1 = Bank::new_from_parent(&bank0, &Pubkey::default(), 1);
        assert_ne!(bank1.hash_internal_state(), initial_state);

        info!("transfer bank1");
        let pubkey = solana_sdk::pubkey::new_rand();
        bank1.transfer(amount, &mint_keypair, &pubkey).unwrap();
        assert_ne!(bank1.hash_internal_state(), initial_state);

        info!("transfer bank2");
        // bank2 should not hash the same as bank1
        let bank2 = Bank::new_from_parent(&bank0, &Pubkey::default(), 2);
        bank2.transfer(amount, &mint_keypair, &pubkey).unwrap();
        assert_ne!(bank2.hash_internal_state(), initial_state);
        assert_ne!(bank1.hash_internal_state(), bank2.hash_internal_state());
    }

    #[test]
    fn test_hash_internal_state_genesis() {
        let bank0 = Bank::new_for_tests(&create_genesis_config(10).0);
        let bank1 = Bank::new_for_tests(&create_genesis_config(20).0);
        assert_ne!(bank0.hash_internal_state(), bank1.hash_internal_state());
    }

    // See that the order of two transfers does not affect the result
    // of hash_internal_state
    #[test]
    fn test_hash_internal_state_order() {
        let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.));
        let amount = genesis_config.rent.minimum_balance(0);
        let bank0 = Bank::new_for_tests(&genesis_config);
        let bank1 = Bank::new_for_tests(&genesis_config);
        assert_eq!(bank0.hash_internal_state(), bank1.hash_internal_state());
        let key0 = solana_sdk::pubkey::new_rand();
        let key1 = solana_sdk::pubkey::new_rand();
        bank0.transfer(amount, &mint_keypair, &key0).unwrap();
        bank0.transfer(amount * 2, &mint_keypair, &key1).unwrap();

        bank1.transfer(amount * 2, &mint_keypair, &key1).unwrap();
        bank1.transfer(amount, &mint_keypair, &key0).unwrap();

        assert_eq!(bank0.hash_internal_state(), bank1.hash_internal_state());
    }

    #[test]
    fn test_hash_internal_state_error() {
        solana_logger::setup();
        let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.));
        let amount = genesis_config.rent.minimum_balance(0);
        let bank = Bank::new_for_tests(&genesis_config);
        let key0 = solana_sdk::pubkey::new_rand();
        bank.transfer(amount, &mint_keypair, &key0).unwrap();
        let orig = bank.hash_internal_state();

        // Transfer will error but still take a fee
        assert!(bank
            .transfer(sol_to_lamports(1.), &mint_keypair, &key0)
            .is_err());
        assert_ne!(orig, bank.hash_internal_state());

        let orig = bank.hash_internal_state();
        let empty_keypair = Keypair::new();
        assert!(bank.transfer(amount, &empty_keypair, &key0).is_err());
        assert_eq!(orig, bank.hash_internal_state());
    }

    #[test]
    fn test_bank_hash_internal_state_squash() {
        let collector_id = Pubkey::default();
        let bank0 = Arc::new(Bank::new_for_tests(&create_genesis_config(10).0));
        let hash0 = bank0.hash_internal_state();
        // save hash0 because new_from_parent
        // updates sysvar entries

        let bank1 = Bank::new_from_parent(&bank0, &collector_id, 1);

        // no delta in bank1, hashes should always update
        assert_ne!(hash0, bank1.hash_internal_state());

        // remove parent
        bank1.squash();
        assert!(bank1.parents().is_empty());
    }

    /// Verifies that last ids and accounts are correctly referenced from parent
    #[test]
    fn test_bank_squash() {
        solana_logger::setup();
        let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(2.));
        let key1 = Keypair::new();
        let key2 = Keypair::new();
        let parent = Arc::new(Bank::new_for_tests(&genesis_config));
        let amount = genesis_config.rent.minimum_balance(0);

        let tx_transfer_mint_to_1 = system_transaction::transfer(
            &mint_keypair,
            &key1.pubkey(),
            amount,
            genesis_config.hash(),
        );
        trace!("parent process tx ");
        assert_eq!(parent.process_transaction(&tx_transfer_mint_to_1), Ok(()));
        trace!("done parent process tx ");
        assert_eq!(parent.transaction_count(), 1);
        assert_eq!(parent.non_vote_transaction_count_since_restart(), 1);
        assert_eq!(
            parent.get_signature_status(&tx_transfer_mint_to_1.signatures[0]),
            Some(Ok(()))
        );

        trace!("new from parent");
        let bank = new_from_parent(&parent);
        trace!("done new from parent");
        assert_eq!(
            bank.get_signature_status(&tx_transfer_mint_to_1.signatures[0]),
            Some(Ok(()))
        );

        assert_eq!(bank.transaction_count(), parent.transaction_count());
        assert_eq!(
            bank.non_vote_transaction_count_since_restart(),
            parent.non_vote_transaction_count_since_restart()
        );
        let tx_transfer_1_to_2 =
            system_transaction::transfer(&key1, &key2.pubkey(), amount, genesis_config.hash());
        assert_eq!(bank.process_transaction(&tx_transfer_1_to_2), Ok(()));
        assert_eq!(bank.transaction_count(), 2);
        assert_eq!(bank.non_vote_transaction_count_since_restart(), 2);
        assert_eq!(parent.transaction_count(), 1);
        assert_eq!(parent.non_vote_transaction_count_since_restart(), 1);
        assert_eq!(
            parent.get_signature_status(&tx_transfer_1_to_2.signatures[0]),
            None
        );

        for _ in 0..3 {
            // first time these should match what happened above, assert that parents are ok
            assert_eq!(bank.get_balance(&key1.pubkey()), 0);
            assert_eq!(bank.get_account(&key1.pubkey()), None);
            assert_eq!(bank.get_balance(&key2.pubkey()), amount);
            trace!("start");
            assert_eq!(
                bank.get_signature_status(&tx_transfer_mint_to_1.signatures[0]),
                Some(Ok(()))
            );
            assert_eq!(
                bank.get_signature_status(&tx_transfer_1_to_2.signatures[0]),
                Some(Ok(()))
            );

            // works iteration 0, no-ops on iteration 1 and 2
            trace!("SQUASH");
            bank.squash();

            assert_eq!(parent.transaction_count(), 1);
            assert_eq!(parent.non_vote_transaction_count_since_restart(), 1);
            assert_eq!(bank.transaction_count(), 2);
            assert_eq!(bank.non_vote_transaction_count_since_restart(), 2);
        }
    }

    #[test]
    fn test_bank_get_account_in_parent_after_squash() {
        let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.));
        let parent = Arc::new(Bank::new_for_tests(&genesis_config));
        let amount = genesis_config.rent.minimum_balance(0);

        let key1 = Keypair::new();

        parent
            .transfer(amount, &mint_keypair, &key1.pubkey())
            .unwrap();
        assert_eq!(parent.get_balance(&key1.pubkey()), amount);
        let bank = new_from_parent(&parent);
        bank.squash();
        assert_eq!(parent.get_balance(&key1.pubkey()), amount);
    }

    #[test]
    fn test_bank_get_account_in_parent_after_squash2() {
        solana_logger::setup();
        let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.));
        let bank0 = Arc::new(Bank::new_for_tests(&genesis_config));
        let amount = genesis_config.rent.minimum_balance(0);

        let key1 = Keypair::new();

        bank0
            .transfer(amount, &mint_keypair, &key1.pubkey())
            .unwrap();
        assert_eq!(bank0.get_balance(&key1.pubkey()), amount);

        let bank1 = Arc::new(Bank::new_from_parent(&bank0, &Pubkey::default(), 1));
        bank1
            .transfer(3 * amount, &mint_keypair, &key1.pubkey())
            .unwrap();
        let bank2 = Arc::new(Bank::new_from_parent(&bank0, &Pubkey::default(), 2));
        bank2
            .transfer(2 * amount, &mint_keypair, &key1.pubkey())
            .unwrap();
        let bank3 = Arc::new(Bank::new_from_parent(&bank1, &Pubkey::default(), 3));
        bank1.squash();

        // This picks up the values from 1 which is the highest root:
        // TODO: if we need to access rooted banks older than this,
        // need to fix the lookup.
        assert_eq!(bank0.get_balance(&key1.pubkey()), 4 * amount);
        assert_eq!(bank3.get_balance(&key1.pubkey()), 4 * amount);
        assert_eq!(bank2.get_balance(&key1.pubkey()), 3 * amount);
        bank3.squash();
        assert_eq!(bank1.get_balance(&key1.pubkey()), 4 * amount);

        let bank4 = Arc::new(Bank::new_from_parent(&bank3, &Pubkey::default(), 4));
        bank4
            .transfer(4 * amount, &mint_keypair, &key1.pubkey())
            .unwrap();
        assert_eq!(bank4.get_balance(&key1.pubkey()), 8 * amount);
        assert_eq!(bank3.get_balance(&key1.pubkey()), 4 * amount);
        bank4.squash();
        let bank5 = Arc::new(Bank::new_from_parent(&bank4, &Pubkey::default(), 5));
        bank5.squash();
        let bank6 = Arc::new(Bank::new_from_parent(&bank5, &Pubkey::default(), 6));
        bank6.squash();

        // This picks up the values from 4 which is the highest root:
        // TODO: if we need to access rooted banks older than this,
        // need to fix the lookup.
        assert_eq!(bank3.get_balance(&key1.pubkey()), 8 * amount);
        assert_eq!(bank2.get_balance(&key1.pubkey()), 8 * amount);

        assert_eq!(bank4.get_balance(&key1.pubkey()), 8 * amount);
    }

    #[test]
    fn test_bank_get_account_modified_since_parent_with_fixed_root() {
        let pubkey = solana_sdk::pubkey::new_rand();

        let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.));
        let amount = genesis_config.rent.minimum_balance(0);
        let bank1 = Arc::new(Bank::new_for_tests(&genesis_config));
        bank1.transfer(amount, &mint_keypair, &pubkey).unwrap();
        let result = bank1.get_account_modified_since_parent_with_fixed_root(&pubkey);
        assert!(result.is_some());
        let (account, slot) = result.unwrap();
        assert_eq!(account.lamports(), amount);
        assert_eq!(slot, 0);

        let bank2 = Arc::new(Bank::new_from_parent(&bank1, &Pubkey::default(), 1));
        assert!(bank2
            .get_account_modified_since_parent_with_fixed_root(&pubkey)
            .is_none());
        bank2.transfer(2 * amount, &mint_keypair, &pubkey).unwrap();
        let result = bank1.get_account_modified_since_parent_with_fixed_root(&pubkey);
        assert!(result.is_some());
        let (account, slot) = result.unwrap();
        assert_eq!(account.lamports(), amount);
        assert_eq!(slot, 0);
        let result = bank2.get_account_modified_since_parent_with_fixed_root(&pubkey);
        assert!(result.is_some());
        let (account, slot) = result.unwrap();
        assert_eq!(account.lamports(), 3 * amount);
        assert_eq!(slot, 1);

        bank1.squash();

        let bank3 = Bank::new_from_parent(&bank2, &Pubkey::default(), 3);
        assert_eq!(
            None,
            bank3.get_account_modified_since_parent_with_fixed_root(&pubkey)
        );
    }

    #[test]
    fn test_bank_update_sysvar_account() {
        solana_logger::setup();
        // flushing the write cache is destructive, so test has to restart each time we flush and want to do 'illegal' operations once flushed
        for pass in 0..5 {
            use sysvar::clock::Clock;

            let dummy_clock_id = solana_sdk::pubkey::new_rand();
            let dummy_rent_epoch = 44;
            let (mut genesis_config, _mint_keypair) = create_genesis_config(500);

            let expected_previous_slot = 3;
            let mut expected_next_slot = expected_previous_slot + 1;

            // First, initialize the clock sysvar
            for feature_id in FeatureSet::default().inactive {
                activate_feature(&mut genesis_config, feature_id);
            }
            let bank1 = Arc::new(Bank::new_for_tests_with_config(
                &genesis_config,
                BankTestConfig::default(),
            ));
            if pass == 0 {
                add_root_and_flush_write_cache(&bank1);
                assert_eq!(bank1.calculate_capitalization(true), bank1.capitalization());
                continue;
            }

            assert_capitalization_diff(
                &bank1,
                || {
                    bank1.update_sysvar_account(&dummy_clock_id, |optional_account| {
                        assert!(optional_account.is_none());

                        let mut account = create_account(
                            &Clock {
                                slot: expected_previous_slot,
                                ..Clock::default()
                            },
                            bank1.inherit_specially_retained_account_fields(optional_account),
                        );
                        account.set_rent_epoch(dummy_rent_epoch);
                        account
                    });
                    let current_account = bank1.get_account(&dummy_clock_id).unwrap();
                    assert_eq!(
                        expected_previous_slot,
                        from_account::<Clock, _>(&current_account).unwrap().slot
                    );
                    assert_eq!(dummy_rent_epoch, current_account.rent_epoch());
                },
                |old, new| {
                    assert_eq!(
                        old + min_rent_exempt_balance_for_sysvars(&bank1, &[sysvar::clock::id()]),
                        new
                    );
                    pass == 1
                },
            );
            if pass == 1 {
                continue;
            }

            assert_capitalization_diff(
                &bank1,
                || {
                    bank1.update_sysvar_account(&dummy_clock_id, |optional_account| {
                        assert!(optional_account.is_some());

                        create_account(
                            &Clock {
                                slot: expected_previous_slot,
                                ..Clock::default()
                            },
                            bank1.inherit_specially_retained_account_fields(optional_account),
                        )
                    })
                },
                |old, new| {
                    // creating new sysvar twice in a slot shouldn't increment capitalization twice
                    assert_eq!(old, new);
                    pass == 2
                },
            );
            if pass == 2 {
                continue;
            }

            // Updating should increment the clock's slot
            let bank2 = Arc::new(Bank::new_from_parent(&bank1, &Pubkey::default(), 1));
            add_root_and_flush_write_cache(&bank1);
            assert_capitalization_diff(
                &bank2,
                || {
                    bank2.update_sysvar_account(&dummy_clock_id, |optional_account| {
                        let slot = from_account::<Clock, _>(optional_account.as_ref().unwrap())
                            .unwrap()
                            .slot
                            + 1;

                        create_account(
                            &Clock {
                                slot,
                                ..Clock::default()
                            },
                            bank2.inherit_specially_retained_account_fields(optional_account),
                        )
                    });
                    let current_account = bank2.get_account(&dummy_clock_id).unwrap();
                    assert_eq!(
                        expected_next_slot,
                        from_account::<Clock, _>(&current_account).unwrap().slot
                    );
                    assert_eq!(dummy_rent_epoch, current_account.rent_epoch());
                },
                |old, new| {
                    // if existing, capitalization shouldn't change
                    assert_eq!(old, new);
                    pass == 3
                },
            );
            if pass == 3 {
                continue;
            }

            // Updating again should give bank2's sysvar to the closure not bank1's.
            // Thus, increment expected_next_slot accordingly
            expected_next_slot += 1;
            assert_capitalization_diff(
                &bank2,
                || {
                    bank2.update_sysvar_account(&dummy_clock_id, |optional_account| {
                        let slot = from_account::<Clock, _>(optional_account.as_ref().unwrap())
                            .unwrap()
                            .slot
                            + 1;

                        create_account(
                            &Clock {
                                slot,
                                ..Clock::default()
                            },
                            bank2.inherit_specially_retained_account_fields(optional_account),
                        )
                    });
                    let current_account = bank2.get_account(&dummy_clock_id).unwrap();
                    assert_eq!(
                        expected_next_slot,
                        from_account::<Clock, _>(&current_account).unwrap().slot
                    );
                },
                |old, new| {
                    // updating twice in a slot shouldn't increment capitalization twice
                    assert_eq!(old, new);
                    true
                },
            );
        }
    }

    #[test]
    fn test_bank_epoch_vote_accounts() {
        let leader_pubkey = solana_sdk::pubkey::new_rand();
        let leader_lamports = 3;
        let mut genesis_config =
            create_genesis_config_with_leader(5, &leader_pubkey, leader_lamports).genesis_config;

        // set this up weird, forces future generation, odd mod(), etc.
        //  this says: "vote_accounts for epoch X should be generated at slot index 3 in epoch X-2...
        const SLOTS_PER_EPOCH: u64 = MINIMUM_SLOTS_PER_EPOCH;
        const LEADER_SCHEDULE_SLOT_OFFSET: u64 = SLOTS_PER_EPOCH * 3 - 3;
        // no warmup allows me to do the normal division stuff below
        genesis_config.epoch_schedule =
            EpochSchedule::custom(SLOTS_PER_EPOCH, LEADER_SCHEDULE_SLOT_OFFSET, false);

        let parent = Arc::new(Bank::new_for_tests(&genesis_config));
        let mut leader_vote_stake: Vec<_> = parent
            .epoch_vote_accounts(0)
            .map(|accounts| {
                accounts
                    .iter()
                    .filter_map(|(pubkey, (stake, account))| {
                        if let Ok(vote_state) = account.vote_state().as_ref() {
                            if vote_state.node_pubkey == leader_pubkey {
                                Some((*pubkey, *stake))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .unwrap();
        assert_eq!(leader_vote_stake.len(), 1);
        let (leader_vote_account, leader_stake) = leader_vote_stake.pop().unwrap();
        assert!(leader_stake > 0);

        let leader_stake = Stake {
            delegation: Delegation {
                stake: leader_lamports,
                activation_epoch: std::u64::MAX, // bootstrap
                ..Delegation::default()
            },
            ..Stake::default()
        };

        let mut epoch = 1;
        loop {
            if epoch > LEADER_SCHEDULE_SLOT_OFFSET / SLOTS_PER_EPOCH {
                break;
            }
            let vote_accounts = parent.epoch_vote_accounts(epoch);
            assert!(vote_accounts.is_some());

            // epoch_stakes are a snapshot at the leader_schedule_slot_offset boundary
            //   in the prior epoch (0 in this case)
            assert_eq!(
                leader_stake.stake(0, None),
                vote_accounts.unwrap().get(&leader_vote_account).unwrap().0
            );

            epoch += 1;
        }

        // child crosses epoch boundary and is the first slot in the epoch
        let child = Bank::new_from_parent(
            &parent,
            &leader_pubkey,
            SLOTS_PER_EPOCH - (LEADER_SCHEDULE_SLOT_OFFSET % SLOTS_PER_EPOCH),
        );

        assert!(child.epoch_vote_accounts(epoch).is_some());
        assert_eq!(
            leader_stake.stake(child.epoch(), None),
            child
                .epoch_vote_accounts(epoch)
                .unwrap()
                .get(&leader_vote_account)
                .unwrap()
                .0
        );

        // child crosses epoch boundary but isn't the first slot in the epoch, still
        //  makes an epoch stakes snapshot at 1
        let child = Bank::new_from_parent(
            &parent,
            &leader_pubkey,
            SLOTS_PER_EPOCH - (LEADER_SCHEDULE_SLOT_OFFSET % SLOTS_PER_EPOCH) + 1,
        );
        assert!(child.epoch_vote_accounts(epoch).is_some());
        assert_eq!(
            leader_stake.stake(child.epoch(), None),
            child
                .epoch_vote_accounts(epoch)
                .unwrap()
                .get(&leader_vote_account)
                .unwrap()
                .0
        );
    }

    #[test]
    fn test_zero_signatures() {
        solana_logger::setup();
        let (genesis_config, mint_keypair) = create_genesis_config(500);
        let mut bank = Bank::new_for_tests(&genesis_config);
        bank.fee_rate_governor.lamports_per_signature = 2;
        let key = solana_sdk::pubkey::new_rand();

        let mut transfer_instruction =
            system_instruction::transfer(&mint_keypair.pubkey(), &key, 0);
        transfer_instruction.accounts[0].is_signer = false;
        let message = Message::new(&[transfer_instruction], None);
        let tx = Transaction::new(&[&Keypair::new(); 0], message, bank.last_blockhash());

        assert_eq!(
            bank.process_transaction(&tx),
            Err(TransactionError::SanitizeFailure)
        );
        assert_eq!(bank.get_balance(&key), 0);
    }

    #[test]
    fn test_bank_get_slots_in_epoch() {
        let (genesis_config, _) = create_genesis_config(500);

        let bank = Bank::new_for_tests(&genesis_config);

        assert_eq!(bank.get_slots_in_epoch(0), MINIMUM_SLOTS_PER_EPOCH);
        assert_eq!(bank.get_slots_in_epoch(2), (MINIMUM_SLOTS_PER_EPOCH * 4));
        assert_eq!(
            bank.get_slots_in_epoch(5000),
            genesis_config.epoch_schedule.slots_per_epoch
        );
    }

    #[test]
    fn test_is_delta_true() {
        let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.0));
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));
        let key1 = Keypair::new();
        let tx_transfer_mint_to_1 = system_transaction::transfer(
            &mint_keypair,
            &key1.pubkey(),
            genesis_config.rent.minimum_balance(0),
            genesis_config.hash(),
        );
        assert_eq!(bank.process_transaction(&tx_transfer_mint_to_1), Ok(()));
        assert!(bank.is_delta.load(Relaxed));

        let bank1 = new_from_parent(&bank);
        let hash1 = bank1.hash_internal_state();
        assert!(!bank1.is_delta.load(Relaxed));
        assert_ne!(hash1, bank.hash());
        // ticks don't make a bank into a delta or change its state unless a block boundary is crossed
        bank1.register_tick(&Hash::default());
        assert!(!bank1.is_delta.load(Relaxed));
        assert_eq!(bank1.hash_internal_state(), hash1);
    }

    #[test]
    fn test_is_empty() {
        let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.0));
        let bank0 = Arc::new(Bank::new_for_tests(&genesis_config));
        let key1 = Keypair::new();

        // The zeroth bank is empty becasue there are no transactions
        assert!(bank0.is_empty());

        // Set is_delta to true, bank is no longer empty
        let tx_transfer_mint_to_1 = system_transaction::transfer(
            &mint_keypair,
            &key1.pubkey(),
            genesis_config.rent.minimum_balance(0),
            genesis_config.hash(),
        );
        assert_eq!(bank0.process_transaction(&tx_transfer_mint_to_1), Ok(()));
        assert!(!bank0.is_empty());
    }

    #[test]
    fn test_bank_inherit_tx_count() {
        let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.0));
        let bank0 = Arc::new(Bank::new_for_tests(&genesis_config));

        // Bank 1
        let bank1 = Arc::new(Bank::new_from_parent(
            &bank0,
            &solana_sdk::pubkey::new_rand(),
            1,
        ));
        // Bank 2
        let bank2 = Bank::new_from_parent(&bank0, &solana_sdk::pubkey::new_rand(), 2);

        // transfer a token
        assert_eq!(
            bank1.process_transaction(&system_transaction::transfer(
                &mint_keypair,
                &Keypair::new().pubkey(),
                genesis_config.rent.minimum_balance(0),
                genesis_config.hash(),
            )),
            Ok(())
        );

        assert_eq!(bank0.transaction_count(), 0);
        assert_eq!(bank0.non_vote_transaction_count_since_restart(), 0);
        assert_eq!(bank2.transaction_count(), 0);
        assert_eq!(bank2.non_vote_transaction_count_since_restart(), 0);
        assert_eq!(bank1.transaction_count(), 1);
        assert_eq!(bank1.non_vote_transaction_count_since_restart(), 1);

        bank1.squash();

        assert_eq!(bank0.transaction_count(), 0);
        assert_eq!(bank0.non_vote_transaction_count_since_restart(), 0);
        assert_eq!(bank2.transaction_count(), 0);
        assert_eq!(bank2.non_vote_transaction_count_since_restart(), 0);
        assert_eq!(bank1.transaction_count(), 1);
        assert_eq!(bank1.non_vote_transaction_count_since_restart(), 1);

        let bank6 = Bank::new_from_parent(&bank1, &solana_sdk::pubkey::new_rand(), 3);
        assert_eq!(bank1.transaction_count(), 1);
        assert_eq!(bank1.non_vote_transaction_count_since_restart(), 1);
        assert_eq!(bank6.transaction_count(), 1);
        assert_eq!(bank6.non_vote_transaction_count_since_restart(), 1);

        bank6.squash();
        assert_eq!(bank6.transaction_count(), 1);
        assert_eq!(bank6.non_vote_transaction_count_since_restart(), 1);
    }

    #[test]
    fn test_bank_inherit_fee_rate_governor() {
        let (mut genesis_config, _mint_keypair) = create_genesis_config(500);
        genesis_config
            .fee_rate_governor
            .target_lamports_per_signature = 123;

        let bank0 = Arc::new(Bank::new_for_tests(&genesis_config));
        let bank1 = Arc::new(new_from_parent(&bank0));
        assert_eq!(
            bank0.fee_rate_governor.target_lamports_per_signature / 2,
            bank1
                .fee_rate_governor
                .create_fee_calculator()
                .lamports_per_signature
        );
    }

    #[test]
    fn test_bank_vote_accounts() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config_with_leader(500, &solana_sdk::pubkey::new_rand(), 1);
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));

        let vote_accounts = bank.vote_accounts();
        assert_eq!(vote_accounts.len(), 1); // bootstrap validator has
                                            // to have a vote account

        let vote_keypair = Keypair::new();
        let instructions = vote_instruction::create_account(
            &mint_keypair.pubkey(),
            &vote_keypair.pubkey(),
            &VoteInit {
                node_pubkey: mint_keypair.pubkey(),
                authorized_voter: vote_keypair.pubkey(),
                authorized_withdrawer: vote_keypair.pubkey(),
                commission: 0,
            },
            10,
        );

        let message = Message::new(&instructions, Some(&mint_keypair.pubkey()));
        let transaction = Transaction::new(
            &[&mint_keypair, &vote_keypair],
            message,
            bank.last_blockhash(),
        );

        bank.process_transaction(&transaction).unwrap();

        let vote_accounts = bank.vote_accounts();

        assert_eq!(vote_accounts.len(), 2);

        assert!(vote_accounts.get(&vote_keypair.pubkey()).is_some());

        assert!(bank.withdraw(&vote_keypair.pubkey(), 10).is_ok());

        let vote_accounts = bank.vote_accounts();

        assert_eq!(vote_accounts.len(), 1);
    }

    #[test]
    fn test_bank_cloned_stake_delegations() {
        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config_with_leader(
            123_456_000_000_000,
            &solana_sdk::pubkey::new_rand(),
            123_000_000_000,
        );
        genesis_config.rent = Rent::default();
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));

        let stake_delegations = bank.stakes_cache.stakes().stake_delegations().clone();
        assert_eq!(stake_delegations.len(), 1); // bootstrap validator has
                                                // to have a stake delegation

        let (vote_balance, stake_balance) = {
            let rent = &bank.rent_collector().rent;
            let vote_rent_exempt_reserve = rent.minimum_balance(VoteState::size_of());
            let stake_rent_exempt_reserve = rent.minimum_balance(StakeState::size_of());
            let minimum_delegation =
                solana_stake_program::get_minimum_delegation(&bank.feature_set);
            (
                vote_rent_exempt_reserve,
                stake_rent_exempt_reserve + minimum_delegation,
            )
        };

        let vote_keypair = Keypair::new();
        let mut instructions = vote_instruction::create_account(
            &mint_keypair.pubkey(),
            &vote_keypair.pubkey(),
            &VoteInit {
                node_pubkey: mint_keypair.pubkey(),
                authorized_voter: vote_keypair.pubkey(),
                authorized_withdrawer: vote_keypair.pubkey(),
                commission: 0,
            },
            vote_balance,
        );

        let stake_keypair = Keypair::new();
        instructions.extend(stake_instruction::create_account_and_delegate_stake(
            &mint_keypair.pubkey(),
            &stake_keypair.pubkey(),
            &vote_keypair.pubkey(),
            &Authorized::auto(&stake_keypair.pubkey()),
            &Lockup::default(),
            stake_balance,
        ));

        let message = Message::new(&instructions, Some(&mint_keypair.pubkey()));
        let transaction = Transaction::new(
            &[&mint_keypair, &vote_keypair, &stake_keypair],
            message,
            bank.last_blockhash(),
        );

        bank.process_transaction(&transaction).unwrap();

        let stake_delegations = bank.stakes_cache.stakes().stake_delegations().clone();
        assert_eq!(stake_delegations.len(), 2);
        assert!(stake_delegations.get(&stake_keypair.pubkey()).is_some());
    }

    #[allow(deprecated)]
    #[test]
    fn test_bank_fees_account() {
        let (mut genesis_config, _) = create_genesis_config(500);
        genesis_config.fee_rate_governor = FeeRateGovernor::new(12345, 0);
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));

        let fees_account = bank.get_account(&sysvar::fees::id()).unwrap();
        let fees = from_account::<Fees, _>(&fees_account).unwrap();
        assert_eq!(
            bank.fee_rate_governor.lamports_per_signature,
            fees.fee_calculator.lamports_per_signature
        );
        assert_eq!(fees.fee_calculator.lamports_per_signature, 12345);
    }

    #[test]
    fn test_is_delta_with_no_committables() {
        let (genesis_config, mint_keypair) = create_genesis_config(8000);
        let bank = Bank::new_for_tests(&genesis_config);
        bank.is_delta.store(false, Relaxed);

        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();
        let fail_tx =
            system_transaction::transfer(&keypair1, &keypair2.pubkey(), 1, bank.last_blockhash());

        // Should fail with TransactionError::AccountNotFound, which means
        // the account which this tx operated on will not be committed. Thus
        // the bank is_delta should still be false
        assert_eq!(
            bank.process_transaction(&fail_tx),
            Err(TransactionError::AccountNotFound)
        );

        // Check the bank is_delta is still false
        assert!(!bank.is_delta.load(Relaxed));

        // Should fail with InstructionError, but InstructionErrors are committable,
        // so is_delta should be true
        assert_eq!(
            bank.transfer(10_001, &mint_keypair, &solana_sdk::pubkey::new_rand()),
            Err(TransactionError::InstructionError(
                0,
                SystemError::ResultWithNegativeLamports.into(),
            ))
        );

        assert!(bank.is_delta.load(Relaxed));
    }

    #[test]
    fn test_bank_get_program_accounts() {
        let (genesis_config, mint_keypair) = create_genesis_config(500);
        let parent = Arc::new(Bank::new_for_tests(&genesis_config));
        parent.restore_old_behavior_for_fragile_tests();

        let genesis_accounts: Vec<_> = parent.get_all_accounts_with_modified_slots().unwrap();
        assert!(
            genesis_accounts
                .iter()
                .any(|(pubkey, _, _)| *pubkey == mint_keypair.pubkey()),
            "mint pubkey not found"
        );
        assert!(
            genesis_accounts
                .iter()
                .any(|(pubkey, _, _)| solana_sdk::sysvar::is_sysvar_id(pubkey)),
            "no sysvars found"
        );

        let bank0 = Arc::new(new_from_parent(&parent));
        let pubkey0 = solana_sdk::pubkey::new_rand();
        let program_id = Pubkey::new(&[2; 32]);
        let account0 = AccountSharedData::new(1, 0, &program_id);
        bank0.store_account(&pubkey0, &account0);

        assert_eq!(
            bank0.get_program_accounts_modified_since_parent(&program_id),
            vec![(pubkey0, account0.clone())]
        );

        let bank1 = Arc::new(new_from_parent(&bank0));
        bank1.squash();
        assert_eq!(
            bank0
                .get_program_accounts(&program_id, &ScanConfig::default(),)
                .unwrap(),
            vec![(pubkey0, account0.clone())]
        );
        assert_eq!(
            bank1
                .get_program_accounts(&program_id, &ScanConfig::default(),)
                .unwrap(),
            vec![(pubkey0, account0)]
        );
        assert_eq!(
            bank1.get_program_accounts_modified_since_parent(&program_id),
            vec![]
        );

        let bank2 = Arc::new(new_from_parent(&bank1));
        let pubkey1 = solana_sdk::pubkey::new_rand();
        let account1 = AccountSharedData::new(3, 0, &program_id);
        bank2.store_account(&pubkey1, &account1);
        // Accounts with 0 lamports should be filtered out by Accounts::load_by_program()
        let pubkey2 = solana_sdk::pubkey::new_rand();
        let account2 = AccountSharedData::new(0, 0, &program_id);
        bank2.store_account(&pubkey2, &account2);

        let bank3 = Arc::new(new_from_parent(&bank2));
        bank3.squash();
        assert_eq!(
            bank1
                .get_program_accounts(&program_id, &ScanConfig::default(),)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            bank3
                .get_program_accounts(&program_id, &ScanConfig::default(),)
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn test_get_filtered_indexed_accounts_limit_exceeded() {
        let (genesis_config, _mint_keypair) = create_genesis_config(500);
        let mut account_indexes = AccountSecondaryIndexes::default();
        account_indexes.indexes.insert(AccountIndex::ProgramId);
        let bank = Arc::new(Bank::new_with_config_for_tests(
            &genesis_config,
            account_indexes,
            AccountShrinkThreshold::default(),
        ));

        let address = Pubkey::new_unique();
        let program_id = Pubkey::new_unique();
        let limit = 100;
        let account = AccountSharedData::new(1, limit, &program_id);
        bank.store_account(&address, &account);

        assert!(bank
            .get_filtered_indexed_accounts(
                &IndexKey::ProgramId(program_id),
                |_| true,
                &ScanConfig::default(),
                Some(limit), // limit here will be exceeded, resulting in aborted scan
            )
            .is_err());
    }

    #[test]
    fn test_get_filtered_indexed_accounts() {
        let (genesis_config, _mint_keypair) = create_genesis_config(500);
        let mut account_indexes = AccountSecondaryIndexes::default();
        account_indexes.indexes.insert(AccountIndex::ProgramId);
        let bank = Arc::new(Bank::new_with_config_for_tests(
            &genesis_config,
            account_indexes,
            AccountShrinkThreshold::default(),
        ));

        let address = Pubkey::new_unique();
        let program_id = Pubkey::new_unique();
        let account = AccountSharedData::new(1, 0, &program_id);
        bank.store_account(&address, &account);

        let indexed_accounts = bank
            .get_filtered_indexed_accounts(
                &IndexKey::ProgramId(program_id),
                |_| true,
                &ScanConfig::default(),
                None,
            )
            .unwrap();
        assert_eq!(indexed_accounts.len(), 1);
        assert_eq!(indexed_accounts[0], (address, account));

        // Even though the account is re-stored in the bank (and the index) under a new program id,
        // it is still present in the index under the original program id as well. This
        // demonstrates the need for a redundant post-processing filter.
        let another_program_id = Pubkey::new_unique();
        let new_account = AccountSharedData::new(1, 0, &another_program_id);
        let bank = Arc::new(new_from_parent(&bank));
        bank.store_account(&address, &new_account);
        let indexed_accounts = bank
            .get_filtered_indexed_accounts(
                &IndexKey::ProgramId(program_id),
                |_| true,
                &ScanConfig::default(),
                None,
            )
            .unwrap();
        assert_eq!(indexed_accounts.len(), 1);
        assert_eq!(indexed_accounts[0], (address, new_account.clone()));
        let indexed_accounts = bank
            .get_filtered_indexed_accounts(
                &IndexKey::ProgramId(another_program_id),
                |_| true,
                &ScanConfig::default(),
                None,
            )
            .unwrap();
        assert_eq!(indexed_accounts.len(), 1);
        assert_eq!(indexed_accounts[0], (address, new_account.clone()));

        // Post-processing filter
        let indexed_accounts = bank
            .get_filtered_indexed_accounts(
                &IndexKey::ProgramId(program_id),
                |account| account.owner() == &program_id,
                &ScanConfig::default(),
                None,
            )
            .unwrap();
        assert!(indexed_accounts.is_empty());
        let indexed_accounts = bank
            .get_filtered_indexed_accounts(
                &IndexKey::ProgramId(another_program_id),
                |account| account.owner() == &another_program_id,
                &ScanConfig::default(),
                None,
            )
            .unwrap();
        assert_eq!(indexed_accounts.len(), 1);
        assert_eq!(indexed_accounts[0], (address, new_account));
    }

    #[test]
    fn test_status_cache_ancestors() {
        solana_logger::setup();
        let parent = create_simple_test_arc_bank(500);
        let bank1 = Arc::new(new_from_parent(&parent));
        let mut bank = bank1;
        for _ in 0..MAX_CACHE_ENTRIES * 2 {
            bank = Arc::new(new_from_parent(&bank));
            bank.squash();
        }

        let bank = new_from_parent(&bank);
        assert_eq!(
            bank.status_cache_ancestors(),
            (bank.slot() - MAX_CACHE_ENTRIES as u64..=bank.slot()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_add_builtin() {
        let (genesis_config, mint_keypair) = create_genesis_config(500);
        let mut bank = Bank::new_for_tests(&genesis_config);

        fn mock_vote_program_id() -> Pubkey {
            Pubkey::new(&[42u8; 32])
        }
        fn mock_vote_processor(
            _first_instruction_account: IndexOfAccount,
            invoke_context: &mut InvokeContext,
        ) -> std::result::Result<(), InstructionError> {
            let transaction_context = &invoke_context.transaction_context;
            let instruction_context = transaction_context.get_current_instruction_context()?;
            let program_id = instruction_context.get_last_program_key(transaction_context)?;
            if mock_vote_program_id() != *program_id {
                return Err(InstructionError::IncorrectProgramId);
            }
            Err(InstructionError::Custom(42))
        }

        assert!(bank.get_account(&mock_vote_program_id()).is_none());
        bank.add_builtin(
            "mock_vote_program",
            &mock_vote_program_id(),
            mock_vote_processor,
        );
        assert!(bank.get_account(&mock_vote_program_id()).is_some());

        let mock_account = Keypair::new();
        let mock_validator_identity = Keypair::new();
        let mut instructions = vote_instruction::create_account(
            &mint_keypair.pubkey(),
            &mock_account.pubkey(),
            &VoteInit {
                node_pubkey: mock_validator_identity.pubkey(),
                ..VoteInit::default()
            },
            1,
        );
        instructions[1].program_id = mock_vote_program_id();

        let message = Message::new(&instructions, Some(&mint_keypair.pubkey()));
        let transaction = Transaction::new(
            &[&mint_keypair, &mock_account, &mock_validator_identity],
            message,
            bank.last_blockhash(),
        );

        assert_eq!(
            bank.process_transaction(&transaction),
            Err(TransactionError::InstructionError(
                1,
                InstructionError::Custom(42)
            ))
        );
    }

    #[test]
    fn test_add_duplicate_static_program() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config_with_leader(500, &solana_sdk::pubkey::new_rand(), 0);
        let mut bank = Bank::new_for_tests(&genesis_config);

        fn mock_vote_processor(
            _first_instruction_account: IndexOfAccount,
            _invoke_context: &mut InvokeContext,
        ) -> std::result::Result<(), InstructionError> {
            Err(InstructionError::Custom(42))
        }

        let mock_account = Keypair::new();
        let mock_validator_identity = Keypair::new();
        let instructions = vote_instruction::create_account(
            &mint_keypair.pubkey(),
            &mock_account.pubkey(),
            &VoteInit {
                node_pubkey: mock_validator_identity.pubkey(),
                ..VoteInit::default()
            },
            1,
        );

        let message = Message::new(&instructions, Some(&mint_keypair.pubkey()));
        let transaction = Transaction::new(
            &[&mint_keypair, &mock_account, &mock_validator_identity],
            message,
            bank.last_blockhash(),
        );

        let vote_loader_account = bank.get_account(&solana_vote_program::id()).unwrap();
        bank.add_builtin(
            "solana_vote_program",
            &solana_vote_program::id(),
            mock_vote_processor,
        );
        let new_vote_loader_account = bank.get_account(&solana_vote_program::id()).unwrap();
        // Vote loader account should not be updated since it was included in the genesis config.
        assert_eq!(vote_loader_account.data(), new_vote_loader_account.data());
        assert_eq!(
            bank.process_transaction(&transaction),
            Err(TransactionError::InstructionError(
                1,
                InstructionError::Custom(42)
            ))
        );
    }

    #[test]
    fn test_add_instruction_processor_for_existing_unrelated_accounts() {
        for pass in 0..5 {
            let mut bank = create_simple_test_bank(500);

            fn mock_ix_processor(
                _first_instruction_account: IndexOfAccount,
                _invoke_context: &mut InvokeContext,
            ) -> std::result::Result<(), InstructionError> {
                Err(InstructionError::Custom(42))
            }

            // Non-builtin loader accounts can not be used for instruction processing
            {
                let stakes = bank.stakes_cache.stakes();
                assert!(stakes.vote_accounts().as_ref().is_empty());
            }
            assert!(bank.stakes_cache.stakes().stake_delegations().is_empty());
            if pass == 0 {
                add_root_and_flush_write_cache(&bank);
                assert_eq!(bank.calculate_capitalization(true), bank.capitalization());
                continue;
            }

            let ((vote_id, vote_account), (stake_id, stake_account)) =
                crate::stakes::tests::create_staked_node_accounts(1_0000);
            bank.capitalization
                .fetch_add(vote_account.lamports() + stake_account.lamports(), Relaxed);
            bank.store_account(&vote_id, &vote_account);
            bank.store_account(&stake_id, &stake_account);
            {
                let stakes = bank.stakes_cache.stakes();
                assert!(!stakes.vote_accounts().as_ref().is_empty());
            }
            assert!(!bank.stakes_cache.stakes().stake_delegations().is_empty());
            if pass == 1 {
                add_root_and_flush_write_cache(&bank);
                assert_eq!(bank.calculate_capitalization(true), bank.capitalization());
                continue;
            }

            bank.add_builtin("mock_program1", &vote_id, mock_ix_processor);
            bank.add_builtin("mock_program2", &stake_id, mock_ix_processor);
            {
                let stakes = bank.stakes_cache.stakes();
                assert!(stakes.vote_accounts().as_ref().is_empty());
            }
            assert!(bank.stakes_cache.stakes().stake_delegations().is_empty());
            if pass == 2 {
                add_root_and_flush_write_cache(&bank);
                assert_eq!(bank.calculate_capitalization(true), bank.capitalization());
                continue;
            }
            assert_eq!(
                "mock_program1",
                String::from_utf8_lossy(bank.get_account(&vote_id).unwrap_or_default().data())
            );
            assert_eq!(
                "mock_program2",
                String::from_utf8_lossy(bank.get_account(&stake_id).unwrap_or_default().data())
            );

            // Re-adding builtin programs should be no-op
            bank.update_accounts_hash_for_tests();
            let old_hash = bank.get_accounts_hash();
            bank.add_builtin("mock_program1", &vote_id, mock_ix_processor);
            bank.add_builtin("mock_program2", &stake_id, mock_ix_processor);
            add_root_and_flush_write_cache(&bank);
            bank.update_accounts_hash_for_tests();
            let new_hash = bank.get_accounts_hash();
            assert_eq!(old_hash, new_hash);
            {
                let stakes = bank.stakes_cache.stakes();
                assert!(stakes.vote_accounts().as_ref().is_empty());
            }
            assert!(bank.stakes_cache.stakes().stake_delegations().is_empty());
            assert_eq!(bank.calculate_capitalization(true), bank.capitalization());
            assert_eq!(
                "mock_program1",
                String::from_utf8_lossy(bank.get_account(&vote_id).unwrap_or_default().data())
            );
            assert_eq!(
                "mock_program2",
                String::from_utf8_lossy(bank.get_account(&stake_id).unwrap_or_default().data())
            );
        }
    }

    #[allow(deprecated)]
    #[test]
    fn test_recent_blockhashes_sysvar() {
        let mut bank = create_simple_test_arc_bank(500);
        for i in 1..5 {
            let bhq_account = bank.get_account(&sysvar::recent_blockhashes::id()).unwrap();
            let recent_blockhashes =
                from_account::<sysvar::recent_blockhashes::RecentBlockhashes, _>(&bhq_account)
                    .unwrap();
            // Check length
            assert_eq!(recent_blockhashes.len(), i);
            let most_recent_hash = recent_blockhashes.iter().next().unwrap().blockhash;
            // Check order
            assert!(bank.is_hash_valid_for_age(&most_recent_hash, 0));
            goto_end_of_slot(Arc::get_mut(&mut bank).unwrap());
            bank = Arc::new(new_from_parent(&bank));
        }
    }

    #[allow(deprecated)]
    #[test]
    fn test_blockhash_queue_sysvar_consistency() {
        let mut bank = create_simple_test_arc_bank(100_000);
        goto_end_of_slot(Arc::get_mut(&mut bank).unwrap());

        let bhq_account = bank.get_account(&sysvar::recent_blockhashes::id()).unwrap();
        let recent_blockhashes =
            from_account::<sysvar::recent_blockhashes::RecentBlockhashes, _>(&bhq_account).unwrap();

        let sysvar_recent_blockhash = recent_blockhashes[0].blockhash;
        let bank_last_blockhash = bank.last_blockhash();
        assert_eq!(sysvar_recent_blockhash, bank_last_blockhash);
    }

    #[test]
    fn test_hash_internal_state_unchanged() {
        let (genesis_config, _) = create_genesis_config(500);
        let bank0 = Arc::new(Bank::new_for_tests(&genesis_config));
        bank0.freeze();
        let bank0_hash = bank0.hash();
        let bank1 = Bank::new_from_parent(&bank0, &Pubkey::default(), 1);
        bank1.freeze();
        let bank1_hash = bank1.hash();
        // Checkpointing should always result in a new state
        assert_ne!(bank0_hash, bank1_hash);
    }

    #[test]
    fn test_ticks_change_state() {
        let (genesis_config, _) = create_genesis_config(500);
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));
        let bank1 = new_from_parent(&bank);
        let hash1 = bank1.hash_internal_state();
        // ticks don't change its state unless a block boundary is crossed
        for _ in 0..genesis_config.ticks_per_slot {
            assert_eq!(bank1.hash_internal_state(), hash1);
            bank1.register_tick(&Hash::default());
        }
        assert_ne!(bank1.hash_internal_state(), hash1);
    }

    #[ignore]
    #[test]
    fn test_banks_leak() {
        fn add_lotsa_stake_accounts(genesis_config: &mut GenesisConfig) {
            const LOTSA: usize = 4_096;

            (0..LOTSA).for_each(|_| {
                let pubkey = solana_sdk::pubkey::new_rand();
                genesis_config.add_account(
                    pubkey,
                    stake_state::create_lockup_stake_account(
                        &Authorized::auto(&pubkey),
                        &Lockup::default(),
                        &Rent::default(),
                        50_000_000,
                    ),
                );
            });
        }
        solana_logger::setup();
        let (mut genesis_config, _) = create_genesis_config(100_000_000_000_000);
        add_lotsa_stake_accounts(&mut genesis_config);
        let mut bank = std::sync::Arc::new(Bank::new_for_tests(&genesis_config));
        let mut num_banks = 0;
        let pid = std::process::id();
        #[cfg(not(target_os = "linux"))]
        error!(
            "\nYou can run this to watch RAM:\n   while read -p 'banks: '; do echo $(( $(ps -o vsize= -p {})/$REPLY));done", pid
        );
        loop {
            num_banks += 1;
            bank = std::sync::Arc::new(new_from_parent(&bank));
            if num_banks % 100 == 0 {
                #[cfg(target_os = "linux")]
                {
                    let pages_consumed = std::fs::read_to_string(format!("/proc/{pid}/statm"))
                        .unwrap()
                        .split_whitespace()
                        .next()
                        .unwrap()
                        .parse::<usize>()
                        .unwrap();
                    error!(
                        "at {} banks: {} mem or {}kB/bank",
                        num_banks,
                        pages_consumed * 4096,
                        (pages_consumed * 4) / num_banks
                    );
                }
                #[cfg(not(target_os = "linux"))]
                {
                    error!("{} banks, sleeping for 5 sec", num_banks);
                    std::thread::sleep(Duration::from_secs(5));
                }
            }
        }
    }

    fn get_nonce_blockhash(bank: &Bank, nonce_pubkey: &Pubkey) -> Option<Hash> {
        let account = bank.get_account(nonce_pubkey)?;
        let nonce_versions = StateMut::<nonce::state::Versions>::state(&account);
        match nonce_versions.ok()?.state() {
            nonce::State::Initialized(ref data) => Some(data.blockhash()),
            _ => None,
        }
    }

    fn nonce_setup(
        bank: &mut Arc<Bank>,
        mint_keypair: &Keypair,
        custodian_lamports: u64,
        nonce_lamports: u64,
        nonce_authority: Option<Pubkey>,
    ) -> Result<(Keypair, Keypair)> {
        let custodian_keypair = Keypair::new();
        let nonce_keypair = Keypair::new();
        /* Setup accounts */
        let mut setup_ixs = vec![system_instruction::transfer(
            &mint_keypair.pubkey(),
            &custodian_keypair.pubkey(),
            custodian_lamports,
        )];
        let nonce_authority = nonce_authority.unwrap_or_else(|| nonce_keypair.pubkey());
        setup_ixs.extend_from_slice(&system_instruction::create_nonce_account(
            &custodian_keypair.pubkey(),
            &nonce_keypair.pubkey(),
            &nonce_authority,
            nonce_lamports,
        ));
        let message = Message::new(&setup_ixs, Some(&mint_keypair.pubkey()));
        let setup_tx = Transaction::new(
            &[mint_keypair, &custodian_keypair, &nonce_keypair],
            message,
            bank.last_blockhash(),
        );
        bank.process_transaction(&setup_tx)?;
        Ok((custodian_keypair, nonce_keypair))
    }

    fn setup_nonce_with_bank<F>(
        supply_lamports: u64,
        mut genesis_cfg_fn: F,
        custodian_lamports: u64,
        nonce_lamports: u64,
        nonce_authority: Option<Pubkey>,
        feature_set: FeatureSet,
    ) -> Result<(Arc<Bank>, Keypair, Keypair, Keypair)>
    where
        F: FnMut(&mut GenesisConfig),
    {
        let (mut genesis_config, mint_keypair) = create_genesis_config(supply_lamports);
        genesis_config.rent.lamports_per_byte_year = 0;
        genesis_cfg_fn(&mut genesis_config);
        let mut bank = Bank::new_for_tests(&genesis_config);
        bank.feature_set = Arc::new(feature_set);
        let mut bank = Arc::new(bank);

        // Banks 0 and 1 have no fees, wait two blocks before
        // initializing our nonce accounts
        for _ in 0..2 {
            goto_end_of_slot(Arc::get_mut(&mut bank).unwrap());
            bank = Arc::new(new_from_parent(&bank));
        }

        let (custodian_keypair, nonce_keypair) = nonce_setup(
            &mut bank,
            &mint_keypair,
            custodian_lamports,
            nonce_lamports,
            nonce_authority,
        )?;

        // The setup nonce is not valid to be used until the next bank
        // so wait one more block
        goto_end_of_slot(Arc::get_mut(&mut bank).unwrap());
        bank = Arc::new(new_from_parent(&bank));

        Ok((bank, mint_keypair, custodian_keypair, nonce_keypair))
    }

    impl Bank {
        fn next_durable_nonce(&self) -> DurableNonce {
            let hash_queue = self.blockhash_queue.read().unwrap();
            let last_blockhash = hash_queue.last_hash();
            DurableNonce::from_blockhash(&last_blockhash)
        }
    }

    #[test]
    fn test_check_transaction_for_nonce_ok() {
        let (bank, _mint_keypair, custodian_keypair, nonce_keypair) = setup_nonce_with_bank(
            10_000_000,
            |_| {},
            5_000_000,
            250_000,
            None,
            FeatureSet::all_enabled(),
        )
        .unwrap();
        let custodian_pubkey = custodian_keypair.pubkey();
        let nonce_pubkey = nonce_keypair.pubkey();

        let nonce_hash = get_nonce_blockhash(&bank, &nonce_pubkey).unwrap();
        let tx = Transaction::new_signed_with_payer(
            &[
                system_instruction::advance_nonce_account(&nonce_pubkey, &nonce_pubkey),
                system_instruction::transfer(&custodian_pubkey, &nonce_pubkey, 100_000),
            ],
            Some(&custodian_pubkey),
            &[&custodian_keypair, &nonce_keypair],
            nonce_hash,
        );
        let nonce_account = bank.get_account(&nonce_pubkey).unwrap();
        assert_eq!(
            bank.check_transaction_for_nonce(
                &SanitizedTransaction::from_transaction_for_tests(tx),
                &bank.next_durable_nonce(),
            ),
            Some((nonce_pubkey, nonce_account))
        );
    }

    #[test]
    fn test_check_transaction_for_nonce_not_nonce_fail() {
        let (bank, _mint_keypair, custodian_keypair, nonce_keypair) = setup_nonce_with_bank(
            10_000_000,
            |_| {},
            5_000_000,
            250_000,
            None,
            FeatureSet::all_enabled(),
        )
        .unwrap();
        let custodian_pubkey = custodian_keypair.pubkey();
        let nonce_pubkey = nonce_keypair.pubkey();

        let nonce_hash = get_nonce_blockhash(&bank, &nonce_pubkey).unwrap();
        let tx = Transaction::new_signed_with_payer(
            &[
                system_instruction::transfer(&custodian_pubkey, &nonce_pubkey, 100_000),
                system_instruction::advance_nonce_account(&nonce_pubkey, &nonce_pubkey),
            ],
            Some(&custodian_pubkey),
            &[&custodian_keypair, &nonce_keypair],
            nonce_hash,
        );
        assert!(bank
            .check_transaction_for_nonce(
                &SanitizedTransaction::from_transaction_for_tests(tx,),
                &bank.next_durable_nonce(),
            )
            .is_none());
    }

    #[test]
    fn test_check_transaction_for_nonce_missing_ix_pubkey_fail() {
        let (bank, _mint_keypair, custodian_keypair, nonce_keypair) = setup_nonce_with_bank(
            10_000_000,
            |_| {},
            5_000_000,
            250_000,
            None,
            FeatureSet::all_enabled(),
        )
        .unwrap();
        let custodian_pubkey = custodian_keypair.pubkey();
        let nonce_pubkey = nonce_keypair.pubkey();

        let nonce_hash = get_nonce_blockhash(&bank, &nonce_pubkey).unwrap();
        let mut tx = Transaction::new_signed_with_payer(
            &[
                system_instruction::advance_nonce_account(&nonce_pubkey, &nonce_pubkey),
                system_instruction::transfer(&custodian_pubkey, &nonce_pubkey, 100_000),
            ],
            Some(&custodian_pubkey),
            &[&custodian_keypair, &nonce_keypair],
            nonce_hash,
        );
        tx.message.instructions[0].accounts.clear();
        assert!(bank
            .check_transaction_for_nonce(
                &SanitizedTransaction::from_transaction_for_tests(tx),
                &bank.next_durable_nonce(),
            )
            .is_none());
    }

    #[test]
    fn test_check_transaction_for_nonce_nonce_acc_does_not_exist_fail() {
        let (bank, _mint_keypair, custodian_keypair, nonce_keypair) = setup_nonce_with_bank(
            10_000_000,
            |_| {},
            5_000_000,
            250_000,
            None,
            FeatureSet::all_enabled(),
        )
        .unwrap();
        let custodian_pubkey = custodian_keypair.pubkey();
        let nonce_pubkey = nonce_keypair.pubkey();
        let missing_keypair = Keypair::new();
        let missing_pubkey = missing_keypair.pubkey();

        let nonce_hash = get_nonce_blockhash(&bank, &nonce_pubkey).unwrap();
        let tx = Transaction::new_signed_with_payer(
            &[
                system_instruction::advance_nonce_account(&missing_pubkey, &nonce_pubkey),
                system_instruction::transfer(&custodian_pubkey, &nonce_pubkey, 100_000),
            ],
            Some(&custodian_pubkey),
            &[&custodian_keypair, &nonce_keypair],
            nonce_hash,
        );
        assert!(bank
            .check_transaction_for_nonce(
                &SanitizedTransaction::from_transaction_for_tests(tx),
                &bank.next_durable_nonce(),
            )
            .is_none());
    }

    #[test]
    fn test_check_transaction_for_nonce_bad_tx_hash_fail() {
        let (bank, _mint_keypair, custodian_keypair, nonce_keypair) = setup_nonce_with_bank(
            10_000_000,
            |_| {},
            5_000_000,
            250_000,
            None,
            FeatureSet::all_enabled(),
        )
        .unwrap();
        let custodian_pubkey = custodian_keypair.pubkey();
        let nonce_pubkey = nonce_keypair.pubkey();

        let tx = Transaction::new_signed_with_payer(
            &[
                system_instruction::advance_nonce_account(&nonce_pubkey, &nonce_pubkey),
                system_instruction::transfer(&custodian_pubkey, &nonce_pubkey, 100_000),
            ],
            Some(&custodian_pubkey),
            &[&custodian_keypair, &nonce_keypair],
            Hash::default(),
        );
        assert!(bank
            .check_transaction_for_nonce(
                &SanitizedTransaction::from_transaction_for_tests(tx),
                &bank.next_durable_nonce(),
            )
            .is_none());
    }

    #[test]
    fn test_assign_from_nonce_account_fail() {
        let bank = create_simple_test_arc_bank(100_000_000);
        let nonce = Keypair::new();
        let nonce_account = AccountSharedData::new_data(
            42_424_242,
            &nonce::state::Versions::new(nonce::State::Initialized(nonce::state::Data::default())),
            &system_program::id(),
        )
        .unwrap();
        let blockhash = bank.last_blockhash();
        bank.store_account(&nonce.pubkey(), &nonce_account);

        let ix = system_instruction::assign(&nonce.pubkey(), &Pubkey::new(&[9u8; 32]));
        let message = Message::new(&[ix], Some(&nonce.pubkey()));
        let tx = Transaction::new(&[&nonce], message, blockhash);

        let expect = Err(TransactionError::InstructionError(
            0,
            InstructionError::ModifiedProgramId,
        ));
        assert_eq!(bank.process_transaction(&tx), expect);
    }

    #[test]
    fn test_nonce_must_be_advanceable() {
        let mut bank = create_simple_test_bank(100_000_000);
        bank.feature_set = Arc::new(FeatureSet::all_enabled());
        let bank = Arc::new(bank);
        let nonce_keypair = Keypair::new();
        let nonce_authority = nonce_keypair.pubkey();
        let durable_nonce = DurableNonce::from_blockhash(&bank.last_blockhash());
        let nonce_account = AccountSharedData::new_data(
            42_424_242,
            &nonce::state::Versions::new(nonce::State::Initialized(nonce::state::Data::new(
                nonce_authority,
                durable_nonce,
                5000,
            ))),
            &system_program::id(),
        )
        .unwrap();
        bank.store_account(&nonce_keypair.pubkey(), &nonce_account);

        let ix =
            system_instruction::advance_nonce_account(&nonce_keypair.pubkey(), &nonce_authority);
        let message = Message::new(&[ix], Some(&nonce_keypair.pubkey()));
        let tx = Transaction::new(&[&nonce_keypair], message, *durable_nonce.as_hash());
        assert_eq!(
            bank.process_transaction(&tx),
            Err(TransactionError::BlockhashNotFound)
        );
    }

    #[test]
    fn test_nonce_transaction() {
        let (mut bank, _mint_keypair, custodian_keypair, nonce_keypair) = setup_nonce_with_bank(
            10_000_000,
            |_| {},
            5_000_000,
            250_000,
            None,
            FeatureSet::all_enabled(),
        )
        .unwrap();
        let alice_keypair = Keypair::new();
        let alice_pubkey = alice_keypair.pubkey();
        let custodian_pubkey = custodian_keypair.pubkey();
        let nonce_pubkey = nonce_keypair.pubkey();

        assert_eq!(bank.get_balance(&custodian_pubkey), 4_750_000);
        assert_eq!(bank.get_balance(&nonce_pubkey), 250_000);

        /* Grab the hash stored in the nonce account */
        let nonce_hash = get_nonce_blockhash(&bank, &nonce_pubkey).unwrap();

        /* Kick nonce hash off the blockhash_queue */
        for _ in 0..MAX_RECENT_BLOCKHASHES + 1 {
            goto_end_of_slot(Arc::get_mut(&mut bank).unwrap());
            bank = Arc::new(new_from_parent(&bank));
        }

        /* Expect a non-Nonce transfer to fail */
        assert_eq!(
            bank.process_transaction(&system_transaction::transfer(
                &custodian_keypair,
                &alice_pubkey,
                100_000,
                nonce_hash
            ),),
            Err(TransactionError::BlockhashNotFound),
        );
        /* Check fee not charged */
        assert_eq!(bank.get_balance(&custodian_pubkey), 4_750_000);

        /* Nonce transfer */
        let nonce_tx = Transaction::new_signed_with_payer(
            &[
                system_instruction::advance_nonce_account(&nonce_pubkey, &nonce_pubkey),
                system_instruction::transfer(&custodian_pubkey, &alice_pubkey, 100_000),
            ],
            Some(&custodian_pubkey),
            &[&custodian_keypair, &nonce_keypair],
            nonce_hash,
        );
        assert_eq!(bank.process_transaction(&nonce_tx), Ok(()));

        /* Check balances */
        let mut recent_message = nonce_tx.message;
        recent_message.recent_blockhash = bank.last_blockhash();
        let mut expected_balance = 4_650_000
            - bank
                .get_fee_for_message(&recent_message.try_into().unwrap())
                .unwrap();
        assert_eq!(bank.get_balance(&custodian_pubkey), expected_balance);
        assert_eq!(bank.get_balance(&nonce_pubkey), 250_000);
        assert_eq!(bank.get_balance(&alice_pubkey), 100_000);

        /* Confirm stored nonce has advanced */
        let new_nonce = get_nonce_blockhash(&bank, &nonce_pubkey).unwrap();
        assert_ne!(nonce_hash, new_nonce);

        /* Nonce re-use fails */
        let nonce_tx = Transaction::new_signed_with_payer(
            &[
                system_instruction::advance_nonce_account(&nonce_pubkey, &nonce_pubkey),
                system_instruction::transfer(&custodian_pubkey, &alice_pubkey, 100_000),
            ],
            Some(&custodian_pubkey),
            &[&custodian_keypair, &nonce_keypair],
            nonce_hash,
        );
        assert_eq!(
            bank.process_transaction(&nonce_tx),
            Err(TransactionError::BlockhashNotFound)
        );
        /* Check fee not charged and nonce not advanced */
        assert_eq!(bank.get_balance(&custodian_pubkey), expected_balance);
        assert_eq!(
            new_nonce,
            get_nonce_blockhash(&bank, &nonce_pubkey).unwrap()
        );

        let nonce_hash = new_nonce;

        /* Kick nonce hash off the blockhash_queue */
        for _ in 0..MAX_RECENT_BLOCKHASHES + 1 {
            goto_end_of_slot(Arc::get_mut(&mut bank).unwrap());
            bank = Arc::new(new_from_parent(&bank));
        }

        let nonce_tx = Transaction::new_signed_with_payer(
            &[
                system_instruction::advance_nonce_account(&nonce_pubkey, &nonce_pubkey),
                system_instruction::transfer(&custodian_pubkey, &alice_pubkey, 100_000_000),
            ],
            Some(&custodian_pubkey),
            &[&custodian_keypair, &nonce_keypair],
            nonce_hash,
        );
        assert_eq!(
            bank.process_transaction(&nonce_tx),
            Err(TransactionError::InstructionError(
                1,
                system_instruction::SystemError::ResultWithNegativeLamports.into(),
            ))
        );
        /* Check fee charged and nonce has advanced */
        let mut recent_message = nonce_tx.message.clone();
        recent_message.recent_blockhash = bank.last_blockhash();
        expected_balance -= bank
            .get_fee_for_message(&SanitizedMessage::try_from(recent_message).unwrap())
            .unwrap();
        assert_eq!(bank.get_balance(&custodian_pubkey), expected_balance);
        assert_ne!(
            nonce_hash,
            get_nonce_blockhash(&bank, &nonce_pubkey).unwrap()
        );
        /* Confirm replaying a TX that failed with InstructionError::* now
         * fails with TransactionError::BlockhashNotFound
         */
        assert_eq!(
            bank.process_transaction(&nonce_tx),
            Err(TransactionError::BlockhashNotFound),
        );
    }

    #[test]
    fn test_nonce_transaction_with_tx_wide_caps() {
        let feature_set = FeatureSet::all_enabled();
        let (mut bank, _mint_keypair, custodian_keypair, nonce_keypair) =
            setup_nonce_with_bank(10_000_000, |_| {}, 5_000_000, 250_000, None, feature_set)
                .unwrap();
        let alice_keypair = Keypair::new();
        let alice_pubkey = alice_keypair.pubkey();
        let custodian_pubkey = custodian_keypair.pubkey();
        let nonce_pubkey = nonce_keypair.pubkey();

        assert_eq!(bank.get_balance(&custodian_pubkey), 4_750_000);
        assert_eq!(bank.get_balance(&nonce_pubkey), 250_000);

        /* Grab the hash stored in the nonce account */
        let nonce_hash = get_nonce_blockhash(&bank, &nonce_pubkey).unwrap();

        /* Kick nonce hash off the blockhash_queue */
        for _ in 0..MAX_RECENT_BLOCKHASHES + 1 {
            goto_end_of_slot(Arc::get_mut(&mut bank).unwrap());
            bank = Arc::new(new_from_parent(&bank));
        }

        /* Expect a non-Nonce transfer to fail */
        assert_eq!(
            bank.process_transaction(&system_transaction::transfer(
                &custodian_keypair,
                &alice_pubkey,
                100_000,
                nonce_hash
            ),),
            Err(TransactionError::BlockhashNotFound),
        );
        /* Check fee not charged */
        assert_eq!(bank.get_balance(&custodian_pubkey), 4_750_000);

        /* Nonce transfer */
        let nonce_tx = Transaction::new_signed_with_payer(
            &[
                system_instruction::advance_nonce_account(&nonce_pubkey, &nonce_pubkey),
                system_instruction::transfer(&custodian_pubkey, &alice_pubkey, 100_000),
            ],
            Some(&custodian_pubkey),
            &[&custodian_keypair, &nonce_keypair],
            nonce_hash,
        );
        assert_eq!(bank.process_transaction(&nonce_tx), Ok(()));

        /* Check balances */
        let mut recent_message = nonce_tx.message;
        recent_message.recent_blockhash = bank.last_blockhash();
        let mut expected_balance = 4_650_000
            - bank
                .get_fee_for_message(&recent_message.try_into().unwrap())
                .unwrap();
        assert_eq!(bank.get_balance(&custodian_pubkey), expected_balance);
        assert_eq!(bank.get_balance(&nonce_pubkey), 250_000);
        assert_eq!(bank.get_balance(&alice_pubkey), 100_000);

        /* Confirm stored nonce has advanced */
        let new_nonce = get_nonce_blockhash(&bank, &nonce_pubkey).unwrap();
        assert_ne!(nonce_hash, new_nonce);

        /* Nonce re-use fails */
        let nonce_tx = Transaction::new_signed_with_payer(
            &[
                system_instruction::advance_nonce_account(&nonce_pubkey, &nonce_pubkey),
                system_instruction::transfer(&custodian_pubkey, &alice_pubkey, 100_000),
            ],
            Some(&custodian_pubkey),
            &[&custodian_keypair, &nonce_keypair],
            nonce_hash,
        );
        assert_eq!(
            bank.process_transaction(&nonce_tx),
            Err(TransactionError::BlockhashNotFound)
        );
        /* Check fee not charged and nonce not advanced */
        assert_eq!(bank.get_balance(&custodian_pubkey), expected_balance);
        assert_eq!(
            new_nonce,
            get_nonce_blockhash(&bank, &nonce_pubkey).unwrap()
        );

        let nonce_hash = new_nonce;

        /* Kick nonce hash off the blockhash_queue */
        for _ in 0..MAX_RECENT_BLOCKHASHES + 1 {
            goto_end_of_slot(Arc::get_mut(&mut bank).unwrap());
            bank = Arc::new(new_from_parent(&bank));
        }

        let nonce_tx = Transaction::new_signed_with_payer(
            &[
                system_instruction::advance_nonce_account(&nonce_pubkey, &nonce_pubkey),
                system_instruction::transfer(&custodian_pubkey, &alice_pubkey, 100_000_000),
            ],
            Some(&custodian_pubkey),
            &[&custodian_keypair, &nonce_keypair],
            nonce_hash,
        );
        assert_eq!(
            bank.process_transaction(&nonce_tx),
            Err(TransactionError::InstructionError(
                1,
                system_instruction::SystemError::ResultWithNegativeLamports.into(),
            ))
        );
        /* Check fee charged and nonce has advanced */
        let mut recent_message = nonce_tx.message.clone();
        recent_message.recent_blockhash = bank.last_blockhash();
        expected_balance -= bank
            .get_fee_for_message(&SanitizedMessage::try_from(recent_message).unwrap())
            .unwrap();
        assert_eq!(bank.get_balance(&custodian_pubkey), expected_balance);
        assert_ne!(
            nonce_hash,
            get_nonce_blockhash(&bank, &nonce_pubkey).unwrap()
        );
        /* Confirm replaying a TX that failed with InstructionError::* now
         * fails with TransactionError::BlockhashNotFound
         */
        assert_eq!(
            bank.process_transaction(&nonce_tx),
            Err(TransactionError::BlockhashNotFound),
        );
    }

    #[test]
    fn test_nonce_authority() {
        solana_logger::setup();
        let (mut bank, _mint_keypair, custodian_keypair, nonce_keypair) = setup_nonce_with_bank(
            10_000_000,
            |_| {},
            5_000_000,
            250_000,
            None,
            FeatureSet::all_enabled(),
        )
        .unwrap();
        let alice_keypair = Keypair::new();
        let alice_pubkey = alice_keypair.pubkey();
        let custodian_pubkey = custodian_keypair.pubkey();
        let nonce_pubkey = nonce_keypair.pubkey();
        let bad_nonce_authority_keypair = Keypair::new();
        let bad_nonce_authority = bad_nonce_authority_keypair.pubkey();
        let custodian_account = bank.get_account(&custodian_pubkey).unwrap();

        debug!("alice: {}", alice_pubkey);
        debug!("custodian: {}", custodian_pubkey);
        debug!("nonce: {}", nonce_pubkey);
        debug!("nonce account: {:?}", bank.get_account(&nonce_pubkey));
        debug!("cust: {:?}", custodian_account);
        let nonce_hash = get_nonce_blockhash(&bank, &nonce_pubkey).unwrap();

        for _ in 0..MAX_RECENT_BLOCKHASHES + 1 {
            goto_end_of_slot(Arc::get_mut(&mut bank).unwrap());
            bank = Arc::new(new_from_parent(&bank));
        }

        let nonce_tx = Transaction::new_signed_with_payer(
            &[
                system_instruction::advance_nonce_account(&nonce_pubkey, &bad_nonce_authority),
                system_instruction::transfer(&custodian_pubkey, &alice_pubkey, 42),
            ],
            Some(&custodian_pubkey),
            &[&custodian_keypair, &bad_nonce_authority_keypair],
            nonce_hash,
        );
        debug!("{:?}", nonce_tx);
        let initial_custodian_balance = custodian_account.lamports();
        assert_eq!(
            bank.process_transaction(&nonce_tx),
            Err(TransactionError::BlockhashNotFound),
        );
        /* Check fee was *not* charged and nonce has *not* advanced */
        let mut recent_message = nonce_tx.message;
        recent_message.recent_blockhash = bank.last_blockhash();
        assert_eq!(
            bank.get_balance(&custodian_pubkey),
            initial_custodian_balance
        );
        assert_eq!(
            nonce_hash,
            get_nonce_blockhash(&bank, &nonce_pubkey).unwrap()
        );
    }

    #[test]
    fn test_nonce_payer() {
        solana_logger::setup();
        let nonce_starting_balance = 250_000;
        let (mut bank, _mint_keypair, custodian_keypair, nonce_keypair) = setup_nonce_with_bank(
            10_000_000,
            |_| {},
            5_000_000,
            nonce_starting_balance,
            None,
            FeatureSet::all_enabled(),
        )
        .unwrap();
        let alice_keypair = Keypair::new();
        let alice_pubkey = alice_keypair.pubkey();
        let custodian_pubkey = custodian_keypair.pubkey();
        let nonce_pubkey = nonce_keypair.pubkey();

        debug!("alice: {}", alice_pubkey);
        debug!("custodian: {}", custodian_pubkey);
        debug!("nonce: {}", nonce_pubkey);
        debug!("nonce account: {:?}", bank.get_account(&nonce_pubkey));
        debug!("cust: {:?}", bank.get_account(&custodian_pubkey));
        let nonce_hash = get_nonce_blockhash(&bank, &nonce_pubkey).unwrap();

        for _ in 0..MAX_RECENT_BLOCKHASHES + 1 {
            goto_end_of_slot(Arc::get_mut(&mut bank).unwrap());
            bank = Arc::new(new_from_parent(&bank));
        }

        let nonce_tx = Transaction::new_signed_with_payer(
            &[
                system_instruction::advance_nonce_account(&nonce_pubkey, &nonce_pubkey),
                system_instruction::transfer(&custodian_pubkey, &alice_pubkey, 100_000_000),
            ],
            Some(&nonce_pubkey),
            &[&custodian_keypair, &nonce_keypair],
            nonce_hash,
        );
        debug!("{:?}", nonce_tx);
        assert_eq!(
            bank.process_transaction(&nonce_tx),
            Err(TransactionError::InstructionError(
                1,
                system_instruction::SystemError::ResultWithNegativeLamports.into(),
            ))
        );
        /* Check fee charged and nonce has advanced */
        let mut recent_message = nonce_tx.message;
        recent_message.recent_blockhash = bank.last_blockhash();
        assert_eq!(
            bank.get_balance(&nonce_pubkey),
            nonce_starting_balance
                - bank
                    .get_fee_for_message(&recent_message.try_into().unwrap())
                    .unwrap()
        );
        assert_ne!(
            nonce_hash,
            get_nonce_blockhash(&bank, &nonce_pubkey).unwrap()
        );
    }

    #[test]
    fn test_nonce_payer_tx_wide_cap() {
        solana_logger::setup();
        let nonce_starting_balance =
            250_000 + FeeStructure::default().compute_fee_bins.last().unwrap().fee;
        let feature_set = FeatureSet::all_enabled();
        let (mut bank, _mint_keypair, custodian_keypair, nonce_keypair) = setup_nonce_with_bank(
            10_000_000,
            |_| {},
            5_000_000,
            nonce_starting_balance,
            None,
            feature_set,
        )
        .unwrap();
        let alice_keypair = Keypair::new();
        let alice_pubkey = alice_keypair.pubkey();
        let custodian_pubkey = custodian_keypair.pubkey();
        let nonce_pubkey = nonce_keypair.pubkey();

        debug!("alice: {}", alice_pubkey);
        debug!("custodian: {}", custodian_pubkey);
        debug!("nonce: {}", nonce_pubkey);
        debug!("nonce account: {:?}", bank.get_account(&nonce_pubkey));
        debug!("cust: {:?}", bank.get_account(&custodian_pubkey));
        let nonce_hash = get_nonce_blockhash(&bank, &nonce_pubkey).unwrap();

        for _ in 0..MAX_RECENT_BLOCKHASHES + 1 {
            goto_end_of_slot(Arc::get_mut(&mut bank).unwrap());
            bank = Arc::new(new_from_parent(&bank));
        }

        let nonce_tx = Transaction::new_signed_with_payer(
            &[
                system_instruction::advance_nonce_account(&nonce_pubkey, &nonce_pubkey),
                system_instruction::transfer(&custodian_pubkey, &alice_pubkey, 100_000_000),
            ],
            Some(&nonce_pubkey),
            &[&custodian_keypair, &nonce_keypair],
            nonce_hash,
        );
        debug!("{:?}", nonce_tx);

        assert_eq!(
            bank.process_transaction(&nonce_tx),
            Err(TransactionError::InstructionError(
                1,
                system_instruction::SystemError::ResultWithNegativeLamports.into(),
            ))
        );
        /* Check fee charged and nonce has advanced */
        let mut recent_message = nonce_tx.message;
        recent_message.recent_blockhash = bank.last_blockhash();
        assert_eq!(
            bank.get_balance(&nonce_pubkey),
            nonce_starting_balance
                - bank
                    .get_fee_for_message(&recent_message.try_into().unwrap())
                    .unwrap()
        );
        assert_ne!(
            nonce_hash,
            get_nonce_blockhash(&bank, &nonce_pubkey).unwrap()
        );
    }

    #[test]
    fn test_nonce_fee_calculator_updates() {
        let (mut genesis_config, mint_keypair) = create_genesis_config(1_000_000);
        genesis_config.rent.lamports_per_byte_year = 0;
        let mut bank = Bank::new_for_tests(&genesis_config);
        bank.feature_set = Arc::new(FeatureSet::all_enabled());
        let mut bank = Arc::new(bank);

        // Deliberately use bank 0 to initialize nonce account, so that nonce account fee_calculator indicates 0 fees
        let (custodian_keypair, nonce_keypair) =
            nonce_setup(&mut bank, &mint_keypair, 500_000, 100_000, None).unwrap();
        let custodian_pubkey = custodian_keypair.pubkey();
        let nonce_pubkey = nonce_keypair.pubkey();

        // Grab the hash and fee_calculator stored in the nonce account
        let (stored_nonce_hash, stored_fee_calculator) = bank
            .get_account(&nonce_pubkey)
            .and_then(|acc| {
                let nonce_versions = StateMut::<nonce::state::Versions>::state(&acc);
                match nonce_versions.ok()?.state() {
                    nonce::State::Initialized(ref data) => {
                        Some((data.blockhash(), data.fee_calculator))
                    }
                    _ => None,
                }
            })
            .unwrap();

        // Kick nonce hash off the blockhash_queue
        for _ in 0..MAX_RECENT_BLOCKHASHES + 1 {
            goto_end_of_slot(Arc::get_mut(&mut bank).unwrap());
            bank = Arc::new(new_from_parent(&bank));
        }

        // Nonce transfer
        let nonce_tx = Transaction::new_signed_with_payer(
            &[
                system_instruction::advance_nonce_account(&nonce_pubkey, &nonce_pubkey),
                system_instruction::transfer(
                    &custodian_pubkey,
                    &solana_sdk::pubkey::new_rand(),
                    100_000,
                ),
            ],
            Some(&custodian_pubkey),
            &[&custodian_keypair, &nonce_keypair],
            stored_nonce_hash,
        );
        bank.process_transaction(&nonce_tx).unwrap();

        // Grab the new hash and fee_calculator; both should be updated
        let (nonce_hash, fee_calculator) = bank
            .get_account(&nonce_pubkey)
            .and_then(|acc| {
                let nonce_versions = StateMut::<nonce::state::Versions>::state(&acc);
                match nonce_versions.ok()?.state() {
                    nonce::State::Initialized(ref data) => {
                        Some((data.blockhash(), data.fee_calculator))
                    }
                    _ => None,
                }
            })
            .unwrap();

        assert_ne!(stored_nonce_hash, nonce_hash);
        assert_ne!(stored_fee_calculator, fee_calculator);
    }

    #[test]
    fn test_nonce_fee_calculator_updates_tx_wide_cap() {
        let (mut genesis_config, mint_keypair) = create_genesis_config(1_000_000);
        genesis_config.rent.lamports_per_byte_year = 0;
        let mut bank = Bank::new_for_tests(&genesis_config);
        bank.feature_set = Arc::new(FeatureSet::all_enabled());
        let mut bank = Arc::new(bank);

        // Deliberately use bank 0 to initialize nonce account, so that nonce account fee_calculator indicates 0 fees
        let (custodian_keypair, nonce_keypair) =
            nonce_setup(&mut bank, &mint_keypair, 500_000, 100_000, None).unwrap();
        let custodian_pubkey = custodian_keypair.pubkey();
        let nonce_pubkey = nonce_keypair.pubkey();

        // Grab the hash and fee_calculator stored in the nonce account
        let (stored_nonce_hash, stored_fee_calculator) = bank
            .get_account(&nonce_pubkey)
            .and_then(|acc| {
                let nonce_versions = StateMut::<nonce::state::Versions>::state(&acc);
                match nonce_versions.ok()?.state() {
                    nonce::State::Initialized(ref data) => {
                        Some((data.blockhash(), data.fee_calculator))
                    }
                    _ => None,
                }
            })
            .unwrap();

        // Kick nonce hash off the blockhash_queue
        for _ in 0..MAX_RECENT_BLOCKHASHES + 1 {
            goto_end_of_slot(Arc::get_mut(&mut bank).unwrap());
            bank = Arc::new(new_from_parent(&bank));
        }

        // Nonce transfer
        let nonce_tx = Transaction::new_signed_with_payer(
            &[
                system_instruction::advance_nonce_account(&nonce_pubkey, &nonce_pubkey),
                system_instruction::transfer(
                    &custodian_pubkey,
                    &solana_sdk::pubkey::new_rand(),
                    100_000,
                ),
            ],
            Some(&custodian_pubkey),
            &[&custodian_keypair, &nonce_keypair],
            stored_nonce_hash,
        );
        bank.process_transaction(&nonce_tx).unwrap();

        // Grab the new hash and fee_calculator; both should be updated
        let (nonce_hash, fee_calculator) = bank
            .get_account(&nonce_pubkey)
            .and_then(|acc| {
                let nonce_versions = StateMut::<nonce::state::Versions>::state(&acc);
                match nonce_versions.ok()?.state() {
                    nonce::State::Initialized(ref data) => {
                        Some((data.blockhash(), data.fee_calculator))
                    }
                    _ => None,
                }
            })
            .unwrap();

        assert_ne!(stored_nonce_hash, nonce_hash);
        assert_ne!(stored_fee_calculator, fee_calculator);
    }

    #[test]
    fn test_check_ro_durable_nonce_fails() {
        let (mut bank, _mint_keypair, custodian_keypair, nonce_keypair) = setup_nonce_with_bank(
            10_000_000,
            |_| {},
            5_000_000,
            250_000,
            None,
            FeatureSet::all_enabled(),
        )
        .unwrap();
        let custodian_pubkey = custodian_keypair.pubkey();
        let nonce_pubkey = nonce_keypair.pubkey();

        let nonce_hash = get_nonce_blockhash(&bank, &nonce_pubkey).unwrap();
        let account_metas = vec![
            AccountMeta::new_readonly(nonce_pubkey, false),
            #[allow(deprecated)]
            AccountMeta::new_readonly(sysvar::recent_blockhashes::id(), false),
            AccountMeta::new_readonly(nonce_pubkey, true),
        ];
        let nonce_instruction = Instruction::new_with_bincode(
            system_program::id(),
            &system_instruction::SystemInstruction::AdvanceNonceAccount,
            account_metas,
        );
        let tx = Transaction::new_signed_with_payer(
            &[nonce_instruction],
            Some(&custodian_pubkey),
            &[&custodian_keypair, &nonce_keypair],
            nonce_hash,
        );
        // SanitizedMessage::get_durable_nonce returns None because nonce
        // account is not writable. Durable nonce and blockhash domains are
        // separate, so the recent_blockhash (== durable nonce) in the
        // transaction is not found in the hash queue.
        assert_eq!(
            bank.process_transaction(&tx),
            Err(TransactionError::BlockhashNotFound),
        );
        // Kick nonce hash off the blockhash_queue
        for _ in 0..MAX_RECENT_BLOCKHASHES + 1 {
            goto_end_of_slot(Arc::get_mut(&mut bank).unwrap());
            bank = Arc::new(new_from_parent(&bank));
        }
        // Caught by the runtime because it is a nonce transaction
        assert_eq!(
            bank.process_transaction(&tx),
            Err(TransactionError::BlockhashNotFound)
        );
        assert_eq!(
            bank.check_transaction_for_nonce(
                &SanitizedTransaction::from_transaction_for_tests(tx),
                &bank.next_durable_nonce(),
            ),
            None
        );
    }

    #[test]
    fn test_collect_balances() {
        let parent = create_simple_test_arc_bank(500);
        let bank0 = Arc::new(new_from_parent(&parent));

        let keypair = Keypair::new();
        let pubkey0 = solana_sdk::pubkey::new_rand();
        let pubkey1 = solana_sdk::pubkey::new_rand();
        let program_id = Pubkey::new(&[2; 32]);
        let keypair_account = AccountSharedData::new(8, 0, &program_id);
        let account0 = AccountSharedData::new(11, 0, &program_id);
        let program_account = AccountSharedData::new(1, 10, &Pubkey::default());
        bank0.store_account(&keypair.pubkey(), &keypair_account);
        bank0.store_account(&pubkey0, &account0);
        bank0.store_account(&program_id, &program_account);

        let instructions = vec![CompiledInstruction::new(1, &(), vec![0])];
        let tx0 = Transaction::new_with_compiled_instructions(
            &[&keypair],
            &[pubkey0],
            Hash::default(),
            vec![program_id],
            instructions,
        );
        let instructions = vec![CompiledInstruction::new(1, &(), vec![0])];
        let tx1 = Transaction::new_with_compiled_instructions(
            &[&keypair],
            &[pubkey1],
            Hash::default(),
            vec![program_id],
            instructions,
        );
        let txs = vec![tx0, tx1];
        let batch = bank0.prepare_batch_for_tests(txs.clone());
        let balances = bank0.collect_balances(&batch);
        assert_eq!(balances.len(), 2);
        assert_eq!(balances[0], vec![8, 11, 1]);
        assert_eq!(balances[1], vec![8, 0, 1]);

        let txs: Vec<_> = txs.into_iter().rev().collect();
        let batch = bank0.prepare_batch_for_tests(txs);
        let balances = bank0.collect_balances(&batch);
        assert_eq!(balances.len(), 2);
        assert_eq!(balances[0], vec![8, 0, 1]);
        assert_eq!(balances[1], vec![8, 11, 1]);
    }

    #[test]
    fn test_pre_post_transaction_balances() {
        let (mut genesis_config, _mint_keypair) = create_genesis_config(500_000);
        let fee_rate_governor = FeeRateGovernor::new(5000, 0);
        genesis_config.fee_rate_governor = fee_rate_governor;
        let parent = Arc::new(Bank::new_for_tests(&genesis_config));
        let bank0 = Arc::new(new_from_parent(&parent));

        let keypair0 = Keypair::new();
        let keypair1 = Keypair::new();
        let pubkey0 = solana_sdk::pubkey::new_rand();
        let pubkey1 = solana_sdk::pubkey::new_rand();
        let pubkey2 = solana_sdk::pubkey::new_rand();
        let keypair0_account = AccountSharedData::new(908_000, 0, &Pubkey::default());
        let keypair1_account = AccountSharedData::new(909_000, 0, &Pubkey::default());
        let account0 = AccountSharedData::new(911_000, 0, &Pubkey::default());
        bank0.store_account(&keypair0.pubkey(), &keypair0_account);
        bank0.store_account(&keypair1.pubkey(), &keypair1_account);
        bank0.store_account(&pubkey0, &account0);

        let blockhash = bank0.last_blockhash();

        let tx0 = system_transaction::transfer(&keypair0, &pubkey0, 2_000, blockhash);
        let tx1 = system_transaction::transfer(&Keypair::new(), &pubkey1, 2_000, blockhash);
        let tx2 = system_transaction::transfer(&keypair1, &pubkey2, 912_000, blockhash);
        let txs = vec![tx0, tx1, tx2];

        let lock_result = bank0.prepare_batch_for_tests(txs);
        let (transaction_results, transaction_balances_set) = bank0
            .load_execute_and_commit_transactions(
                &lock_result,
                MAX_PROCESSING_AGE,
                true,
                false,
                false,
                false,
                &mut ExecuteTimings::default(),
                None,
            );

        assert_eq!(transaction_balances_set.pre_balances.len(), 3);
        assert_eq!(transaction_balances_set.post_balances.len(), 3);

        assert!(transaction_results.execution_results[0].was_executed_successfully());
        assert_eq!(
            transaction_balances_set.pre_balances[0],
            vec![908_000, 911_000, 1]
        );
        assert_eq!(
            transaction_balances_set.post_balances[0],
            vec![901_000, 913_000, 1]
        );

        // Failed transactions still produce balance sets
        // This is a TransactionError - not possible to charge fees
        assert!(matches!(
            transaction_results.execution_results[1],
            TransactionExecutionResult::NotExecuted(TransactionError::AccountNotFound),
        ));
        assert_eq!(transaction_balances_set.pre_balances[1], vec![0, 0, 1]);
        assert_eq!(transaction_balances_set.post_balances[1], vec![0, 0, 1]);

        // Failed transactions still produce balance sets
        // This is an InstructionError - fees charged
        assert!(matches!(
            transaction_results.execution_results[2],
            TransactionExecutionResult::Executed {
                details: TransactionExecutionDetails {
                    status: Err(TransactionError::InstructionError(
                        0,
                        InstructionError::Custom(1),
                    )),
                    ..
                },
                ..
            },
        ));
        assert_eq!(
            transaction_balances_set.pre_balances[2],
            vec![909_000, 0, 1]
        );
        assert_eq!(
            transaction_balances_set.post_balances[2],
            vec![904_000, 0, 1]
        );
    }

    #[test]
    fn test_transaction_with_duplicate_accounts_in_instruction() {
        let (genesis_config, mint_keypair) = create_genesis_config(500);
        let mut bank = Bank::new_for_tests(&genesis_config);

        fn mock_process_instruction(
            _first_instruction_account: IndexOfAccount,
            invoke_context: &mut InvokeContext,
        ) -> result::Result<(), InstructionError> {
            let transaction_context = &invoke_context.transaction_context;
            let instruction_context = transaction_context.get_current_instruction_context()?;
            let instruction_data = instruction_context.get_instruction_data();
            let lamports = u64::from_le_bytes(instruction_data.try_into().unwrap());
            instruction_context
                .try_borrow_instruction_account(transaction_context, 2)?
                .checked_sub_lamports(lamports)?;
            instruction_context
                .try_borrow_instruction_account(transaction_context, 1)?
                .checked_add_lamports(lamports)?;
            instruction_context
                .try_borrow_instruction_account(transaction_context, 0)?
                .checked_sub_lamports(lamports)?;
            instruction_context
                .try_borrow_instruction_account(transaction_context, 1)?
                .checked_add_lamports(lamports)?;
            Ok(())
        }

        let mock_program_id = Pubkey::new(&[2u8; 32]);
        bank.add_builtin("mock_program", &mock_program_id, mock_process_instruction);

        let from_pubkey = solana_sdk::pubkey::new_rand();
        let to_pubkey = solana_sdk::pubkey::new_rand();
        let dup_pubkey = from_pubkey;
        let from_account = AccountSharedData::new(sol_to_lamports(100.), 1, &mock_program_id);
        let to_account = AccountSharedData::new(0, 1, &mock_program_id);
        bank.store_account(&from_pubkey, &from_account);
        bank.store_account(&to_pubkey, &to_account);

        let account_metas = vec![
            AccountMeta::new(from_pubkey, false),
            AccountMeta::new(to_pubkey, false),
            AccountMeta::new(dup_pubkey, false),
        ];
        let instruction =
            Instruction::new_with_bincode(mock_program_id, &sol_to_lamports(10.), account_metas);
        let tx = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&mint_keypair.pubkey()),
            &[&mint_keypair],
            bank.last_blockhash(),
        );

        let result = bank.process_transaction(&tx);
        assert_eq!(result, Ok(()));
        assert_eq!(bank.get_balance(&from_pubkey), sol_to_lamports(80.));
        assert_eq!(bank.get_balance(&to_pubkey), sol_to_lamports(20.));
    }

    #[test]
    fn test_transaction_with_program_ids_passed_to_programs() {
        let (genesis_config, mint_keypair) = create_genesis_config(500);
        let mut bank = Bank::new_for_tests(&genesis_config);

        #[allow(clippy::unnecessary_wraps)]
        fn mock_process_instruction(
            _first_instruction_account: IndexOfAccount,
            _invoke_context: &mut InvokeContext,
        ) -> result::Result<(), InstructionError> {
            Ok(())
        }

        let mock_program_id = Pubkey::new(&[2u8; 32]);
        bank.add_builtin("mock_program", &mock_program_id, mock_process_instruction);

        let from_pubkey = solana_sdk::pubkey::new_rand();
        let to_pubkey = solana_sdk::pubkey::new_rand();
        let dup_pubkey = from_pubkey;
        let from_account = AccountSharedData::new(100, 1, &mock_program_id);
        let to_account = AccountSharedData::new(0, 1, &mock_program_id);
        bank.store_account(&from_pubkey, &from_account);
        bank.store_account(&to_pubkey, &to_account);

        let account_metas = vec![
            AccountMeta::new(from_pubkey, false),
            AccountMeta::new(to_pubkey, false),
            AccountMeta::new(dup_pubkey, false),
            AccountMeta::new(mock_program_id, false),
        ];
        let instruction = Instruction::new_with_bincode(mock_program_id, &10, account_metas);
        let tx = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&mint_keypair.pubkey()),
            &[&mint_keypair],
            bank.last_blockhash(),
        );

        let result = bank.process_transaction(&tx);
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn test_account_ids_after_program_ids() {
        solana_logger::setup();
        let (genesis_config, mint_keypair) = create_genesis_config(500);
        let mut bank = Bank::new_for_tests(&genesis_config);

        let from_pubkey = solana_sdk::pubkey::new_rand();
        let to_pubkey = solana_sdk::pubkey::new_rand();

        let account_metas = vec![
            AccountMeta::new(from_pubkey, false),
            AccountMeta::new(to_pubkey, false),
        ];

        let instruction =
            Instruction::new_with_bincode(solana_vote_program::id(), &10, account_metas);
        let mut tx = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&mint_keypair.pubkey()),
            &[&mint_keypair],
            bank.last_blockhash(),
        );

        tx.message.account_keys.push(solana_sdk::pubkey::new_rand());

        bank.add_builtin(
            "mock_vote",
            &solana_vote_program::id(),
            mock_ok_vote_processor,
        );
        let result = bank.process_transaction(&tx);
        assert_eq!(result, Ok(()));
        let account = bank.get_account(&solana_vote_program::id()).unwrap();
        info!("account: {:?}", account);
        assert!(account.executable());
    }

    #[test]
    fn test_incinerator() {
        let (genesis_config, mint_keypair) = create_genesis_config(1_000_000_000_000);
        let bank0 = Arc::new(Bank::new_for_tests(&genesis_config));

        // Move to the first normal slot so normal rent behaviour applies
        let bank = Bank::new_from_parent(
            &bank0,
            &Pubkey::default(),
            genesis_config.epoch_schedule.first_normal_slot,
        );
        let pre_capitalization = bank.capitalization();

        // Burn a non-rent exempt amount
        let burn_amount = bank.get_minimum_balance_for_rent_exemption(0) - 1;

        assert_eq!(bank.get_balance(&incinerator::id()), 0);
        bank.transfer(burn_amount, &mint_keypair, &incinerator::id())
            .unwrap();
        assert_eq!(bank.get_balance(&incinerator::id()), burn_amount);
        bank.freeze();
        assert_eq!(bank.get_balance(&incinerator::id()), 0);

        // Ensure that no rent was collected, and the entire burn amount was removed from bank
        // capitalization
        assert_eq!(bank.capitalization(), pre_capitalization - burn_amount);
    }

    #[test]
    fn test_duplicate_account_key() {
        solana_logger::setup();
        let (genesis_config, mint_keypair) = create_genesis_config(500);
        let mut bank = Bank::new_for_tests(&genesis_config);

        let from_pubkey = solana_sdk::pubkey::new_rand();
        let to_pubkey = solana_sdk::pubkey::new_rand();

        let account_metas = vec![
            AccountMeta::new(from_pubkey, false),
            AccountMeta::new(to_pubkey, false),
        ];

        bank.add_builtin(
            "mock_vote",
            &solana_vote_program::id(),
            mock_ok_vote_processor,
        );

        let instruction =
            Instruction::new_with_bincode(solana_vote_program::id(), &10, account_metas);
        let mut tx = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&mint_keypair.pubkey()),
            &[&mint_keypair],
            bank.last_blockhash(),
        );
        tx.message.account_keys.push(from_pubkey);

        let result = bank.process_transaction(&tx);
        assert_eq!(result, Err(TransactionError::AccountLoadedTwice));
    }

    #[test]
    fn test_process_transaction_with_too_many_account_locks() {
        solana_logger::setup();
        let (genesis_config, mint_keypair) = create_genesis_config(500);
        let mut bank = Bank::new_for_tests(&genesis_config);

        let from_pubkey = solana_sdk::pubkey::new_rand();
        let to_pubkey = solana_sdk::pubkey::new_rand();

        let account_metas = vec![
            AccountMeta::new(from_pubkey, false),
            AccountMeta::new(to_pubkey, false),
        ];

        bank.add_builtin(
            "mock_vote",
            &solana_vote_program::id(),
            mock_ok_vote_processor,
        );

        let instruction =
            Instruction::new_with_bincode(solana_vote_program::id(), &10, account_metas);
        let mut tx = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&mint_keypair.pubkey()),
            &[&mint_keypair],
            bank.last_blockhash(),
        );

        let transaction_account_lock_limit = bank.get_transaction_account_lock_limit();
        while tx.message.account_keys.len() <= transaction_account_lock_limit {
            tx.message.account_keys.push(solana_sdk::pubkey::new_rand());
        }

        let result = bank.process_transaction(&tx);
        assert_eq!(result, Err(TransactionError::TooManyAccountLocks));
    }

    #[test]
    fn test_program_id_as_payer() {
        solana_logger::setup();
        let (genesis_config, mint_keypair) = create_genesis_config(500);
        let mut bank = Bank::new_for_tests(&genesis_config);

        let from_pubkey = solana_sdk::pubkey::new_rand();
        let to_pubkey = solana_sdk::pubkey::new_rand();

        let account_metas = vec![
            AccountMeta::new(from_pubkey, false),
            AccountMeta::new(to_pubkey, false),
        ];

        bank.add_builtin(
            "mock_vote",
            &solana_vote_program::id(),
            mock_ok_vote_processor,
        );

        let instruction =
            Instruction::new_with_bincode(solana_vote_program::id(), &10, account_metas);
        let mut tx = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&mint_keypair.pubkey()),
            &[&mint_keypair],
            bank.last_blockhash(),
        );

        info!(
            "mint: {} account keys: {:?}",
            mint_keypair.pubkey(),
            tx.message.account_keys
        );
        assert_eq!(tx.message.account_keys.len(), 4);
        tx.message.account_keys.clear();
        tx.message.account_keys.push(solana_vote_program::id());
        tx.message.account_keys.push(mint_keypair.pubkey());
        tx.message.account_keys.push(from_pubkey);
        tx.message.account_keys.push(to_pubkey);
        tx.message.instructions[0].program_id_index = 0;
        tx.message.instructions[0].accounts.clear();
        tx.message.instructions[0].accounts.push(2);
        tx.message.instructions[0].accounts.push(3);

        let result = bank.process_transaction(&tx);
        assert_eq!(result, Err(TransactionError::SanitizeFailure));
    }

    #[allow(clippy::unnecessary_wraps)]
    fn mock_ok_vote_processor(
        _first_instruction_account: IndexOfAccount,
        _invoke_context: &mut InvokeContext,
    ) -> std::result::Result<(), InstructionError> {
        Ok(())
    }

    #[test]
    fn test_ref_account_key_after_program_id() {
        let (genesis_config, mint_keypair) = create_genesis_config(500);
        let mut bank = Bank::new_for_tests(&genesis_config);

        let from_pubkey = solana_sdk::pubkey::new_rand();
        let to_pubkey = solana_sdk::pubkey::new_rand();

        let account_metas = vec![
            AccountMeta::new(from_pubkey, false),
            AccountMeta::new(to_pubkey, false),
        ];

        bank.add_builtin(
            "mock_vote",
            &solana_vote_program::id(),
            mock_ok_vote_processor,
        );

        let instruction =
            Instruction::new_with_bincode(solana_vote_program::id(), &10, account_metas);
        let mut tx = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&mint_keypair.pubkey()),
            &[&mint_keypair],
            bank.last_blockhash(),
        );

        tx.message.account_keys.push(solana_sdk::pubkey::new_rand());
        assert_eq!(tx.message.account_keys.len(), 5);
        tx.message.instructions[0].accounts.remove(0);
        tx.message.instructions[0].accounts.push(4);

        let result = bank.process_transaction(&tx);
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn test_fuzz_instructions() {
        solana_logger::setup();
        use rand::{thread_rng, Rng};
        let mut bank = create_simple_test_bank(1_000_000_000);

        let max_programs = 5;
        let program_keys: Vec<_> = (0..max_programs)
            .enumerate()
            .map(|i| {
                let key = solana_sdk::pubkey::new_rand();
                let name = format!("program{i:?}");
                bank.add_builtin(&name, &key, mock_ok_vote_processor);
                (key, name.as_bytes().to_vec())
            })
            .collect();
        let max_keys = 100;
        let keys: Vec<_> = (0..max_keys)
            .enumerate()
            .map(|_| {
                let key = solana_sdk::pubkey::new_rand();
                let balance = if thread_rng().gen_ratio(9, 10) {
                    let lamports = if thread_rng().gen_ratio(1, 5) {
                        thread_rng().gen_range(0, 10)
                    } else {
                        thread_rng().gen_range(20, 100)
                    };
                    let space = thread_rng().gen_range(0, 10);
                    let owner = Pubkey::default();
                    let account = AccountSharedData::new(lamports, space, &owner);
                    bank.store_account(&key, &account);
                    lamports
                } else {
                    0
                };
                (key, balance)
            })
            .collect();
        let mut results = HashMap::new();
        for _ in 0..2_000 {
            let num_keys = if thread_rng().gen_ratio(1, 5) {
                thread_rng().gen_range(0, max_keys)
            } else {
                thread_rng().gen_range(1, 4)
            };
            let num_instructions = thread_rng().gen_range(0, max_keys - num_keys);

            let mut account_keys: Vec<_> = if thread_rng().gen_ratio(1, 5) {
                (0..num_keys)
                    .map(|_| {
                        let idx = thread_rng().gen_range(0, keys.len());
                        keys[idx].0
                    })
                    .collect()
            } else {
                let mut inserted = HashSet::new();
                (0..num_keys)
                    .map(|_| {
                        let mut idx;
                        loop {
                            idx = thread_rng().gen_range(0, keys.len());
                            if !inserted.contains(&idx) {
                                break;
                            }
                        }
                        inserted.insert(idx);
                        keys[idx].0
                    })
                    .collect()
            };

            let instructions: Vec<_> = if num_keys > 0 {
                (0..num_instructions)
                    .map(|_| {
                        let num_accounts_to_pass = thread_rng().gen_range(0, num_keys);
                        let account_indexes = (0..num_accounts_to_pass)
                            .map(|_| thread_rng().gen_range(0, num_keys))
                            .collect();
                        let program_index: u8 = thread_rng().gen_range(0, num_keys);
                        if thread_rng().gen_ratio(4, 5) {
                            let programs_index = thread_rng().gen_range(0, program_keys.len());
                            account_keys[program_index as usize] = program_keys[programs_index].0;
                        }
                        CompiledInstruction::new(program_index, &10, account_indexes)
                    })
                    .collect()
            } else {
                vec![]
            };

            let account_keys_len = std::cmp::max(account_keys.len(), 2);
            let num_signatures = if thread_rng().gen_ratio(1, 5) {
                thread_rng().gen_range(0, account_keys_len + 10)
            } else {
                thread_rng().gen_range(1, account_keys_len)
            };

            let num_required_signatures = if thread_rng().gen_ratio(1, 5) {
                thread_rng().gen_range(0, account_keys_len + 10) as u8
            } else {
                thread_rng().gen_range(1, std::cmp::max(2, num_signatures)) as u8
            };
            let num_readonly_signed_accounts = if thread_rng().gen_ratio(1, 5) {
                thread_rng().gen_range(0, account_keys_len) as u8
            } else {
                let max = if num_required_signatures > 1 {
                    num_required_signatures - 1
                } else {
                    1
                };
                thread_rng().gen_range(0, max)
            };

            let num_readonly_unsigned_accounts = if thread_rng().gen_ratio(1, 5)
                || (num_required_signatures as usize) >= account_keys_len
            {
                thread_rng().gen_range(0, account_keys_len) as u8
            } else {
                thread_rng().gen_range(0, account_keys_len - num_required_signatures as usize) as u8
            };

            let header = MessageHeader {
                num_required_signatures,
                num_readonly_signed_accounts,
                num_readonly_unsigned_accounts,
            };
            let message = Message {
                header,
                account_keys,
                recent_blockhash: bank.last_blockhash(),
                instructions,
            };

            let tx = Transaction {
                signatures: vec![Signature::default(); num_signatures],
                message,
            };

            let result = bank.process_transaction(&tx);
            for (key, balance) in &keys {
                assert_eq!(bank.get_balance(key), *balance);
            }
            for (key, name) in &program_keys {
                let account = bank.get_account(key).unwrap();
                assert!(account.executable());
                assert_eq!(account.data(), name);
            }
            info!("result: {:?}", result);
            let result_key = format!("{result:?}");
            *results.entry(result_key).or_insert(0) += 1;
        }
        info!("results: {:?}", results);
    }

    #[test]
    fn test_bank_hash_consistency() {
        solana_logger::setup();

        let mut genesis_config = GenesisConfig::new(
            &[(
                Pubkey::new(&[42; 32]),
                AccountSharedData::new(1_000_000_000_000, 0, &system_program::id()),
            )],
            &[],
        );
        genesis_config.creation_time = 0;
        genesis_config.cluster_type = ClusterType::MainnetBeta;
        genesis_config.rent.burn_percent = 100;
        let mut bank = Arc::new(Bank::new_for_tests(&genesis_config));
        // Check a few slots, cross an epoch boundary
        assert_eq!(bank.get_slots_in_epoch(0), 32);
        loop {
            goto_end_of_slot(Arc::get_mut(&mut bank).unwrap());
            if bank.slot == 0 {
                assert_eq!(
                    bank.hash().to_string(),
                    "5gY6TCgB9NymbbxgFgAjvYLpXjyXiVyyruS1aEwbWKLK"
                );
            }
            if bank.slot == 32 {
                assert_eq!(
                    bank.hash().to_string(),
                    "6uJ5C4QDXWCN39EjJ5Frcz73nnS2jMJ55KgkQff12Fqp"
                );
            }
            if bank.slot == 64 {
                assert_eq!(
                    bank.hash().to_string(),
                    "Ddk6ouAvSSA1U3Cw6BoKdM5v5LdRc9ShruGDzci9fKbY"
                );
            }
            if bank.slot == 128 {
                assert_eq!(
                    bank.hash().to_string(),
                    "ANodC5vnedLWqeAyhcoErzR3ptNansb5YX6UTQ9cfP7S"
                );
                break;
            }
            bank = Arc::new(new_from_parent(&bank));
        }
    }

    #[test]
    fn test_same_program_id_uses_unqiue_executable_accounts() {
        fn nested_processor(
            _first_instruction_account: IndexOfAccount,
            invoke_context: &mut InvokeContext,
        ) -> result::Result<(), InstructionError> {
            let transaction_context = &invoke_context.transaction_context;
            let instruction_context = transaction_context.get_current_instruction_context()?;
            let _ = instruction_context
                .try_borrow_program_account(transaction_context, 1)?
                .checked_add_lamports(1);
            Ok(())
        }

        let (genesis_config, mint_keypair) = create_genesis_config(50000);
        let mut bank = Bank::new_for_tests(&genesis_config);

        // Add a new program
        let program1_pubkey = solana_sdk::pubkey::new_rand();
        bank.add_builtin("program", &program1_pubkey, nested_processor);

        // Add a new program owned by the first
        let program2_pubkey = solana_sdk::pubkey::new_rand();
        let mut program2_account = AccountSharedData::new(42, 1, &program1_pubkey);
        program2_account.set_executable(true);
        bank.store_account(&program2_pubkey, &program2_account);

        let instruction = Instruction::new_with_bincode(program2_pubkey, &10, vec![]);
        let tx = Transaction::new_signed_with_payer(
            &[instruction.clone(), instruction],
            Some(&mint_keypair.pubkey()),
            &[&mint_keypair],
            bank.last_blockhash(),
        );
        assert!(bank.process_transaction(&tx).is_ok());
        assert_eq!(1, bank.get_balance(&program1_pubkey));
        assert_eq!(42, bank.get_balance(&program2_pubkey));
    }

    fn get_shrink_account_size() -> usize {
        let (genesis_config, _mint_keypair) = create_genesis_config(1_000_000_000);

        // Set root for bank 0, with caching disabled so we can get the size
        // of the storage for this slot
        let mut bank0 = Arc::new(Bank::new_with_config_for_tests(
            &genesis_config,
            AccountSecondaryIndexes::default(),
            AccountShrinkThreshold::default(),
        ));
        bank0.restore_old_behavior_for_fragile_tests();
        goto_end_of_slot(Arc::<Bank>::get_mut(&mut bank0).unwrap());
        bank0.freeze();
        bank0.squash();
        add_root_and_flush_write_cache(&bank0);

        let sizes = bank0
            .rc
            .accounts
            .accounts_db
            .sizes_of_accounts_in_storage_for_tests(0);

        // Create an account such that it takes DEFAULT_ACCOUNTS_SHRINK_RATIO of the total account space for
        // the slot, so when it gets pruned, the storage entry will become a shrink candidate.
        let bank0_total_size: usize = sizes.into_iter().sum();
        let pubkey0_size = (bank0_total_size as f64 / (1.0 - DEFAULT_ACCOUNTS_SHRINK_RATIO)).ceil();
        assert!(
            pubkey0_size / (pubkey0_size + bank0_total_size as f64) > DEFAULT_ACCOUNTS_SHRINK_RATIO
        );
        pubkey0_size as usize
    }

    #[test]
    fn test_clean_nonrooted() {
        solana_logger::setup();

        let (genesis_config, _mint_keypair) = create_genesis_config(1_000_000_000);
        let pubkey0 = Pubkey::new(&[0; 32]);
        let pubkey1 = Pubkey::new(&[1; 32]);

        info!("pubkey0: {}", pubkey0);
        info!("pubkey1: {}", pubkey1);

        // Set root for bank 0, with caching enabled
        let mut bank0 = Arc::new(Bank::new_with_config_for_tests(
            &genesis_config,
            AccountSecondaryIndexes::default(),
            AccountShrinkThreshold::default(),
        ));

        let account_zero = AccountSharedData::new(0, 0, &Pubkey::new_unique());

        goto_end_of_slot(Arc::<Bank>::get_mut(&mut bank0).unwrap());
        bank0.freeze();
        bank0.squash();
        // Flush now so that accounts cache cleaning doesn't clean up bank 0 when later
        // slots add updates to the cache
        bank0.force_flush_accounts_cache();

        // Store some lamports in bank 1
        let some_lamports = 123;
        let mut bank1 = Arc::new(Bank::new_from_parent(&bank0, &Pubkey::default(), 1));
        bank1.deposit(&pubkey0, some_lamports).unwrap();
        goto_end_of_slot(Arc::<Bank>::get_mut(&mut bank1).unwrap());
        bank1.freeze();
        bank1.flush_accounts_cache_slot_for_tests();

        bank1.print_accounts_stats();

        // Store some lamports for pubkey1 in bank 2, root bank 2
        // bank2's parent is bank0
        let mut bank2 = Arc::new(Bank::new_from_parent(&bank0, &Pubkey::default(), 2));
        bank2.deposit(&pubkey1, some_lamports).unwrap();
        bank2.store_account(&pubkey0, &account_zero);
        goto_end_of_slot(Arc::<Bank>::get_mut(&mut bank2).unwrap());
        bank2.freeze();
        bank2.squash();
        bank2.force_flush_accounts_cache();

        bank2.print_accounts_stats();
        drop(bank1);

        // Clean accounts, which should add earlier slots to the shrink
        // candidate set
        bank2.clean_accounts_for_tests();

        let mut bank3 = Arc::new(Bank::new_from_parent(&bank2, &Pubkey::default(), 3));
        bank3.deposit(&pubkey1, some_lamports + 1).unwrap();
        goto_end_of_slot(Arc::<Bank>::get_mut(&mut bank3).unwrap());
        bank3.freeze();
        bank3.squash();
        bank3.force_flush_accounts_cache();

        bank3.clean_accounts_for_tests();
        assert_eq!(
            bank3.rc.accounts.accounts_db.ref_count_for_pubkey(&pubkey0),
            2
        );
        assert!(bank3
            .rc
            .accounts
            .accounts_db
            .storage
            .get_slot_stores(1)
            .is_none());

        bank3.print_accounts_stats();
    }

    #[test]
    fn test_shrink_candidate_slots_cached() {
        solana_logger::setup();

        let (genesis_config, _mint_keypair) = create_genesis_config(1_000_000_000);
        let pubkey0 = solana_sdk::pubkey::new_rand();
        let pubkey1 = solana_sdk::pubkey::new_rand();
        let pubkey2 = solana_sdk::pubkey::new_rand();

        // Set root for bank 0, with caching enabled
        let mut bank0 = Arc::new(Bank::new_with_config_for_tests(
            &genesis_config,
            AccountSecondaryIndexes::default(),
            AccountShrinkThreshold::default(),
        ));
        bank0.restore_old_behavior_for_fragile_tests();

        let pubkey0_size = get_shrink_account_size();

        let account0 = AccountSharedData::new(1000, pubkey0_size, &Pubkey::new_unique());
        bank0.store_account(&pubkey0, &account0);

        goto_end_of_slot(Arc::<Bank>::get_mut(&mut bank0).unwrap());
        bank0.freeze();
        bank0.squash();
        // Flush now so that accounts cache cleaning doesn't clean up bank 0 when later
        // slots add updates to the cache
        bank0.force_flush_accounts_cache();

        // Store some lamports in bank 1
        let some_lamports = 123;
        let mut bank1 = Arc::new(new_from_parent(&bank0));
        bank1.deposit(&pubkey1, some_lamports).unwrap();
        bank1.deposit(&pubkey2, some_lamports).unwrap();
        goto_end_of_slot(Arc::<Bank>::get_mut(&mut bank1).unwrap());
        bank1.freeze();
        bank1.squash();
        // Flush now so that accounts cache cleaning doesn't clean up bank 0 when later
        // slots add updates to the cache
        bank1.force_flush_accounts_cache();

        // Store some lamports for pubkey1 in bank 2, root bank 2
        let mut bank2 = Arc::new(new_from_parent(&bank1));
        bank2.deposit(&pubkey1, some_lamports).unwrap();
        bank2.store_account(&pubkey0, &account0);
        goto_end_of_slot(Arc::<Bank>::get_mut(&mut bank2).unwrap());
        bank2.freeze();
        bank2.squash();
        bank2.force_flush_accounts_cache();

        // Clean accounts, which should add earlier slots to the shrink
        // candidate set
        bank2.clean_accounts_for_tests();

        // Slots 0 and 1 should be candidates for shrinking, but slot 2
        // shouldn't because none of its accounts are outdated by a later
        // root
        assert_eq!(bank2.shrink_candidate_slots(), 2);
        let alive_counts: Vec<usize> = (0..3)
            .map(|slot| {
                bank2
                    .rc
                    .accounts
                    .accounts_db
                    .alive_account_count_in_slot(slot)
            })
            .collect();

        // No more slots should be shrunk
        assert_eq!(bank2.shrink_candidate_slots(), 0);
        // alive_counts represents the count of alive accounts in the three slots 0,1,2
        assert_eq!(alive_counts, vec![11, 1, 7]);
    }

    #[test]
    fn test_add_builtin_no_overwrite() {
        #[allow(clippy::unnecessary_wraps)]
        fn mock_ix_processor(
            _first_instruction_account: IndexOfAccount,
            _invoke_context: &mut InvokeContext,
        ) -> std::result::Result<(), InstructionError> {
            Ok(())
        }

        let slot = 123;
        let program_id = solana_sdk::pubkey::new_rand();

        let mut bank = Arc::new(Bank::new_from_parent(
            &create_simple_test_arc_bank(100_000),
            &Pubkey::default(),
            slot,
        ));
        assert_eq!(bank.get_account_modified_slot(&program_id), None);

        Arc::get_mut(&mut bank).unwrap().add_builtin(
            "mock_program",
            &program_id,
            mock_ix_processor,
        );
        assert_eq!(bank.get_account_modified_slot(&program_id).unwrap().1, slot);

        let mut bank = Arc::new(new_from_parent(&bank));
        Arc::get_mut(&mut bank).unwrap().add_builtin(
            "mock_program",
            &program_id,
            mock_ix_processor,
        );
        assert_eq!(bank.get_account_modified_slot(&program_id).unwrap().1, slot);
    }

    #[test]
    fn test_add_builtin_loader_no_overwrite() {
        #[allow(clippy::unnecessary_wraps)]
        fn mock_ix_processor(
            _first_instruction_account: IndexOfAccount,
            _context: &mut InvokeContext,
        ) -> std::result::Result<(), InstructionError> {
            Ok(())
        }

        let slot = 123;
        let loader_id = solana_sdk::pubkey::new_rand();

        let mut bank = Arc::new(Bank::new_from_parent(
            &create_simple_test_arc_bank(100_000),
            &Pubkey::default(),
            slot,
        ));
        assert_eq!(bank.get_account_modified_slot(&loader_id), None);

        Arc::get_mut(&mut bank)
            .unwrap()
            .add_builtin("mock_program", &loader_id, mock_ix_processor);
        assert_eq!(bank.get_account_modified_slot(&loader_id).unwrap().1, slot);

        let mut bank = Arc::new(new_from_parent(&bank));
        Arc::get_mut(&mut bank)
            .unwrap()
            .add_builtin("mock_program", &loader_id, mock_ix_processor);
        assert_eq!(bank.get_account_modified_slot(&loader_id).unwrap().1, slot);
    }

    #[test]
    fn test_add_builtin_account() {
        for pass in 0..5 {
            let (mut genesis_config, _mint_keypair) = create_genesis_config(100_000);
            activate_all_features(&mut genesis_config);

            let slot = 123;
            let program_id = solana_sdk::pubkey::new_rand();

            let bank = Arc::new(Bank::new_from_parent(
                &Arc::new(Bank::new_for_tests(&genesis_config)),
                &Pubkey::default(),
                slot,
            ));
            add_root_and_flush_write_cache(&bank.parent().unwrap());
            assert_eq!(bank.get_account_modified_slot(&program_id), None);

            assert_capitalization_diff(
                &bank,
                || bank.add_builtin_account("mock_program", &program_id, false),
                |old, new| {
                    assert_eq!(old + 1, new);
                    pass == 0
                },
            );
            if pass == 0 {
                continue;
            }

            assert_eq!(bank.get_account_modified_slot(&program_id).unwrap().1, slot);

            let bank = Arc::new(new_from_parent(&bank));
            add_root_and_flush_write_cache(&bank.parent().unwrap());
            assert_capitalization_diff(
                &bank,
                || bank.add_builtin_account("mock_program", &program_id, false),
                |old, new| {
                    assert_eq!(old, new);
                    pass == 1
                },
            );
            if pass == 1 {
                continue;
            }

            assert_eq!(bank.get_account_modified_slot(&program_id).unwrap().1, slot);

            let bank = Arc::new(new_from_parent(&bank));
            add_root_and_flush_write_cache(&bank.parent().unwrap());
            // When replacing builtin_program, name must change to disambiguate from repeated
            // invocations.
            assert_capitalization_diff(
                &bank,
                || bank.add_builtin_account("mock_program v2", &program_id, true),
                |old, new| {
                    assert_eq!(old, new);
                    pass == 2
                },
            );
            if pass == 2 {
                continue;
            }

            assert_eq!(
                bank.get_account_modified_slot(&program_id).unwrap().1,
                bank.slot()
            );

            let bank = Arc::new(new_from_parent(&bank));
            add_root_and_flush_write_cache(&bank.parent().unwrap());
            assert_capitalization_diff(
                &bank,
                || bank.add_builtin_account("mock_program v2", &program_id, true),
                |old, new| {
                    assert_eq!(old, new);
                    pass == 3
                },
            );
            if pass == 3 {
                continue;
            }

            // replacing with same name shouldn't update account
            assert_eq!(
                bank.get_account_modified_slot(&program_id).unwrap().1,
                bank.parent_slot()
            );
        }
    }

    /// useful to adapt tests written prior to introduction of the write cache
    /// to use the write cache
    fn add_root_and_flush_write_cache(bank: &Bank) {
        bank.rc.accounts.add_root(bank.slot());
        bank.flush_accounts_cache_slot_for_tests()
    }

    #[test]
    fn test_add_builtin_account_inherited_cap_while_replacing() {
        for pass in 0..4 {
            let (genesis_config, mint_keypair) = create_genesis_config(100_000);
            let bank = Bank::new_for_tests(&genesis_config);
            let program_id = solana_sdk::pubkey::new_rand();

            bank.add_builtin_account("mock_program", &program_id, false);
            if pass == 0 {
                add_root_and_flush_write_cache(&bank);
                assert_eq!(bank.capitalization(), bank.calculate_capitalization(true));
                continue;
            }

            // someone mess with program_id's balance
            bank.withdraw(&mint_keypair.pubkey(), 10).unwrap();
            if pass == 1 {
                add_root_and_flush_write_cache(&bank);
                assert_ne!(bank.capitalization(), bank.calculate_capitalization(true));
                continue;
            }
            bank.deposit(&program_id, 10).unwrap();
            if pass == 2 {
                add_root_and_flush_write_cache(&bank);
                assert_eq!(bank.capitalization(), bank.calculate_capitalization(true));
                continue;
            }

            bank.add_builtin_account("mock_program v2", &program_id, true);
            add_root_and_flush_write_cache(&bank);
            assert_eq!(bank.capitalization(), bank.calculate_capitalization(true));
        }
    }

    #[test]
    fn test_add_builtin_account_squatted_while_not_replacing() {
        for pass in 0..3 {
            let (genesis_config, mint_keypair) = create_genesis_config(100_000);
            let bank = Bank::new_for_tests(&genesis_config);
            let program_id = solana_sdk::pubkey::new_rand();

            // someone managed to squat at program_id!
            bank.withdraw(&mint_keypair.pubkey(), 10).unwrap();
            if pass == 0 {
                add_root_and_flush_write_cache(&bank);
                assert_ne!(bank.capitalization(), bank.calculate_capitalization(true));
                continue;
            }
            bank.deposit(&program_id, 10).unwrap();
            if pass == 1 {
                add_root_and_flush_write_cache(&bank);
                assert_eq!(bank.capitalization(), bank.calculate_capitalization(true));
                continue;
            }

            bank.add_builtin_account("mock_program", &program_id, false);
            add_root_and_flush_write_cache(&bank);
            assert_eq!(bank.capitalization(), bank.calculate_capitalization(true));
        }
    }

    #[test]
    #[should_panic(
        expected = "Can't change frozen bank by adding not-existing new builtin \
                   program (mock_program, CiXgo2KHKSDmDnV1F6B69eWFgNAPiSBjjYvfB4cvRNre). \
                   Maybe, inconsistent program activation is detected on snapshot restore?"
    )]
    fn test_add_builtin_account_after_frozen() {
        let slot = 123;
        let program_id = Pubkey::from_str("CiXgo2KHKSDmDnV1F6B69eWFgNAPiSBjjYvfB4cvRNre").unwrap();

        let bank = Bank::new_from_parent(
            &create_simple_test_arc_bank(100_000),
            &Pubkey::default(),
            slot,
        );
        bank.freeze();

        bank.add_builtin_account("mock_program", &program_id, false);
    }

    #[test]
    #[should_panic(
        expected = "There is no account to replace with builtin program (mock_program, \
                    CiXgo2KHKSDmDnV1F6B69eWFgNAPiSBjjYvfB4cvRNre)."
    )]
    fn test_add_builtin_account_replace_none() {
        let slot = 123;
        let program_id = Pubkey::from_str("CiXgo2KHKSDmDnV1F6B69eWFgNAPiSBjjYvfB4cvRNre").unwrap();

        let bank = Bank::new_from_parent(
            &create_simple_test_arc_bank(100_000),
            &Pubkey::default(),
            slot,
        );

        bank.add_builtin_account("mock_program", &program_id, true);
    }

    #[test]
    fn test_add_precompiled_account() {
        for pass in 0..2 {
            let (mut genesis_config, _mint_keypair) = create_genesis_config(100_000);
            activate_all_features(&mut genesis_config);

            let slot = 123;
            let program_id = solana_sdk::pubkey::new_rand();

            let bank = Arc::new(Bank::new_from_parent(
                &Arc::new(Bank::new_for_tests_with_config(
                    &genesis_config,
                    BankTestConfig::default(),
                )),
                &Pubkey::default(),
                slot,
            ));
            add_root_and_flush_write_cache(&bank.parent().unwrap());
            assert_eq!(bank.get_account_modified_slot(&program_id), None);

            assert_capitalization_diff(
                &bank,
                || bank.add_precompiled_account(&program_id),
                |old, new| {
                    assert_eq!(old + 1, new);
                    pass == 0
                },
            );
            if pass == 0 {
                continue;
            }

            assert_eq!(bank.get_account_modified_slot(&program_id).unwrap().1, slot);

            let bank = Arc::new(new_from_parent(&bank));
            add_root_and_flush_write_cache(&bank.parent().unwrap());
            assert_capitalization_diff(
                &bank,
                || bank.add_precompiled_account(&program_id),
                |old, new| {
                    assert_eq!(old, new);
                    true
                },
            );

            assert_eq!(bank.get_account_modified_slot(&program_id).unwrap().1, slot);
        }
    }

    #[test]
    fn test_add_precompiled_account_inherited_cap_while_replacing() {
        // when we flush the cache, it has side effects, so we have to restart the test each time we flush the cache
        // and then want to continue modifying the bank
        for pass in 0..4 {
            let (genesis_config, mint_keypair) = create_genesis_config(100_000);
            let bank = Bank::new_for_tests_with_config(&genesis_config, BankTestConfig::default());
            let program_id = solana_sdk::pubkey::new_rand();

            bank.add_precompiled_account(&program_id);
            if pass == 0 {
                add_root_and_flush_write_cache(&bank);
                assert_eq!(bank.capitalization(), bank.calculate_capitalization(true));
                continue;
            }

            // someone mess with program_id's balance
            bank.withdraw(&mint_keypair.pubkey(), 10).unwrap();
            if pass == 1 {
                add_root_and_flush_write_cache(&bank);
                assert_ne!(bank.capitalization(), bank.calculate_capitalization(true));
                continue;
            }
            bank.deposit(&program_id, 10).unwrap();
            if pass == 2 {
                add_root_and_flush_write_cache(&bank);
                assert_eq!(bank.capitalization(), bank.calculate_capitalization(true));
                continue;
            }

            bank.add_precompiled_account(&program_id);
            add_root_and_flush_write_cache(&bank);
            assert_eq!(bank.capitalization(), bank.calculate_capitalization(true));
        }
    }

    #[test]
    fn test_add_precompiled_account_squatted_while_not_replacing() {
        for pass in 0..3 {
            let (genesis_config, mint_keypair) = create_genesis_config(100_000);
            let bank = Bank::new_for_tests_with_config(&genesis_config, BankTestConfig::default());
            let program_id = solana_sdk::pubkey::new_rand();

            // someone managed to squat at program_id!
            bank.withdraw(&mint_keypair.pubkey(), 10).unwrap();
            if pass == 0 {
                add_root_and_flush_write_cache(&bank);

                assert_ne!(bank.capitalization(), bank.calculate_capitalization(true));
                continue;
            }
            bank.deposit(&program_id, 10).unwrap();
            if pass == 1 {
                add_root_and_flush_write_cache(&bank);
                assert_eq!(bank.capitalization(), bank.calculate_capitalization(true));
                continue;
            }

            bank.add_precompiled_account(&program_id);
            add_root_and_flush_write_cache(&bank);

            assert_eq!(bank.capitalization(), bank.calculate_capitalization(true));
        }
    }

    #[test]
    #[should_panic(
        expected = "Can't change frozen bank by adding not-existing new precompiled \
                   program (CiXgo2KHKSDmDnV1F6B69eWFgNAPiSBjjYvfB4cvRNre). \
                   Maybe, inconsistent program activation is detected on snapshot restore?"
    )]
    fn test_add_precompiled_account_after_frozen() {
        let slot = 123;
        let program_id = Pubkey::from_str("CiXgo2KHKSDmDnV1F6B69eWFgNAPiSBjjYvfB4cvRNre").unwrap();

        let bank = Bank::new_from_parent(
            &create_simple_test_arc_bank(100_000),
            &Pubkey::default(),
            slot,
        );
        bank.freeze();

        bank.add_precompiled_account(&program_id);
    }

    #[test]
    fn test_reconfigure_token2_native_mint() {
        solana_logger::setup();

        let mut genesis_config =
            create_genesis_config_with_leader(5, &solana_sdk::pubkey::new_rand(), 0).genesis_config;

        // ClusterType::Development - Native mint exists immediately
        assert_eq!(genesis_config.cluster_type, ClusterType::Development);
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));
        assert_eq!(
            bank.get_balance(&inline_spl_token::native_mint::id()),
            1000000000
        );

        // Testnet - Native mint blinks into existence at epoch 93
        genesis_config.cluster_type = ClusterType::Testnet;
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));
        assert_eq!(bank.get_balance(&inline_spl_token::native_mint::id()), 0);
        bank.deposit(&inline_spl_token::native_mint::id(), 4200000000)
            .unwrap();

        let bank = Bank::new_from_parent(
            &bank,
            &Pubkey::default(),
            genesis_config.epoch_schedule.get_first_slot_in_epoch(93),
        );

        let native_mint_account = bank
            .get_account(&inline_spl_token::native_mint::id())
            .unwrap();
        assert_eq!(native_mint_account.data().len(), 82);
        assert_eq!(
            bank.get_balance(&inline_spl_token::native_mint::id()),
            4200000000
        );
        assert_eq!(native_mint_account.owner(), &inline_spl_token::id());

        // MainnetBeta - Native mint blinks into existence at epoch 75
        genesis_config.cluster_type = ClusterType::MainnetBeta;
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));
        assert_eq!(bank.get_balance(&inline_spl_token::native_mint::id()), 0);
        bank.deposit(&inline_spl_token::native_mint::id(), 4200000000)
            .unwrap();

        let bank = Bank::new_from_parent(
            &bank,
            &Pubkey::default(),
            genesis_config.epoch_schedule.get_first_slot_in_epoch(75),
        );

        let native_mint_account = bank
            .get_account(&inline_spl_token::native_mint::id())
            .unwrap();
        assert_eq!(native_mint_account.data().len(), 82);
        assert_eq!(
            bank.get_balance(&inline_spl_token::native_mint::id()),
            4200000000
        );
        assert_eq!(native_mint_account.owner(), &inline_spl_token::id());
    }

    #[derive(Debug)]
    struct TestExecutor {}
    impl Executor for TestExecutor {
        fn execute(
            &self,
            _invoke_context: &mut InvokeContext,
        ) -> std::result::Result<(), InstructionError> {
            Ok(())
        }
    }

    #[test]
    fn test_bank_executor_cache() {
        solana_logger::setup();

        let (genesis_config, _) = create_genesis_config(1);
        let bank = Bank::new_for_tests(&genesis_config);

        let key1 = solana_sdk::pubkey::new_rand();
        let key2 = solana_sdk::pubkey::new_rand();
        let key3 = solana_sdk::pubkey::new_rand();
        let key4 = solana_sdk::pubkey::new_rand();
        let key5 = solana_sdk::pubkey::new_rand();
        let executor: Arc<dyn Executor> = Arc::new(TestExecutor {});

        fn new_executable_account(owner: Pubkey) -> AccountSharedData {
            AccountSharedData::from(Account {
                owner,
                executable: true,
                ..Account::default()
            })
        }

        let accounts = &[
            (key1, new_executable_account(bpf_loader_upgradeable::id())),
            (key2, new_executable_account(bpf_loader::id())),
            (key3, new_executable_account(bpf_loader_deprecated::id())),
            (key4, new_executable_account(native_loader::id())),
            (key5, AccountSharedData::default()),
        ];

        // don't do any work if not dirty
        let executors =
            TransactionExecutorCache::new((0..4).map(|i| (accounts[i].0, executor.clone())));
        let executors = Rc::new(RefCell::new(executors));
        bank.store_missing_executors(&executors);
        bank.store_updated_executors(&executors);
        let stored_executors = bank.get_tx_executor_cache(accounts);
        assert_eq!(stored_executors.borrow().executors.len(), 0);

        // do work
        let mut executors =
            TransactionExecutorCache::new((2..3).map(|i| (accounts[i].0, executor.clone())));
        executors.set(key1, executor.clone(), false);
        executors.set(key2, executor.clone(), false);
        executors.set(key3, executor.clone(), true);
        executors.set(key4, executor.clone(), false);
        let executors = Rc::new(RefCell::new(executors));

        // store Missing
        bank.store_missing_executors(&executors);
        let stored_executors = bank.get_tx_executor_cache(accounts);
        assert_eq!(stored_executors.borrow().executors.len(), 2);
        assert!(stored_executors.borrow().executors.contains_key(&key1));
        assert!(stored_executors.borrow().executors.contains_key(&key2));

        // store Updated
        bank.store_updated_executors(&executors);
        let stored_executors = bank.get_tx_executor_cache(accounts);
        assert_eq!(stored_executors.borrow().executors.len(), 3);
        assert!(stored_executors.borrow().executors.contains_key(&key1));
        assert!(stored_executors.borrow().executors.contains_key(&key2));
        assert!(stored_executors.borrow().executors.contains_key(&key3));

        // Check inheritance
        let bank = Bank::new_from_parent(&Arc::new(bank), &solana_sdk::pubkey::new_rand(), 1);
        let stored_executors = bank.get_tx_executor_cache(accounts);
        assert_eq!(stored_executors.borrow().executors.len(), 3);
        assert!(stored_executors.borrow().executors.contains_key(&key1));
        assert!(stored_executors.borrow().executors.contains_key(&key2));
        assert!(stored_executors.borrow().executors.contains_key(&key3));

        // Force compilation of an executor
        let mut file = File::open("../programs/bpf_loader/test_elfs/out/noop_aligned.so").unwrap();
        let mut elf = Vec::new();
        file.read_to_end(&mut elf).unwrap();
        let programdata_key = solana_sdk::pubkey::new_rand();
        let mut program_account = AccountSharedData::new_data(
            40,
            &UpgradeableLoaderState::Program {
                programdata_address: programdata_key,
            },
            &bpf_loader_upgradeable::id(),
        )
        .unwrap();
        program_account.set_executable(true);
        program_account.set_rent_epoch(1);
        let programdata_data_offset = UpgradeableLoaderState::size_of_programdata_metadata();
        let mut programdata_account = AccountSharedData::new(
            40,
            programdata_data_offset + elf.len(),
            &bpf_loader_upgradeable::id(),
        );
        programdata_account
            .set_state(&UpgradeableLoaderState::ProgramData {
                slot: 42,
                upgrade_authority_address: None,
            })
            .unwrap();
        programdata_account.data_mut()[programdata_data_offset..].copy_from_slice(&elf);
        programdata_account.set_rent_epoch(1);
        bank.store_account_and_update_capitalization(&key1, &program_account);
        bank.store_account_and_update_capitalization(&programdata_key, &programdata_account);
        bank.create_executor(&key1).unwrap();

        // Remove all
        bank.remove_executor(&key1);
        bank.remove_executor(&key2);
        bank.remove_executor(&key3);
        bank.remove_executor(&key4);
        let stored_executors = bank.get_tx_executor_cache(accounts);
        assert_eq!(stored_executors.borrow().executors.len(), 0);
    }

    #[test]
    fn test_bank_executor_cow() {
        solana_logger::setup();

        let (genesis_config, _) = create_genesis_config(1);
        let root = Arc::new(Bank::new_for_tests(&genesis_config));

        let key1 = solana_sdk::pubkey::new_rand();
        let key2 = solana_sdk::pubkey::new_rand();
        let executor: Arc<dyn Executor> = Arc::new(TestExecutor {});
        let executable_account = AccountSharedData::from(Account {
            owner: bpf_loader_upgradeable::id(),
            executable: true,
            ..Account::default()
        });

        let accounts = &[
            (key1, executable_account.clone()),
            (key2, executable_account),
        ];

        // add one to root bank
        let mut executors = TransactionExecutorCache::default();
        executors.set(key1, executor.clone(), false);
        let executors = Rc::new(RefCell::new(executors));
        root.store_missing_executors(&executors);
        let executors = root.get_tx_executor_cache(accounts);
        assert_eq!(executors.borrow().executors.len(), 1);

        let fork1 = Bank::new_from_parent(&root, &Pubkey::default(), 1);
        let fork2 = Bank::new_from_parent(&root, &Pubkey::default(), 2);

        let executors = fork1.get_tx_executor_cache(accounts);
        assert_eq!(executors.borrow().executors.len(), 1);
        let executors = fork2.get_tx_executor_cache(accounts);
        assert_eq!(executors.borrow().executors.len(), 1);

        let mut executors = TransactionExecutorCache::default();
        executors.set(key2, executor.clone(), false);
        let executors = Rc::new(RefCell::new(executors));
        fork1.store_missing_executors(&executors);

        let executors = fork1.get_tx_executor_cache(accounts);
        assert_eq!(executors.borrow().executors.len(), 2);
        let executors = fork2.get_tx_executor_cache(accounts);
        assert_eq!(executors.borrow().executors.len(), 1);

        fork1.remove_executor(&key1);

        let executors = fork1.get_tx_executor_cache(accounts);
        assert_eq!(executors.borrow().executors.len(), 1);
        let executors = fork2.get_tx_executor_cache(accounts);
        assert_eq!(executors.borrow().executors.len(), 1);
    }

    #[test]
    fn test_bpf_loader_upgradeable_deploy_with_max_len() {
        let (genesis_config, mint_keypair) = create_genesis_config(1_000_000_000);
        let mut bank = Bank::new_for_tests(&genesis_config);
        bank.feature_set = Arc::new(FeatureSet::all_enabled());
        bank.add_builtin(
            "solana_bpf_loader_upgradeable_program",
            &bpf_loader_upgradeable::id(),
            solana_bpf_loader_program::process_instruction,
        );
        let bank = Arc::new(bank);
        let bank_client = BankClient::new_shared(&bank);

        // Setup keypairs and addresses
        let payer_keypair = Keypair::new();
        let program_keypair = Keypair::new();
        let buffer_address = Pubkey::new_unique();
        let (programdata_address, _) = Pubkey::find_program_address(
            &[program_keypair.pubkey().as_ref()],
            &bpf_loader_upgradeable::id(),
        );
        let upgrade_authority_keypair = Keypair::new();

        // Load program file
        let mut file = File::open("../programs/bpf_loader/test_elfs/out/noop_aligned.so")
            .expect("file open failed");
        let mut elf = Vec::new();
        file.read_to_end(&mut elf).unwrap();

        // Compute rent exempt balances
        let program_len = elf.len();
        let min_program_balance =
            bank.get_minimum_balance_for_rent_exemption(UpgradeableLoaderState::size_of_program());
        let min_buffer_balance = bank.get_minimum_balance_for_rent_exemption(
            UpgradeableLoaderState::size_of_buffer(program_len),
        );
        let min_programdata_balance = bank.get_minimum_balance_for_rent_exemption(
            UpgradeableLoaderState::size_of_programdata(program_len),
        );

        // Setup accounts
        let buffer_account = {
            let mut account = AccountSharedData::new(
                min_buffer_balance,
                UpgradeableLoaderState::size_of_buffer(elf.len()),
                &bpf_loader_upgradeable::id(),
            );
            account
                .set_state(&UpgradeableLoaderState::Buffer {
                    authority_address: Some(upgrade_authority_keypair.pubkey()),
                })
                .unwrap();
            account
                .data_as_mut_slice()
                .get_mut(UpgradeableLoaderState::size_of_buffer_metadata()..)
                .unwrap()
                .copy_from_slice(&elf);
            account
        };
        let program_account = AccountSharedData::new(
            min_programdata_balance,
            UpgradeableLoaderState::size_of_program(),
            &bpf_loader_upgradeable::id(),
        );
        let programdata_account = AccountSharedData::new(
            1,
            UpgradeableLoaderState::size_of_programdata(elf.len()),
            &bpf_loader_upgradeable::id(),
        );

        // Test successful deploy
        let payer_base_balance = LAMPORTS_PER_SOL;
        let deploy_fees = {
            let fee_calculator = genesis_config.fee_rate_governor.create_fee_calculator();
            3 * fee_calculator.lamports_per_signature
        };
        let min_payer_balance = min_program_balance
            .saturating_add(min_programdata_balance)
            .saturating_sub(min_buffer_balance.saturating_add(deploy_fees));
        bank.store_account(
            &payer_keypair.pubkey(),
            &AccountSharedData::new(
                payer_base_balance.saturating_add(min_payer_balance),
                0,
                &system_program::id(),
            ),
        );
        bank.store_account(&buffer_address, &buffer_account);
        bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
        bank.store_account(&programdata_address, &AccountSharedData::default());
        let message = Message::new(
            &bpf_loader_upgradeable::deploy_with_max_program_len(
                &payer_keypair.pubkey(),
                &program_keypair.pubkey(),
                &buffer_address,
                &upgrade_authority_keypair.pubkey(),
                min_program_balance,
                elf.len(),
            )
            .unwrap(),
            Some(&payer_keypair.pubkey()),
        );
        assert!(bank_client
            .send_and_confirm_message(
                &[&payer_keypair, &program_keypair, &upgrade_authority_keypair],
                message
            )
            .is_ok());
        assert_eq!(
            bank.get_balance(&payer_keypair.pubkey()),
            payer_base_balance
        );
        assert_eq!(bank.get_balance(&buffer_address), 0);
        assert_eq!(None, bank.get_account(&buffer_address));
        let post_program_account = bank.get_account(&program_keypair.pubkey()).unwrap();
        assert_eq!(post_program_account.lamports(), min_program_balance);
        assert_eq!(post_program_account.owner(), &bpf_loader_upgradeable::id());
        assert_eq!(
            post_program_account.data().len(),
            UpgradeableLoaderState::size_of_program()
        );
        let state: UpgradeableLoaderState = post_program_account.state().unwrap();
        assert_eq!(
            state,
            UpgradeableLoaderState::Program {
                programdata_address
            }
        );
        let post_programdata_account = bank.get_account(&programdata_address).unwrap();
        assert_eq!(post_programdata_account.lamports(), min_programdata_balance);
        assert_eq!(
            post_programdata_account.owner(),
            &bpf_loader_upgradeable::id()
        );
        let state: UpgradeableLoaderState = post_programdata_account.state().unwrap();
        assert_eq!(
            state,
            UpgradeableLoaderState::ProgramData {
                slot: bank_client.get_slot().unwrap(),
                upgrade_authority_address: Some(upgrade_authority_keypair.pubkey())
            }
        );
        for (i, byte) in post_programdata_account
            .data()
            .get(UpgradeableLoaderState::size_of_programdata_metadata()..)
            .unwrap()
            .iter()
            .enumerate()
        {
            assert_eq!(*elf.get(i).unwrap(), *byte);
        }

        // Invoke deployed program
        mock_process_instruction(
            &bpf_loader_upgradeable::id(),
            vec![0, 1],
            &[],
            vec![
                (programdata_address, post_programdata_account),
                (program_keypair.pubkey(), post_program_account),
            ],
            Vec::new(),
            None,
            None,
            Ok(()),
            solana_bpf_loader_program::process_instruction,
        );

        // Test initialized program account
        bank.clear_signatures();
        bank.store_account(&buffer_address, &buffer_account);
        let message = Message::new(
            &[Instruction::new_with_bincode(
                bpf_loader_upgradeable::id(),
                &UpgradeableLoaderInstruction::DeployWithMaxDataLen {
                    max_data_len: elf.len(),
                },
                vec![
                    AccountMeta::new(mint_keypair.pubkey(), true),
                    AccountMeta::new(programdata_address, false),
                    AccountMeta::new(program_keypair.pubkey(), false),
                    AccountMeta::new(buffer_address, false),
                    AccountMeta::new_readonly(sysvar::rent::id(), false),
                    AccountMeta::new_readonly(sysvar::clock::id(), false),
                    AccountMeta::new_readonly(system_program::id(), false),
                    AccountMeta::new_readonly(upgrade_authority_keypair.pubkey(), true),
                ],
            )],
            Some(&mint_keypair.pubkey()),
        );
        assert_eq!(
            TransactionError::InstructionError(0, InstructionError::AccountAlreadyInitialized),
            bank_client
                .send_and_confirm_message(&[&mint_keypair, &upgrade_authority_keypair], message)
                .unwrap_err()
                .unwrap()
        );

        // Test initialized ProgramData account
        bank.clear_signatures();
        bank.store_account(&buffer_address, &buffer_account);
        bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
        let message = Message::new(
            &bpf_loader_upgradeable::deploy_with_max_program_len(
                &mint_keypair.pubkey(),
                &program_keypair.pubkey(),
                &buffer_address,
                &upgrade_authority_keypair.pubkey(),
                min_program_balance,
                elf.len(),
            )
            .unwrap(),
            Some(&mint_keypair.pubkey()),
        );
        assert_eq!(
            TransactionError::InstructionError(1, InstructionError::Custom(0)),
            bank_client
                .send_and_confirm_message(
                    &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                    message
                )
                .unwrap_err()
                .unwrap()
        );

        // Test deploy no authority
        bank.clear_signatures();
        bank.store_account(&buffer_address, &buffer_account);
        bank.store_account(&program_keypair.pubkey(), &program_account);
        bank.store_account(&programdata_address, &programdata_account);
        let message = Message::new(
            &[Instruction::new_with_bincode(
                bpf_loader_upgradeable::id(),
                &UpgradeableLoaderInstruction::DeployWithMaxDataLen {
                    max_data_len: elf.len(),
                },
                vec![
                    AccountMeta::new(mint_keypair.pubkey(), true),
                    AccountMeta::new(programdata_address, false),
                    AccountMeta::new(program_keypair.pubkey(), false),
                    AccountMeta::new(buffer_address, false),
                    AccountMeta::new_readonly(sysvar::rent::id(), false),
                    AccountMeta::new_readonly(sysvar::clock::id(), false),
                    AccountMeta::new_readonly(system_program::id(), false),
                ],
            )],
            Some(&mint_keypair.pubkey()),
        );
        assert_eq!(
            TransactionError::InstructionError(0, InstructionError::NotEnoughAccountKeys),
            bank_client
                .send_and_confirm_message(&[&mint_keypair], message)
                .unwrap_err()
                .unwrap()
        );

        // Test deploy authority not a signer
        bank.clear_signatures();
        bank.store_account(&buffer_address, &buffer_account);
        bank.store_account(&program_keypair.pubkey(), &program_account);
        bank.store_account(&programdata_address, &programdata_account);
        let message = Message::new(
            &[Instruction::new_with_bincode(
                bpf_loader_upgradeable::id(),
                &UpgradeableLoaderInstruction::DeployWithMaxDataLen {
                    max_data_len: elf.len(),
                },
                vec![
                    AccountMeta::new(mint_keypair.pubkey(), true),
                    AccountMeta::new(programdata_address, false),
                    AccountMeta::new(program_keypair.pubkey(), false),
                    AccountMeta::new(buffer_address, false),
                    AccountMeta::new_readonly(sysvar::rent::id(), false),
                    AccountMeta::new_readonly(sysvar::clock::id(), false),
                    AccountMeta::new_readonly(system_program::id(), false),
                    AccountMeta::new_readonly(upgrade_authority_keypair.pubkey(), false),
                ],
            )],
            Some(&mint_keypair.pubkey()),
        );
        assert_eq!(
            TransactionError::InstructionError(0, InstructionError::MissingRequiredSignature),
            bank_client
                .send_and_confirm_message(&[&mint_keypair], message)
                .unwrap_err()
                .unwrap()
        );

        // Test invalid Buffer account state
        bank.clear_signatures();
        bank.store_account(&buffer_address, &AccountSharedData::default());
        bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
        bank.store_account(&programdata_address, &AccountSharedData::default());
        let message = Message::new(
            &bpf_loader_upgradeable::deploy_with_max_program_len(
                &mint_keypair.pubkey(),
                &program_keypair.pubkey(),
                &buffer_address,
                &upgrade_authority_keypair.pubkey(),
                min_program_balance,
                elf.len(),
            )
            .unwrap(),
            Some(&mint_keypair.pubkey()),
        );
        assert_eq!(
            TransactionError::InstructionError(1, InstructionError::InvalidAccountData),
            bank_client
                .send_and_confirm_message(
                    &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                    message
                )
                .unwrap_err()
                .unwrap()
        );

        // Test program account not rent exempt
        bank.clear_signatures();
        bank.store_account(&buffer_address, &buffer_account);
        bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
        bank.store_account(&programdata_address, &AccountSharedData::default());
        let message = Message::new(
            &bpf_loader_upgradeable::deploy_with_max_program_len(
                &mint_keypair.pubkey(),
                &program_keypair.pubkey(),
                &buffer_address,
                &upgrade_authority_keypair.pubkey(),
                min_program_balance.saturating_sub(1),
                elf.len(),
            )
            .unwrap(),
            Some(&mint_keypair.pubkey()),
        );
        assert_eq!(
            TransactionError::InstructionError(1, InstructionError::ExecutableAccountNotRentExempt),
            bank_client
                .send_and_confirm_message(
                    &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                    message
                )
                .unwrap_err()
                .unwrap()
        );

        // Test program account not rent exempt because data is larger than needed
        bank.clear_signatures();
        bank.store_account(&buffer_address, &buffer_account);
        bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
        bank.store_account(&programdata_address, &AccountSharedData::default());
        let mut instructions = bpf_loader_upgradeable::deploy_with_max_program_len(
            &mint_keypair.pubkey(),
            &program_keypair.pubkey(),
            &buffer_address,
            &upgrade_authority_keypair.pubkey(),
            min_program_balance,
            elf.len(),
        )
        .unwrap();
        *instructions.get_mut(0).unwrap() = system_instruction::create_account(
            &mint_keypair.pubkey(),
            &program_keypair.pubkey(),
            min_program_balance,
            (UpgradeableLoaderState::size_of_program() as u64).saturating_add(1),
            &bpf_loader_upgradeable::id(),
        );
        let message = Message::new(&instructions, Some(&mint_keypair.pubkey()));
        assert_eq!(
            TransactionError::InstructionError(1, InstructionError::ExecutableAccountNotRentExempt),
            bank_client
                .send_and_confirm_message(
                    &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                    message
                )
                .unwrap_err()
                .unwrap()
        );

        // Test program account too small
        bank.clear_signatures();
        bank.store_account(&buffer_address, &buffer_account);
        bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
        bank.store_account(&programdata_address, &AccountSharedData::default());
        let mut instructions = bpf_loader_upgradeable::deploy_with_max_program_len(
            &mint_keypair.pubkey(),
            &program_keypair.pubkey(),
            &buffer_address,
            &upgrade_authority_keypair.pubkey(),
            min_program_balance,
            elf.len(),
        )
        .unwrap();
        *instructions.get_mut(0).unwrap() = system_instruction::create_account(
            &mint_keypair.pubkey(),
            &program_keypair.pubkey(),
            min_program_balance,
            (UpgradeableLoaderState::size_of_program() as u64).saturating_sub(1),
            &bpf_loader_upgradeable::id(),
        );
        let message = Message::new(&instructions, Some(&mint_keypair.pubkey()));
        assert_eq!(
            TransactionError::InstructionError(1, InstructionError::AccountDataTooSmall),
            bank_client
                .send_and_confirm_message(
                    &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                    message
                )
                .unwrap_err()
                .unwrap()
        );

        // Test Insufficient payer funds (need more funds to cover the
        // difference between buffer lamports and programdata lamports)
        bank.clear_signatures();
        bank.store_account(
            &mint_keypair.pubkey(),
            &AccountSharedData::new(
                deploy_fees.saturating_add(min_program_balance),
                0,
                &system_program::id(),
            ),
        );
        bank.store_account(&buffer_address, &buffer_account);
        bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
        bank.store_account(&programdata_address, &AccountSharedData::default());
        let message = Message::new(
            &bpf_loader_upgradeable::deploy_with_max_program_len(
                &mint_keypair.pubkey(),
                &program_keypair.pubkey(),
                &buffer_address,
                &upgrade_authority_keypair.pubkey(),
                min_program_balance,
                elf.len(),
            )
            .unwrap(),
            Some(&mint_keypair.pubkey()),
        );
        assert_eq!(
            TransactionError::InstructionError(1, InstructionError::Custom(1)),
            bank_client
                .send_and_confirm_message(
                    &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                    message
                )
                .unwrap_err()
                .unwrap()
        );
        bank.store_account(
            &mint_keypair.pubkey(),
            &AccountSharedData::new(1_000_000_000, 0, &system_program::id()),
        );

        // Test max_data_len
        bank.clear_signatures();
        bank.store_account(&buffer_address, &buffer_account);
        bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
        bank.store_account(&programdata_address, &AccountSharedData::default());
        let message = Message::new(
            &bpf_loader_upgradeable::deploy_with_max_program_len(
                &mint_keypair.pubkey(),
                &program_keypair.pubkey(),
                &buffer_address,
                &upgrade_authority_keypair.pubkey(),
                min_program_balance,
                elf.len().saturating_sub(1),
            )
            .unwrap(),
            Some(&mint_keypair.pubkey()),
        );
        assert_eq!(
            TransactionError::InstructionError(1, InstructionError::AccountDataTooSmall),
            bank_client
                .send_and_confirm_message(
                    &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                    message
                )
                .unwrap_err()
                .unwrap()
        );

        // Test max_data_len too large
        bank.clear_signatures();
        bank.store_account(
            &mint_keypair.pubkey(),
            &AccountSharedData::new(u64::MAX / 2, 0, &system_program::id()),
        );
        let mut modified_buffer_account = buffer_account.clone();
        modified_buffer_account.set_lamports(u64::MAX / 2);
        bank.store_account(&buffer_address, &modified_buffer_account);
        bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
        bank.store_account(&programdata_address, &AccountSharedData::default());
        let message = Message::new(
            &bpf_loader_upgradeable::deploy_with_max_program_len(
                &mint_keypair.pubkey(),
                &program_keypair.pubkey(),
                &buffer_address,
                &upgrade_authority_keypair.pubkey(),
                min_program_balance,
                usize::MAX,
            )
            .unwrap(),
            Some(&mint_keypair.pubkey()),
        );
        assert_eq!(
            TransactionError::InstructionError(1, InstructionError::InvalidArgument),
            bank_client
                .send_and_confirm_message(
                    &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                    message
                )
                .unwrap_err()
                .unwrap()
        );

        // Test not the system account
        bank.clear_signatures();
        bank.store_account(&buffer_address, &buffer_account);
        bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
        bank.store_account(&programdata_address, &AccountSharedData::default());
        let mut instructions = bpf_loader_upgradeable::deploy_with_max_program_len(
            &mint_keypair.pubkey(),
            &program_keypair.pubkey(),
            &buffer_address,
            &upgrade_authority_keypair.pubkey(),
            min_program_balance,
            elf.len(),
        )
        .unwrap();
        *instructions
            .get_mut(1)
            .unwrap()
            .accounts
            .get_mut(6)
            .unwrap() = AccountMeta::new_readonly(Pubkey::new_unique(), false);
        let message = Message::new(&instructions, Some(&mint_keypair.pubkey()));
        assert_eq!(
            TransactionError::InstructionError(1, InstructionError::MissingAccount),
            bank_client
                .send_and_confirm_message(
                    &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                    message
                )
                .unwrap_err()
                .unwrap()
        );

        fn truncate_data(account: &mut AccountSharedData, len: usize) {
            let mut data = account.data().to_vec();
            data.truncate(len);
            account.set_data(data);
        }

        // Test Bad ELF data
        bank.clear_signatures();
        let mut modified_buffer_account = buffer_account;
        truncate_data(
            &mut modified_buffer_account,
            UpgradeableLoaderState::size_of_buffer(1),
        );
        bank.store_account(&buffer_address, &modified_buffer_account);
        bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
        bank.store_account(&programdata_address, &AccountSharedData::default());
        let message = Message::new(
            &bpf_loader_upgradeable::deploy_with_max_program_len(
                &mint_keypair.pubkey(),
                &program_keypair.pubkey(),
                &buffer_address,
                &upgrade_authority_keypair.pubkey(),
                min_program_balance,
                elf.len(),
            )
            .unwrap(),
            Some(&mint_keypair.pubkey()),
        );
        assert_eq!(
            TransactionError::InstructionError(1, InstructionError::InvalidAccountData),
            bank_client
                .send_and_confirm_message(
                    &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                    message
                )
                .unwrap_err()
                .unwrap()
        );

        // Test small buffer account
        bank.clear_signatures();
        let mut modified_buffer_account = AccountSharedData::new(
            min_programdata_balance,
            UpgradeableLoaderState::size_of_buffer(elf.len()),
            &bpf_loader_upgradeable::id(),
        );
        modified_buffer_account
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(upgrade_authority_keypair.pubkey()),
            })
            .unwrap();
        modified_buffer_account
            .data_as_mut_slice()
            .get_mut(UpgradeableLoaderState::size_of_buffer_metadata()..)
            .unwrap()
            .copy_from_slice(&elf);
        truncate_data(&mut modified_buffer_account, 5);
        bank.store_account(&buffer_address, &modified_buffer_account);
        bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
        bank.store_account(&programdata_address, &AccountSharedData::default());
        let message = Message::new(
            &bpf_loader_upgradeable::deploy_with_max_program_len(
                &mint_keypair.pubkey(),
                &program_keypair.pubkey(),
                &buffer_address,
                &upgrade_authority_keypair.pubkey(),
                min_program_balance,
                elf.len(),
            )
            .unwrap(),
            Some(&mint_keypair.pubkey()),
        );
        assert_eq!(
            TransactionError::InstructionError(1, InstructionError::InvalidAccountData),
            bank_client
                .send_and_confirm_message(
                    &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                    message
                )
                .unwrap_err()
                .unwrap()
        );

        // Mismatched buffer and program authority
        bank.clear_signatures();
        let mut modified_buffer_account = AccountSharedData::new(
            min_programdata_balance,
            UpgradeableLoaderState::size_of_buffer(elf.len()),
            &bpf_loader_upgradeable::id(),
        );
        modified_buffer_account
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(buffer_address),
            })
            .unwrap();
        modified_buffer_account
            .data_as_mut_slice()
            .get_mut(UpgradeableLoaderState::size_of_buffer_metadata()..)
            .unwrap()
            .copy_from_slice(&elf);
        bank.store_account(&buffer_address, &modified_buffer_account);
        bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
        bank.store_account(&programdata_address, &AccountSharedData::default());
        let message = Message::new(
            &bpf_loader_upgradeable::deploy_with_max_program_len(
                &mint_keypair.pubkey(),
                &program_keypair.pubkey(),
                &buffer_address,
                &upgrade_authority_keypair.pubkey(),
                min_program_balance,
                elf.len(),
            )
            .unwrap(),
            Some(&mint_keypair.pubkey()),
        );
        assert_eq!(
            TransactionError::InstructionError(1, InstructionError::IncorrectAuthority),
            bank_client
                .send_and_confirm_message(
                    &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                    message
                )
                .unwrap_err()
                .unwrap()
        );

        // Deploy buffer with mismatched None authority
        bank.clear_signatures();
        let mut modified_buffer_account = AccountSharedData::new(
            min_programdata_balance,
            UpgradeableLoaderState::size_of_buffer(elf.len()),
            &bpf_loader_upgradeable::id(),
        );
        modified_buffer_account
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: None,
            })
            .unwrap();
        modified_buffer_account
            .data_as_mut_slice()
            .get_mut(UpgradeableLoaderState::size_of_buffer_metadata()..)
            .unwrap()
            .copy_from_slice(&elf);
        bank.store_account(&buffer_address, &modified_buffer_account);
        bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
        bank.store_account(&programdata_address, &AccountSharedData::default());
        let message = Message::new(
            &bpf_loader_upgradeable::deploy_with_max_program_len(
                &mint_keypair.pubkey(),
                &program_keypair.pubkey(),
                &buffer_address,
                &upgrade_authority_keypair.pubkey(),
                min_program_balance,
                elf.len(),
            )
            .unwrap(),
            Some(&mint_keypair.pubkey()),
        );
        assert_eq!(
            TransactionError::InstructionError(1, InstructionError::IncorrectAuthority),
            bank_client
                .send_and_confirm_message(
                    &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                    message
                )
                .unwrap_err()
                .unwrap()
        );
    }

    #[test]
    fn test_compute_active_feature_set() {
        let bank0 = create_simple_test_arc_bank(100_000);
        let mut bank = Bank::new_from_parent(&bank0, &Pubkey::default(), 1);

        let test_feature = "TestFeature11111111111111111111111111111111"
            .parse::<Pubkey>()
            .unwrap();
        let mut feature_set = FeatureSet::default();
        feature_set.inactive.insert(test_feature);
        bank.feature_set = Arc::new(feature_set.clone());

        let new_activations = bank.compute_active_feature_set(true);
        assert!(new_activations.is_empty());
        assert!(!bank.feature_set.is_active(&test_feature));

        // Depositing into the `test_feature` account should do nothing
        bank.deposit(&test_feature, 42).unwrap();
        let new_activations = bank.compute_active_feature_set(true);
        assert!(new_activations.is_empty());
        assert!(!bank.feature_set.is_active(&test_feature));

        // Request `test_feature` activation
        let feature = Feature::default();
        assert_eq!(feature.activated_at, None);
        bank.store_account(&test_feature, &feature::create_account(&feature, 42));

        // Run `compute_active_feature_set` disallowing new activations
        let new_activations = bank.compute_active_feature_set(false);
        assert!(new_activations.is_empty());
        assert!(!bank.feature_set.is_active(&test_feature));
        let feature = feature::from_account(&bank.get_account(&test_feature).expect("get_account"))
            .expect("from_account");
        assert_eq!(feature.activated_at, None);

        // Run `compute_active_feature_set` allowing new activations
        let new_activations = bank.compute_active_feature_set(true);
        assert_eq!(new_activations.len(), 1);
        assert!(bank.feature_set.is_active(&test_feature));
        let feature = feature::from_account(&bank.get_account(&test_feature).expect("get_account"))
            .expect("from_account");
        assert_eq!(feature.activated_at, Some(1));

        // Reset the bank's feature set
        bank.feature_set = Arc::new(feature_set);
        assert!(!bank.feature_set.is_active(&test_feature));

        // Running `compute_active_feature_set` will not cause new activations, but
        // `test_feature` is now be active
        let new_activations = bank.compute_active_feature_set(true);
        assert!(new_activations.is_empty());
        assert!(bank.feature_set.is_active(&test_feature));
    }

    #[test]
    fn test_program_replacement() {
        let mut bank = create_simple_test_bank(0);

        // Setup original program account
        let old_address = Pubkey::new_unique();
        let new_address = Pubkey::new_unique();
        bank.store_account_and_update_capitalization(
            &old_address,
            &AccountSharedData::from(Account {
                lamports: 100,
                ..Account::default()
            }),
        );
        assert_eq!(bank.get_balance(&old_address), 100);

        // Setup new program account
        let new_program_account = AccountSharedData::from(Account {
            lamports: 123,
            ..Account::default()
        });
        bank.store_account_and_update_capitalization(&new_address, &new_program_account);
        assert_eq!(bank.get_balance(&new_address), 123);

        let original_capitalization = bank.capitalization();

        bank.replace_program_account(&old_address, &new_address, "bank-apply_program_replacement");

        // New program account is now empty
        assert_eq!(bank.get_balance(&new_address), 0);

        // Old program account holds the new program account
        assert_eq!(bank.get_account(&old_address), Some(new_program_account));

        // Lamports in the old token account were burnt
        assert_eq!(bank.capitalization(), original_capitalization - 100);
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

    fn min_rent_exempt_balance_for_sysvars(bank: &Bank, sysvar_ids: &[Pubkey]) -> u64 {
        sysvar_ids
            .iter()
            .map(|sysvar_id| {
                trace!("min_rent_excempt_balance_for_sysvars: {}", sysvar_id);
                bank.get_minimum_balance_for_rent_exemption(
                    bank.get_account(sysvar_id).unwrap().data().len(),
                )
            })
            .sum()
    }

    #[test]
    fn test_adjust_sysvar_balance_for_rent() {
        let bank = create_simple_test_bank(0);
        let mut smaller_sample_sysvar = AccountSharedData::new(1, 0, &Pubkey::default());
        assert_eq!(smaller_sample_sysvar.lamports(), 1);
        bank.adjust_sysvar_balance_for_rent(&mut smaller_sample_sysvar);
        assert_eq!(
            smaller_sample_sysvar.lamports(),
            bank.get_minimum_balance_for_rent_exemption(smaller_sample_sysvar.data().len()),
        );

        let mut bigger_sample_sysvar = AccountSharedData::new(
            1,
            smaller_sample_sysvar.data().len() + 1,
            &Pubkey::default(),
        );
        bank.adjust_sysvar_balance_for_rent(&mut bigger_sample_sysvar);
        assert!(smaller_sample_sysvar.lamports() < bigger_sample_sysvar.lamports());

        // excess lamports shouldn't be reduced by adjust_sysvar_balance_for_rent()
        let excess_lamports = smaller_sample_sysvar.lamports() + 999;
        smaller_sample_sysvar.set_lamports(excess_lamports);
        bank.adjust_sysvar_balance_for_rent(&mut smaller_sample_sysvar);
        assert_eq!(smaller_sample_sysvar.lamports(), excess_lamports);
    }

    #[test]
    fn test_update_clock_timestamp() {
        let leader_pubkey = solana_sdk::pubkey::new_rand();
        let GenesisConfigInfo {
            genesis_config,
            voting_keypair,
            ..
        } = create_genesis_config_with_leader(5, &leader_pubkey, 3);
        let mut bank = Bank::new_for_tests(&genesis_config);
        // Advance past slot 0, which has special handling.
        bank = new_from_parent(&Arc::new(bank));
        bank = new_from_parent(&Arc::new(bank));
        assert_eq!(
            bank.clock().unix_timestamp,
            bank.unix_timestamp_from_genesis()
        );

        bank.update_clock(None);
        assert_eq!(
            bank.clock().unix_timestamp,
            bank.unix_timestamp_from_genesis()
        );

        update_vote_account_timestamp(
            BlockTimestamp {
                slot: bank.slot(),
                timestamp: bank.unix_timestamp_from_genesis() - 1,
            },
            &bank,
            &voting_keypair.pubkey(),
        );
        bank.update_clock(None);
        assert_eq!(
            bank.clock().unix_timestamp,
            bank.unix_timestamp_from_genesis()
        );

        update_vote_account_timestamp(
            BlockTimestamp {
                slot: bank.slot(),
                timestamp: bank.unix_timestamp_from_genesis(),
            },
            &bank,
            &voting_keypair.pubkey(),
        );
        bank.update_clock(None);
        assert_eq!(
            bank.clock().unix_timestamp,
            bank.unix_timestamp_from_genesis()
        );

        update_vote_account_timestamp(
            BlockTimestamp {
                slot: bank.slot(),
                timestamp: bank.unix_timestamp_from_genesis() + 1,
            },
            &bank,
            &voting_keypair.pubkey(),
        );
        bank.update_clock(None);
        assert_eq!(
            bank.clock().unix_timestamp,
            bank.unix_timestamp_from_genesis() + 1
        );

        // Timestamp cannot go backward from ancestor Bank to child
        bank = new_from_parent(&Arc::new(bank));
        update_vote_account_timestamp(
            BlockTimestamp {
                slot: bank.slot(),
                timestamp: bank.unix_timestamp_from_genesis() - 1,
            },
            &bank,
            &voting_keypair.pubkey(),
        );
        bank.update_clock(None);
        assert_eq!(
            bank.clock().unix_timestamp,
            bank.unix_timestamp_from_genesis()
        );
    }

    fn poh_estimate_offset(bank: &Bank) -> Duration {
        let mut epoch_start_slot = bank.epoch_schedule.get_first_slot_in_epoch(bank.epoch());
        if epoch_start_slot == bank.slot() {
            epoch_start_slot = bank
                .epoch_schedule
                .get_first_slot_in_epoch(bank.epoch() - 1);
        }
        bank.slot().saturating_sub(epoch_start_slot) as u32
            * Duration::from_nanos(bank.ns_per_slot as u64)
    }

    #[test]
    fn test_timestamp_slow() {
        fn max_allowable_delta_since_epoch(bank: &Bank, max_allowable_drift: u32) -> i64 {
            let poh_estimate_offset = poh_estimate_offset(bank);
            (poh_estimate_offset.as_secs()
                + (poh_estimate_offset * max_allowable_drift / 100).as_secs()) as i64
        }

        let leader_pubkey = solana_sdk::pubkey::new_rand();
        let GenesisConfigInfo {
            mut genesis_config,
            voting_keypair,
            ..
        } = create_genesis_config_with_leader(5, &leader_pubkey, 3);
        let slots_in_epoch = 32;
        genesis_config.epoch_schedule = EpochSchedule::new(slots_in_epoch);
        let mut bank = Bank::new_for_tests(&genesis_config);
        let slot_duration = Duration::from_nanos(bank.ns_per_slot as u64);

        let recent_timestamp: UnixTimestamp = bank.unix_timestamp_from_genesis();
        let additional_secs = ((slot_duration * MAX_ALLOWABLE_DRIFT_PERCENTAGE_SLOW_V2 * 32) / 100)
            .as_secs() as i64
            + 1; // Greater than max_allowable_drift_slow_v2 for full epoch
        update_vote_account_timestamp(
            BlockTimestamp {
                slot: bank.slot(),
                timestamp: recent_timestamp + additional_secs,
            },
            &bank,
            &voting_keypair.pubkey(),
        );

        // additional_secs greater than MAX_ALLOWABLE_DRIFT_PERCENTAGE_SLOW_V2 for an epoch
        // timestamp bounded to 150% deviation
        for _ in 0..31 {
            bank = new_from_parent(&Arc::new(bank));
            assert_eq!(
                bank.clock().unix_timestamp,
                bank.clock().epoch_start_timestamp
                    + max_allowable_delta_since_epoch(
                        &bank,
                        MAX_ALLOWABLE_DRIFT_PERCENTAGE_SLOW_V2
                    ),
            );
            assert_eq!(bank.clock().epoch_start_timestamp, recent_timestamp);
        }
    }

    #[test]
    fn test_timestamp_fast() {
        fn max_allowable_delta_since_epoch(bank: &Bank, max_allowable_drift: u32) -> i64 {
            let poh_estimate_offset = poh_estimate_offset(bank);
            (poh_estimate_offset.as_secs()
                - (poh_estimate_offset * max_allowable_drift / 100).as_secs()) as i64
        }

        let leader_pubkey = solana_sdk::pubkey::new_rand();
        let GenesisConfigInfo {
            mut genesis_config,
            voting_keypair,
            ..
        } = create_genesis_config_with_leader(5, &leader_pubkey, 3);
        let slots_in_epoch = 32;
        genesis_config.epoch_schedule = EpochSchedule::new(slots_in_epoch);
        let mut bank = Bank::new_for_tests(&genesis_config);

        let recent_timestamp: UnixTimestamp = bank.unix_timestamp_from_genesis();
        let additional_secs = 5; // Greater than MAX_ALLOWABLE_DRIFT_PERCENTAGE_FAST for full epoch
        update_vote_account_timestamp(
            BlockTimestamp {
                slot: bank.slot(),
                timestamp: recent_timestamp - additional_secs,
            },
            &bank,
            &voting_keypair.pubkey(),
        );

        // additional_secs greater than MAX_ALLOWABLE_DRIFT_PERCENTAGE_FAST for an epoch
        // timestamp bounded to 25% deviation
        for _ in 0..31 {
            bank = new_from_parent(&Arc::new(bank));
            assert_eq!(
                bank.clock().unix_timestamp,
                bank.clock().epoch_start_timestamp
                    + max_allowable_delta_since_epoch(&bank, MAX_ALLOWABLE_DRIFT_PERCENTAGE_FAST),
            );
            assert_eq!(bank.clock().epoch_start_timestamp, recent_timestamp);
        }
    }

    #[test]
    fn test_program_is_native_loader() {
        let (genesis_config, mint_keypair) = create_genesis_config(50000);
        let bank = Bank::new_for_tests(&genesis_config);

        let tx = Transaction::new_signed_with_payer(
            &[Instruction::new_with_bincode(
                native_loader::id(),
                &(),
                vec![],
            )],
            Some(&mint_keypair.pubkey()),
            &[&mint_keypair],
            bank.last_blockhash(),
        );
        assert_eq!(
            bank.process_transaction(&tx),
            Err(TransactionError::InstructionError(
                0,
                InstructionError::UnsupportedProgramId
            ))
        );
    }

    #[test]
    fn test_debug_bank() {
        let (genesis_config, _mint_keypair) = create_genesis_config(50000);
        let mut bank = Bank::new_for_tests(&genesis_config);
        bank.finish_init(&genesis_config, None, false);
        let debug = format!("{bank:#?}");
        assert!(!debug.is_empty());
    }

    #[derive(Debug)]
    enum AcceptableScanResults {
        DroppedSlotError,
        NoFailure,
        Both,
    }

    fn test_store_scan_consistency<F: 'static>(
        update_f: F,
        drop_callback: Option<Box<dyn DropCallback + Send + Sync>>,
        acceptable_scan_results: AcceptableScanResults,
    ) where
        F: Fn(
                Arc<Bank>,
                crossbeam_channel::Sender<Arc<Bank>>,
                crossbeam_channel::Receiver<BankId>,
                Arc<HashSet<Pubkey>>,
                Pubkey,
                u64,
            ) + std::marker::Send,
    {
        solana_logger::setup();
        // Set up initial bank
        let mut genesis_config = create_genesis_config_with_leader(
            10,
            &solana_sdk::pubkey::new_rand(),
            374_999_998_287_840,
        )
        .genesis_config;
        genesis_config.rent = Rent::free();
        let bank0 = Arc::new(Bank::new_with_config_for_tests(
            &genesis_config,
            AccountSecondaryIndexes::default(),
            AccountShrinkThreshold::default(),
        ));
        bank0.set_callback(drop_callback);

        // Set up pubkeys to write to
        let total_pubkeys = ITER_BATCH_SIZE * 10;
        let total_pubkeys_to_modify = 10;
        let all_pubkeys: Vec<Pubkey> = std::iter::repeat_with(solana_sdk::pubkey::new_rand)
            .take(total_pubkeys)
            .collect();
        let program_id = system_program::id();
        let starting_lamports = 1;
        let starting_account = AccountSharedData::new(starting_lamports, 0, &program_id);

        // Write accounts to the store
        for key in &all_pubkeys {
            bank0.store_account(key, &starting_account);
        }

        // Set aside a subset of accounts to modify
        let pubkeys_to_modify: Arc<HashSet<Pubkey>> = Arc::new(
            all_pubkeys
                .into_iter()
                .take(total_pubkeys_to_modify)
                .collect(),
        );
        let exit = Arc::new(AtomicBool::new(false));

        // Thread that runs scan and constantly checks for
        // consistency
        let pubkeys_to_modify_ = pubkeys_to_modify.clone();

        // Channel over which the bank to scan is sent
        let (bank_to_scan_sender, bank_to_scan_receiver): (
            crossbeam_channel::Sender<Arc<Bank>>,
            crossbeam_channel::Receiver<Arc<Bank>>,
        ) = bounded(1);

        let (scan_finished_sender, scan_finished_receiver): (
            crossbeam_channel::Sender<BankId>,
            crossbeam_channel::Receiver<BankId>,
        ) = unbounded();
        let num_banks_scanned = Arc::new(AtomicU64::new(0));
        let scan_thread = {
            let exit = exit.clone();
            let num_banks_scanned = num_banks_scanned.clone();
            Builder::new()
                .name("scan".to_string())
                .spawn(move || {
                    loop {
                        info!("starting scan iteration");
                        if exit.load(Relaxed) {
                            info!("scan exiting");
                            return;
                        }
                        if let Ok(bank_to_scan) =
                            bank_to_scan_receiver.recv_timeout(Duration::from_millis(10))
                        {
                            info!("scanning program accounts for slot {}", bank_to_scan.slot());
                            let accounts_result = bank_to_scan
                                .get_program_accounts(&program_id, &ScanConfig::default());
                            let _ = scan_finished_sender.send(bank_to_scan.bank_id());
                            num_banks_scanned.fetch_add(1, Relaxed);
                            match (&acceptable_scan_results, accounts_result.is_err()) {
                                (AcceptableScanResults::DroppedSlotError, _)
                                | (AcceptableScanResults::Both, true) => {
                                    assert_eq!(
                                        accounts_result,
                                        Err(ScanError::SlotRemoved {
                                            slot: bank_to_scan.slot(),
                                            bank_id: bank_to_scan.bank_id()
                                        })
                                    );
                                }
                                (AcceptableScanResults::NoFailure, _)
                                | (AcceptableScanResults::Both, false) => {
                                    assert!(accounts_result.is_ok())
                                }
                            }

                            // Should never see empty accounts because no slot ever deleted
                            // any of the original accounts, and the scan should reflect the
                            // account state at some frozen slot `X` (no partial updates).
                            if let Ok(accounts) = accounts_result {
                                assert!(!accounts.is_empty());
                                let mut expected_lamports = None;
                                let mut target_accounts_found = HashSet::new();
                                for (pubkey, account) in accounts {
                                    let account_balance = account.lamports();
                                    if pubkeys_to_modify_.contains(&pubkey) {
                                        target_accounts_found.insert(pubkey);
                                        if let Some(expected_lamports) = expected_lamports {
                                            assert_eq!(account_balance, expected_lamports);
                                        } else {
                                            // All pubkeys in the specified set should have the same balance
                                            expected_lamports = Some(account_balance);
                                        }
                                    }
                                }

                                // Should've found all the accounts, i.e. no partial cleans should
                                // be detected
                                assert_eq!(target_accounts_found.len(), total_pubkeys_to_modify);
                            }
                        }
                    }
                })
                .unwrap()
        };

        // Thread that constantly updates the accounts, sets
        // roots, and cleans
        let update_thread = Builder::new()
            .name("update".to_string())
            .spawn(move || {
                update_f(
                    bank0,
                    bank_to_scan_sender,
                    scan_finished_receiver,
                    pubkeys_to_modify,
                    program_id,
                    starting_lamports,
                );
            })
            .unwrap();

        // Let threads run for a while, check the scans didn't see any mixed slots
        let min_expected_number_of_scans = 5;
        std::thread::sleep(Duration::new(5, 0));
        // This can be reduced when you are running this test locally to deal with hangs
        // But, if it is too low, the ci fails intermittently.
        let mut remaining_loops = 2000;
        loop {
            if num_banks_scanned.load(Relaxed) > min_expected_number_of_scans {
                break;
            } else {
                std::thread::sleep(Duration::from_millis(100));
            }
            remaining_loops -= 1;
            if remaining_loops == 0 {
                break; // just quit and try to get the thread result (panic, etc.)
            }
        }
        exit.store(true, Relaxed);
        scan_thread.join().unwrap();
        update_thread.join().unwrap();
        assert!(remaining_loops > 0, "test timed out");
    }

    #[test]
    fn test_store_scan_consistency_unrooted() {
        let (pruned_banks_sender, pruned_banks_receiver) = unbounded();
        let pruned_banks_request_handler = PrunedBanksRequestHandler {
            pruned_banks_receiver,
        };
        test_store_scan_consistency(
            move |bank0,
                  bank_to_scan_sender,
                  _scan_finished_receiver,
                  pubkeys_to_modify,
                  program_id,
                  starting_lamports| {
                let mut current_major_fork_bank = bank0;
                loop {
                    let mut current_minor_fork_bank = current_major_fork_bank.clone();
                    let num_new_banks = 2;
                    let lamports = current_minor_fork_bank.slot() + starting_lamports + 1;
                    // Modify banks on the two banks on the minor fork
                    for pubkeys_to_modify in &pubkeys_to_modify
                        .iter()
                        .chunks(pubkeys_to_modify.len() / num_new_banks)
                    {
                        current_minor_fork_bank = Arc::new(Bank::new_from_parent(
                            &current_minor_fork_bank,
                            &solana_sdk::pubkey::new_rand(),
                            current_minor_fork_bank.slot() + 2,
                        ));
                        let account = AccountSharedData::new(lamports, 0, &program_id);
                        // Write partial updates to each of the banks in the minor fork so if any of them
                        // get cleaned up, there will be keys with the wrong account value/missing.
                        for key in pubkeys_to_modify {
                            current_minor_fork_bank.store_account(key, &account);
                        }
                        current_minor_fork_bank.freeze();
                    }

                    // All the parent banks made in this iteration of the loop
                    // are currently discoverable, previous parents should have
                    // been squashed
                    assert_eq!(
                        current_minor_fork_bank.clone().parents_inclusive().len(),
                        num_new_banks + 1,
                    );

                    // `next_major_bank` needs to be sandwiched between the minor fork banks
                    // That way, after the squash(), the minor fork has the potential to see a
                    // *partial* clean of the banks < `next_major_bank`.
                    current_major_fork_bank = Arc::new(Bank::new_from_parent(
                        &current_major_fork_bank,
                        &solana_sdk::pubkey::new_rand(),
                        current_minor_fork_bank.slot() - 1,
                    ));
                    let lamports = current_major_fork_bank.slot() + starting_lamports + 1;
                    let account = AccountSharedData::new(lamports, 0, &program_id);
                    for key in pubkeys_to_modify.iter() {
                        // Store rooted updates to these pubkeys such that the minor
                        // fork updates to the same keys will be deleted by clean
                        current_major_fork_bank.store_account(key, &account);
                    }

                    // Send the last new bank to the scan thread to perform the scan.
                    // Meanwhile this thread will continually set roots on a separate fork
                    // and squash/clean, purging the account entries from the minor forks
                    /*
                                bank 0
                            /         \
                    minor bank 1       \
                        /         current_major_fork_bank
                    minor bank 2

                    */
                    // The capacity of the channel is 1 so that this thread will wait for the scan to finish before starting
                    // the next iteration, allowing the scan to stay in sync with these updates
                    // such that every scan will see this interruption.
                    if bank_to_scan_sender.send(current_minor_fork_bank).is_err() {
                        // Channel was disconnected, exit
                        return;
                    }
                    current_major_fork_bank.freeze();
                    current_major_fork_bank.squash();
                    // Try to get cache flush/clean to overlap with the scan
                    current_major_fork_bank.force_flush_accounts_cache();
                    current_major_fork_bank.clean_accounts_for_tests();
                    // Move purge here so that Bank::drop()->purge_slots() doesn't race
                    // with clean. Simulates the call from AccountsBackgroundService
                    pruned_banks_request_handler.handle_request(&current_major_fork_bank, true);
                }
            },
            Some(Box::new(SendDroppedBankCallback::new(pruned_banks_sender))),
            AcceptableScanResults::NoFailure,
        )
    }

    #[test]
    fn test_store_scan_consistency_root() {
        test_store_scan_consistency(
            |bank0,
             bank_to_scan_sender,
             _scan_finished_receiver,
             pubkeys_to_modify,
             program_id,
             starting_lamports| {
                let mut current_bank = bank0.clone();
                let mut prev_bank = bank0;
                loop {
                    let lamports_this_round = current_bank.slot() + starting_lamports + 1;
                    let account = AccountSharedData::new(lamports_this_round, 0, &program_id);
                    for key in pubkeys_to_modify.iter() {
                        current_bank.store_account(key, &account);
                    }
                    current_bank.freeze();
                    // Send the previous bank to the scan thread to perform the scan.
                    // Meanwhile this thread will squash and update roots immediately after
                    // so the roots will update while scanning.
                    //
                    // The capacity of the channel is 1 so that this thread will wait for the scan to finish before starting
                    // the next iteration, allowing the scan to stay in sync with these updates
                    // such that every scan will see this interruption.
                    if bank_to_scan_sender.send(prev_bank).is_err() {
                        // Channel was disconnected, exit
                        return;
                    }
                    current_bank.squash();
                    if current_bank.slot() % 2 == 0 {
                        current_bank.force_flush_accounts_cache();
                        current_bank.clean_accounts(None);
                    }
                    prev_bank = current_bank.clone();
                    current_bank = Arc::new(Bank::new_from_parent(
                        &current_bank,
                        &solana_sdk::pubkey::new_rand(),
                        current_bank.slot() + 1,
                    ));
                }
            },
            None,
            AcceptableScanResults::NoFailure,
        );
    }

    fn setup_banks_on_fork_to_remove(
        bank0: Arc<Bank>,
        pubkeys_to_modify: Arc<HashSet<Pubkey>>,
        program_id: &Pubkey,
        starting_lamports: u64,
        num_banks_on_fork: usize,
        step_size: usize,
    ) -> (Arc<Bank>, Vec<(Slot, BankId)>, Ancestors) {
        // Need at least 2 keys to create inconsistency in account balances when deleting
        // slots
        assert!(pubkeys_to_modify.len() > 1);

        // Tracks the bank at the tip of the to be created fork
        let mut bank_at_fork_tip = bank0;

        // All the slots on the fork except slot 0
        let mut slots_on_fork = Vec::with_capacity(num_banks_on_fork);

        // All accounts in each set of `step_size` slots will have the same account balances.
        // The account balances of the accounts changes every `step_size` banks. Thus if you
        // delete any one of the latest `step_size` slots, then you will see varying account
        // balances when loading the accounts.
        assert!(num_banks_on_fork >= 2);
        assert!(step_size >= 2);
        let pubkeys_to_modify: Vec<Pubkey> = pubkeys_to_modify.iter().cloned().collect();
        let pubkeys_to_modify_per_slot = (pubkeys_to_modify.len() / step_size).max(1);
        for _ in (0..num_banks_on_fork).step_by(step_size) {
            let mut lamports_this_round = 0;
            for i in 0..step_size {
                bank_at_fork_tip = Arc::new(Bank::new_from_parent(
                    &bank_at_fork_tip,
                    &solana_sdk::pubkey::new_rand(),
                    bank_at_fork_tip.slot() + 1,
                ));
                if lamports_this_round == 0 {
                    lamports_this_round = bank_at_fork_tip.bank_id() + starting_lamports + 1;
                }
                let pubkey_to_modify_starting_index = i * pubkeys_to_modify_per_slot;
                let account = AccountSharedData::new(lamports_this_round, 0, program_id);
                for pubkey_index_to_modify in pubkey_to_modify_starting_index
                    ..pubkey_to_modify_starting_index + pubkeys_to_modify_per_slot
                {
                    let key = pubkeys_to_modify[pubkey_index_to_modify % pubkeys_to_modify.len()];
                    bank_at_fork_tip.store_account(&key, &account);
                }
                bank_at_fork_tip.freeze();
                slots_on_fork.push((bank_at_fork_tip.slot(), bank_at_fork_tip.bank_id()));
            }
        }

        let ancestors: Vec<(Slot, usize)> = slots_on_fork.iter().map(|(s, _)| (*s, 0)).collect();
        let ancestors = Ancestors::from(ancestors);

        (bank_at_fork_tip, slots_on_fork, ancestors)
    }

    #[test]
    fn test_remove_unrooted_before_scan() {
        test_store_scan_consistency(
            |bank0,
             bank_to_scan_sender,
             scan_finished_receiver,
             pubkeys_to_modify,
             program_id,
             starting_lamports| {
                loop {
                    let (bank_at_fork_tip, slots_on_fork, ancestors) =
                        setup_banks_on_fork_to_remove(
                            bank0.clone(),
                            pubkeys_to_modify.clone(),
                            &program_id,
                            starting_lamports,
                            10,
                            2,
                        );
                    // Test removing the slot before the scan starts, should cause
                    // SlotRemoved error every time
                    for k in pubkeys_to_modify.iter() {
                        assert!(bank_at_fork_tip.load_slow(&ancestors, k).is_some());
                    }
                    bank_at_fork_tip.remove_unrooted_slots(&slots_on_fork);

                    // Accounts on this fork should not be found after removal
                    for k in pubkeys_to_modify.iter() {
                        assert!(bank_at_fork_tip.load_slow(&ancestors, k).is_none());
                    }
                    if bank_to_scan_sender.send(bank_at_fork_tip.clone()).is_err() {
                        return;
                    }

                    // Wait for scan to finish before starting next iteration
                    let finished_scan_bank_id = scan_finished_receiver.recv();
                    if finished_scan_bank_id.is_err() {
                        return;
                    }
                    assert_eq!(finished_scan_bank_id.unwrap(), bank_at_fork_tip.bank_id());
                }
            },
            None,
            // Test removing the slot before the scan starts, should error every time
            AcceptableScanResults::DroppedSlotError,
        );
    }

    #[test]
    fn test_remove_unrooted_scan_then_recreate_same_slot_before_scan() {
        test_store_scan_consistency(
            |bank0,
             bank_to_scan_sender,
             scan_finished_receiver,
             pubkeys_to_modify,
             program_id,
             starting_lamports| {
                let mut prev_bank = bank0.clone();
                loop {
                    let start = Instant::now();
                    let (bank_at_fork_tip, slots_on_fork, ancestors) =
                        setup_banks_on_fork_to_remove(
                            bank0.clone(),
                            pubkeys_to_modify.clone(),
                            &program_id,
                            starting_lamports,
                            10,
                            2,
                        );
                    info!("setting up banks elapsed: {}", start.elapsed().as_millis());
                    // Remove the fork. Then we'll recreate the slots and only after we've
                    // recreated the slots, do we send this old bank for scanning.
                    // Skip scanning bank 0 on first iteration of loop, since those accounts
                    // aren't being removed
                    if prev_bank.slot() != 0 {
                        info!(
                            "sending bank with slot: {:?}, elapsed: {}",
                            prev_bank.slot(),
                            start.elapsed().as_millis()
                        );
                        // Although we dumped the slots last iteration via `remove_unrooted_slots()`,
                        // we've recreated those slots this iteration, so they should be findable
                        // again
                        for k in pubkeys_to_modify.iter() {
                            assert!(bank_at_fork_tip.load_slow(&ancestors, k).is_some());
                        }

                        // Now after we've recreated the slots removed in the previous loop
                        // iteration, send the previous bank, should fail even though the
                        // same slots were recreated
                        if bank_to_scan_sender.send(prev_bank.clone()).is_err() {
                            return;
                        }

                        let finished_scan_bank_id = scan_finished_receiver.recv();
                        if finished_scan_bank_id.is_err() {
                            return;
                        }
                        // Wait for scan to finish before starting next iteration
                        assert_eq!(finished_scan_bank_id.unwrap(), prev_bank.bank_id());
                    }
                    bank_at_fork_tip.remove_unrooted_slots(&slots_on_fork);
                    prev_bank = bank_at_fork_tip;
                }
            },
            None,
            // Test removing the slot before the scan starts, should error every time
            AcceptableScanResults::DroppedSlotError,
        );
    }

    #[test]
    fn test_remove_unrooted_scan_interleaved_with_remove_unrooted_slots() {
        test_store_scan_consistency(
            |bank0,
             bank_to_scan_sender,
             scan_finished_receiver,
             pubkeys_to_modify,
             program_id,
             starting_lamports| {
                loop {
                    let step_size = 2;
                    let (bank_at_fork_tip, slots_on_fork, ancestors) =
                        setup_banks_on_fork_to_remove(
                            bank0.clone(),
                            pubkeys_to_modify.clone(),
                            &program_id,
                            starting_lamports,
                            10,
                            step_size,
                        );
                    // Although we dumped the slots last iteration via `remove_unrooted_slots()`,
                    // we've recreated those slots this iteration, so they should be findable
                    // again
                    for k in pubkeys_to_modify.iter() {
                        assert!(bank_at_fork_tip.load_slow(&ancestors, k).is_some());
                    }

                    // Now after we've recreated the slots removed in the previous loop
                    // iteration, send the previous bank, should fail even though the
                    // same slots were recreated
                    if bank_to_scan_sender.send(bank_at_fork_tip.clone()).is_err() {
                        return;
                    }

                    // Remove 1 < `step_size` of the *latest* slots while the scan is happening.
                    // This should create inconsistency between the account balances of accounts
                    // stored in that slot, and the accounts stored in earlier slots
                    let slot_to_remove = *slots_on_fork.last().unwrap();
                    bank_at_fork_tip.remove_unrooted_slots(&[slot_to_remove]);

                    // Wait for scan to finish before starting next iteration
                    let finished_scan_bank_id = scan_finished_receiver.recv();
                    if finished_scan_bank_id.is_err() {
                        return;
                    }
                    assert_eq!(finished_scan_bank_id.unwrap(), bank_at_fork_tip.bank_id());

                    // Remove the rest of the slots before the next iteration
                    for (slot, bank_id) in slots_on_fork {
                        bank_at_fork_tip.remove_unrooted_slots(&[(slot, bank_id)]);
                    }
                }
            },
            None,
            // Test removing the slot before the scan starts, should error every time
            AcceptableScanResults::Both,
        );
    }

    #[test]
    fn test_get_inflation_start_slot_devnet_testnet() {
        let GenesisConfigInfo {
            mut genesis_config, ..
        } = create_genesis_config_with_leader(42, &solana_sdk::pubkey::new_rand(), 42);
        genesis_config
            .accounts
            .remove(&feature_set::pico_inflation::id())
            .unwrap();
        genesis_config
            .accounts
            .remove(&feature_set::full_inflation::devnet_and_testnet::id())
            .unwrap();
        for pair in feature_set::FULL_INFLATION_FEATURE_PAIRS.iter() {
            genesis_config.accounts.remove(&pair.vote_id).unwrap();
            genesis_config.accounts.remove(&pair.enable_id).unwrap();
        }

        let bank = Bank::new_for_tests(&genesis_config);

        // Advance slot
        let mut bank = new_from_parent(&Arc::new(bank));
        bank = new_from_parent(&Arc::new(bank));
        assert_eq!(bank.get_inflation_start_slot(), 0);
        assert_eq!(bank.slot(), 2);

        // Request `pico_inflation` activation
        bank.store_account(
            &feature_set::pico_inflation::id(),
            &feature::create_account(
                &Feature {
                    activated_at: Some(1),
                },
                42,
            ),
        );
        bank.compute_active_feature_set(true);
        assert_eq!(bank.get_inflation_start_slot(), 1);

        // Advance slot
        bank = new_from_parent(&Arc::new(bank));
        assert_eq!(bank.slot(), 3);

        // Request `full_inflation::devnet_and_testnet` activation,
        // which takes priority over pico_inflation
        bank.store_account(
            &feature_set::full_inflation::devnet_and_testnet::id(),
            &feature::create_account(
                &Feature {
                    activated_at: Some(2),
                },
                42,
            ),
        );
        bank.compute_active_feature_set(true);
        assert_eq!(bank.get_inflation_start_slot(), 2);

        // Request `full_inflation::mainnet::certusone` activation,
        // which should have no effect on `get_inflation_start_slot`
        bank.store_account(
            &feature_set::full_inflation::mainnet::certusone::vote::id(),
            &feature::create_account(
                &Feature {
                    activated_at: Some(3),
                },
                42,
            ),
        );
        bank.store_account(
            &feature_set::full_inflation::mainnet::certusone::enable::id(),
            &feature::create_account(
                &Feature {
                    activated_at: Some(3),
                },
                42,
            ),
        );
        bank.compute_active_feature_set(true);
        assert_eq!(bank.get_inflation_start_slot(), 2);
    }

    #[test]
    fn test_get_inflation_start_slot_mainnet() {
        let GenesisConfigInfo {
            mut genesis_config, ..
        } = create_genesis_config_with_leader(42, &solana_sdk::pubkey::new_rand(), 42);
        genesis_config
            .accounts
            .remove(&feature_set::pico_inflation::id())
            .unwrap();
        genesis_config
            .accounts
            .remove(&feature_set::full_inflation::devnet_and_testnet::id())
            .unwrap();
        for pair in feature_set::FULL_INFLATION_FEATURE_PAIRS.iter() {
            genesis_config.accounts.remove(&pair.vote_id).unwrap();
            genesis_config.accounts.remove(&pair.enable_id).unwrap();
        }

        let bank = Bank::new_for_tests(&genesis_config);

        // Advance slot
        let mut bank = new_from_parent(&Arc::new(bank));
        bank = new_from_parent(&Arc::new(bank));
        assert_eq!(bank.get_inflation_start_slot(), 0);
        assert_eq!(bank.slot(), 2);

        // Request `pico_inflation` activation
        bank.store_account(
            &feature_set::pico_inflation::id(),
            &feature::create_account(
                &Feature {
                    activated_at: Some(1),
                },
                42,
            ),
        );
        bank.compute_active_feature_set(true);
        assert_eq!(bank.get_inflation_start_slot(), 1);

        // Advance slot
        bank = new_from_parent(&Arc::new(bank));
        assert_eq!(bank.slot(), 3);

        // Request `full_inflation::mainnet::certusone` activation,
        // which takes priority over pico_inflation
        bank.store_account(
            &feature_set::full_inflation::mainnet::certusone::vote::id(),
            &feature::create_account(
                &Feature {
                    activated_at: Some(2),
                },
                42,
            ),
        );
        bank.store_account(
            &feature_set::full_inflation::mainnet::certusone::enable::id(),
            &feature::create_account(
                &Feature {
                    activated_at: Some(2),
                },
                42,
            ),
        );
        bank.compute_active_feature_set(true);
        assert_eq!(bank.get_inflation_start_slot(), 2);

        // Advance slot
        bank = new_from_parent(&Arc::new(bank));
        assert_eq!(bank.slot(), 4);

        // Request `full_inflation::devnet_and_testnet` activation,
        // which should have no effect on `get_inflation_start_slot`
        bank.store_account(
            &feature_set::full_inflation::devnet_and_testnet::id(),
            &feature::create_account(
                &Feature {
                    activated_at: Some(bank.slot()),
                },
                42,
            ),
        );
        bank.compute_active_feature_set(true);
        assert_eq!(bank.get_inflation_start_slot(), 2);
    }

    #[test]
    fn test_get_inflation_num_slots_with_activations() {
        let GenesisConfigInfo {
            mut genesis_config, ..
        } = create_genesis_config_with_leader(42, &solana_sdk::pubkey::new_rand(), 42);
        let slots_per_epoch = 32;
        genesis_config.epoch_schedule = EpochSchedule::new(slots_per_epoch);
        genesis_config
            .accounts
            .remove(&feature_set::pico_inflation::id())
            .unwrap();
        genesis_config
            .accounts
            .remove(&feature_set::full_inflation::devnet_and_testnet::id())
            .unwrap();
        for pair in feature_set::FULL_INFLATION_FEATURE_PAIRS.iter() {
            genesis_config.accounts.remove(&pair.vote_id).unwrap();
            genesis_config.accounts.remove(&pair.enable_id).unwrap();
        }

        let mut bank = Bank::new_for_tests(&genesis_config);
        assert_eq!(bank.get_inflation_num_slots(), 0);
        for _ in 0..2 * slots_per_epoch {
            bank = new_from_parent(&Arc::new(bank));
        }
        assert_eq!(bank.get_inflation_num_slots(), 2 * slots_per_epoch);

        // Activate pico_inflation
        let pico_inflation_activation_slot = bank.slot();
        bank.store_account(
            &feature_set::pico_inflation::id(),
            &feature::create_account(
                &Feature {
                    activated_at: Some(pico_inflation_activation_slot),
                },
                42,
            ),
        );
        bank.compute_active_feature_set(true);
        assert_eq!(bank.get_inflation_num_slots(), slots_per_epoch);
        for _ in 0..slots_per_epoch {
            bank = new_from_parent(&Arc::new(bank));
        }
        assert_eq!(bank.get_inflation_num_slots(), 2 * slots_per_epoch);

        // Activate full_inflation::devnet_and_testnet
        let full_inflation_activation_slot = bank.slot();
        bank.store_account(
            &feature_set::full_inflation::devnet_and_testnet::id(),
            &feature::create_account(
                &Feature {
                    activated_at: Some(full_inflation_activation_slot),
                },
                42,
            ),
        );
        bank.compute_active_feature_set(true);
        assert_eq!(bank.get_inflation_num_slots(), slots_per_epoch);
        for _ in 0..slots_per_epoch {
            bank = new_from_parent(&Arc::new(bank));
        }
        assert_eq!(bank.get_inflation_num_slots(), 2 * slots_per_epoch);
    }

    #[test]
    fn test_get_inflation_num_slots_already_activated() {
        let GenesisConfigInfo {
            mut genesis_config, ..
        } = create_genesis_config_with_leader(42, &solana_sdk::pubkey::new_rand(), 42);
        let slots_per_epoch = 32;
        genesis_config.epoch_schedule = EpochSchedule::new(slots_per_epoch);
        let mut bank = Bank::new_for_tests(&genesis_config);
        assert_eq!(bank.get_inflation_num_slots(), 0);
        for _ in 0..slots_per_epoch {
            bank = new_from_parent(&Arc::new(bank));
        }
        assert_eq!(bank.get_inflation_num_slots(), slots_per_epoch);
        for _ in 0..slots_per_epoch {
            bank = new_from_parent(&Arc::new(bank));
        }
        assert_eq!(bank.get_inflation_num_slots(), 2 * slots_per_epoch);
    }

    #[test]
    fn test_stake_vote_account_validity() {
        let thread_pool = ThreadPoolBuilder::new().num_threads(1).build().unwrap();
        check_stake_vote_account_validity(
            true, // check owner change,
            |bank: &Bank| {
                bank.load_vote_and_stake_accounts_with_thread_pool(&thread_pool, null_tracer())
            },
        );
        // TODO: stakes cache should be hardened for the case when the account
        // owner is changed from vote/stake program to something else. see:
        // https://github.com/solana-labs/solana/pull/24200#discussion_r849935444
        check_stake_vote_account_validity(
            false, // check owner change
            |bank: &Bank| bank.load_vote_and_stake_accounts(&thread_pool, null_tracer()),
        );
    }

    fn check_stake_vote_account_validity<F>(
        check_owner_change: bool,
        load_vote_and_stake_accounts: F,
    ) where
        F: Fn(&Bank) -> LoadVoteAndStakeAccountsResult,
    {
        let validator_vote_keypairs0 = ValidatorVoteKeypairs::new_rand();
        let validator_vote_keypairs1 = ValidatorVoteKeypairs::new_rand();
        let validator_keypairs = vec![&validator_vote_keypairs0, &validator_vote_keypairs1];
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config_with_vote_accounts(
            1_000_000_000,
            &validator_keypairs,
            vec![10_000; 2],
        );
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));
        let vote_and_stake_accounts =
            load_vote_and_stake_accounts(&bank).vote_with_stake_delegations_map;
        assert_eq!(vote_and_stake_accounts.len(), 2);

        let mut vote_account = bank
            .get_account(&validator_vote_keypairs0.vote_keypair.pubkey())
            .unwrap_or_default();
        let original_lamports = vote_account.lamports();
        vote_account.set_lamports(0);
        // Simulate vote account removal via full withdrawal
        bank.store_account(
            &validator_vote_keypairs0.vote_keypair.pubkey(),
            &vote_account,
        );

        // Modify staked vote account owner; a vote account owned by another program could be
        // freely modified with malicious data
        let bogus_vote_program = Pubkey::new_unique();
        vote_account.set_lamports(original_lamports);
        vote_account.set_owner(bogus_vote_program);
        bank.store_account(
            &validator_vote_keypairs0.vote_keypair.pubkey(),
            &vote_account,
        );

        assert_eq!(bank.vote_accounts().len(), 1);

        // Modify stake account owner; a stake account owned by another program could be freely
        // modified with malicious data
        let bogus_stake_program = Pubkey::new_unique();
        let mut stake_account = bank
            .get_account(&validator_vote_keypairs1.stake_keypair.pubkey())
            .unwrap_or_default();
        stake_account.set_owner(bogus_stake_program);
        bank.store_account(
            &validator_vote_keypairs1.stake_keypair.pubkey(),
            &stake_account,
        );

        // Accounts must be valid stake and vote accounts
        let vote_and_stake_accounts =
            load_vote_and_stake_accounts(&bank).vote_with_stake_delegations_map;
        assert_eq!(
            vote_and_stake_accounts.len(),
            usize::from(!check_owner_change)
        );
    }

    #[test]
    fn test_vote_epoch_panic() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config_with_leader(
            1_000_000_000_000_000,
            &Pubkey::new_unique(),
            bootstrap_validator_stake_lamports(),
        );
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));

        let vote_keypair = keypair_from_seed(&[1u8; 32]).unwrap();
        let stake_keypair = keypair_from_seed(&[2u8; 32]).unwrap();

        let mut setup_ixs = Vec::new();
        setup_ixs.extend(
            vote_instruction::create_account(
                &mint_keypair.pubkey(),
                &vote_keypair.pubkey(),
                &VoteInit {
                    node_pubkey: mint_keypair.pubkey(),
                    authorized_voter: vote_keypair.pubkey(),
                    authorized_withdrawer: mint_keypair.pubkey(),
                    commission: 0,
                },
                1_000_000_000,
            )
            .into_iter(),
        );
        setup_ixs.extend(
            stake_instruction::create_account_and_delegate_stake(
                &mint_keypair.pubkey(),
                &stake_keypair.pubkey(),
                &vote_keypair.pubkey(),
                &Authorized::auto(&mint_keypair.pubkey()),
                &Lockup::default(),
                1_000_000_000_000,
            )
            .into_iter(),
        );
        setup_ixs.push(vote_instruction::withdraw(
            &vote_keypair.pubkey(),
            &mint_keypair.pubkey(),
            1_000_000_000,
            &mint_keypair.pubkey(),
        ));
        setup_ixs.push(system_instruction::transfer(
            &mint_keypair.pubkey(),
            &vote_keypair.pubkey(),
            1_000_000_000,
        ));

        let result = bank.process_transaction(&Transaction::new(
            &[&mint_keypair, &vote_keypair, &stake_keypair],
            Message::new(&setup_ixs, Some(&mint_keypair.pubkey())),
            bank.last_blockhash(),
        ));
        assert!(result.is_ok());

        let _bank = Bank::new_from_parent(
            &bank,
            &mint_keypair.pubkey(),
            genesis_config.epoch_schedule.get_first_slot_in_epoch(1),
        );
    }

    #[test]
    fn test_tx_log_order() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config_with_leader(
            1_000_000_000_000_000,
            &Pubkey::new_unique(),
            bootstrap_validator_stake_lamports(),
        );
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));
        *bank.transaction_log_collector_config.write().unwrap() = TransactionLogCollectorConfig {
            mentioned_addresses: HashSet::new(),
            filter: TransactionLogCollectorFilter::All,
        };
        let blockhash = bank.last_blockhash();

        let sender0 = Keypair::new();
        let sender1 = Keypair::new();
        bank.transfer(100, &mint_keypair, &sender0.pubkey())
            .unwrap();
        bank.transfer(100, &mint_keypair, &sender1.pubkey())
            .unwrap();

        let recipient0 = Pubkey::new_unique();
        let recipient1 = Pubkey::new_unique();
        let tx0 = system_transaction::transfer(&sender0, &recipient0, 10, blockhash);
        let success_sig = tx0.signatures[0];
        let tx1 = system_transaction::transfer(&sender1, &recipient1, 110, blockhash); // Should produce insufficient funds log
        let failure_sig = tx1.signatures[0];
        let tx2 = system_transaction::transfer(&sender0, &recipient0, 1, blockhash);
        let txs = vec![tx0, tx1, tx2];
        let batch = bank.prepare_batch_for_tests(txs);

        let execution_results = bank
            .load_execute_and_commit_transactions(
                &batch,
                MAX_PROCESSING_AGE,
                false,
                false,
                true,
                false,
                &mut ExecuteTimings::default(),
                None,
            )
            .0
            .execution_results;

        assert_eq!(execution_results.len(), 3);

        assert!(execution_results[0].details().is_some());
        assert!(execution_results[0]
            .details()
            .unwrap()
            .log_messages
            .as_ref()
            .unwrap()[1]
            .contains(&"success".to_string()));
        assert!(execution_results[1].details().is_some());
        assert!(execution_results[1]
            .details()
            .unwrap()
            .log_messages
            .as_ref()
            .unwrap()[2]
            .contains(&"failed".to_string()));
        assert!(!execution_results[2].was_executed());

        let stored_logs = &bank.transaction_log_collector.read().unwrap().logs;
        let success_log_info = stored_logs
            .iter()
            .find(|transaction_log_info| transaction_log_info.signature == success_sig)
            .unwrap();
        assert!(success_log_info.result.is_ok());
        let success_log = success_log_info.log_messages.clone().pop().unwrap();
        assert!(success_log.contains(&"success".to_string()));
        let failure_log_info = stored_logs
            .iter()
            .find(|transaction_log_info| transaction_log_info.signature == failure_sig)
            .unwrap();
        assert!(failure_log_info.result.is_err());
        let failure_log = failure_log_info.log_messages.clone().pop().unwrap();
        assert!(failure_log.contains(&"failed".to_string()));
    }

    #[test]
    fn test_tx_return_data() {
        solana_logger::setup();
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config_with_leader(
            1_000_000_000_000_000,
            &Pubkey::new_unique(),
            bootstrap_validator_stake_lamports(),
        );
        let mut bank = Bank::new_for_tests(&genesis_config);

        let mock_program_id = Pubkey::new(&[2u8; 32]);
        fn mock_process_instruction(
            _first_instruction_account: IndexOfAccount,
            invoke_context: &mut InvokeContext,
        ) -> result::Result<(), InstructionError> {
            let mock_program_id = Pubkey::new(&[2u8; 32]);
            let transaction_context = &mut invoke_context.transaction_context;
            let instruction_context = transaction_context.get_current_instruction_context()?;
            let instruction_data = instruction_context.get_instruction_data();
            let mut return_data = [0u8; MAX_RETURN_DATA];
            if !instruction_data.is_empty() {
                let index = usize::from_le_bytes(instruction_data.try_into().unwrap());
                return_data[index] = 1;
                transaction_context
                    .set_return_data(mock_program_id, return_data.to_vec())
                    .unwrap();
            }
            Ok(())
        }
        let blockhash = bank.last_blockhash();
        bank.add_builtin("mock_program", &mock_program_id, mock_process_instruction);

        for index in [
            None,
            Some(0),
            Some(MAX_RETURN_DATA / 2),
            Some(MAX_RETURN_DATA - 1),
        ] {
            let data = if let Some(index) = index {
                usize::to_le_bytes(index).to_vec()
            } else {
                Vec::new()
            };
            let txs = vec![Transaction::new_signed_with_payer(
                &[Instruction {
                    program_id: mock_program_id,
                    data,
                    accounts: vec![AccountMeta::new(Pubkey::new_unique(), false)],
                }],
                Some(&mint_keypair.pubkey()),
                &[&mint_keypair],
                blockhash,
            )];
            let batch = bank.prepare_batch_for_tests(txs);
            let return_data = bank
                .load_execute_and_commit_transactions(
                    &batch,
                    MAX_PROCESSING_AGE,
                    false,
                    false,
                    false,
                    true,
                    &mut ExecuteTimings::default(),
                    None,
                )
                .0
                .execution_results[0]
                .details()
                .unwrap()
                .return_data
                .clone();
            if let Some(index) = index {
                let return_data = return_data.unwrap();
                assert_eq!(return_data.program_id, mock_program_id);
                let mut expected_data = vec![0u8; index];
                expected_data.push(1u8);
                assert_eq!(return_data.data, expected_data);
            } else {
                assert!(return_data.is_none());
            }
        }
    }

    #[test]
    fn test_get_largest_accounts() {
        let GenesisConfigInfo { genesis_config, .. } =
            create_genesis_config_with_leader(42, &solana_sdk::pubkey::new_rand(), 42);
        let bank = Bank::new_for_tests(&genesis_config);

        let pubkeys: Vec<_> = (0..5).map(|_| Pubkey::new_unique()).collect();
        let pubkeys_hashset: HashSet<_> = pubkeys.iter().cloned().collect();

        let pubkeys_balances: Vec<_> = pubkeys
            .iter()
            .cloned()
            .zip(vec![
                sol_to_lamports(2.0),
                sol_to_lamports(3.0),
                sol_to_lamports(3.0),
                sol_to_lamports(4.0),
                sol_to_lamports(5.0),
            ])
            .collect();

        // Initialize accounts; all have larger SOL balances than current Bank built-ins
        let account0 = AccountSharedData::new(pubkeys_balances[0].1, 0, &Pubkey::default());
        bank.store_account(&pubkeys_balances[0].0, &account0);
        let account1 = AccountSharedData::new(pubkeys_balances[1].1, 0, &Pubkey::default());
        bank.store_account(&pubkeys_balances[1].0, &account1);
        let account2 = AccountSharedData::new(pubkeys_balances[2].1, 0, &Pubkey::default());
        bank.store_account(&pubkeys_balances[2].0, &account2);
        let account3 = AccountSharedData::new(pubkeys_balances[3].1, 0, &Pubkey::default());
        bank.store_account(&pubkeys_balances[3].0, &account3);
        let account4 = AccountSharedData::new(pubkeys_balances[4].1, 0, &Pubkey::default());
        bank.store_account(&pubkeys_balances[4].0, &account4);

        // Create HashSet to exclude an account
        let exclude4: HashSet<_> = pubkeys[4..].iter().cloned().collect();

        let mut sorted_accounts = pubkeys_balances.clone();
        sorted_accounts.sort_by(|a, b| a.1.cmp(&b.1).reverse());

        // Return only one largest account
        assert_eq!(
            bank.get_largest_accounts(1, &pubkeys_hashset, AccountAddressFilter::Include)
                .unwrap(),
            vec![(pubkeys[4], sol_to_lamports(5.0))]
        );
        assert_eq!(
            bank.get_largest_accounts(1, &HashSet::new(), AccountAddressFilter::Exclude)
                .unwrap(),
            vec![(pubkeys[4], sol_to_lamports(5.0))]
        );
        assert_eq!(
            bank.get_largest_accounts(1, &exclude4, AccountAddressFilter::Exclude)
                .unwrap(),
            vec![(pubkeys[3], sol_to_lamports(4.0))]
        );

        // Return all added accounts
        let results = bank
            .get_largest_accounts(10, &pubkeys_hashset, AccountAddressFilter::Include)
            .unwrap();
        assert_eq!(results.len(), sorted_accounts.len());
        for pubkey_balance in sorted_accounts.iter() {
            assert!(results.contains(pubkey_balance));
        }
        let mut sorted_results = results.clone();
        sorted_results.sort_by(|a, b| a.1.cmp(&b.1).reverse());
        assert_eq!(sorted_results, results);

        let expected_accounts = sorted_accounts[1..].to_vec();
        let results = bank
            .get_largest_accounts(10, &exclude4, AccountAddressFilter::Exclude)
            .unwrap();
        // results include 5 Bank builtins
        assert_eq!(results.len(), 10);
        for pubkey_balance in expected_accounts.iter() {
            assert!(results.contains(pubkey_balance));
        }
        let mut sorted_results = results.clone();
        sorted_results.sort_by(|a, b| a.1.cmp(&b.1).reverse());
        assert_eq!(sorted_results, results);

        // Return 3 added accounts
        let expected_accounts = sorted_accounts[0..4].to_vec();
        let results = bank
            .get_largest_accounts(4, &pubkeys_hashset, AccountAddressFilter::Include)
            .unwrap();
        assert_eq!(results.len(), expected_accounts.len());
        for pubkey_balance in expected_accounts.iter() {
            assert!(results.contains(pubkey_balance));
        }

        let expected_accounts = expected_accounts[1..4].to_vec();
        let results = bank
            .get_largest_accounts(3, &exclude4, AccountAddressFilter::Exclude)
            .unwrap();
        assert_eq!(results.len(), expected_accounts.len());
        for pubkey_balance in expected_accounts.iter() {
            assert!(results.contains(pubkey_balance));
        }

        // Exclude more, and non-sequential, accounts
        let exclude: HashSet<_> = vec![pubkeys[0], pubkeys[2], pubkeys[4]]
            .iter()
            .cloned()
            .collect();
        assert_eq!(
            bank.get_largest_accounts(2, &exclude, AccountAddressFilter::Exclude)
                .unwrap(),
            vec![pubkeys_balances[3], pubkeys_balances[1]]
        );
    }

    #[test]
    fn test_transfer_sysvar() {
        solana_logger::setup();
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config_with_leader(
            1_000_000_000_000_000,
            &Pubkey::new_unique(),
            bootstrap_validator_stake_lamports(),
        );
        let mut bank = Bank::new_for_tests(&genesis_config);

        fn mock_ix_processor(
            _first_instruction_account: IndexOfAccount,
            invoke_context: &mut InvokeContext,
        ) -> std::result::Result<(), InstructionError> {
            let transaction_context = &invoke_context.transaction_context;
            let instruction_context = transaction_context.get_current_instruction_context()?;
            instruction_context
                .try_borrow_instruction_account(transaction_context, 1)?
                .set_data(vec![0; 40])?;
            Ok(())
        }

        let program_id = solana_sdk::pubkey::new_rand();
        bank.add_builtin("mock_program1", &program_id, mock_ix_processor);

        let blockhash = bank.last_blockhash();
        #[allow(deprecated)]
        let blockhash_sysvar = sysvar::clock::id();
        #[allow(deprecated)]
        let orig_lamports = bank.get_account(&sysvar::clock::id()).unwrap().lamports();
        let tx = system_transaction::transfer(&mint_keypair, &blockhash_sysvar, 10, blockhash);
        assert_eq!(
            bank.process_transaction(&tx),
            Err(TransactionError::InstructionError(
                0,
                InstructionError::ReadonlyLamportChange
            ))
        );
        assert_eq!(
            bank.get_account(&sysvar::clock::id()).unwrap().lamports(),
            orig_lamports
        );

        let accounts = vec![
            AccountMeta::new(mint_keypair.pubkey(), true),
            AccountMeta::new(blockhash_sysvar, false),
        ];
        let ix = Instruction::new_with_bincode(program_id, &0, accounts);
        let message = Message::new(&[ix], Some(&mint_keypair.pubkey()));
        let tx = Transaction::new(&[&mint_keypair], message, blockhash);
        assert_eq!(
            bank.process_transaction(&tx),
            Err(TransactionError::InstructionError(
                0,
                InstructionError::ReadonlyDataModified
            ))
        );
    }

    #[test]
    fn test_clean_dropped_unrooted_frozen_banks() {
        solana_logger::setup();
        do_test_clean_dropped_unrooted_banks(FreezeBank1::Yes);
    }

    #[test]
    fn test_clean_dropped_unrooted_unfrozen_banks() {
        solana_logger::setup();
        do_test_clean_dropped_unrooted_banks(FreezeBank1::No);
    }

    /// A simple enum to toggle freezing Bank1 or not.  Used in the clean_dropped_unrooted tests.
    enum FreezeBank1 {
        No,
        Yes,
    }

    fn do_test_clean_dropped_unrooted_banks(freeze_bank1: FreezeBank1) {
        //! Test that dropped unrooted banks are cleaned up properly
        //!
        //! slot 0:       bank0 (rooted)
        //!               /   \
        //! slot 1:      /   bank1 (unrooted and dropped)
        //!             /
        //! slot 2:  bank2 (rooted)
        //!
        //! In the scenario above, when `clean_accounts()` is called on bank2, the keys that exist
        //! _only_ in bank1 should be cleaned up, since those keys are unreachable.
        //!
        //! The following scenarios are tested:
        //!
        //! 1. A key is written _only_ in an unrooted bank (key1)
        //!     - In this case, key1 should be cleaned up
        //! 2. A key is written in both an unrooted _and_ rooted bank (key3)
        //!     - In this case, key3's ref-count should be decremented correctly
        //! 3. A key with zero lamports is _only_ in an unrooted bank (key4)
        //!     - In this case, key4 should be cleaned up
        //! 4. A key with zero lamports is in both an unrooted _and_ rooted bank (key5)
        //!     - In this case, key5's ref-count should be decremented correctly

        let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1.));
        let bank0 = Arc::new(Bank::new_for_tests(&genesis_config));
        let amount = genesis_config.rent.minimum_balance(0);

        let collector = Pubkey::new_unique();
        let owner = Pubkey::new_unique();

        let key1 = Keypair::new(); // only touched in bank1
        let key2 = Keypair::new(); // only touched in bank2
        let key3 = Keypair::new(); // touched in both bank1 and bank2
        let key4 = Keypair::new(); // in only bank1, and has zero lamports
        let key5 = Keypair::new(); // in both bank1 and bank2, and has zero lamports
        bank0
            .transfer(amount, &mint_keypair, &key2.pubkey())
            .unwrap();
        bank0.freeze();

        let slot = 1;
        let bank1 = Bank::new_from_parent(&bank0, &collector, slot);
        add_root_and_flush_write_cache(&bank0);
        bank1
            .transfer(amount, &mint_keypair, &key1.pubkey())
            .unwrap();
        bank1.store_account(&key4.pubkey(), &AccountSharedData::new(0, 0, &owner));
        bank1.store_account(&key5.pubkey(), &AccountSharedData::new(0, 0, &owner));

        if let FreezeBank1::Yes = freeze_bank1 {
            bank1.freeze();
        }

        let slot = slot + 1;
        let bank2 = Bank::new_from_parent(&bank0, &collector, slot);
        bank2
            .transfer(amount * 2, &mint_keypair, &key2.pubkey())
            .unwrap();
        bank2
            .transfer(amount, &mint_keypair, &key3.pubkey())
            .unwrap();
        bank2.store_account(&key5.pubkey(), &AccountSharedData::new(0, 0, &owner));

        bank2.freeze(); // the freeze here is not strictly necessary, but more for illustration
        bank2.squash();
        add_root_and_flush_write_cache(&bank2);

        drop(bank1);
        bank2.clean_accounts_for_tests();

        let expected_ref_count_for_cleaned_up_keys = 0;
        let expected_ref_count_for_keys_in_both_slot1_and_slot2 = 1;

        assert_eq!(
            bank2
                .rc
                .accounts
                .accounts_db
                .accounts_index
                .ref_count_from_storage(&key1.pubkey()),
            expected_ref_count_for_cleaned_up_keys
        );
        assert_ne!(
            bank2
                .rc
                .accounts
                .accounts_db
                .accounts_index
                .ref_count_from_storage(&key3.pubkey()),
            expected_ref_count_for_cleaned_up_keys
        );
        assert_eq!(
            bank2
                .rc
                .accounts
                .accounts_db
                .accounts_index
                .ref_count_from_storage(&key4.pubkey()),
            expected_ref_count_for_cleaned_up_keys
        );
        assert_eq!(
            bank2
                .rc
                .accounts
                .accounts_db
                .accounts_index
                .ref_count_from_storage(&key5.pubkey()),
            expected_ref_count_for_keys_in_both_slot1_and_slot2,
        );

        assert_eq!(
            bank2.rc.accounts.accounts_db.alive_account_count_in_slot(1),
            0
        );
    }

    #[test]
    fn test_rent_debits() {
        let mut rent_debits = RentDebits::default();

        // No entry for 0 rewards
        rent_debits.insert(&Pubkey::new_unique(), 0, 0);
        assert_eq!(rent_debits.0.len(), 0);

        // Some that actually work
        rent_debits.insert(&Pubkey::new_unique(), 1, 0);
        assert_eq!(rent_debits.0.len(), 1);
        rent_debits.insert(&Pubkey::new_unique(), i64::MAX as u64, 0);
        assert_eq!(rent_debits.0.len(), 2);
    }

    #[test]
    fn test_compute_budget_program_noop() {
        solana_logger::setup();
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config_with_leader(
            1_000_000_000_000_000,
            &Pubkey::new_unique(),
            bootstrap_validator_stake_lamports(),
        );
        let mut bank = Bank::new_for_tests(&genesis_config);

        fn mock_ix_processor(
            _first_instruction_account: IndexOfAccount,
            invoke_context: &mut InvokeContext,
        ) -> std::result::Result<(), InstructionError> {
            let compute_budget = invoke_context.get_compute_budget();
            assert_eq!(
                *compute_budget,
                ComputeBudget {
                    compute_unit_limit: 1,
                    heap_size: Some(48 * 1024),
                    ..ComputeBudget::default()
                }
            );
            Ok(())
        }
        let program_id = solana_sdk::pubkey::new_rand();
        bank.add_builtin("mock_program", &program_id, mock_ix_processor);

        let message = Message::new(
            &[
                ComputeBudgetInstruction::set_compute_unit_limit(1),
                ComputeBudgetInstruction::request_heap_frame(48 * 1024),
                Instruction::new_with_bincode(program_id, &0, vec![]),
            ],
            Some(&mint_keypair.pubkey()),
        );
        let tx = Transaction::new(&[&mint_keypair], message, bank.last_blockhash());
        bank.process_transaction(&tx).unwrap();
    }

    #[test]
    fn test_compute_request_instruction() {
        solana_logger::setup();
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config_with_leader(
            1_000_000_000_000_000,
            &Pubkey::new_unique(),
            bootstrap_validator_stake_lamports(),
        );
        let mut bank = Bank::new_for_tests(&genesis_config);

        fn mock_ix_processor(
            _first_instruction_account: IndexOfAccount,
            invoke_context: &mut InvokeContext,
        ) -> std::result::Result<(), InstructionError> {
            let compute_budget = invoke_context.get_compute_budget();
            assert_eq!(
                *compute_budget,
                ComputeBudget {
                    compute_unit_limit: 1,
                    heap_size: Some(48 * 1024),
                    ..ComputeBudget::default()
                }
            );
            Ok(())
        }
        let program_id = solana_sdk::pubkey::new_rand();
        bank.add_builtin("mock_program", &program_id, mock_ix_processor);

        let message = Message::new(
            &[
                ComputeBudgetInstruction::set_compute_unit_limit(1),
                ComputeBudgetInstruction::request_heap_frame(48 * 1024),
                Instruction::new_with_bincode(program_id, &0, vec![]),
            ],
            Some(&mint_keypair.pubkey()),
        );
        let tx = Transaction::new(&[&mint_keypair], message, bank.last_blockhash());
        bank.process_transaction(&tx).unwrap();
    }

    #[test]
    fn test_failed_compute_request_instruction() {
        solana_logger::setup();
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config_with_leader(
            1_000_000_000_000_000,
            &Pubkey::new_unique(),
            bootstrap_validator_stake_lamports(),
        );
        let mut bank = Bank::new_for_tests(&genesis_config);

        let payer0_keypair = Keypair::new();
        let payer1_keypair = Keypair::new();
        bank.transfer(10, &mint_keypair, &payer0_keypair.pubkey())
            .unwrap();
        bank.transfer(10, &mint_keypair, &payer1_keypair.pubkey())
            .unwrap();

        fn mock_ix_processor(
            _first_instruction_account: IndexOfAccount,
            invoke_context: &mut InvokeContext,
        ) -> std::result::Result<(), InstructionError> {
            let compute_budget = invoke_context.get_compute_budget();
            assert_eq!(
                *compute_budget,
                ComputeBudget {
                    compute_unit_limit: 1,
                    heap_size: Some(48 * 1024),
                    ..ComputeBudget::default()
                }
            );
            Ok(())
        }
        let program_id = solana_sdk::pubkey::new_rand();
        bank.add_builtin("mock_program", &program_id, mock_ix_processor);

        // This message will not be executed because the compute budget request is invalid
        let message0 = Message::new(
            &[
                ComputeBudgetInstruction::request_heap_frame(1),
                Instruction::new_with_bincode(program_id, &0, vec![]),
            ],
            Some(&payer0_keypair.pubkey()),
        );
        // This message will be processed successfully
        let message1 = Message::new(
            &[
                ComputeBudgetInstruction::set_compute_unit_limit(1),
                ComputeBudgetInstruction::request_heap_frame(48 * 1024),
                Instruction::new_with_bincode(program_id, &0, vec![]),
            ],
            Some(&payer1_keypair.pubkey()),
        );
        let txs = vec![
            Transaction::new(&[&payer0_keypair], message0, bank.last_blockhash()),
            Transaction::new(&[&payer1_keypair], message1, bank.last_blockhash()),
        ];
        let results = bank.process_transactions(txs.iter());

        assert_eq!(
            results[0],
            Err(TransactionError::InstructionError(
                0,
                InstructionError::InvalidInstructionData
            ))
        );
        assert_eq!(results[1], Ok(()));
        // two transfers and the mock program
        assert_eq!(bank.signature_count(), 3);
    }

    #[test]
    fn test_verify_and_hash_transaction_sig_len() {
        let GenesisConfigInfo {
            mut genesis_config, ..
        } = create_genesis_config_with_leader(42, &solana_sdk::pubkey::new_rand(), 42);

        // activate all features but verify_tx_signatures_len
        activate_all_features(&mut genesis_config);
        genesis_config
            .accounts
            .remove(&feature_set::verify_tx_signatures_len::id());
        let bank = Bank::new_for_tests(&genesis_config);

        let mut rng = rand::thread_rng();
        let recent_blockhash = hash::new_rand(&mut rng);
        let from_keypair = Keypair::new();
        let to_keypair = Keypair::new();
        let from_pubkey = from_keypair.pubkey();
        let to_pubkey = to_keypair.pubkey();

        enum TestCase {
            AddSignature,
            RemoveSignature,
        }

        let make_transaction = |case: TestCase| {
            let message = Message::new(
                &[system_instruction::transfer(&from_pubkey, &to_pubkey, 1)],
                Some(&from_pubkey),
            );
            let mut tx = Transaction::new(&[&from_keypair], message, recent_blockhash);
            assert_eq!(tx.message.header.num_required_signatures, 1);
            match case {
                TestCase::AddSignature => {
                    let signature = to_keypair.sign_message(&tx.message.serialize());
                    tx.signatures.push(signature);
                }
                TestCase::RemoveSignature => {
                    tx.signatures.remove(0);
                }
            }
            tx
        };

        // Too few signatures: Sanitization failure
        {
            let tx = make_transaction(TestCase::RemoveSignature);
            assert_eq!(
                bank.verify_transaction(tx.into(), TransactionVerificationMode::FullVerification)
                    .err(),
                Some(TransactionError::SanitizeFailure),
            );
        }
        // Too many signatures: Sanitization failure
        {
            let tx = make_transaction(TestCase::AddSignature);
            assert_eq!(
                bank.verify_transaction(tx.into(), TransactionVerificationMode::FullVerification)
                    .err(),
                Some(TransactionError::SanitizeFailure),
            );
        }
    }

    #[test]
    fn test_verify_transactions_packet_data_size() {
        let GenesisConfigInfo { genesis_config, .. } =
            create_genesis_config_with_leader(42, &solana_sdk::pubkey::new_rand(), 42);
        let bank = Bank::new_for_tests(&genesis_config);

        let mut rng = rand::thread_rng();
        let recent_blockhash = hash::new_rand(&mut rng);
        let keypair = Keypair::new();
        let pubkey = keypair.pubkey();
        let make_transaction = |size| {
            let ixs: Vec<_> = std::iter::repeat_with(|| {
                system_instruction::transfer(&pubkey, &Pubkey::new_unique(), 1)
            })
            .take(size)
            .collect();
            let message = Message::new(&ixs[..], Some(&pubkey));
            Transaction::new(&[&keypair], message, recent_blockhash)
        };
        // Small transaction.
        {
            let tx = make_transaction(5);
            assert!(bincode::serialized_size(&tx).unwrap() <= PACKET_DATA_SIZE as u64);
            assert!(bank
                .verify_transaction(tx.into(), TransactionVerificationMode::FullVerification)
                .is_ok(),);
        }
        // Big transaction.
        {
            let tx = make_transaction(25);
            assert!(bincode::serialized_size(&tx).unwrap() > PACKET_DATA_SIZE as u64);
            assert_eq!(
                bank.verify_transaction(tx.into(), TransactionVerificationMode::FullVerification)
                    .err(),
                Some(TransactionError::SanitizeFailure),
            );
        }
        // Assert that verify fails as soon as serialized
        // size exceeds packet data size.
        for size in 1..30 {
            let tx = make_transaction(size);
            assert_eq!(
                bincode::serialized_size(&tx).unwrap() <= PACKET_DATA_SIZE as u64,
                bank.verify_transaction(tx.into(), TransactionVerificationMode::FullVerification)
                    .is_ok(),
            );
        }
    }

    #[test]
    fn test_call_precomiled_program() {
        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config_with_leader(42, &Pubkey::new_unique(), 42);
        activate_all_features(&mut genesis_config);
        let bank = Bank::new_for_tests(&genesis_config);

        // libsecp256k1
        let secp_privkey = libsecp256k1::SecretKey::random(&mut rand::thread_rng());
        let message_arr = b"hello";
        let instruction = solana_sdk::secp256k1_instruction::new_secp256k1_instruction(
            &secp_privkey,
            message_arr,
        );
        let tx = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&mint_keypair.pubkey()),
            &[&mint_keypair],
            bank.last_blockhash(),
        );
        // calling the program should be successful when called from the bank
        // even if the program itself is not called
        bank.process_transaction(&tx).unwrap();

        // ed25519
        let privkey = ed25519_dalek::Keypair::generate(&mut rand::thread_rng());
        let message_arr = b"hello";
        let instruction =
            solana_sdk::ed25519_instruction::new_ed25519_instruction(&privkey, message_arr);
        let tx = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&mint_keypair.pubkey()),
            &[&mint_keypair],
            bank.last_blockhash(),
        );
        // calling the program should be successful when called from the bank
        // even if the program itself is not called
        bank.process_transaction(&tx).unwrap();
    }

    #[test]
    fn test_calculate_fee() {
        // Default: no fee.
        let message =
            SanitizedMessage::try_from(Message::new(&[], Some(&Pubkey::new_unique()))).unwrap();
        for cap_transaction_accounts_data_size in &[true, false] {
            assert_eq!(
                Bank::calculate_fee(
                    &message,
                    0,
                    &FeeStructure {
                        lamports_per_signature: 0,
                        ..FeeStructure::default()
                    },
                    true,
                    false,
                    *cap_transaction_accounts_data_size,
                    compute_budget::LoadedAccountsDataLimitType::V0,
                ),
                0
            );
        }

        // One signature, a fee.
        for cap_transaction_accounts_data_size in &[true, false] {
            assert_eq!(
                Bank::calculate_fee(
                    &message,
                    1,
                    &FeeStructure {
                        lamports_per_signature: 1,
                        ..FeeStructure::default()
                    },
                    true,
                    false,
                    *cap_transaction_accounts_data_size,
                    compute_budget::LoadedAccountsDataLimitType::V0,
                ),
                1
            );
        }

        // Two signatures, double the fee.
        let key0 = Pubkey::new_unique();
        let key1 = Pubkey::new_unique();
        let ix0 = system_instruction::transfer(&key0, &key1, 1);
        let ix1 = system_instruction::transfer(&key1, &key0, 1);
        let message = SanitizedMessage::try_from(Message::new(&[ix0, ix1], Some(&key0))).unwrap();
        for cap_transaction_accounts_data_size in &[true, false] {
            assert_eq!(
                Bank::calculate_fee(
                    &message,
                    2,
                    &FeeStructure {
                        lamports_per_signature: 2,
                        ..FeeStructure::default()
                    },
                    true,
                    false,
                    *cap_transaction_accounts_data_size,
                    compute_budget::LoadedAccountsDataLimitType::V0,
                ),
                4
            );
        }
    }

    #[test]
    fn test_calculate_fee_compute_units() {
        let fee_structure = FeeStructure {
            lamports_per_signature: 1,
            ..FeeStructure::default()
        };
        let max_fee = fee_structure.compute_fee_bins.last().unwrap().fee;
        let lamports_per_signature = fee_structure.lamports_per_signature;

        // One signature, no unit request

        let message =
            SanitizedMessage::try_from(Message::new(&[], Some(&Pubkey::new_unique()))).unwrap();
        for cap_transaction_accounts_data_size in &[true, false] {
            assert_eq!(
                Bank::calculate_fee(
                    &message,
                    1,
                    &fee_structure,
                    true,
                    false,
                    *cap_transaction_accounts_data_size,
                    compute_budget::LoadedAccountsDataLimitType::V0
                ),
                max_fee + lamports_per_signature
            );
        }

        // Three signatures, two instructions, no unit request

        let ix0 = system_instruction::transfer(&Pubkey::new_unique(), &Pubkey::new_unique(), 1);
        let ix1 = system_instruction::transfer(&Pubkey::new_unique(), &Pubkey::new_unique(), 1);
        let message =
            SanitizedMessage::try_from(Message::new(&[ix0, ix1], Some(&Pubkey::new_unique())))
                .unwrap();
        for cap_transaction_accounts_data_size in &[true, false] {
            assert_eq!(
                Bank::calculate_fee(
                    &message,
                    1,
                    &fee_structure,
                    true,
                    false,
                    *cap_transaction_accounts_data_size,
                    compute_budget::LoadedAccountsDataLimitType::V0
                ),
                max_fee + 3 * lamports_per_signature
            );
        }

        // Explicit fee schedule

        for requested_compute_units in [
            0,
            5_000,
            10_000,
            100_000,
            300_000,
            500_000,
            700_000,
            900_000,
            1_100_000,
            1_300_000,
            MAX_COMPUTE_UNIT_LIMIT,
        ] {
            const PRIORITIZATION_FEE_RATE: u64 = 42;
            let prioritization_fee_details = PrioritizationFeeDetails::new(
                PrioritizationFeeType::ComputeUnitPrice(PRIORITIZATION_FEE_RATE),
                requested_compute_units as u64,
            );
            let message = SanitizedMessage::try_from(Message::new(
                &[
                    ComputeBudgetInstruction::set_compute_unit_limit(requested_compute_units),
                    ComputeBudgetInstruction::set_compute_unit_price(PRIORITIZATION_FEE_RATE),
                    Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                ],
                Some(&Pubkey::new_unique()),
            ))
            .unwrap();
            for cap_transaction_accounts_data_size in &[true, false] {
                let fee = Bank::calculate_fee(
                    &message,
                    1,
                    &fee_structure,
                    true,
                    false,
                    *cap_transaction_accounts_data_size,
                    compute_budget::LoadedAccountsDataLimitType::V0,
                );
                assert_eq!(
                    fee,
                    lamports_per_signature + prioritization_fee_details.get_fee()
                );
            }
        }
    }

    #[test]
    fn test_calculate_fee_secp256k1() {
        let fee_structure = FeeStructure {
            lamports_per_signature: 1,
            ..FeeStructure::default()
        };
        let key0 = Pubkey::new_unique();
        let key1 = Pubkey::new_unique();
        let ix0 = system_instruction::transfer(&key0, &key1, 1);

        let mut secp_instruction1 = Instruction {
            program_id: secp256k1_program::id(),
            accounts: vec![],
            data: vec![],
        };
        let mut secp_instruction2 = Instruction {
            program_id: secp256k1_program::id(),
            accounts: vec![],
            data: vec![1],
        };

        let message = SanitizedMessage::try_from(Message::new(
            &[
                ix0.clone(),
                secp_instruction1.clone(),
                secp_instruction2.clone(),
            ],
            Some(&key0),
        ))
        .unwrap();
        for cap_transaction_accounts_data_size in &[true, false] {
            assert_eq!(
                Bank::calculate_fee(
                    &message,
                    1,
                    &fee_structure,
                    true,
                    false,
                    *cap_transaction_accounts_data_size,
                    compute_budget::LoadedAccountsDataLimitType::V0
                ),
                2
            );
        }

        secp_instruction1.data = vec![0];
        secp_instruction2.data = vec![10];
        let message = SanitizedMessage::try_from(Message::new(
            &[ix0, secp_instruction1, secp_instruction2],
            Some(&key0),
        ))
        .unwrap();
        for cap_transaction_accounts_data_size in &[true, false] {
            assert_eq!(
                Bank::calculate_fee(
                    &message,
                    1,
                    &fee_structure,
                    true,
                    false,
                    *cap_transaction_accounts_data_size,
                    compute_budget::LoadedAccountsDataLimitType::V0
                ),
                11
            );
        }
    }

    #[test]
    fn test_an_empty_instruction_without_program() {
        let (genesis_config, mint_keypair) = create_genesis_config(1);
        let destination = solana_sdk::pubkey::new_rand();
        let mut ix = system_instruction::transfer(&mint_keypair.pubkey(), &destination, 0);
        ix.program_id = native_loader::id(); // Empty executable account chain
        let message = Message::new(&[ix], Some(&mint_keypair.pubkey()));
        let tx = Transaction::new(&[&mint_keypair], message, genesis_config.hash());

        let bank = Bank::new_for_tests(&genesis_config);
        assert_eq!(
            bank.process_transaction(&tx).unwrap_err(),
            TransactionError::InstructionError(0, InstructionError::UnsupportedProgramId),
        );
    }

    #[test]
    fn test_transaction_log_collector_get_logs_for_address() {
        let address = Pubkey::new_unique();
        let mut mentioned_address_map = HashMap::new();
        mentioned_address_map.insert(address, vec![0]);
        let transaction_log_collector = TransactionLogCollector {
            mentioned_address_map,
            ..TransactionLogCollector::default()
        };
        assert_eq!(
            transaction_log_collector.get_logs_for_address(Some(&address)),
            Some(Vec::<TransactionLogInfo>::new()),
        );
    }

    /// Test processing a good transaction correctly modifies the accounts data size
    #[test]
    fn test_accounts_data_size_with_good_transaction() {
        const ACCOUNT_SIZE: u64 = MAX_PERMITTED_DATA_LENGTH;
        let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1_000.));
        let mut bank = Bank::new_for_tests(&genesis_config);
        bank.activate_feature(&feature_set::cap_accounts_data_len::id());
        let transaction = system_transaction::create_account(
            &mint_keypair,
            &Keypair::new(),
            bank.last_blockhash(),
            genesis_config
                .rent
                .minimum_balance(ACCOUNT_SIZE.try_into().unwrap()),
            ACCOUNT_SIZE,
            &solana_sdk::system_program::id(),
        );

        let accounts_data_size_before = bank.load_accounts_data_size();
        let accounts_data_size_delta_before = bank.load_accounts_data_size_delta();
        let accounts_data_size_delta_on_chain_before =
            bank.load_accounts_data_size_delta_on_chain();
        let result = bank.process_transaction(&transaction);
        let accounts_data_size_after = bank.load_accounts_data_size();
        let accounts_data_size_delta_after = bank.load_accounts_data_size_delta();
        let accounts_data_size_delta_on_chain_after = bank.load_accounts_data_size_delta_on_chain();

        assert!(result.is_ok());
        assert_eq!(
            accounts_data_size_after - accounts_data_size_before,
            ACCOUNT_SIZE,
        );
        assert_eq!(
            accounts_data_size_delta_after - accounts_data_size_delta_before,
            ACCOUNT_SIZE as i64,
        );
        assert_eq!(
            accounts_data_size_delta_on_chain_after - accounts_data_size_delta_on_chain_before,
            ACCOUNT_SIZE as i64,
        );
    }

    /// Test processing a bad transaction correctly modifies the accounts data size
    #[test]
    fn test_accounts_data_size_with_bad_transaction() {
        const ACCOUNT_SIZE: u64 = MAX_PERMITTED_DATA_LENGTH;
        let mut bank = create_simple_test_bank(1_000_000_000_000);
        bank.activate_feature(&feature_set::cap_accounts_data_len::id());
        let transaction = system_transaction::create_account(
            &Keypair::new(),
            &Keypair::new(),
            bank.last_blockhash(),
            LAMPORTS_PER_SOL,
            ACCOUNT_SIZE,
            &solana_sdk::system_program::id(),
        );

        let accounts_data_size_before = bank.load_accounts_data_size();
        let accounts_data_size_delta_before = bank.load_accounts_data_size_delta();
        let accounts_data_size_delta_on_chain_before =
            bank.load_accounts_data_size_delta_on_chain();
        let result = bank.process_transaction(&transaction);
        let accounts_data_size_after = bank.load_accounts_data_size();
        let accounts_data_size_delta_after = bank.load_accounts_data_size_delta();
        let accounts_data_size_delta_on_chain_after = bank.load_accounts_data_size_delta_on_chain();

        assert!(result.is_err());
        assert_eq!(accounts_data_size_after, accounts_data_size_before,);
        assert_eq!(
            accounts_data_size_delta_after,
            accounts_data_size_delta_before,
        );
        assert_eq!(
            accounts_data_size_delta_on_chain_after,
            accounts_data_size_delta_on_chain_before,
        );
    }

    #[derive(Serialize, Deserialize)]
    enum MockTransferInstruction {
        Transfer(u64),
    }

    fn mock_transfer_process_instruction(
        _first_instruction_account: IndexOfAccount,
        invoke_context: &mut InvokeContext,
    ) -> result::Result<(), InstructionError> {
        let transaction_context = &invoke_context.transaction_context;
        let instruction_context = transaction_context.get_current_instruction_context()?;
        let instruction_data = instruction_context.get_instruction_data();
        if let Ok(instruction) = bincode::deserialize(instruction_data) {
            match instruction {
                MockTransferInstruction::Transfer(amount) => {
                    instruction_context
                        .try_borrow_instruction_account(transaction_context, 1)?
                        .checked_sub_lamports(amount)?;
                    instruction_context
                        .try_borrow_instruction_account(transaction_context, 2)?
                        .checked_add_lamports(amount)?;
                    Ok(())
                }
            }
        } else {
            Err(InstructionError::InvalidInstructionData)
        }
    }

    fn create_mock_transfer(
        payer: &Keypair,
        from: &Keypair,
        to: &Keypair,
        amount: u64,
        mock_program_id: Pubkey,
        recent_blockhash: Hash,
    ) -> Transaction {
        let account_metas = vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(from.pubkey(), true),
            AccountMeta::new(to.pubkey(), true),
        ];
        let transfer_instruction = Instruction::new_with_bincode(
            mock_program_id,
            &MockTransferInstruction::Transfer(amount),
            account_metas,
        );
        Transaction::new_signed_with_payer(
            &[transfer_instruction],
            Some(&payer.pubkey()),
            &[payer, from, to],
            recent_blockhash,
        )
    }

    #[test]
    fn test_invalid_rent_state_changes_existing_accounts() {
        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config_with_leader(sol_to_lamports(100.), &Pubkey::new_unique(), 42);
        genesis_config.rent = Rent::default();

        let mock_program_id = Pubkey::new_unique();
        let account_data_size = 100;
        let rent_exempt_minimum = genesis_config.rent.minimum_balance(account_data_size);

        // Create legacy accounts of various kinds
        let rent_paying_account = Keypair::new();
        genesis_config.accounts.insert(
            rent_paying_account.pubkey(),
            Account::new_rent_epoch(
                rent_exempt_minimum - 1,
                account_data_size,
                &mock_program_id,
                INITIAL_RENT_EPOCH + 1,
            ),
        );
        let rent_exempt_account = Keypair::new();
        genesis_config.accounts.insert(
            rent_exempt_account.pubkey(),
            Account::new_rent_epoch(
                rent_exempt_minimum,
                account_data_size,
                &mock_program_id,
                INITIAL_RENT_EPOCH + 1,
            ),
        );

        let mut bank = Bank::new_for_tests(&genesis_config);
        bank.add_builtin(
            "mock_program",
            &mock_program_id,
            mock_transfer_process_instruction,
        );
        let recent_blockhash = bank.last_blockhash();

        let check_account_is_rent_exempt = |pubkey: &Pubkey| -> bool {
            let account = bank.get_account(pubkey).unwrap();
            Rent::default().is_exempt(account.lamports(), account.data().len())
        };

        // RentPaying account can be left as Uninitialized, in other RentPaying states, or RentExempt
        let tx = create_mock_transfer(
            &mint_keypair,        // payer
            &rent_paying_account, // from
            &mint_keypair,        // to
            1,
            mock_program_id,
            recent_blockhash,
        );
        let result = bank.process_transaction(&tx);
        assert!(result.is_ok());
        assert!(!check_account_is_rent_exempt(&rent_paying_account.pubkey()));
        let tx = create_mock_transfer(
            &mint_keypair,        // payer
            &rent_paying_account, // from
            &mint_keypair,        // to
            rent_exempt_minimum - 2,
            mock_program_id,
            recent_blockhash,
        );
        let result = bank.process_transaction(&tx);
        assert!(result.is_ok());
        assert!(bank.get_account(&rent_paying_account.pubkey()).is_none());

        bank.store_account(
            // restore program-owned account
            &rent_paying_account.pubkey(),
            &AccountSharedData::new(rent_exempt_minimum - 1, account_data_size, &mock_program_id),
        );
        let result = bank.transfer(1, &mint_keypair, &rent_paying_account.pubkey());
        assert!(result.is_ok());
        assert!(check_account_is_rent_exempt(&rent_paying_account.pubkey()));

        // RentExempt account can only remain RentExempt or be Uninitialized
        let tx = create_mock_transfer(
            &mint_keypair,        // payer
            &rent_exempt_account, // from
            &mint_keypair,        // to
            1,
            mock_program_id,
            recent_blockhash,
        );
        let result = bank.process_transaction(&tx);
        assert!(result.is_err());
        assert!(check_account_is_rent_exempt(&rent_exempt_account.pubkey()));
        let result = bank.transfer(1, &mint_keypair, &rent_exempt_account.pubkey());
        assert!(result.is_ok());
        assert!(check_account_is_rent_exempt(&rent_exempt_account.pubkey()));
        let tx = create_mock_transfer(
            &mint_keypair,        // payer
            &rent_exempt_account, // from
            &mint_keypair,        // to
            rent_exempt_minimum + 1,
            mock_program_id,
            recent_blockhash,
        );
        let result = bank.process_transaction(&tx);
        assert!(result.is_ok());
        assert!(bank.get_account(&rent_exempt_account.pubkey()).is_none());
    }

    #[test]
    fn test_invalid_rent_state_changes_new_accounts() {
        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config_with_leader(sol_to_lamports(100.), &Pubkey::new_unique(), 42);
        genesis_config.rent = Rent::default();

        let mock_program_id = Pubkey::new_unique();
        let account_data_size = 100;
        let rent_exempt_minimum = genesis_config.rent.minimum_balance(account_data_size);

        let mut bank = Bank::new_for_tests(&genesis_config);
        bank.add_builtin(
            "mock_program",
            &mock_program_id,
            mock_transfer_process_instruction,
        );
        let recent_blockhash = bank.last_blockhash();

        let check_account_is_rent_exempt = |pubkey: &Pubkey| -> bool {
            let account = bank.get_account(pubkey).unwrap();
            Rent::default().is_exempt(account.lamports(), account.data().len())
        };

        // Try to create RentPaying account
        let rent_paying_account = Keypair::new();
        let tx = system_transaction::create_account(
            &mint_keypair,
            &rent_paying_account,
            recent_blockhash,
            rent_exempt_minimum - 1,
            account_data_size as u64,
            &mock_program_id,
        );
        let result = bank.process_transaction(&tx);
        assert!(result.is_err());
        assert!(bank.get_account(&rent_paying_account.pubkey()).is_none());

        // Try to create RentExempt account
        let rent_exempt_account = Keypair::new();
        let tx = system_transaction::create_account(
            &mint_keypair,
            &rent_exempt_account,
            recent_blockhash,
            rent_exempt_minimum,
            account_data_size as u64,
            &mock_program_id,
        );
        let result = bank.process_transaction(&tx);
        assert!(result.is_ok());
        assert!(check_account_is_rent_exempt(&rent_exempt_account.pubkey()));
    }

    #[test]
    fn test_drained_created_account() {
        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config_with_leader(sol_to_lamports(100.), &Pubkey::new_unique(), 42);
        genesis_config.rent = Rent::default();
        activate_all_features(&mut genesis_config);

        let mock_program_id = Pubkey::new_unique();
        // small enough to not pay rent, thus bypassing the data clearing rent
        // mechanism
        let data_size_no_rent = 100;
        // large enough to pay rent, will have data cleared
        let data_size_rent = 10000;
        let lamports_to_transfer = 100;

        // Create legacy accounts of various kinds
        let created_keypair = Keypair::new();

        let mut bank = Bank::new_for_tests(&genesis_config);
        bank.add_builtin(
            "mock_program",
            &mock_program_id,
            mock_transfer_process_instruction,
        );
        let recent_blockhash = bank.last_blockhash();

        // Create and drain a small data size account
        let create_instruction = system_instruction::create_account(
            &mint_keypair.pubkey(),
            &created_keypair.pubkey(),
            lamports_to_transfer,
            data_size_no_rent,
            &mock_program_id,
        );
        let account_metas = vec![
            AccountMeta::new(mint_keypair.pubkey(), true),
            AccountMeta::new(created_keypair.pubkey(), true),
            AccountMeta::new(mint_keypair.pubkey(), false),
        ];
        let transfer_from_instruction = Instruction::new_with_bincode(
            mock_program_id,
            &MockTransferInstruction::Transfer(lamports_to_transfer),
            account_metas,
        );
        let tx = Transaction::new_signed_with_payer(
            &[create_instruction, transfer_from_instruction],
            Some(&mint_keypair.pubkey()),
            &[&mint_keypair, &created_keypair],
            recent_blockhash,
        );

        let result = bank.process_transaction(&tx);
        assert!(result.is_ok());
        // account data is not stored because of zero balance even though its
        // data wasn't cleared
        assert!(bank.get_account(&created_keypair.pubkey()).is_none());

        // Create and drain a large data size account
        let create_instruction = system_instruction::create_account(
            &mint_keypair.pubkey(),
            &created_keypair.pubkey(),
            lamports_to_transfer,
            data_size_rent,
            &mock_program_id,
        );
        let account_metas = vec![
            AccountMeta::new(mint_keypair.pubkey(), true),
            AccountMeta::new(created_keypair.pubkey(), true),
            AccountMeta::new(mint_keypair.pubkey(), false),
        ];
        let transfer_from_instruction = Instruction::new_with_bincode(
            mock_program_id,
            &MockTransferInstruction::Transfer(lamports_to_transfer),
            account_metas,
        );
        let tx = Transaction::new_signed_with_payer(
            &[create_instruction, transfer_from_instruction],
            Some(&mint_keypair.pubkey()),
            &[&mint_keypair, &created_keypair],
            recent_blockhash,
        );

        let result = bank.process_transaction(&tx);
        assert!(result.is_ok());
        // account data is not stored because of zero balance
        assert!(bank.get_account(&created_keypair.pubkey()).is_none());
    }

    #[test]
    fn test_rent_state_changes_sysvars() {
        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config_with_leader(sol_to_lamports(100.), &Pubkey::new_unique(), 42);
        genesis_config.rent = Rent::default();

        let validator_pubkey = solana_sdk::pubkey::new_rand();
        let validator_stake_lamports = sol_to_lamports(1.);
        let validator_staking_keypair = Keypair::new();
        let validator_voting_keypair = Keypair::new();

        let validator_vote_account = vote_state::create_account(
            &validator_voting_keypair.pubkey(),
            &validator_pubkey,
            0,
            validator_stake_lamports,
        );

        let validator_stake_account = stake_state::create_account(
            &validator_staking_keypair.pubkey(),
            &validator_voting_keypair.pubkey(),
            &validator_vote_account,
            &genesis_config.rent,
            validator_stake_lamports,
        );

        genesis_config.accounts.insert(
            validator_pubkey,
            Account::new(
                genesis_config.rent.minimum_balance(0),
                0,
                &system_program::id(),
            ),
        );
        genesis_config.accounts.insert(
            validator_staking_keypair.pubkey(),
            Account::from(validator_stake_account),
        );
        genesis_config.accounts.insert(
            validator_voting_keypair.pubkey(),
            Account::from(validator_vote_account),
        );

        let bank = Bank::new_for_tests(&genesis_config);

        // Ensure transactions with sysvars succeed, even though sysvars appear RentPaying by balance
        let tx = Transaction::new_signed_with_payer(
            &[stake_instruction::deactivate_stake(
                &validator_staking_keypair.pubkey(),
                &validator_staking_keypair.pubkey(),
            )],
            Some(&mint_keypair.pubkey()),
            &[&mint_keypair, &validator_staking_keypair],
            bank.last_blockhash(),
        );
        let result = bank.process_transaction(&tx);
        assert!(result.is_ok());
    }

    #[test]
    fn test_invalid_rent_state_changes_fee_payer() {
        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config_with_leader(sol_to_lamports(100.), &Pubkey::new_unique(), 42);
        genesis_config.rent = Rent::default();
        genesis_config.fee_rate_governor = FeeRateGovernor::new(
            solana_sdk::fee_calculator::DEFAULT_TARGET_LAMPORTS_PER_SIGNATURE,
            solana_sdk::fee_calculator::DEFAULT_TARGET_SIGNATURES_PER_SLOT,
        );
        let rent_exempt_minimum = genesis_config.rent.minimum_balance(0);

        // Create legacy rent-paying System account
        let rent_paying_fee_payer = Keypair::new();
        genesis_config.accounts.insert(
            rent_paying_fee_payer.pubkey(),
            Account::new(rent_exempt_minimum - 1, 0, &system_program::id()),
        );
        // Create RentExempt recipient account
        let recipient = Pubkey::new_unique();
        genesis_config.accounts.insert(
            recipient,
            Account::new(rent_exempt_minimum, 0, &system_program::id()),
        );

        let bank = Bank::new_for_tests(&genesis_config);
        let recent_blockhash = bank.last_blockhash();

        let check_account_is_rent_exempt = |pubkey: &Pubkey| -> bool {
            let account = bank.get_account(pubkey).unwrap();
            Rent::default().is_exempt(account.lamports(), account.data().len())
        };

        // Create just-rent-exempt fee-payer
        let rent_exempt_fee_payer = Keypair::new();
        bank.transfer(
            rent_exempt_minimum,
            &mint_keypair,
            &rent_exempt_fee_payer.pubkey(),
        )
        .unwrap();

        // Dummy message to determine fee amount
        let dummy_message = SanitizedMessage::try_from(Message::new_with_blockhash(
            &[system_instruction::transfer(
                &rent_exempt_fee_payer.pubkey(),
                &recipient,
                sol_to_lamports(1.),
            )],
            Some(&rent_exempt_fee_payer.pubkey()),
            &recent_blockhash,
        ))
        .unwrap();
        let fee = bank.get_fee_for_message(&dummy_message).unwrap();

        // RentPaying fee-payer can remain RentPaying
        let tx = Transaction::new(
            &[&rent_paying_fee_payer, &mint_keypair],
            Message::new(
                &[system_instruction::transfer(
                    &mint_keypair.pubkey(),
                    &recipient,
                    rent_exempt_minimum,
                )],
                Some(&rent_paying_fee_payer.pubkey()),
            ),
            recent_blockhash,
        );
        let result = bank.process_transaction(&tx);
        assert!(result.is_ok());
        assert!(!check_account_is_rent_exempt(
            &rent_paying_fee_payer.pubkey()
        ));

        // RentPaying fee-payer can remain RentPaying on failed executed tx
        let sender = Keypair::new();
        let fee_payer_balance = bank.get_balance(&rent_paying_fee_payer.pubkey());
        let tx = Transaction::new(
            &[&rent_paying_fee_payer, &sender],
            Message::new(
                &[system_instruction::transfer(
                    &sender.pubkey(),
                    &recipient,
                    rent_exempt_minimum,
                )],
                Some(&rent_paying_fee_payer.pubkey()),
            ),
            recent_blockhash,
        );
        let result = bank.process_transaction(&tx);
        assert_eq!(
            result.unwrap_err(),
            TransactionError::InstructionError(0, InstructionError::Custom(1))
        );
        assert_ne!(
            fee_payer_balance,
            bank.get_balance(&rent_paying_fee_payer.pubkey())
        );
        assert!(!check_account_is_rent_exempt(
            &rent_paying_fee_payer.pubkey()
        ));

        // RentPaying fee-payer can be emptied with fee and transaction
        let tx = Transaction::new(
            &[&rent_paying_fee_payer],
            Message::new(
                &[system_instruction::transfer(
                    &rent_paying_fee_payer.pubkey(),
                    &recipient,
                    bank.get_balance(&rent_paying_fee_payer.pubkey()) - fee,
                )],
                Some(&rent_paying_fee_payer.pubkey()),
            ),
            recent_blockhash,
        );
        let result = bank.process_transaction(&tx);
        assert!(result.is_ok());
        assert_eq!(0, bank.get_balance(&rent_paying_fee_payer.pubkey()));

        // RentExempt fee-payer cannot become RentPaying from transaction fee
        let tx = Transaction::new(
            &[&rent_exempt_fee_payer, &mint_keypair],
            Message::new(
                &[system_instruction::transfer(
                    &mint_keypair.pubkey(),
                    &recipient,
                    rent_exempt_minimum,
                )],
                Some(&rent_exempt_fee_payer.pubkey()),
            ),
            recent_blockhash,
        );
        let result = bank.process_transaction(&tx);
        assert_eq!(
            result.unwrap_err(),
            TransactionError::InsufficientFundsForRent { account_index: 0 }
        );
        assert!(check_account_is_rent_exempt(
            &rent_exempt_fee_payer.pubkey()
        ));

        // RentExempt fee-payer cannot become RentPaying via failed executed tx
        let tx = Transaction::new(
            &[&rent_exempt_fee_payer, &sender],
            Message::new(
                &[system_instruction::transfer(
                    &sender.pubkey(),
                    &recipient,
                    rent_exempt_minimum,
                )],
                Some(&rent_exempt_fee_payer.pubkey()),
            ),
            recent_blockhash,
        );
        let result = bank.process_transaction(&tx);
        assert_eq!(
            result.unwrap_err(),
            TransactionError::InsufficientFundsForRent { account_index: 0 }
        );
        assert!(check_account_is_rent_exempt(
            &rent_exempt_fee_payer.pubkey()
        ));

        // For good measure, show that a RentExempt fee-payer that is also debited by a transaction
        // cannot become RentPaying by that debit, but can still be charged for the fee
        bank.transfer(fee, &mint_keypair, &rent_exempt_fee_payer.pubkey())
            .unwrap();
        let fee_payer_balance = bank.get_balance(&rent_exempt_fee_payer.pubkey());
        assert_eq!(fee_payer_balance, rent_exempt_minimum + fee);
        let tx = Transaction::new(
            &[&rent_exempt_fee_payer],
            Message::new(
                &[system_instruction::transfer(
                    &rent_exempt_fee_payer.pubkey(),
                    &recipient,
                    fee,
                )],
                Some(&rent_exempt_fee_payer.pubkey()),
            ),
            recent_blockhash,
        );
        let result = bank.process_transaction(&tx);
        assert_eq!(
            result.unwrap_err(),
            TransactionError::InsufficientFundsForRent { account_index: 0 }
        );
        assert_eq!(
            fee_payer_balance - fee,
            bank.get_balance(&rent_exempt_fee_payer.pubkey())
        );
        assert!(check_account_is_rent_exempt(
            &rent_exempt_fee_payer.pubkey()
        ));

        // Also show that a RentExempt fee-payer can be completely emptied via fee and transaction
        bank.transfer(fee + 1, &mint_keypair, &rent_exempt_fee_payer.pubkey())
            .unwrap();
        assert!(bank.get_balance(&rent_exempt_fee_payer.pubkey()) > rent_exempt_minimum + fee);
        let tx = Transaction::new(
            &[&rent_exempt_fee_payer],
            Message::new(
                &[system_instruction::transfer(
                    &rent_exempt_fee_payer.pubkey(),
                    &recipient,
                    bank.get_balance(&rent_exempt_fee_payer.pubkey()) - fee,
                )],
                Some(&rent_exempt_fee_payer.pubkey()),
            ),
            recent_blockhash,
        );
        let result = bank.process_transaction(&tx);
        assert!(result.is_ok());
        assert_eq!(0, bank.get_balance(&rent_exempt_fee_payer.pubkey()));

        // ... but not if the fee alone would make it RentPaying
        bank.transfer(
            rent_exempt_minimum + 1,
            &mint_keypair,
            &rent_exempt_fee_payer.pubkey(),
        )
        .unwrap();
        assert!(bank.get_balance(&rent_exempt_fee_payer.pubkey()) < rent_exempt_minimum + fee);
        let tx = Transaction::new(
            &[&rent_exempt_fee_payer],
            Message::new(
                &[system_instruction::transfer(
                    &rent_exempt_fee_payer.pubkey(),
                    &recipient,
                    bank.get_balance(&rent_exempt_fee_payer.pubkey()) - fee,
                )],
                Some(&rent_exempt_fee_payer.pubkey()),
            ),
            recent_blockhash,
        );
        let result = bank.process_transaction(&tx);
        assert_eq!(
            result.unwrap_err(),
            TransactionError::InsufficientFundsForRent { account_index: 0 }
        );
        assert!(check_account_is_rent_exempt(
            &rent_exempt_fee_payer.pubkey()
        ));
    }

    // Ensure System transfers of any size can be made to the incinerator
    #[test]
    fn test_rent_state_incinerator() {
        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config_with_leader(sol_to_lamports(100.), &Pubkey::new_unique(), 42);
        genesis_config.rent = Rent::default();
        let rent_exempt_minimum = genesis_config.rent.minimum_balance(0);

        let bank = Bank::new_for_tests(&genesis_config);

        for amount in [rent_exempt_minimum - 1, rent_exempt_minimum] {
            bank.transfer(amount, &mint_keypair, &solana_sdk::incinerator::id())
                .unwrap();
        }
    }

    #[test]
    fn test_rent_state_list_len() {
        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config_with_leader(sol_to_lamports(100.), &Pubkey::new_unique(), 42);
        genesis_config.rent = Rent::default();

        let bank = Bank::new_for_tests(&genesis_config);
        let recipient = Pubkey::new_unique();
        let tx = system_transaction::transfer(
            &mint_keypair,
            &recipient,
            sol_to_lamports(1.),
            bank.last_blockhash(),
        );
        let num_accounts = tx.message().account_keys.len();
        let sanitized_tx = SanitizedTransaction::try_from_legacy_transaction(tx).unwrap();
        let mut error_counters = TransactionErrorMetrics::default();
        let loaded_txs = bank.rc.accounts.load_accounts(
            &bank.ancestors,
            &[sanitized_tx.clone()],
            vec![(Ok(()), None)],
            &bank.blockhash_queue.read().unwrap(),
            &mut error_counters,
            &bank.rent_collector,
            &bank.feature_set,
            &FeeStructure::default(),
            None,
        );

        let compute_budget = bank.runtime_config.compute_budget.unwrap_or_else(|| {
            ComputeBudget::new(compute_budget::DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT as u64)
        });
        let transaction_context = TransactionContext::new(
            loaded_txs[0].0.as_ref().unwrap().accounts.clone(),
            Some(Rent::default()),
            compute_budget.max_invoke_stack_height,
            compute_budget.max_instruction_trace_length,
        );

        assert_eq!(
            bank.get_transaction_account_state_info(&transaction_context, sanitized_tx.message())
                .len(),
            num_accounts,
        );
    }

    #[test]
    fn test_update_accounts_data_size() {
        // Test: Subtraction saturates at 0
        {
            let bank = create_simple_test_bank(100);
            let initial_data_size = bank.load_accounts_data_size() as i64;
            let data_size = 567;
            bank.accounts_data_size_delta_on_chain
                .store(data_size, Release);
            bank.update_accounts_data_size_delta_on_chain(
                (initial_data_size + data_size + 1).saturating_neg(),
            );
            assert_eq!(bank.load_accounts_data_size(), 0);
        }

        // Test: Addition saturates at u64::MAX
        {
            let mut bank = create_simple_test_bank(100);
            let data_size_remaining = 567;
            bank.accounts_data_size_initial = u64::MAX - data_size_remaining;
            bank.accounts_data_size_delta_off_chain
                .store((data_size_remaining + 1) as i64, Release);
            assert_eq!(bank.load_accounts_data_size(), u64::MAX);
        }

        // Test: Updates work as expected
        {
            // Set the accounts data size to be in the middle, then perform a bunch of small
            // updates, checking the results after each one.
            let mut bank = create_simple_test_bank(100);
            bank.accounts_data_size_initial = u32::MAX as u64;
            let mut rng = rand::thread_rng();
            for _ in 0..100 {
                let initial = bank.load_accounts_data_size() as i64;
                let delta1 = rng.gen_range(-500, 500);
                bank.update_accounts_data_size_delta_on_chain(delta1);
                let delta2 = rng.gen_range(-500, 500);
                bank.update_accounts_data_size_delta_off_chain(delta2);
                assert_eq!(
                    bank.load_accounts_data_size() as i64,
                    initial.saturating_add(delta1).saturating_add(delta2),
                );
            }
        }
    }

    #[test]
    fn test_skip_rewrite() {
        solana_logger::setup();
        let mut account = AccountSharedData::default();
        let bank_slot = 10;
        for account_rent_epoch in 0..3 {
            account.set_rent_epoch(account_rent_epoch);
            for rent_amount in [0, 1] {
                for loaded_slot in (bank_slot - 1)..=bank_slot {
                    for old_rent_epoch in account_rent_epoch.saturating_sub(1)..=account_rent_epoch
                    {
                        let skip = Bank::skip_rewrite(rent_amount, &account);
                        let mut should_skip = true;
                        if rent_amount != 0 || account_rent_epoch == 0 {
                            should_skip = false;
                        }
                        assert_eq!(
                            skip,
                            should_skip,
                            "{:?}",
                            (
                                account_rent_epoch,
                                old_rent_epoch,
                                rent_amount,
                                loaded_slot,
                                old_rent_epoch
                            )
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn test_inner_instructions_list_from_instruction_trace() {
        let instruction_trace = [1, 2, 1, 1, 2, 3, 2];
        let mut transaction_context =
            TransactionContext::new(vec![], None, 3, instruction_trace.len());
        for (index_in_trace, stack_height) in instruction_trace.into_iter().enumerate() {
            while stack_height <= transaction_context.get_instruction_context_stack_height() {
                transaction_context.pop().unwrap();
            }
            if stack_height > transaction_context.get_instruction_context_stack_height() {
                transaction_context
                    .get_next_instruction_context()
                    .unwrap()
                    .configure(&[], &[], &[index_in_trace as u8]);
                transaction_context.push().unwrap();
            }
        }
        let inner_instructions =
            inner_instructions_list_from_instruction_trace(&transaction_context);

        assert_eq!(
            inner_instructions,
            vec![
                vec![InnerInstruction {
                    instruction: CompiledInstruction::new_from_raw_parts(0, vec![1], vec![]),
                    stack_height: 2,
                }],
                vec![],
                vec![
                    InnerInstruction {
                        instruction: CompiledInstruction::new_from_raw_parts(0, vec![4], vec![]),
                        stack_height: 2,
                    },
                    InnerInstruction {
                        instruction: CompiledInstruction::new_from_raw_parts(0, vec![5], vec![]),
                        stack_height: 3,
                    },
                    InnerInstruction {
                        instruction: CompiledInstruction::new_from_raw_parts(0, vec![6], vec![]),
                        stack_height: 2,
                    },
                ]
            ]
        );
    }

    #[derive(Serialize, Deserialize)]
    enum MockReallocInstruction {
        Realloc(usize, u64, Pubkey),
    }

    fn mock_realloc_process_instruction(
        _first_instruction_account: IndexOfAccount,
        invoke_context: &mut InvokeContext,
    ) -> result::Result<(), InstructionError> {
        let transaction_context = &invoke_context.transaction_context;
        let instruction_context = transaction_context.get_current_instruction_context()?;
        let instruction_data = instruction_context.get_instruction_data();
        if let Ok(instruction) = bincode::deserialize(instruction_data) {
            match instruction {
                MockReallocInstruction::Realloc(new_size, new_balance, _) => {
                    // Set data length
                    instruction_context
                        .try_borrow_instruction_account(transaction_context, 1)?
                        .set_data_length(new_size)?;

                    // set balance
                    let current_balance = instruction_context
                        .try_borrow_instruction_account(transaction_context, 1)?
                        .get_lamports();
                    let diff_balance = (new_balance as i64).saturating_sub(current_balance as i64);
                    let amount = diff_balance.unsigned_abs();
                    if diff_balance.is_positive() {
                        instruction_context
                            .try_borrow_instruction_account(transaction_context, 0)?
                            .checked_sub_lamports(amount)?;
                        instruction_context
                            .try_borrow_instruction_account(transaction_context, 1)?
                            .set_lamports(new_balance)?;
                    } else {
                        instruction_context
                            .try_borrow_instruction_account(transaction_context, 0)?
                            .checked_add_lamports(amount)?;
                        instruction_context
                            .try_borrow_instruction_account(transaction_context, 1)?
                            .set_lamports(new_balance)?;
                    }
                    Ok(())
                }
            }
        } else {
            Err(InstructionError::InvalidInstructionData)
        }
    }

    fn create_mock_realloc_tx(
        payer: &Keypair,
        funder: &Keypair,
        reallocd: &Pubkey,
        new_size: usize,
        new_balance: u64,
        mock_program_id: Pubkey,
        recent_blockhash: Hash,
    ) -> Transaction {
        let account_metas = vec![
            AccountMeta::new(funder.pubkey(), false),
            AccountMeta::new(*reallocd, false),
        ];
        let instruction = Instruction::new_with_bincode(
            mock_program_id,
            &MockReallocInstruction::Realloc(new_size, new_balance, Pubkey::new_unique()),
            account_metas,
        );
        Transaction::new_signed_with_payer(
            &[
                instruction,
                ComputeBudgetInstruction::set_accounts_data_size_limit(u32::MAX),
            ],
            Some(&payer.pubkey()),
            &[payer],
            recent_blockhash,
        )
    }

    #[test]
    fn test_resize_and_rent() {
        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config_with_leader(1_000_000_000, &Pubkey::new_unique(), 42);
        genesis_config.rent = Rent::default();
        activate_all_features(&mut genesis_config);

        let mut bank = Bank::new_for_tests(&genesis_config);

        let mock_program_id = Pubkey::new_unique();
        bank.add_builtin(
            "mock_realloc_program",
            &mock_program_id,
            mock_realloc_process_instruction,
        );
        let recent_blockhash = bank.last_blockhash();

        let account_data_size_small = 1024;
        let rent_exempt_minimum_small =
            genesis_config.rent.minimum_balance(account_data_size_small);
        let account_data_size_large = 2048;
        let rent_exempt_minimum_large =
            genesis_config.rent.minimum_balance(account_data_size_large);

        let funding_keypair = Keypair::new();
        bank.store_account(
            &funding_keypair.pubkey(),
            &AccountSharedData::new(1_000_000_000, 0, &mock_program_id),
        );

        let rent_paying_pubkey = solana_sdk::pubkey::new_rand();
        let mut rent_paying_account = AccountSharedData::new(
            rent_exempt_minimum_small - 1,
            account_data_size_small,
            &mock_program_id,
        );
        rent_paying_account.set_rent_epoch(1);

        // restore program-owned account
        bank.store_account(&rent_paying_pubkey, &rent_paying_account);

        // rent paying, realloc larger, fail because not rent exempt
        let tx = create_mock_realloc_tx(
            &mint_keypair,
            &funding_keypair,
            &rent_paying_pubkey,
            account_data_size_large,
            rent_exempt_minimum_small - 1,
            mock_program_id,
            recent_blockhash,
        );
        let expected_err = {
            let account_index = tx
                .message
                .account_keys
                .iter()
                .position(|key| key == &rent_paying_pubkey)
                .unwrap() as u8;
            TransactionError::InsufficientFundsForRent { account_index }
        };
        assert_eq!(bank.process_transaction(&tx).unwrap_err(), expected_err);
        assert_eq!(
            rent_exempt_minimum_small - 1,
            bank.get_account(&rent_paying_pubkey).unwrap().lamports()
        );

        // rent paying, realloc larger and rent exempt
        let tx = create_mock_realloc_tx(
            &mint_keypair,
            &funding_keypair,
            &rent_paying_pubkey,
            account_data_size_large,
            rent_exempt_minimum_large,
            mock_program_id,
            recent_blockhash,
        );
        let result = bank.process_transaction(&tx);
        assert!(result.is_ok());
        assert_eq!(
            rent_exempt_minimum_large,
            bank.get_account(&rent_paying_pubkey).unwrap().lamports()
        );

        // rent exempt, realloc small, fail because not rent exempt
        let tx = create_mock_realloc_tx(
            &mint_keypair,
            &funding_keypair,
            &rent_paying_pubkey,
            account_data_size_small,
            rent_exempt_minimum_small - 1,
            mock_program_id,
            recent_blockhash,
        );
        let expected_err = {
            let account_index = tx
                .message
                .account_keys
                .iter()
                .position(|key| key == &rent_paying_pubkey)
                .unwrap() as u8;
            TransactionError::InsufficientFundsForRent { account_index }
        };
        assert_eq!(bank.process_transaction(&tx).unwrap_err(), expected_err);
        assert_eq!(
            rent_exempt_minimum_large,
            bank.get_account(&rent_paying_pubkey).unwrap().lamports()
        );

        // rent exempt, realloc smaller and rent exempt
        let tx = create_mock_realloc_tx(
            &mint_keypair,
            &funding_keypair,
            &rent_paying_pubkey,
            account_data_size_small,
            rent_exempt_minimum_small,
            mock_program_id,
            recent_blockhash,
        );
        let result = bank.process_transaction(&tx);
        assert!(result.is_ok());
        assert_eq!(
            rent_exempt_minimum_small,
            bank.get_account(&rent_paying_pubkey).unwrap().lamports()
        );

        // rent exempt, realloc large, fail because not rent exempt
        let tx = create_mock_realloc_tx(
            &mint_keypair,
            &funding_keypair,
            &rent_paying_pubkey,
            account_data_size_large,
            rent_exempt_minimum_large - 1,
            mock_program_id,
            recent_blockhash,
        );
        let expected_err = {
            let account_index = tx
                .message
                .account_keys
                .iter()
                .position(|key| key == &rent_paying_pubkey)
                .unwrap() as u8;
            TransactionError::InsufficientFundsForRent { account_index }
        };
        assert_eq!(bank.process_transaction(&tx).unwrap_err(), expected_err);
        assert_eq!(
            rent_exempt_minimum_small,
            bank.get_account(&rent_paying_pubkey).unwrap().lamports()
        );

        // rent exempt, realloc large and rent exempt
        let tx = create_mock_realloc_tx(
            &mint_keypair,
            &funding_keypair,
            &rent_paying_pubkey,
            account_data_size_large,
            rent_exempt_minimum_large,
            mock_program_id,
            recent_blockhash,
        );
        let result = bank.process_transaction(&tx);
        assert!(result.is_ok());
        assert_eq!(
            rent_exempt_minimum_large,
            bank.get_account(&rent_paying_pubkey).unwrap().lamports()
        );

        let created_keypair = Keypair::new();

        // create account, not rent exempt
        let tx = system_transaction::create_account(
            &mint_keypair,
            &created_keypair,
            recent_blockhash,
            rent_exempt_minimum_small - 1,
            account_data_size_small as u64,
            &system_program::id(),
        );
        let expected_err = {
            let account_index = tx
                .message
                .account_keys
                .iter()
                .position(|key| key == &created_keypair.pubkey())
                .unwrap() as u8;
            TransactionError::InsufficientFundsForRent { account_index }
        };
        assert_eq!(bank.process_transaction(&tx).unwrap_err(), expected_err);

        // create account, rent exempt
        let tx = system_transaction::create_account(
            &mint_keypair,
            &created_keypair,
            recent_blockhash,
            rent_exempt_minimum_small,
            account_data_size_small as u64,
            &system_program::id(),
        );
        let result = bank.process_transaction(&tx);
        assert!(result.is_ok());
        assert_eq!(
            rent_exempt_minimum_small,
            bank.get_account(&created_keypair.pubkey())
                .unwrap()
                .lamports()
        );

        let created_keypair = Keypair::new();
        // create account, no data
        let tx = system_transaction::create_account(
            &mint_keypair,
            &created_keypair,
            recent_blockhash,
            rent_exempt_minimum_small - 1,
            0,
            &system_program::id(),
        );
        let result = bank.process_transaction(&tx);
        assert!(result.is_ok());
        assert_eq!(
            rent_exempt_minimum_small - 1,
            bank.get_account(&created_keypair.pubkey())
                .unwrap()
                .lamports()
        );

        // alloc but not rent exempt
        let tx = system_transaction::allocate(
            &mint_keypair,
            &created_keypair,
            recent_blockhash,
            (account_data_size_small + 1) as u64,
        );
        let expected_err = {
            let account_index = tx
                .message
                .account_keys
                .iter()
                .position(|key| key == &created_keypair.pubkey())
                .unwrap() as u8;
            TransactionError::InsufficientFundsForRent { account_index }
        };
        assert_eq!(bank.process_transaction(&tx).unwrap_err(), expected_err);

        // bring balance of account up to rent exemption
        let tx = system_transaction::transfer(
            &mint_keypair,
            &created_keypair.pubkey(),
            1,
            recent_blockhash,
        );
        let result = bank.process_transaction(&tx);
        assert!(result.is_ok());
        assert_eq!(
            rent_exempt_minimum_small,
            bank.get_account(&created_keypair.pubkey())
                .unwrap()
                .lamports()
        );

        // allocate as rent exempt
        let tx = system_transaction::allocate(
            &mint_keypair,
            &created_keypair,
            recent_blockhash,
            account_data_size_small as u64,
        );
        let result = bank.process_transaction(&tx);
        assert!(result.is_ok());
        assert_eq!(
            rent_exempt_minimum_small,
            bank.get_account(&created_keypair.pubkey())
                .unwrap()
                .lamports()
        );
    }

    /// Ensure that accounts data size is updated correctly on resize transactions
    #[test]
    fn test_accounts_data_size_and_resize_transactions() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = genesis_utils::create_genesis_config(100 * LAMPORTS_PER_SOL);
        let mut bank = Bank::new_for_tests(&genesis_config);
        let mock_program_id = Pubkey::new_unique();
        bank.add_builtin(
            "mock_realloc_program",
            &mock_program_id,
            mock_realloc_process_instruction,
        );

        let recent_blockhash = bank.last_blockhash();

        let funding_keypair = Keypair::new();
        bank.store_account(
            &funding_keypair.pubkey(),
            &AccountSharedData::new(10 * LAMPORTS_PER_SOL, 0, &mock_program_id),
        );

        let mut rng = rand::thread_rng();

        // Test case: Grow account
        {
            let account_pubkey = Pubkey::new_unique();
            let account_balance = LAMPORTS_PER_SOL;
            let account_size = rng.gen_range(
                1,
                MAX_PERMITTED_DATA_LENGTH as usize - MAX_PERMITTED_DATA_INCREASE,
            );
            let account_data =
                AccountSharedData::new(account_balance, account_size, &mock_program_id);
            bank.store_account(&account_pubkey, &account_data);

            let accounts_data_size_before = bank.load_accounts_data_size();
            let account_grow_size = rng.gen_range(1, MAX_PERMITTED_DATA_INCREASE);
            let transaction = create_mock_realloc_tx(
                &mint_keypair,
                &funding_keypair,
                &account_pubkey,
                account_size + account_grow_size,
                account_balance,
                mock_program_id,
                recent_blockhash,
            );
            let result = bank.process_transaction(&transaction);
            assert!(result.is_ok());
            let accounts_data_size_after = bank.load_accounts_data_size();
            assert_eq!(
                accounts_data_size_after,
                accounts_data_size_before.saturating_add(account_grow_size as u64),
            );
        }

        // Test case: Shrink account
        {
            let account_pubkey = Pubkey::new_unique();
            let account_balance = LAMPORTS_PER_SOL;
            let account_size =
                rng.gen_range(MAX_PERMITTED_DATA_LENGTH / 2, MAX_PERMITTED_DATA_LENGTH) as usize;
            let account_data =
                AccountSharedData::new(account_balance, account_size, &mock_program_id);
            bank.store_account(&account_pubkey, &account_data);

            let accounts_data_size_before = bank.load_accounts_data_size();
            let account_shrink_size = rng.gen_range(1, account_size);
            let transaction = create_mock_realloc_tx(
                &mint_keypair,
                &funding_keypair,
                &account_pubkey,
                account_size - account_shrink_size,
                account_balance,
                mock_program_id,
                recent_blockhash,
            );
            let result = bank.process_transaction(&transaction);
            assert!(result.is_ok());
            let accounts_data_size_after = bank.load_accounts_data_size();
            assert_eq!(
                accounts_data_size_after,
                accounts_data_size_before.saturating_sub(account_shrink_size as u64),
            );
        }
    }

    #[test]
    fn test_get_partition_end_indexes() {
        for n in 5..7 {
            assert_eq!(vec![0], Bank::get_partition_end_indexes(&(0, 0, n)));
            assert!(Bank::get_partition_end_indexes(&(1, 1, n)).is_empty());
            assert_eq!(vec![1], Bank::get_partition_end_indexes(&(0, 1, n)));
            assert_eq!(vec![1, 2], Bank::get_partition_end_indexes(&(0, 2, n)));
            assert_eq!(vec![3, 4], Bank::get_partition_end_indexes(&(2, 4, n)));
        }
    }

    #[test]
    fn test_get_rent_paying_pubkeys() {
        let lamports = 1;
        let bank = create_simple_test_bank(lamports);

        let n = 432_000;
        assert!(bank.get_rent_paying_pubkeys(&(0, 1, n)).is_none());
        assert!(bank.get_rent_paying_pubkeys(&(0, 2, n)).is_none());
        assert!(bank.get_rent_paying_pubkeys(&(0, 0, n)).is_none());

        let pk1 = Pubkey::new(&[2; 32]);
        let pk2 = Pubkey::new(&[3; 32]);
        let index1 = Bank::partition_from_pubkey(&pk1, n);
        let index2 = Bank::partition_from_pubkey(&pk2, n);
        assert!(index1 > 0, "{}", index1);
        assert!(index2 > index1, "{index2}, {index1}");

        let epoch_schedule = EpochSchedule::custom(n, 0, false);

        let mut rent_paying_accounts_by_partition =
            RentPayingAccountsByPartition::new(&epoch_schedule);
        rent_paying_accounts_by_partition.add_account(&pk1);
        rent_paying_accounts_by_partition.add_account(&pk2);

        bank.rc
            .accounts
            .accounts_db
            .accounts_index
            .rent_paying_accounts_by_partition
            .set(rent_paying_accounts_by_partition)
            .unwrap();

        assert_eq!(
            bank.get_rent_paying_pubkeys(&(0, 1, n)),
            Some(HashSet::default())
        );
        assert_eq!(
            bank.get_rent_paying_pubkeys(&(0, 2, n)),
            Some(HashSet::default())
        );
        assert_eq!(
            bank.get_rent_paying_pubkeys(&(index1.saturating_sub(1), index1, n)),
            Some(HashSet::from([pk1]))
        );
        assert_eq!(
            bank.get_rent_paying_pubkeys(&(index2.saturating_sub(1), index2, n)),
            Some(HashSet::from([pk2]))
        );
        assert_eq!(
            bank.get_rent_paying_pubkeys(&(index1.saturating_sub(1), index2, n)),
            Some(HashSet::from([pk2, pk1]))
        );
        assert_eq!(
            bank.get_rent_paying_pubkeys(&(0, 0, n)),
            Some(HashSet::default())
        );
    }

    /// Ensure that accounts data size is updated correctly by rent collection
    #[test]
    fn test_accounts_data_size_and_rent_collection() {
        for set_exempt_rent_epoch_max in [false, true] {
            let GenesisConfigInfo {
                mut genesis_config, ..
            } = genesis_utils::create_genesis_config(100 * LAMPORTS_PER_SOL);
            genesis_config.rent = Rent::default();
            activate_all_features(&mut genesis_config);
            let bank = Arc::new(Bank::new_for_tests(&genesis_config));
            let bank = Arc::new(Bank::new_from_parent(
                &bank,
                &Pubkey::default(),
                bank.slot() + bank.slot_count_per_normal_epoch(),
            ));

            // make another bank so that any reclaimed accounts from the previous bank do not impact
            // this test
            let bank = Arc::new(Bank::new_from_parent(
                &bank,
                &Pubkey::default(),
                bank.slot() + bank.slot_count_per_normal_epoch(),
            ));

            // Store an account into the bank that is rent-paying and has data
            let data_size = 123;
            let mut account = AccountSharedData::new(1, data_size, &Pubkey::default());
            let keypair = Keypair::new();
            bank.store_account(&keypair.pubkey(), &account);

            // Ensure if we collect rent from the account that it will be reclaimed
            {
                let info = bank.rent_collector.collect_from_existing_account(
                    &keypair.pubkey(),
                    &mut account,
                    None,
                    set_exempt_rent_epoch_max,
                );
                assert_eq!(info.account_data_len_reclaimed, data_size as u64);
            }

            // Collect rent for real
            let accounts_data_size_delta_before_collecting_rent =
                bank.load_accounts_data_size_delta();
            bank.collect_rent_eagerly();
            let accounts_data_size_delta_after_collecting_rent =
                bank.load_accounts_data_size_delta();

            let accounts_data_size_delta_delta = accounts_data_size_delta_after_collecting_rent
                - accounts_data_size_delta_before_collecting_rent;
            assert!(accounts_data_size_delta_delta < 0);
            let reclaimed_data_size = accounts_data_size_delta_delta.saturating_neg() as usize;

            // Ensure the account is reclaimed by rent collection
            assert_eq!(reclaimed_data_size, data_size,);
        }
    }

    #[test]
    fn test_accounts_data_size_with_default_bank() {
        let bank = Bank::default_for_tests();
        assert_eq!(
            bank.load_accounts_data_size() as usize,
            bank.get_total_accounts_stats().unwrap().data_len
        );
    }

    #[test]
    fn test_accounts_data_size_from_genesis() {
        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair,
            ..
        } = genesis_utils::create_genesis_config_with_leader(
            1_000_000 * LAMPORTS_PER_SOL,
            &Pubkey::new_unique(),
            100 * LAMPORTS_PER_SOL,
        );
        genesis_config.rent = Rent::default();
        genesis_config.ticks_per_slot = 3;

        let mut bank = Arc::new(Bank::new_for_tests(&genesis_config));
        assert_eq!(
            bank.load_accounts_data_size() as usize,
            bank.get_total_accounts_stats().unwrap().data_len
        );

        // Create accounts over a number of banks and ensure the accounts data size remains correct
        for _ in 0..10 {
            bank = Arc::new(Bank::new_from_parent(
                &bank,
                &Pubkey::default(),
                bank.slot() + 1,
            ));

            // Store an account into the bank that is rent-exempt and has data
            let data_size = rand::thread_rng().gen_range(3333, 4444);
            let transaction = system_transaction::create_account(
                &mint_keypair,
                &Keypair::new(),
                bank.last_blockhash(),
                genesis_config.rent.minimum_balance(data_size),
                data_size as u64,
                &solana_sdk::system_program::id(),
            );
            bank.process_transaction(&transaction).unwrap();
            bank.fill_bank_with_ticks_for_tests();

            assert_eq!(
                bank.load_accounts_data_size() as usize,
                bank.get_total_accounts_stats().unwrap().data_len,
            );
        }
    }

    /// Ensures that if a transaction exceeds the maximum allowed accounts data allocation size:
    /// 1. The transaction fails
    /// 2. The bank's accounts_data_size is unmodified
    #[test]
    fn test_cap_accounts_data_allocations_per_transaction() {
        const NUM_MAX_SIZE_ALLOCATIONS_PER_TRANSACTION: usize =
            MAX_PERMITTED_ACCOUNTS_DATA_ALLOCATIONS_PER_TRANSACTION as usize
                / MAX_PERMITTED_DATA_LENGTH as usize;

        let (genesis_config, mint_keypair) = create_genesis_config(1_000_000 * LAMPORTS_PER_SOL);
        let mut bank = Bank::new_for_tests(&genesis_config);
        bank.activate_feature(
            &feature_set::enable_early_verification_of_account_modifications::id(),
        );
        bank.activate_feature(&feature_set::cap_accounts_data_allocations_per_transaction::id());

        let mut instructions = Vec::new();
        let mut keypairs = vec![mint_keypair.insecure_clone()];
        for _ in 0..=NUM_MAX_SIZE_ALLOCATIONS_PER_TRANSACTION {
            let keypair = Keypair::new();
            let instruction = system_instruction::create_account(
                &mint_keypair.pubkey(),
                &keypair.pubkey(),
                bank.rent_collector()
                    .rent
                    .minimum_balance(MAX_PERMITTED_DATA_LENGTH as usize),
                MAX_PERMITTED_DATA_LENGTH,
                &solana_sdk::system_program::id(),
            );
            keypairs.push(keypair);
            instructions.push(instruction);
        }
        let message = Message::new(&instructions, Some(&mint_keypair.pubkey()));
        let signers: Vec<_> = keypairs.iter().collect();
        let transaction = Transaction::new(&signers, message, bank.last_blockhash());

        let accounts_data_size_before = bank.load_accounts_data_size();
        let result = bank.process_transaction(&transaction);
        let accounts_data_size_after = bank.load_accounts_data_size();

        assert_eq!(accounts_data_size_before, accounts_data_size_after);
        assert_eq!(
            result,
            Err(TransactionError::InstructionError(
                NUM_MAX_SIZE_ALLOCATIONS_PER_TRANSACTION as u8,
                solana_sdk::instruction::InstructionError::MaxAccountsDataAllocationsExceeded,
            )),
        );
    }
}

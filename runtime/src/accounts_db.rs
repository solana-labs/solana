//! Persistent accounts are stored in below path location:
//!  <path>/<pid>/data/
//!
//! The persistent store would allow for this mode of operation:
//!  - Concurrent single thread append with many concurrent readers.
//!
//! The underlying memory is memory mapped to a file. The accounts would be
//! stored across multiple files and the mappings of file and offset of a
//! particular account would be stored in a shared index. This will allow for
//! concurrent commits without blocking reads, which will sequentially write
//! to memory, ssd or disk, and should be as fast as the hardware allow for.
//! The only required in memory data structure with a write lock is the index,
//! which should be fast to update.
//!
//! AppendVec's only store accounts for single slots.  To bootstrap the
//! index from a persistent store of AppendVec's, the entries include
//! a "write_version".  A single global atomic `AccountsDb::write_version`
//! tracks the number of commits to the entire data store. So the latest
//! commit for each slot entry would be indexed.

use {
    crate::{
        account_info::{AccountInfo, Offset, StorageLocation, StoredSize},
        account_storage::{AccountStorage, AccountStorageStatus, ShrinkInProgress},
        accounts_background_service::{DroppedSlotsSender, SendDroppedBankCallback},
        accounts_cache::{AccountsCache, CachedAccount, SlotCache},
        accounts_hash::{
            AccountsHash, AccountsHasher, CalcAccountsHashConfig, CalculateHashIntermediate,
            HashStats,
        },
        accounts_index::{
            AccountIndexGetResult, AccountSecondaryIndexes, AccountsIndex, AccountsIndexConfig,
            AccountsIndexRootsStats, AccountsIndexScanResult, IndexKey, IndexValue, IsCached,
            RefCount, ScanConfig, ScanResult, SlotList, UpsertReclaim, ZeroLamport,
            ACCOUNTS_INDEX_CONFIG_FOR_BENCHMARKS, ACCOUNTS_INDEX_CONFIG_FOR_TESTING,
        },
        accounts_index_storage::Startup,
        accounts_update_notifier_interface::AccountsUpdateNotifier,
        active_stats::{ActiveStatItem, ActiveStats},
        ancestors::Ancestors,
        ancient_append_vecs::{
            get_ancient_append_vec_capacity, is_ancient, AccountsToStore, StorageSelector,
        },
        append_vec::{
            aligned_stored_size, AppendVec, StorableAccountsWithHashesAndWriteVersions,
            StoredAccountMeta, StoredMetaWriteVersion, APPEND_VEC_MMAPPED_FILES_OPEN,
            STORE_META_OVERHEAD,
        },
        cache_hash_data::{CacheHashData, CacheHashDataFile},
        contains::Contains,
        epoch_accounts_hash::EpochAccountsHashManager,
        pubkey_bins::PubkeyBinCalculator24,
        read_only_accounts_cache::ReadOnlyAccountsCache,
        rent_collector::RentCollector,
        rent_paying_accounts_by_partition::RentPayingAccountsByPartition,
        sorted_storages::SortedStorages,
        storable_accounts::StorableAccounts,
        verify_accounts_hash_in_background::VerifyAccountsHashInBackground,
    },
    blake3::traits::digest::Digest,
    crossbeam_channel::{unbounded, Receiver, Sender},
    dashmap::{
        mapref::entry::Entry::{Occupied, Vacant},
        DashMap, DashSet,
    },
    log::*,
    rand::{thread_rng, Rng},
    rayon::{prelude::*, ThreadPool},
    serde::{Deserialize, Serialize},
    solana_measure::{measure, measure::Measure},
    solana_rayon_threadlimit::get_thread_count,
    solana_sdk::{
        account::{Account, AccountSharedData, ReadableAccount, WritableAccount},
        clock::{BankId, Epoch, Slot, SlotCount},
        epoch_schedule::EpochSchedule,
        genesis_config::{ClusterType, GenesisConfig},
        hash::Hash,
        pubkey::Pubkey,
        rent::Rent,
        signature::Signature,
        timing::AtomicInterval,
    },
    std::{
        borrow::{Borrow, Cow},
        boxed::Box,
        collections::{hash_map::Entry, BTreeSet, HashMap, HashSet},
        convert::TryFrom,
        hash::{Hash as StdHash, Hasher as StdHasher},
        io::{Error as IoError, Result as IoResult},
        ops::{Range, RangeBounds},
        path::{Path, PathBuf},
        str::FromStr,
        sync::{
            atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering},
            Arc, Condvar, Mutex, RwLock,
        },
        thread::{sleep, Builder},
        time::{Duration, Instant},
    },
    tempfile::TempDir,
};

const PAGE_SIZE: u64 = 4 * 1024;
const MAX_RECYCLE_STORES: usize = 1000;
// when the accounts write cache exceeds this many bytes, we will flush it
// this can be specified on the command line, too (--accounts-db-cache-limit-mb)
const WRITE_CACHE_LIMIT_BYTES_DEFAULT: u64 = 15_000_000_000;
const SCAN_SLOT_PAR_ITER_THRESHOLD: usize = 4000;

const UNREF_ACCOUNTS_BATCH_SIZE: usize = 10_000;

pub const DEFAULT_FILE_SIZE: u64 = PAGE_SIZE * 1024;
pub const DEFAULT_NUM_THREADS: u32 = 8;
pub const DEFAULT_NUM_DIRS: u32 = 4;

// When calculating hashes, it is helpful to break the pubkeys found into bins based on the pubkey value.
// More bins means smaller vectors to sort, copy, etc.
pub const PUBKEY_BINS_FOR_CALCULATING_HASHES: usize = 65536;

// Without chunks, we end up with 1 output vec for each outer snapshot storage.
// This results in too many vectors to be efficient.
// Chunks when scanning storages to calculate hashes.
// If this is too big, we don't get enough parallelism of scanning storages.
// If this is too small, then we produce too many output vectors to iterate.
// Metrics indicate a sweet spot in the 2.5k-5k range for mnb.
const MAX_ITEMS_PER_CHUNK: Slot = 2_500;

// A specially reserved write version (identifier for ordering writes in an AppendVec)
// for entries in the cache, so that  operations that take a storage entry can maintain
// a common interface when interacting with cached accounts. This version is "virtual" in
// that it doesn't actually map to an entry in an AppendVec.
const CACHE_VIRTUAL_WRITE_VERSION: StoredMetaWriteVersion = 0;

// A specially reserved offset (represents an offset into an AppendVec)
// for entries in the cache, so that  operations that take a storage entry can maintain
// a common interface when interacting with cached accounts. This version is "virtual" in
// that it doesn't actually map to an entry in an AppendVec.
pub(crate) const CACHE_VIRTUAL_OFFSET: Offset = 0;
const CACHE_VIRTUAL_STORED_SIZE: StoredSize = 0;

// When getting accounts for shrinking from the index, this is the # of accounts to lookup per thread.
// This allows us to split up accounts index accesses across multiple threads.
const SHRINK_COLLECT_CHUNK_SIZE: usize = 50;

/// temporary enum during feature activation of
/// ignore slot when calculating an account hash #28420
#[derive(Debug, Clone, Copy)]
pub enum IncludeSlotInHash {
    /// this is the status quo, prior to feature activation
    /// INCLUDE the slot in the account hash calculation
    IncludeSlot,
    /// this is the value once feature activation occurs
    /// do NOT include the slot in the account hash calculation
    RemoveSlot,
    /// this option should not be used.
    /// If it is, this is a panic worthy event.
    /// There are code paths where the feature activation status isn't known, but this value should not possibly be used.
    IrrelevantAssertOnUse,
}

/// used by tests for 'include_slot_in_hash' parameter
/// Tests just need to be self-consistent, so any value should work here.
pub const INCLUDE_SLOT_IN_HASH_TESTS: IncludeSlotInHash = IncludeSlotInHash::IncludeSlot;

// This value is irrelevant because we are reading from append vecs and the hash is already computed and saved.
// The hash will just be loaded from the append vec as opposed to being calculated initially.
// A shrink-type operation involves reading from an append vec and writing a subset of the read accounts to a new append vec.
// So, by definition, we will just read hashes and write hashes. The hash will not be calculated.
// The 'store' apis are shared, such that the initial store from a bank (where we need to know whether to include the slot)
// must include a feature-based value for 'include_slot_in_hash'. Other uses, specifically shrink, do NOT need to pass this
// parameter, but the shared api requires a value.
pub const INCLUDE_SLOT_IN_HASH_IRRELEVANT_APPEND_VEC_OPERATION: IncludeSlotInHash =
    IncludeSlotInHash::IrrelevantAssertOnUse;

// This value is irrelevant because the the debug-only check_hash debug option is not possible to enable at the moment.
// This has been true for some time now, due to fallout from disabling rewrites.
// The check_hash debug option can be re-enabled once this feature and the 'rent_epoch' features are enabled.
pub const INCLUDE_SLOT_IN_HASH_IRRELEVANT_CHECK_HASH: IncludeSlotInHash =
    IncludeSlotInHash::IrrelevantAssertOnUse;

pub enum StoreReclaims {
    /// normal reclaim mode
    Default,
    /// do not return reclaims from accounts index upsert
    Ignore,
}

/// while combining into ancient append vecs, we need to keep track of the current one that is receiving new data
/// The pattern for callers is:
/// 1. this is a mut local
/// 2. do some version of create/new
/// 3. use it (slot, append_vec, etc.)
/// 4. re-create it sometimes
/// 5. goto 3
/// If a caller uses it before initializing it, it will be a runtime unwrap() error, similar to an assert.
/// That condition is an illegal use pattern and is justifiably an assertable condition.
#[derive(Default)]
struct CurrentAncientAppendVec {
    slot_and_append_vec: Option<(Slot, Arc<AccountStorageEntry>)>,
}

impl CurrentAncientAppendVec {
    fn new(slot: Slot, append_vec: Arc<AccountStorageEntry>) -> CurrentAncientAppendVec {
        Self {
            slot_and_append_vec: Some((slot, append_vec)),
        }
    }

    #[must_use]
    fn create_ancient_append_vec<'a>(
        &mut self,
        slot: Slot,
        db: &'a AccountsDb,
    ) -> ShrinkInProgress<'a> {
        let shrink_in_progress = db.create_ancient_append_vec(slot);
        *self = Self::new(slot, Arc::clone(shrink_in_progress.new_storage()));
        shrink_in_progress
    }
    #[must_use]
    fn create_if_necessary<'a>(
        &mut self,
        slot: Slot,
        db: &'a AccountsDb,
    ) -> Option<ShrinkInProgress<'a>> {
        if self.slot_and_append_vec.is_none() {
            Some(self.create_ancient_append_vec(slot, db))
        } else {
            None
        }
    }

    /// note this requires that 'slot_and_append_vec' is Some
    fn slot(&self) -> Slot {
        self.slot_and_append_vec.as_ref().unwrap().0
    }

    /// note this requires that 'slot_and_append_vec' is Some
    fn append_vec(&self) -> &Arc<AccountStorageEntry> {
        &self.slot_and_append_vec.as_ref().unwrap().1
    }

    /// helper function to cleanup call to 'store_accounts_frozen'
    fn store_ancient_accounts(
        &self,
        db: &AccountsDb,
        accounts_to_store: &AccountsToStore,
        storage_selector: StorageSelector,
    ) -> StoreAccountsTiming {
        let accounts = accounts_to_store.get(storage_selector);

        db.store_accounts_frozen(
            (
                self.slot(),
                accounts,
                INCLUDE_SLOT_IN_HASH_IRRELEVANT_APPEND_VEC_OPERATION,
                accounts_to_store.slot,
            ),
            None::<Vec<Hash>>,
            Some(self.append_vec()),
            None,
            StoreReclaims::Ignore,
        )
    }
}

/// specifies how to return zero lamport accounts from a load
#[derive(Clone, Copy)]
enum LoadZeroLamports {
    /// return None if loaded account has zero lamports
    None,
    /// return Some(account with zero lamports) if loaded account has zero lamports
    /// This used to be the only behavior.
    /// Note that this is non-deterministic if clean is running asynchronously.
    /// If a zero lamport account exists in the index, then Some is returned.
    /// Once it is cleaned from the index, None is returned.
    #[cfg(test)]
    SomeWithZeroLamportAccountForTests,
}

#[derive(Debug)]
struct AncientSlotPubkeysInner {
    pubkeys: HashSet<Pubkey>,
    slot: Slot,
}

#[derive(Debug, Default)]
struct AncientSlotPubkeys {
    inner: Option<AncientSlotPubkeysInner>,
}

impl AncientSlotPubkeys {
    /// All accounts in 'slot' will be moved to 'current_ancient'
    /// If 'slot' is different than the 'current_ancient'.slot, then an account in 'slot' may ALREADY be in the current ancient append vec.
    /// In that case, we need to unref the pubkey because it will now only be referenced from 'current_ancient'.slot and no longer from 'slot'.
    /// 'self' is also changed to accumulate the pubkeys that now exist in 'current_ancient'
    /// When 'slot' differs from the previous inner slot, then we have moved to a new ancient append vec, and inner.pubkeys gets reset to the
    ///  pubkeys in the new 'current_ancient'.append_vec
    fn maybe_unref_accounts_already_in_ancient(
        &mut self,
        slot: Slot,
        db: &AccountsDb,
        current_ancient: &CurrentAncientAppendVec,
        to_store: &AccountsToStore,
    ) {
        if slot != current_ancient.slot() {
            // we are taking accounts from 'slot' and putting them into 'current_ancient.slot()'
            // StorageSelector::Primary here because only the accounts that are moving from 'slot' to 'current_ancient.slot()'
            // Any overflow accounts will get written into a new append vec AT 'slot', so they don't need to be unrefed
            let accounts = to_store.get(StorageSelector::Primary);
            if Some(current_ancient.slot()) != self.inner.as_ref().map(|ap| ap.slot) {
                let pubkeys = current_ancient
                    .append_vec()
                    .accounts
                    .account_iter()
                    .map(|account| *account.pubkey())
                    .collect::<HashSet<_>>();
                self.inner = Some(AncientSlotPubkeysInner {
                    pubkeys,
                    slot: current_ancient.slot(),
                });
            }
            // accounts in 'slot' but ALSO already in the ancient append vec at a different slot need to be unref'd since 'slot' is going away
            // unwrap cannot fail because the code above will cause us to set it to Some(...) if it is None
            db.unref_accounts_already_in_storage(
                accounts,
                self.inner.as_mut().map(|p| &mut p.pubkeys).unwrap(),
            );
        }
    }
}

struct ShrinkCollect<'a> {
    original_bytes: u64,
    aligned_total_bytes: u64,
    unrefed_pubkeys: Vec<&'a Pubkey>,
    alive_accounts: Vec<&'a FoundStoredAccount<'a>>,
    /// total size in storage of all alive accounts
    alive_total_bytes: usize,
    total_starting_accounts: usize,
    /// true if all alive accounts are zero lamports
    all_are_zero_lamports: bool,
}

// the current best way to add filler accounts is gradually.
// In other scenarios, such as monitoring catchup with large # of accounts, it may be useful to be able to
// add filler accounts at the beginning, so that code path remains but won't execute at the moment.
const ADD_FILLER_ACCOUNTS_GRADUALLY: bool = true;

pub const ACCOUNTS_DB_CONFIG_FOR_TESTING: AccountsDbConfig = AccountsDbConfig {
    index: Some(ACCOUNTS_INDEX_CONFIG_FOR_TESTING),
    accounts_hash_cache_path: None,
    filler_accounts_config: FillerAccountsConfig::const_default(),
    write_cache_limit_bytes: None,
    ancient_append_vec_offset: None,
    skip_initial_hash_calc: false,
    exhaustively_verify_refcounts: false,
};
pub const ACCOUNTS_DB_CONFIG_FOR_BENCHMARKS: AccountsDbConfig = AccountsDbConfig {
    index: Some(ACCOUNTS_INDEX_CONFIG_FOR_BENCHMARKS),
    accounts_hash_cache_path: None,
    filler_accounts_config: FillerAccountsConfig::const_default(),
    write_cache_limit_bytes: None,
    ancient_append_vec_offset: None,
    skip_initial_hash_calc: false,
    exhaustively_verify_refcounts: false,
};

pub type BinnedHashData = Vec<Vec<CalculateHashIntermediate>>;

struct LoadAccountsIndexForShrink<'a> {
    /// total stored bytes for all alive accounts
    alive_total_bytes: usize,
    /// the specific alive accounts
    alive_accounts: Vec<&'a FoundStoredAccount<'a>>,
    /// pubkeys that were unref'd in the accounts index because they were dead
    unrefed_pubkeys: Vec<&'a Pubkey>,
    /// true if all alive accounts are zero lamport accounts
    all_are_zero_lamports: bool,
}

pub struct GetUniqueAccountsResult<'a> {
    pub stored_accounts: Vec<FoundStoredAccount<'a>>,
    pub original_bytes: u64,
}

pub struct AccountsAddRootTiming {
    pub index_us: u64,
    pub cache_us: u64,
    pub store_us: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct FillerAccountsConfig {
    /// Number of filler accounts
    pub count: usize,
    /// Data size per account, in bytes
    pub size: usize,
}

impl FillerAccountsConfig {
    pub const fn const_default() -> Self {
        Self { count: 0, size: 0 }
    }
}

impl Default for FillerAccountsConfig {
    fn default() -> Self {
        Self::const_default()
    }
}

#[derive(Debug, Default, Clone)]
pub struct AccountsDbConfig {
    pub index: Option<AccountsIndexConfig>,
    pub accounts_hash_cache_path: Option<PathBuf>,
    pub filler_accounts_config: FillerAccountsConfig,
    pub write_cache_limit_bytes: Option<u64>,
    /// if None, ancient append vecs are disabled
    /// Some(offset) means include slots up to (max_slot - (slots_per_epoch - 'offset'))
    pub ancient_append_vec_offset: Option<Slot>,
    pub skip_initial_hash_calc: bool,
    pub exhaustively_verify_refcounts: bool,
}

pub struct FoundStoredAccount<'a> {
    pub account: StoredAccountMeta<'a>,
    pub store_id: AppendVecId,
}

impl<'a> FoundStoredAccount<'a> {
    pub fn pubkey(&self) -> &Pubkey {
        self.account.pubkey()
    }
}

/// this tuple assumes storing from a slot to the same slot
/// accounts are `StoredAccountMeta` inside `FoundStoredAccount`
impl<'a> StorableAccounts<'a, StoredAccountMeta<'a>>
    for (Slot, &'a [&FoundStoredAccount<'a>], IncludeSlotInHash)
{
    fn pubkey(&self, index: usize) -> &Pubkey {
        self.1[index].pubkey()
    }
    fn account(&self, index: usize) -> &StoredAccountMeta<'a> {
        &self.1[index].account
    }
    fn slot(&self, _index: usize) -> Slot {
        // same other slot for all accounts
        self.0
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
    fn has_hash_and_write_version(&self) -> bool {
        true
    }
    fn hash(&self, index: usize) -> &Hash {
        self.1[index].account.hash
    }
    fn write_version(&self, index: usize) -> u64 {
        self.1[index].account.meta.write_version_obsolete
    }
}

#[cfg(not(test))]
const ABSURD_CONSECUTIVE_FAILED_ITERATIONS: usize = 100;

type DashMapVersionHash = DashMap<Pubkey, (u64, Hash)>;

#[derive(Debug, Clone, Copy)]
pub enum AccountShrinkThreshold {
    /// Measure the total space sparseness across all candidates
    /// And select the candidates by using the top sparse account storage entries to shrink.
    /// The value is the overall shrink threshold measured as ratio of the total live bytes
    /// over the total bytes.
    TotalSpace { shrink_ratio: f64 },
    /// Use the following option to shrink all stores whose alive ratio is below
    /// the specified threshold.
    IndividualStore { shrink_ratio: f64 },
}
pub const DEFAULT_ACCOUNTS_SHRINK_OPTIMIZE_TOTAL_SPACE: bool = true;
pub const DEFAULT_ACCOUNTS_SHRINK_RATIO: f64 = 0.80;
// The default extra account space in percentage from the ideal target
const DEFAULT_ACCOUNTS_SHRINK_THRESHOLD_OPTION: AccountShrinkThreshold =
    AccountShrinkThreshold::TotalSpace {
        shrink_ratio: DEFAULT_ACCOUNTS_SHRINK_RATIO,
    };

impl Default for AccountShrinkThreshold {
    fn default() -> AccountShrinkThreshold {
        DEFAULT_ACCOUNTS_SHRINK_THRESHOLD_OPTION
    }
}

pub enum ScanStorageResult<R, B> {
    Cached(Vec<R>),
    Stored(B),
}

#[derive(Debug, Default)]
pub struct IndexGenerationInfo {
    pub accounts_data_len: u64,
    pub rent_paying_accounts_by_partition: RentPayingAccountsByPartition,
}

#[derive(Debug, Default)]
struct SlotIndexGenerationInfo {
    insert_time_us: u64,
    num_accounts: u64,
    num_accounts_rent_paying: usize,
    accounts_data_len: u64,
    amount_to_top_off_rent: u64,
    rent_paying_accounts_by_partition: Vec<Pubkey>,
}

#[derive(Default, Debug)]
struct GenerateIndexTimings {
    pub index_time: u64,
    pub scan_time: u64,
    pub insertion_time_us: u64,
    pub min_bin_size: usize,
    pub max_bin_size: usize,
    pub total_items: usize,
    pub storage_size_accounts_map_us: u64,
    pub storage_size_storages_us: u64,
    pub storage_size_accounts_map_flatten_us: u64,
    pub index_flush_us: u64,
    pub rent_paying: AtomicUsize,
    pub amount_to_top_off_rent: AtomicU64,
    pub total_duplicates: u64,
    pub accounts_data_len_dedup_time_us: u64,
}

#[derive(Default, Debug, PartialEq, Eq)]
struct StorageSizeAndCount {
    pub stored_size: usize,
    pub count: usize,
}
type StorageSizeAndCountMap = DashMap<AppendVecId, StorageSizeAndCount>;

impl GenerateIndexTimings {
    pub fn report(&self) {
        datapoint_info!(
            "generate_index",
            // we cannot accurately measure index insertion time because of many threads and lock contention
            ("total_us", self.index_time, i64),
            ("scan_stores_us", self.scan_time, i64),
            ("insertion_time_us", self.insertion_time_us, i64),
            ("min_bin_size", self.min_bin_size as i64, i64),
            ("max_bin_size", self.max_bin_size as i64, i64),
            (
                "storage_size_accounts_map_us",
                self.storage_size_accounts_map_us as i64,
                i64
            ),
            (
                "storage_size_storages_us",
                self.storage_size_storages_us as i64,
                i64
            ),
            (
                "storage_size_accounts_map_flatten_us",
                self.storage_size_accounts_map_flatten_us as i64,
                i64
            ),
            ("index_flush_us", self.index_flush_us as i64, i64),
            (
                "total_rent_paying",
                self.rent_paying.load(Ordering::Relaxed) as i64,
                i64
            ),
            (
                "amount_to_top_off_rent",
                self.amount_to_top_off_rent.load(Ordering::Relaxed) as i64,
                i64
            ),
            (
                "total_items_with_duplicates",
                self.total_duplicates as i64,
                i64
            ),
            ("total_items", self.total_items as i64, i64),
            (
                "accounts_data_len_dedup_time_us",
                self.accounts_data_len_dedup_time_us as i64,
                i64
            ),
        );
    }
}

impl IndexValue for AccountInfo {}

impl ZeroLamport for AccountSharedData {
    fn is_zero_lamport(&self) -> bool {
        self.lamports() == 0
    }
}

impl ZeroLamport for Account {
    fn is_zero_lamport(&self) -> bool {
        self.lamports() == 0
    }
}

struct MultiThreadProgress<'a> {
    last_update: Instant,
    my_last_report_count: u64,
    total_count: &'a AtomicU64,
    report_delay_secs: u64,
    first_caller: bool,
    ultimate_count: u64,
}

impl<'a> MultiThreadProgress<'a> {
    fn new(total_count: &'a AtomicU64, report_delay_secs: u64, ultimate_count: u64) -> Self {
        Self {
            last_update: Instant::now(),
            my_last_report_count: 0,
            total_count,
            report_delay_secs,
            first_caller: false,
            ultimate_count,
        }
    }
    fn report(&mut self, my_current_count: u64) {
        let now = Instant::now();
        if now.duration_since(self.last_update).as_secs() >= self.report_delay_secs {
            let my_total_newly_processed_slots_since_last_report =
                my_current_count - self.my_last_report_count;

            self.my_last_report_count = my_current_count;
            let previous_total_processed_slots_across_all_threads = self.total_count.fetch_add(
                my_total_newly_processed_slots_since_last_report,
                Ordering::Relaxed,
            );
            self.first_caller =
                self.first_caller || 0 == previous_total_processed_slots_across_all_threads;
            if self.first_caller {
                info!(
                    "generating index: {}/{} slots...",
                    previous_total_processed_slots_across_all_threads
                        + my_total_newly_processed_slots_since_last_report,
                    self.ultimate_count
                );
            }
            self.last_update = now;
        }
    }
}

/// An offset into the AccountsDb::storage vector
pub type AtomicAppendVecId = AtomicU32;
pub type AppendVecId = u32;
pub type SnapshotStorage = Vec<Arc<AccountStorageEntry>>;
pub type SnapshotStorages = Vec<SnapshotStorage>;

// Each slot has a set of storage entries.
pub(crate) type SlotStores = Arc<RwLock<HashMap<AppendVecId, Arc<AccountStorageEntry>>>>;

type AccountSlots = HashMap<Pubkey, HashSet<Slot>>;
type AppendVecOffsets = HashMap<AppendVecId, HashSet<usize>>;
type ReclaimResult = (AccountSlots, AppendVecOffsets);
type PubkeysRemovedFromAccountsIndex = HashSet<Pubkey>;
type ShrinkCandidates = HashMap<Slot, HashMap<AppendVecId, Arc<AccountStorageEntry>>>;

trait Versioned {
    fn version(&self) -> u64;
}

impl Versioned for (u64, Hash) {
    fn version(&self) -> u64 {
        self.0
    }
}

impl Versioned for (u64, AccountInfo) {
    fn version(&self) -> u64 {
        self.0
    }
}

// Some hints for applicability of additional sanity checks for the do_load fast-path;
// Slower fallback code path will be taken if the fast path has failed over the retry
// threshold, regardless of these hints. Also, load cannot fail not-deterministically
// even under very rare circumstances, unlike previously did allow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadHint {
    // Caller hints that it's loading transactions for a block which is
    // descended from the current root, and at the tip of its fork.
    // Thereby, further this assumes AccountIndex::max_root should not increase
    // during this load, meaning there should be no squash.
    // Overall, this enables us to assert!() strictly while running the fast-path for
    // account loading, while maintaining the determinism of account loading and resultant
    // transaction execution thereof.
    FixedMaxRoot,
    // Caller can't hint the above safety assumption. Generally RPC and miscellaneous
    // other call-site falls into this category. The likelihood of slower path is slightly
    // increased as well.
    Unspecified,
}

#[derive(Debug)]
pub enum LoadedAccountAccessor<'a> {
    // StoredAccountMeta can't be held directly here due to its lifetime dependency to
    // AccountStorageEntry
    Stored(Option<(Arc<AccountStorageEntry>, usize)>),
    // None value in Cached variant means the cache was flushed
    Cached(Option<Cow<'a, CachedAccount>>),
}

mod geyser_plugin_utils;

impl<'a> LoadedAccountAccessor<'a> {
    fn check_and_get_loaded_account(&mut self) -> LoadedAccount {
        // all of these following .expect() and .unwrap() are like serious logic errors,
        // ideal for representing this as rust type system....

        match self {
            LoadedAccountAccessor::Cached(None) | LoadedAccountAccessor::Stored(None) => {
                panic!("Should have already been taken care of when creating this LoadedAccountAccessor");
            }
            LoadedAccountAccessor::Cached(Some(_cached_account)) => {
                // Cached(Some(x)) variant always produces `Some` for get_loaded_account() since
                // it just returns the inner `x` without additional fetches
                self.get_loaded_account().unwrap()
            }
            LoadedAccountAccessor::Stored(Some(_maybe_storage_entry)) => {
                // If we do find the storage entry, we can guarantee that the storage entry is
                // safe to read from because we grabbed a reference to the storage entry while it
                // was still in the storage map. This means even if the storage entry is removed
                // from the storage map after we grabbed the storage entry, the recycler should not
                // reset the storage entry until we drop the reference to the storage entry.
                self.get_loaded_account()
                    .expect("If a storage entry was found in the storage map, it must not have been reset yet")
            }
        }
    }

    fn get_loaded_account(&mut self) -> Option<LoadedAccount> {
        match self {
            LoadedAccountAccessor::Cached(cached_account) => {
                let cached_account: Cow<'a, CachedAccount> = cached_account.take().expect(
                    "Cache flushed/purged should be handled before trying to fetch account",
                );
                Some(LoadedAccount::Cached(cached_account))
            }
            LoadedAccountAccessor::Stored(maybe_storage_entry) => {
                // storage entry may not be present if slot was cleaned up in
                // between reading the accounts index and calling this function to
                // get account meta from the storage entry here
                maybe_storage_entry
                    .as_ref()
                    .and_then(|(storage_entry, offset)| {
                        storage_entry
                            .get_stored_account_meta(*offset)
                            .map(LoadedAccount::Stored)
                    })
            }
        }
    }
}

pub enum LoadedAccount<'a> {
    Stored(StoredAccountMeta<'a>),
    Cached(Cow<'a, CachedAccount>),
}

impl<'a> LoadedAccount<'a> {
    pub fn loaded_hash(&self) -> Hash {
        match self {
            LoadedAccount::Stored(stored_account_meta) => *stored_account_meta.hash,
            LoadedAccount::Cached(cached_account) => cached_account.hash(),
        }
    }

    pub fn pubkey(&self) -> &Pubkey {
        match self {
            LoadedAccount::Stored(stored_account_meta) => stored_account_meta.pubkey(),
            LoadedAccount::Cached(cached_account) => cached_account.pubkey(),
        }
    }

    pub fn write_version(&self) -> StoredMetaWriteVersion {
        match self {
            LoadedAccount::Stored(stored_account_meta) => {
                stored_account_meta.meta.write_version_obsolete
            }
            LoadedAccount::Cached(_) => CACHE_VIRTUAL_WRITE_VERSION,
        }
    }

    pub fn compute_hash(
        &self,
        slot: Slot,
        pubkey: &Pubkey,
        include_slot: IncludeSlotInHash,
    ) -> Hash {
        match self {
            LoadedAccount::Stored(stored_account_meta) => AccountsDb::hash_account(
                slot,
                stored_account_meta,
                stored_account_meta.pubkey(),
                include_slot,
            ),
            LoadedAccount::Cached(cached_account) => {
                AccountsDb::hash_account(slot, &cached_account.account, pubkey, include_slot)
            }
        }
    }

    pub fn take_account(self) -> AccountSharedData {
        match self {
            LoadedAccount::Stored(stored_account_meta) => stored_account_meta.clone_account(),
            LoadedAccount::Cached(cached_account) => match cached_account {
                Cow::Owned(cached_account) => cached_account.account.clone(),
                Cow::Borrowed(cached_account) => cached_account.account.clone(),
            },
        }
    }

    pub fn is_cached(&self) -> bool {
        match self {
            LoadedAccount::Stored(_) => false,
            LoadedAccount::Cached(_) => true,
        }
    }
}

impl<'a> ReadableAccount for LoadedAccount<'a> {
    fn lamports(&self) -> u64 {
        match self {
            LoadedAccount::Stored(stored_account_meta) => stored_account_meta.account_meta.lamports,
            LoadedAccount::Cached(cached_account) => cached_account.account.lamports(),
        }
    }

    fn data(&self) -> &[u8] {
        match self {
            LoadedAccount::Stored(stored_account_meta) => stored_account_meta.data,
            LoadedAccount::Cached(cached_account) => cached_account.account.data(),
        }
    }
    fn owner(&self) -> &Pubkey {
        match self {
            LoadedAccount::Stored(stored_account_meta) => &stored_account_meta.account_meta.owner,
            LoadedAccount::Cached(cached_account) => cached_account.account.owner(),
        }
    }
    fn executable(&self) -> bool {
        match self {
            LoadedAccount::Stored(stored_account_meta) => {
                stored_account_meta.account_meta.executable
            }
            LoadedAccount::Cached(cached_account) => cached_account.account.executable(),
        }
    }
    fn rent_epoch(&self) -> Epoch {
        match self {
            LoadedAccount::Stored(stored_account_meta) => {
                stored_account_meta.account_meta.rent_epoch
            }
            LoadedAccount::Cached(cached_account) => cached_account.account.rent_epoch(),
        }
    }
    fn to_account_shared_data(&self) -> AccountSharedData {
        match self {
            LoadedAccount::Stored(_stored_account_meta) => AccountSharedData::create(
                self.lamports(),
                self.data().to_vec(),
                *self.owner(),
                self.executable(),
                self.rent_epoch(),
            ),
            // clone here to prevent data copy
            LoadedAccount::Cached(cached_account) => cached_account.account.clone(),
        }
    }
}

#[derive(Debug)]
pub enum BankHashVerificationError {
    MismatchedAccountHash,
    MismatchedBankHash,
    MissingBankHash,
    MismatchedTotalLamports(u64, u64),
}

#[derive(Default)]
struct CleanKeyTimings {
    collect_delta_keys_us: u64,
    delta_insert_us: u64,
    hashset_to_vec_us: u64,
    dirty_store_processing_us: u64,
    delta_key_count: u64,
    dirty_pubkeys_count: u64,
    oldest_dirty_slot: Slot,
    /// number of ancient append vecs that were scanned because they were dirty when clean started
    dirty_ancient_stores: usize,
}

/// Persistent storage structure holding the accounts
#[derive(Debug)]
pub struct AccountStorageEntry {
    pub(crate) id: AtomicAppendVecId,

    pub(crate) slot: AtomicU64,

    /// storage holding the accounts
    pub(crate) accounts: AppendVec,

    /// Keeps track of the number of accounts stored in a specific AppendVec.
    ///  This is periodically checked to reuse the stores that do not have
    ///  any accounts in it
    /// status corresponding to the storage, lets us know that
    ///  the append_vec, once maxed out, then emptied, can be reclaimed
    count_and_status: RwLock<(usize, AccountStorageStatus)>,

    /// This is the total number of accounts stored ever since initialized to keep
    /// track of lifetime count of all store operations. And this differs from
    /// count_and_status in that this field won't be decremented.
    ///
    /// This is used as a rough estimate for slot shrinking. As such a relaxed
    /// use case, this value ARE NOT strictly synchronized with count_and_status!
    approx_store_count: AtomicUsize,

    alive_bytes: AtomicUsize,
}

impl AccountStorageEntry {
    pub fn new(path: &Path, slot: Slot, id: AppendVecId, file_size: u64) -> Self {
        let tail = AppendVec::file_name(slot, id);
        let path = Path::new(path).join(tail);
        let accounts = AppendVec::new(&path, true, file_size as usize);

        Self {
            id: AtomicAppendVecId::new(id),
            slot: AtomicU64::new(slot),
            accounts,
            count_and_status: RwLock::new((0, AccountStorageStatus::Available)),
            approx_store_count: AtomicUsize::new(0),
            alive_bytes: AtomicUsize::new(0),
        }
    }

    pub(crate) fn new_existing(
        slot: Slot,
        id: AppendVecId,
        accounts: AppendVec,
        num_accounts: usize,
    ) -> Self {
        Self {
            id: AtomicAppendVecId::new(id),
            slot: AtomicU64::new(slot),
            accounts,
            count_and_status: RwLock::new((0, AccountStorageStatus::Available)),
            approx_store_count: AtomicUsize::new(num_accounts),
            alive_bytes: AtomicUsize::new(0),
        }
    }

    pub fn set_status(&self, mut status: AccountStorageStatus) {
        let mut count_and_status = self.count_and_status.write().unwrap();

        let count = count_and_status.0;

        if status == AccountStorageStatus::Full && count == 0 {
            // this case arises when the append_vec is full (store_ptrs fails),
            //  but all accounts have already been removed from the storage
            //
            // the only time it's safe to call reset() on an append_vec is when
            //  every account has been removed
            //          **and**
            //  the append_vec has previously been completely full
            //
            self.accounts.reset();
            status = AccountStorageStatus::Available;
        }

        *count_and_status = (count, status);
    }

    pub fn recycle(&self, slot: Slot, id: AppendVecId) {
        let mut count_and_status = self.count_and_status.write().unwrap();
        self.accounts.reset();
        *count_and_status = (0, AccountStorageStatus::Available);
        self.slot.store(slot, Ordering::Release);
        self.id.store(id, Ordering::Release);
        self.approx_store_count.store(0, Ordering::Relaxed);
        self.alive_bytes.store(0, Ordering::Release);
    }

    pub fn status(&self) -> AccountStorageStatus {
        self.count_and_status.read().unwrap().1
    }

    pub fn count(&self) -> usize {
        self.count_and_status.read().unwrap().0
    }

    pub fn approx_stored_count(&self) -> usize {
        self.approx_store_count.load(Ordering::Relaxed)
    }

    pub fn alive_bytes(&self) -> usize {
        self.alive_bytes.load(Ordering::SeqCst)
    }

    pub fn written_bytes(&self) -> u64 {
        self.accounts.len() as u64
    }

    pub fn total_bytes(&self) -> u64 {
        self.accounts.capacity()
    }

    pub fn has_accounts(&self) -> bool {
        self.count() > 0
    }

    pub fn slot(&self) -> Slot {
        self.slot.load(Ordering::Acquire)
    }

    pub fn append_vec_id(&self) -> AppendVecId {
        self.id.load(Ordering::Acquire)
    }

    pub fn flush(&self) -> Result<(), IoError> {
        self.accounts.flush()
    }

    fn get_stored_account_meta(&self, offset: usize) -> Option<StoredAccountMeta> {
        Some(self.accounts.get_account(offset)?.0)
    }

    fn add_account(&self, num_bytes: usize) {
        let mut count_and_status = self.count_and_status.write().unwrap();
        *count_and_status = (count_and_status.0 + 1, count_and_status.1);
        self.approx_store_count.fetch_add(1, Ordering::Relaxed);
        self.alive_bytes.fetch_add(num_bytes, Ordering::SeqCst);
    }

    fn try_available(&self) -> bool {
        let mut count_and_status = self.count_and_status.write().unwrap();
        let (count, status) = *count_and_status;

        if status == AccountStorageStatus::Available {
            *count_and_status = (count, AccountStorageStatus::Candidate);
            true
        } else {
            false
        }
    }

    pub fn all_accounts(&self) -> Vec<StoredAccountMeta> {
        self.accounts.accounts(0)
    }

    fn remove_account(&self, num_bytes: usize, reset_accounts: bool) -> usize {
        let mut count_and_status = self.count_and_status.write().unwrap();
        let (mut count, mut status) = *count_and_status;

        if count == 1 && status == AccountStorageStatus::Full && reset_accounts {
            // this case arises when we remove the last account from the
            //  storage, but we've learned from previous write attempts that
            //  the storage is full
            //
            // the only time it's safe to call reset() on an append_vec is when
            //  every account has been removed
            //          **and**
            //  the append_vec has previously been completely full
            //
            // otherwise, the storage may be in flight with a store()
            //   call
            self.accounts.reset();
            status = AccountStorageStatus::Available;
        }

        // Some code path is removing accounts too many; this may result in an
        // unintended reveal of old state for unrelated accounts.
        assert!(
            count > 0,
            "double remove of account in slot: {}/store: {}!!",
            self.slot(),
            self.append_vec_id(),
        );

        self.alive_bytes.fetch_sub(num_bytes, Ordering::SeqCst);
        count -= 1;
        *count_and_status = (count, status);
        count
    }

    pub fn get_path(&self) -> PathBuf {
        self.accounts.get_path()
    }
}

pub fn get_temp_accounts_paths(count: u32) -> IoResult<(Vec<TempDir>, Vec<PathBuf>)> {
    let temp_dirs: IoResult<Vec<TempDir>> = (0..count).map(|_| TempDir::new()).collect();
    let temp_dirs = temp_dirs?;
    let paths: Vec<PathBuf> = temp_dirs.iter().map(|t| t.path().to_path_buf()).collect();
    Ok((temp_dirs, paths))
}

#[derive(Clone, Default, Debug, Serialize, Deserialize, PartialEq, Eq, AbiExample)]
pub struct BankHashStats {
    pub num_updated_accounts: u64,
    pub num_removed_accounts: u64,
    pub num_lamports_stored: u64,
    pub total_data_len: u64,
    pub num_executable_accounts: u64,
}

impl BankHashStats {
    pub fn update<T: ReadableAccount + ZeroLamport>(&mut self, account: &T) {
        if account.is_zero_lamport() {
            self.num_removed_accounts += 1;
        } else {
            self.num_updated_accounts += 1;
        }
        self.total_data_len = self
            .total_data_len
            .wrapping_add(account.data().len() as u64);
        if account.executable() {
            self.num_executable_accounts += 1;
        }
        self.num_lamports_stored = self.num_lamports_stored.wrapping_add(account.lamports());
    }

    pub fn accumulate(&mut self, other: &BankHashStats) {
        self.num_updated_accounts += other.num_updated_accounts;
        self.num_removed_accounts += other.num_removed_accounts;
        self.total_data_len = self.total_data_len.wrapping_add(other.total_data_len);
        self.num_lamports_stored = self
            .num_lamports_stored
            .wrapping_add(other.num_lamports_stored);
        self.num_executable_accounts += other.num_executable_accounts;
    }
}

#[derive(Clone, Default, Debug, Serialize, Deserialize, PartialEq, Eq, AbiExample)]
pub struct BankHashInfo {
    pub accounts_delta_hash: Hash,
    pub accounts_hash: AccountsHash,
    pub stats: BankHashStats,
}

#[derive(Default)]
pub struct StoreAccountsTiming {
    store_accounts_elapsed: u64,
    update_index_elapsed: u64,
    handle_reclaims_elapsed: u64,
}

impl StoreAccountsTiming {
    fn accumulate(&mut self, other: &Self) {
        self.store_accounts_elapsed += other.store_accounts_elapsed;
        self.update_index_elapsed += other.update_index_elapsed;
        self.handle_reclaims_elapsed += other.handle_reclaims_elapsed;
    }
}

#[derive(Debug, Default)]
struct RecycleStores {
    entries: Vec<(Instant, Arc<AccountStorageEntry>)>,
    total_bytes: u64,
}

// 30 min should be enough to be certain there won't be any prospective recycle uses for given
// store entry
// That's because it already processed ~2500 slots and ~25 passes of AccountsBackgroundService
pub const EXPIRATION_TTL_SECONDS: u64 = 1800;

impl RecycleStores {
    fn add_entry(&mut self, new_entry: Arc<AccountStorageEntry>) {
        self.total_bytes += new_entry.total_bytes();
        self.entries.push((Instant::now(), new_entry))
    }

    fn iter(&self) -> std::slice::Iter<(Instant, Arc<AccountStorageEntry>)> {
        self.entries.iter()
    }

    fn add_entries(&mut self, new_entries: SnapshotStorage) {
        let now = Instant::now();
        for new_entry in new_entries {
            self.total_bytes += new_entry.total_bytes();
            self.entries.push((now, new_entry));
        }
    }

    fn expire_old_entries(&mut self) -> SnapshotStorage {
        let mut expired = vec![];
        let now = Instant::now();
        let mut expired_bytes = 0;
        self.entries.retain(|(recycled_time, entry)| {
            if now.duration_since(*recycled_time).as_secs() > EXPIRATION_TTL_SECONDS {
                if Arc::strong_count(entry) >= 2 {
                    warn!(
                        "Expiring still in-use recycled StorageEntry anyway...: id: {} slot: {}",
                        entry.append_vec_id(),
                        entry.slot(),
                    );
                }
                expired_bytes += entry.total_bytes();
                expired.push(entry.clone());
                false
            } else {
                true
            }
        });

        self.total_bytes -= expired_bytes;

        expired
    }

    fn remove_entry(&mut self, index: usize) -> Arc<AccountStorageEntry> {
        let (_added_time, removed_entry) = self.entries.swap_remove(index);
        self.total_bytes -= removed_entry.total_bytes();
        removed_entry
    }

    fn entry_count(&self) -> usize {
        self.entries.len()
    }

    fn total_bytes(&self) -> u64 {
        self.total_bytes
    }
}

/// Removing unrooted slots in Accounts Background Service needs to be synchronized with flushing
/// slots from the Accounts Cache.  This keeps track of those slots and the Mutex + Condvar for
/// synchronization.
#[derive(Debug, Default)]
struct RemoveUnrootedSlotsSynchronization {
    // slots being flushed from the cache or being purged
    slots_under_contention: Mutex<HashSet<Slot>>,
    signal: Condvar,
}

type AccountInfoAccountsIndex = AccountsIndex<AccountInfo>;

// This structure handles the load/store of the accounts
#[derive(Debug)]
pub struct AccountsDb {
    /// Keeps tracks of index into AppendVec on a per slot basis
    pub accounts_index: AccountInfoAccountsIndex,

    /// slot that is one epoch older than the highest slot where accounts hash calculation has completed
    pub accounts_hash_complete_one_epoch_old: RwLock<Slot>,

    /// Some(offset) iff we want to squash old append vecs together into 'ancient append vecs'
    /// Some(offset) means for slots up to (max_slot - (slots_per_epoch - 'offset')), put them in ancient append vecs
    pub ancient_append_vec_offset: Option<Slot>,

    /// true iff we want to skip the initial hash calculation on startup
    pub skip_initial_hash_calc: bool,

    pub(crate) storage: AccountStorage,

    pub accounts_cache: AccountsCache,

    write_cache_limit_bytes: Option<u64>,

    sender_bg_hasher: Option<Sender<CachedAccount>>,
    read_only_accounts_cache: ReadOnlyAccountsCache,

    recycle_stores: RwLock<RecycleStores>,

    /// distribute the accounts across storage lists
    pub next_id: AtomicAppendVecId,

    /// Set of shrinkable stores organized by map of slot to append_vec_id
    pub shrink_candidate_slots: Mutex<ShrinkCandidates>,

    pub(crate) write_version: AtomicU64,

    /// Set of storage paths to pick from
    pub(crate) paths: Vec<PathBuf>,

    accounts_hash_cache_path: PathBuf,

    // used by tests
    // holds this until we are dropped
    #[allow(dead_code)]
    temp_accounts_hash_cache_path: Option<TempDir>,

    pub shrink_paths: RwLock<Option<Vec<PathBuf>>>,

    /// Directory of paths this accounts_db needs to hold/remove
    #[allow(dead_code)]
    pub(crate) temp_paths: Option<Vec<TempDir>>,

    /// Starting file size of appendvecs
    file_size: u64,

    /// Thread pool used for par_iter
    pub thread_pool: ThreadPool,

    pub thread_pool_clean: ThreadPool,

    pub bank_hashes: RwLock<HashMap<Slot, BankHashInfo>>,

    pub stats: AccountsStats,

    clean_accounts_stats: CleanAccountsStats,

    // Stats for purges called outside of clean_accounts()
    external_purge_slots_stats: PurgeStats,

    pub(crate) shrink_stats: ShrinkStats,

    shrink_ancient_stats: ShrinkAncientStats,

    pub cluster_type: Option<ClusterType>,

    pub account_indexes: AccountSecondaryIndexes,

    /// Set of unique keys per slot which is used
    /// to drive clean_accounts
    /// Generated by get_accounts_delta_hash
    uncleaned_pubkeys: DashMap<Slot, Vec<Pubkey>>,

    #[cfg(test)]
    load_delay: u64,

    #[cfg(test)]
    load_limit: AtomicU64,

    /// true if drop_callback is attached to the bank.
    is_bank_drop_callback_enabled: AtomicBool,

    /// Set of slots currently being flushed by `flush_slot_cache()` or removed
    /// by `remove_unrooted_slot()`. Used to ensure `remove_unrooted_slots(slots)`
    /// can safely clear the set of unrooted slots `slots`.
    remove_unrooted_slots_synchronization: RemoveUnrootedSlotsSynchronization,

    shrink_ratio: AccountShrinkThreshold,

    /// Set of stores which are recently rooted or had accounts removed
    /// such that potentially a 0-lamport account update could be present which
    /// means we can remove the account from the index entirely.
    dirty_stores: DashMap<(Slot, AppendVecId), Arc<AccountStorageEntry>>,

    /// Zero-lamport accounts that are *not* purged during clean because they need to stay alive
    /// for incremental snapshot support.
    zero_lamport_accounts_to_purge_after_full_snapshot: DashSet<(Slot, Pubkey)>,

    /// GeyserPlugin accounts update notifier
    accounts_update_notifier: Option<AccountsUpdateNotifier>,

    filler_accounts_config: FillerAccountsConfig,
    pub filler_account_suffix: Option<Pubkey>,

    active_stats: ActiveStats,

    /// number of filler accounts to add for each slot
    pub filler_accounts_per_slot: AtomicU64,

    /// number of slots remaining where filler accounts should be added
    pub filler_account_slots_remaining: AtomicU64,

    pub(crate) verify_accounts_hash_in_bg: VerifyAccountsHashInBackground,

    /// Used to disable logging dead slots during removal.
    /// allow disabling noisy log
    pub(crate) log_dead_slots: AtomicBool,

    /// debug feature to scan every append vec and verify refcounts are equal
    exhaustively_verify_refcounts: bool,

    /// the full accounts hash calculation as of a predetermined block height 'N'
    /// to be included in the bank hash at a predetermined block height 'M'
    /// The cadence is once per epoch, all nodes calculate a full accounts hash as of a known slot calculated using 'N'
    /// Some time later (to allow for slow calculation time), the bank hash at a slot calculated using 'M' includes the full accounts hash.
    /// Thus, the state of all accounts on a validator is known to be correct at least once per epoch.
    pub epoch_accounts_hash_manager: EpochAccountsHashManager,
}

#[derive(Debug, Default)]
pub struct AccountsStats {
    delta_hash_scan_time_total_us: AtomicU64,
    delta_hash_accumulate_time_total_us: AtomicU64,
    delta_hash_num: AtomicU64,

    last_store_report: AtomicInterval,
    store_hash_accounts: AtomicU64,
    calc_stored_meta: AtomicU64,
    store_accounts: AtomicU64,
    store_update_index: AtomicU64,
    store_handle_reclaims: AtomicU64,
    store_append_accounts: AtomicU64,
    pub stakes_cache_check_and_store_us: AtomicU64,
    store_find_store: AtomicU64,
    store_num_accounts: AtomicU64,
    store_total_data: AtomicU64,
    recycle_store_count: AtomicU64,
    create_store_count: AtomicU64,
    store_get_slot_store: AtomicU64,
    store_find_existing: AtomicU64,
    dropped_stores: AtomicU64,
    store_uncleaned_update: AtomicU64,
}

#[derive(Debug, Default)]
pub(crate) struct PurgeStats {
    last_report: AtomicInterval,
    safety_checks_elapsed: AtomicU64,
    remove_cache_elapsed: AtomicU64,
    remove_storage_entries_elapsed: AtomicU64,
    drop_storage_entries_elapsed: AtomicU64,
    num_cached_slots_removed: AtomicUsize,
    num_stored_slots_removed: AtomicUsize,
    total_removed_storage_entries: AtomicUsize,
    total_removed_cached_bytes: AtomicU64,
    total_removed_stored_bytes: AtomicU64,
    recycle_stores_write_elapsed: AtomicU64,
    scan_storages_elapsed: AtomicU64,
    purge_accounts_index_elapsed: AtomicU64,
    handle_reclaims_elapsed: AtomicU64,
}

impl PurgeStats {
    fn report(&self, metric_name: &'static str, report_interval_ms: Option<u64>) {
        let should_report = report_interval_ms
            .map(|report_interval_ms| self.last_report.should_update(report_interval_ms))
            .unwrap_or(true);

        if should_report {
            datapoint_info!(
                metric_name,
                (
                    "safety_checks_elapsed",
                    self.safety_checks_elapsed.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "remove_cache_elapsed",
                    self.remove_cache_elapsed.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "remove_storage_entries_elapsed",
                    self.remove_storage_entries_elapsed
                        .swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "drop_storage_entries_elapsed",
                    self.drop_storage_entries_elapsed.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "num_cached_slots_removed",
                    self.num_cached_slots_removed.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "num_stored_slots_removed",
                    self.num_stored_slots_removed.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "total_removed_storage_entries",
                    self.total_removed_storage_entries
                        .swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "total_removed_cached_bytes",
                    self.total_removed_cached_bytes.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "total_removed_stored_bytes",
                    self.total_removed_stored_bytes.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "recycle_stores_write_elapsed",
                    self.recycle_stores_write_elapsed.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "scan_storages_elapsed",
                    self.scan_storages_elapsed.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "purge_accounts_index_elapsed",
                    self.purge_accounts_index_elapsed.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "handle_reclaims_elapsed",
                    self.handle_reclaims_elapsed.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
            );
        }
    }
}

/// results from 'split_storages_ancient'
#[derive(Debug, Default, PartialEq)]
struct SplitAncientStorages {
    /// # ancient slots
    ancient_slot_count: usize,
    /// the specific ancient slots
    ancient_slots: Vec<Slot>,
    /// lowest slot that is not an ancient append vec
    first_non_ancient_slot: Slot,
    /// slot # of beginning of first aligned chunk starting from the first non ancient slot
    first_chunk_start: Slot,
    /// # non-ancient slots to scan
    non_ancient_slot_count: usize,
    /// # chunks to use to iterate the storages
    /// all ancient chunks, the special 0 and last chunks for non-full chunks, and all the 'full' chunks of normal slots
    chunk_count: usize,
    /// start and end(exclusive) of normal (non-ancient) slots to be scanned
    normal_slot_range: Range<Slot>,
}

impl SplitAncientStorages {
    /// When calculating accounts hash, we break the slots/storages into chunks that remain the same during an entire epoch.
    /// a slot is in this chunk of slots:
    /// start:         (slot / MAX_ITEMS_PER_CHUNK) * MAX_ITEMS_PER_CHUNK
    /// end_exclusive: start + MAX_ITEMS_PER_CHUNK
    /// So a slot remains in the same chunk whenever it is included in the accounts hash.
    /// When the slot gets deleted or gets consumed in an ancient append vec, it will no longer be in its chunk.
    /// The results of scanning a chunk of appendvecs can be cached to avoid scanning large amounts of data over and over.
    fn new(one_epoch_old_slot: Slot, snapshot_storages: &SortedStorages) -> Self {
        let range = snapshot_storages.range();

        // any ancient append vecs should definitely be cached
        // We need to break the ranges into:
        // 1. individual ancient append vecs (may be empty)
        // 2. first unevenly divided chunk starting at 1 epoch old slot (may be empty)
        // 3. evenly divided full chunks in the middle
        // 4. unevenly divided chunk of most recent slots (may be empty)
        let ancient_slots = Self::get_ancient_slots(one_epoch_old_slot, snapshot_storages);

        let first_non_ancient_slot = ancient_slots
            .last()
            .map(|last_ancient_slot| last_ancient_slot.saturating_add(1))
            .unwrap_or(range.start);
        Self::new_with_ancient_info(range, ancient_slots, first_non_ancient_slot)
    }

    /// return all ancient append vec slots from the early slots referenced by 'snapshot_storages'
    fn get_ancient_slots(
        one_epoch_old_slot: Slot,
        snapshot_storages: &SortedStorages,
    ) -> Vec<Slot> {
        let range = snapshot_storages.range();
        let mut ancient_slots = Vec::default();
        for (slot, storages) in snapshot_storages.iter_range(&(range.start..one_epoch_old_slot)) {
            if let Some(storages) = storages {
                if storages.len() == 1 && is_ancient(&storages.first().unwrap().accounts) {
                    ancient_slots.push(slot);
                    continue; // was ancient, keep looking
                }
                // we found a slot with a non-ancient append vec
                break;
            }
        }
        ancient_slots
    }

    /// create once ancient slots have been identified
    /// This is easier to test, removing SortedStorges as a type to deal with here.
    fn new_with_ancient_info(
        range: &Range<Slot>,
        ancient_slots: Vec<Slot>,
        first_non_ancient_slot: Slot,
    ) -> Self {
        if range.is_empty() {
            // Corner case mainly for tests, but gives us a consistent base case. Makes more sense to return default here than anything else.
            // caller is asking to split for empty set of slots
            return SplitAncientStorages::default();
        }

        let max_slot_inclusive = range.end.saturating_sub(1);
        let ancient_slot_count = ancient_slots.len();
        let first_chunk_start = ((first_non_ancient_slot + MAX_ITEMS_PER_CHUNK)
            / MAX_ITEMS_PER_CHUNK)
            * MAX_ITEMS_PER_CHUNK;

        let non_ancient_slot_count = (max_slot_inclusive - first_non_ancient_slot + 1) as usize;

        let normal_slot_range = Range {
            start: first_non_ancient_slot,
            end: range.end,
        };

        // 2 is for 2 special chunks - unaligned slots at the beginning and end
        let chunk_count =
            ancient_slot_count + 2 + non_ancient_slot_count / (MAX_ITEMS_PER_CHUNK as usize);

        SplitAncientStorages {
            ancient_slot_count,
            ancient_slots,
            first_non_ancient_slot,
            first_chunk_start,
            non_ancient_slot_count,
            chunk_count,
            normal_slot_range,
        }
    }

    /// given 'normal_chunk', return the starting slot of that chunk in the normal/non-ancient range
    /// a normal_chunk is 0<=normal_chunk<=non_ancient_chunk_count
    /// non_ancient_chunk_count is chunk_count-ancient_slot_count
    fn get_starting_slot_from_normal_chunk(&self, normal_chunk: usize) -> Slot {
        if normal_chunk == 0 {
            self.normal_slot_range.start
        } else {
            assert!(
                normal_chunk.saturating_add(self.ancient_slot_count) < self.chunk_count,
                "out of bounds: {}, {}",
                normal_chunk,
                self.chunk_count
            );

            let normal_chunk = normal_chunk.saturating_sub(1);
            (self.first_chunk_start + MAX_ITEMS_PER_CHUNK * (normal_chunk as Slot))
                .max(self.normal_slot_range.start)
        }
    }

    /// ancient slots are the first chunks
    fn is_chunk_ancient(&self, chunk: usize) -> bool {
        chunk < self.ancient_slot_count
    }

    /// given chunk in 0<=chunk<self.chunk_count
    /// return the range of slots in that chunk
    /// None indicates the range is empty for that chunk.
    fn get_slot_range(&self, chunk: usize) -> Option<Range<Slot>> {
        let range = if self.is_chunk_ancient(chunk) {
            // ancient append vecs are handled individually
            let slot = self.ancient_slots[chunk];
            Range {
                start: slot,
                end: slot + 1,
            }
        } else {
            // normal chunks are after ancient chunks
            let normal_chunk = chunk - self.ancient_slot_count;
            if normal_chunk == 0 {
                // first slot
                Range {
                    start: self.normal_slot_range.start,
                    end: self.first_chunk_start.min(self.normal_slot_range.end),
                }
            } else {
                // normal full chunk or the last chunk
                let first_slot = self.get_starting_slot_from_normal_chunk(normal_chunk);
                Range {
                    start: first_slot,
                    end: (first_slot + MAX_ITEMS_PER_CHUNK).min(self.normal_slot_range.end),
                }
            }
        };
        // return empty range as None
        (!range.is_empty()).then_some(range)
    }
}

#[derive(Debug, Default)]
struct FlushStats {
    num_flushed: usize,
    num_purged: usize,
    total_size: u64,
}

#[derive(Debug, Default)]
struct LatestAccountsIndexRootsStats {
    roots_len: AtomicUsize,
    historical_roots_len: AtomicUsize,
    uncleaned_roots_len: AtomicUsize,
    previous_uncleaned_roots_len: AtomicUsize,
    roots_range: AtomicU64,
    rooted_cleaned_count: AtomicUsize,
    unrooted_cleaned_count: AtomicUsize,
    clean_unref_from_storage_us: AtomicU64,
    clean_dead_slot_us: AtomicU64,
}

impl LatestAccountsIndexRootsStats {
    fn update(&self, accounts_index_roots_stats: &AccountsIndexRootsStats) {
        if let Some(value) = accounts_index_roots_stats.roots_len {
            self.roots_len.store(value, Ordering::Relaxed);
        }
        if let Some(value) = accounts_index_roots_stats.uncleaned_roots_len {
            self.uncleaned_roots_len.store(value, Ordering::Relaxed);
        }
        if let Some(value) = accounts_index_roots_stats.previous_uncleaned_roots_len {
            self.previous_uncleaned_roots_len
                .store(value, Ordering::Relaxed);
        }
        if let Some(value) = accounts_index_roots_stats.historical_roots_len {
            self.historical_roots_len.store(value, Ordering::Relaxed);
        }
        if let Some(value) = accounts_index_roots_stats.roots_range {
            self.roots_range.store(value, Ordering::Relaxed);
        }
        self.rooted_cleaned_count.fetch_add(
            accounts_index_roots_stats.rooted_cleaned_count,
            Ordering::Relaxed,
        );
        self.unrooted_cleaned_count.fetch_add(
            accounts_index_roots_stats.unrooted_cleaned_count,
            Ordering::Relaxed,
        );
        self.clean_unref_from_storage_us.fetch_add(
            accounts_index_roots_stats.clean_unref_from_storage_us,
            Ordering::Relaxed,
        );
        self.clean_dead_slot_us.fetch_add(
            accounts_index_roots_stats.clean_dead_slot_us,
            Ordering::Relaxed,
        );
    }

    fn report(&self) {
        datapoint_info!(
            "accounts_index_roots_len",
            (
                "roots_len",
                self.roots_len.load(Ordering::Relaxed) as i64,
                i64
            ),
            (
                "historical_roots_len",
                self.historical_roots_len.load(Ordering::Relaxed) as i64,
                i64
            ),
            (
                "uncleaned_roots_len",
                self.uncleaned_roots_len.load(Ordering::Relaxed) as i64,
                i64
            ),
            (
                "previous_uncleaned_roots_len",
                self.previous_uncleaned_roots_len.load(Ordering::Relaxed) as i64,
                i64
            ),
            (
                "roots_range_width",
                self.roots_range.load(Ordering::Relaxed) as i64,
                i64
            ),
            (
                "unrooted_cleaned_count",
                self.unrooted_cleaned_count.swap(0, Ordering::Relaxed) as i64,
                i64
            ),
            (
                "rooted_cleaned_count",
                self.rooted_cleaned_count.swap(0, Ordering::Relaxed) as i64,
                i64
            ),
            (
                "clean_unref_from_storage_us",
                self.clean_unref_from_storage_us.swap(0, Ordering::Relaxed) as i64,
                i64
            ),
            (
                "clean_dead_slot_us",
                self.clean_dead_slot_us.swap(0, Ordering::Relaxed) as i64,
                i64
            ),
            (
                "append_vecs_open",
                APPEND_VEC_MMAPPED_FILES_OPEN.load(Ordering::Relaxed) as i64,
                i64
            )
        );

        // Don't need to reset since this tracks the latest updates, not a cumulative total
    }
}

#[derive(Debug, Default)]
struct CleanAccountsStats {
    purge_stats: PurgeStats,
    latest_accounts_index_roots_stats: LatestAccountsIndexRootsStats,

    // stats held here and reported by clean_accounts
    clean_old_root_us: AtomicU64,
    clean_old_root_reclaim_us: AtomicU64,
    reset_uncleaned_roots_us: AtomicU64,
    remove_dead_accounts_remove_us: AtomicU64,
    remove_dead_accounts_shrink_us: AtomicU64,
    clean_stored_dead_slots_us: AtomicU64,
}

impl CleanAccountsStats {
    fn report(&self) {
        self.purge_stats.report("clean_purge_slots_stats", None);
        self.latest_accounts_index_roots_stats.report();
    }
}

#[derive(Debug, Default)]
struct ShrinkAncientStats {
    shrink_stats: ShrinkStats,
    ancient_append_vecs_shrunk: AtomicU64,
    total_us: AtomicU64,
    random_shrink: AtomicU64,
    slots_considered: AtomicU64,
    ancient_scanned: AtomicU64,
}

#[derive(Debug, Default)]
pub(crate) struct ShrinkStats {
    last_report: AtomicInterval,
    num_slots_shrunk: AtomicUsize,
    storage_read_elapsed: AtomicU64,
    index_read_elapsed: AtomicU64,
    find_alive_elapsed: AtomicU64,
    create_and_insert_store_elapsed: AtomicU64,
    store_accounts_elapsed: AtomicU64,
    update_index_elapsed: AtomicU64,
    handle_reclaims_elapsed: AtomicU64,
    remove_old_stores_shrink_us: AtomicU64,
    rewrite_elapsed: AtomicU64,
    drop_storage_entries_elapsed: AtomicU64,
    recycle_stores_write_elapsed: AtomicU64,
    accounts_removed: AtomicUsize,
    bytes_removed: AtomicU64,
    bytes_written: AtomicU64,
    skipped_shrink: AtomicU64,
    dead_accounts: AtomicU64,
    alive_accounts: AtomicU64,
    accounts_loaded: AtomicU64,
}

impl ShrinkStats {
    fn report(&self) {
        if self.last_report.should_update(1000) {
            datapoint_info!(
                "shrink_stats",
                (
                    "num_slots_shrunk",
                    self.num_slots_shrunk.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "storage_read_elapsed",
                    self.storage_read_elapsed.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "index_read_elapsed",
                    self.index_read_elapsed.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "create_and_insert_store_elapsed",
                    self.create_and_insert_store_elapsed
                        .swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "store_accounts_elapsed",
                    self.store_accounts_elapsed.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "update_index_elapsed",
                    self.update_index_elapsed.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "handle_reclaims_elapsed",
                    self.handle_reclaims_elapsed.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "remove_old_stores_shrink_us",
                    self.remove_old_stores_shrink_us.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "rewrite_elapsed",
                    self.rewrite_elapsed.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "drop_storage_entries_elapsed",
                    self.drop_storage_entries_elapsed.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "recycle_stores_write_time",
                    self.recycle_stores_write_elapsed.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "accounts_removed",
                    self.accounts_removed.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "bytes_removed",
                    self.bytes_removed.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "bytes_written",
                    self.bytes_written.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "skipped_shrink",
                    self.skipped_shrink.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "alive_accounts",
                    self.alive_accounts.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "dead_accounts",
                    self.dead_accounts.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "accounts_loaded",
                    self.accounts_loaded.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
            );
        }
    }
}

impl ShrinkAncientStats {
    fn report(&self) {
        if self.shrink_stats.last_report.should_update(1000) {
            datapoint_info!(
                "shrink_ancient_stats",
                (
                    "num_slots_shrunk",
                    self.shrink_stats
                        .num_slots_shrunk
                        .swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "storage_read_elapsed",
                    self.shrink_stats
                        .storage_read_elapsed
                        .swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "index_read_elapsed",
                    self.shrink_stats
                        .index_read_elapsed
                        .swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "find_alive_elapsed",
                    self.shrink_stats
                        .find_alive_elapsed
                        .swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "create_and_insert_store_elapsed",
                    self.shrink_stats
                        .create_and_insert_store_elapsed
                        .swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "store_accounts_elapsed",
                    self.shrink_stats
                        .store_accounts_elapsed
                        .swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "update_index_elapsed",
                    self.shrink_stats
                        .update_index_elapsed
                        .swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "handle_reclaims_elapsed",
                    self.shrink_stats
                        .handle_reclaims_elapsed
                        .swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "remove_old_stores_shrink_us",
                    self.shrink_stats
                        .remove_old_stores_shrink_us
                        .swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "rewrite_elapsed",
                    self.shrink_stats.rewrite_elapsed.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "drop_storage_entries_elapsed",
                    self.shrink_stats
                        .drop_storage_entries_elapsed
                        .swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "recycle_stores_write_time",
                    self.shrink_stats
                        .recycle_stores_write_elapsed
                        .swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "accounts_removed",
                    self.shrink_stats
                        .accounts_removed
                        .swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "bytes_removed",
                    self.shrink_stats.bytes_removed.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "bytes_written",
                    self.shrink_stats.bytes_written.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "alive_accounts",
                    self.shrink_stats.alive_accounts.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "dead_accounts",
                    self.shrink_stats.dead_accounts.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "accounts_loaded",
                    self.shrink_stats.accounts_loaded.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "ancient_append_vecs_shrunk",
                    self.ancient_append_vecs_shrunk.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "random",
                    self.random_shrink.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "slots_considered",
                    self.slots_considered.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "ancient_scanned",
                    self.ancient_scanned.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "total_us",
                    self.total_us.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
            );
        }
    }
}

pub fn quarter_thread_count() -> usize {
    std::cmp::max(2, num_cpus::get() / 4)
}

pub fn make_min_priority_thread_pool() -> ThreadPool {
    // Use lower thread count to reduce priority.
    let num_threads = quarter_thread_count();
    rayon::ThreadPoolBuilder::new()
        .thread_name(|i| format!("solAccountsLo{i:02}"))
        .num_threads(num_threads)
        .build()
        .unwrap()
}

#[cfg(all(test, RUSTC_WITH_SPECIALIZATION))]
impl solana_frozen_abi::abi_example::AbiExample for AccountsDb {
    fn example() -> Self {
        let accounts_db = AccountsDb::new_single_for_tests();
        let key = Pubkey::default();
        let some_data_len = 5;
        let some_slot: Slot = 0;
        let account = AccountSharedData::new(1, some_data_len, &key);
        accounts_db.store_uncached(some_slot, &[(&key, &account)]);
        accounts_db.add_root(0);

        accounts_db
    }
}

impl<'a> ZeroLamport for StoredAccountMeta<'a> {
    fn is_zero_lamport(&self) -> bool {
        self.lamports() == 0
    }
}

impl<'a> ReadableAccount for StoredAccountMeta<'a> {
    fn lamports(&self) -> u64 {
        self.account_meta.lamports
    }
    fn data(&self) -> &[u8] {
        self.data
    }
    fn owner(&self) -> &Pubkey {
        &self.account_meta.owner
    }
    fn executable(&self) -> bool {
        self.account_meta.executable
    }
    fn rent_epoch(&self) -> Epoch {
        self.account_meta.rent_epoch
    }
}

struct IndexAccountMapEntry<'a> {
    pub write_version: StoredMetaWriteVersion,
    pub store_id: AppendVecId,
    pub stored_account: StoredAccountMeta<'a>,
}

type GenerateIndexAccountsMap<'a> = HashMap<Pubkey, IndexAccountMapEntry<'a>>;

/// called on a struct while scanning append vecs
trait AppendVecScan: Send + Sync + Clone {
    /// return true if this pubkey should be included
    fn filter(&mut self, pubkey: &Pubkey) -> bool;
    /// set current slot of the scan
    fn set_slot(&mut self, slot: Slot);
    /// found `account` in the append vec
    fn found_account(&mut self, account: &LoadedAccount);
    /// scanning is done
    fn scanning_complete(self) -> BinnedHashData;
    /// initialize accumulator
    fn init_accum(&mut self, count: usize);
    fn get_accum(&mut self) -> BinnedHashData;
    fn set_accum(&mut self, accum: BinnedHashData);
}

#[derive(Clone)]
/// state to keep while scanning append vec accounts for hash calculation
/// These would have been captured in a fn from within the scan function.
/// Some of these are constant across all pubkeys, some are constant across a slot.
/// Some could be unique per pubkey.
struct ScanState<'a> {
    /// slot we're currently scanning
    current_slot: Slot,
    /// accumulated results
    accum: BinnedHashData,
    bin_calculator: &'a PubkeyBinCalculator24,
    bin_range: &'a Range<usize>,
    config: &'a CalcAccountsHashConfig<'a>,
    mismatch_found: Arc<AtomicU64>,
    filler_account_suffix: Option<&'a Pubkey>,
    range: usize,
    sort_time: Arc<AtomicU64>,
    pubkey_to_bin_index: usize,
}

impl<'a> AppendVecScan for ScanState<'a> {
    fn set_slot(&mut self, slot: Slot) {
        self.current_slot = slot;
    }
    fn filter(&mut self, pubkey: &Pubkey) -> bool {
        self.pubkey_to_bin_index = self.bin_calculator.bin_from_pubkey(pubkey);
        self.bin_range.contains(&self.pubkey_to_bin_index)
    }
    fn init_accum(&mut self, count: usize) {
        if self.accum.is_empty() {
            self.accum.append(&mut vec![Vec::new(); count]);
        }
    }
    fn found_account(&mut self, loaded_account: &LoadedAccount) {
        let pubkey = loaded_account.pubkey();
        assert!(self.bin_range.contains(&self.pubkey_to_bin_index)); // get rid of this once we have confidence

        // when we are scanning with bin ranges, we don't need to use exact bin numbers. Subtract to make first bin we care about at index 0.
        self.pubkey_to_bin_index -= self.bin_range.start;

        let balance = loaded_account.lamports();
        let loaded_hash = loaded_account.loaded_hash();
        let source_item = CalculateHashIntermediate::new(loaded_hash, balance, *pubkey);

        if self.config.check_hash
            && !AccountsDb::is_filler_account_helper(pubkey, self.filler_account_suffix)
        {
            // this will not be supported anymore
            let computed_hash = loaded_account.compute_hash(
                self.current_slot,
                pubkey,
                INCLUDE_SLOT_IN_HASH_IRRELEVANT_CHECK_HASH,
            );
            if computed_hash != source_item.hash {
                info!(
                    "hash mismatch found: computed: {}, loaded: {}, pubkey: {}",
                    computed_hash, source_item.hash, pubkey
                );
                self.mismatch_found.fetch_add(1, Ordering::Relaxed);
            }
        }
        self.init_accum(self.range);
        self.accum[self.pubkey_to_bin_index].push(source_item);
    }
    fn scanning_complete(self) -> BinnedHashData {
        let (result, timing) = AccountsDb::sort_slot_storage_scan(self.accum);
        self.sort_time.fetch_add(timing, Ordering::Relaxed);
        result
    }
    fn get_accum(&mut self) -> BinnedHashData {
        std::mem::take(&mut self.accum)
    }
    fn set_accum(&mut self, accum: BinnedHashData) {
        self.accum = accum;
    }
}

impl AccountsDb {
    pub fn default_for_tests() -> Self {
        Self::default_with_accounts_index(AccountInfoAccountsIndex::default_for_tests(), None)
    }

    fn default_with_accounts_index(
        accounts_index: AccountInfoAccountsIndex,
        accounts_hash_cache_path: Option<PathBuf>,
    ) -> Self {
        let num_threads = get_thread_count();
        const MAX_READ_ONLY_CACHE_DATA_SIZE: usize = 400_000_000; // 400M bytes

        let mut temp_accounts_hash_cache_path = None;
        let accounts_hash_cache_path = accounts_hash_cache_path.unwrap_or_else(|| {
            temp_accounts_hash_cache_path = Some(TempDir::new().unwrap());
            temp_accounts_hash_cache_path
                .as_ref()
                .unwrap()
                .path()
                .to_path_buf()
        });

        let mut bank_hashes = HashMap::new();
        bank_hashes.insert(0, BankHashInfo::default());

        // Increase the stack for accounts threads
        // rayon needs a lot of stack
        const ACCOUNTS_STACK_SIZE: usize = 8 * 1024 * 1024;

        AccountsDb {
            verify_accounts_hash_in_bg: VerifyAccountsHashInBackground::default(),
            filler_accounts_per_slot: AtomicU64::default(),
            filler_account_slots_remaining: AtomicU64::default(),
            active_stats: ActiveStats::default(),
            accounts_hash_complete_one_epoch_old: RwLock::default(),
            skip_initial_hash_calc: false,
            ancient_append_vec_offset: None,
            accounts_index,
            storage: AccountStorage::default(),
            accounts_cache: AccountsCache::default(),
            sender_bg_hasher: None,
            read_only_accounts_cache: ReadOnlyAccountsCache::new(MAX_READ_ONLY_CACHE_DATA_SIZE),
            recycle_stores: RwLock::new(RecycleStores::default()),
            uncleaned_pubkeys: DashMap::new(),
            next_id: AtomicAppendVecId::new(0),
            shrink_candidate_slots: Mutex::new(HashMap::new()),
            write_cache_limit_bytes: None,
            write_version: AtomicU64::new(0),
            paths: vec![],
            accounts_hash_cache_path,
            temp_accounts_hash_cache_path,
            shrink_paths: RwLock::new(None),
            temp_paths: None,
            file_size: DEFAULT_FILE_SIZE,
            thread_pool: rayon::ThreadPoolBuilder::new()
                .num_threads(num_threads)
                .thread_name(|i| format!("solAccounts{i:02}"))
                .stack_size(ACCOUNTS_STACK_SIZE)
                .build()
                .unwrap(),
            thread_pool_clean: make_min_priority_thread_pool(),
            bank_hashes: RwLock::new(bank_hashes),
            external_purge_slots_stats: PurgeStats::default(),
            clean_accounts_stats: CleanAccountsStats::default(),
            shrink_stats: ShrinkStats::default(),
            shrink_ancient_stats: ShrinkAncientStats::default(),
            stats: AccountsStats::default(),
            cluster_type: None,
            account_indexes: AccountSecondaryIndexes::default(),
            #[cfg(test)]
            load_delay: u64::default(),
            #[cfg(test)]
            load_limit: AtomicU64::default(),
            is_bank_drop_callback_enabled: AtomicBool::default(),
            remove_unrooted_slots_synchronization: RemoveUnrootedSlotsSynchronization::default(),
            shrink_ratio: AccountShrinkThreshold::default(),
            dirty_stores: DashMap::default(),
            zero_lamport_accounts_to_purge_after_full_snapshot: DashSet::default(),
            accounts_update_notifier: None,
            filler_accounts_config: FillerAccountsConfig::default(),
            filler_account_suffix: None,
            log_dead_slots: AtomicBool::new(true),
            exhaustively_verify_refcounts: false,
            epoch_accounts_hash_manager: EpochAccountsHashManager::new_invalid(),
        }
    }

    pub fn new_for_tests(paths: Vec<PathBuf>, cluster_type: &ClusterType) -> Self {
        AccountsDb::new_with_config(
            paths,
            cluster_type,
            AccountSecondaryIndexes::default(),
            AccountShrinkThreshold::default(),
            Some(ACCOUNTS_DB_CONFIG_FOR_TESTING),
            None,
            &Arc::default(),
        )
    }

    pub fn new_for_tests_with_caching(paths: Vec<PathBuf>, cluster_type: &ClusterType) -> Self {
        AccountsDb::new_with_config(
            paths,
            cluster_type,
            AccountSecondaryIndexes::default(),
            AccountShrinkThreshold::default(),
            Some(ACCOUNTS_DB_CONFIG_FOR_TESTING),
            None,
            &Arc::default(),
        )
    }

    pub fn new_with_config(
        paths: Vec<PathBuf>,
        cluster_type: &ClusterType,
        account_indexes: AccountSecondaryIndexes,
        shrink_ratio: AccountShrinkThreshold,
        mut accounts_db_config: Option<AccountsDbConfig>,
        accounts_update_notifier: Option<AccountsUpdateNotifier>,
        exit: &Arc<AtomicBool>,
    ) -> Self {
        let accounts_index = AccountsIndex::new(
            accounts_db_config.as_mut().and_then(|x| x.index.take()),
            exit,
        );
        let accounts_hash_cache_path = accounts_db_config
            .as_ref()
            .and_then(|x| x.accounts_hash_cache_path.clone());

        let filler_accounts_config = accounts_db_config
            .as_ref()
            .map(|config| config.filler_accounts_config)
            .unwrap_or_default();
        let skip_initial_hash_calc = accounts_db_config
            .as_ref()
            .map(|config| config.skip_initial_hash_calc)
            .unwrap_or_default();

        let ancient_append_vec_offset = accounts_db_config
            .as_ref()
            .map(|config| config.ancient_append_vec_offset)
            .unwrap_or_default();

        let exhaustively_verify_refcounts = accounts_db_config
            .as_ref()
            .map(|config| config.exhaustively_verify_refcounts)
            .unwrap_or_default();

        let filler_account_suffix = if filler_accounts_config.count > 0 {
            Some(solana_sdk::pubkey::new_rand())
        } else {
            None
        };
        let paths_is_empty = paths.is_empty();
        let mut new = Self {
            paths,
            skip_initial_hash_calc,
            ancient_append_vec_offset,
            cluster_type: Some(*cluster_type),
            account_indexes,
            shrink_ratio,
            accounts_update_notifier,
            filler_accounts_config,
            filler_account_suffix,
            write_cache_limit_bytes: accounts_db_config
                .as_ref()
                .and_then(|x| x.write_cache_limit_bytes),
            exhaustively_verify_refcounts,
            ..Self::default_with_accounts_index(accounts_index, accounts_hash_cache_path)
        };
        if paths_is_empty {
            // Create a temporary set of accounts directories, used primarily
            // for testing
            let (temp_dirs, paths) = get_temp_accounts_paths(DEFAULT_NUM_DIRS).unwrap();
            new.accounts_update_notifier = None;
            new.paths = paths;
            new.temp_paths = Some(temp_dirs);
        };

        new.start_background_hasher();
        {
            for path in new.paths.iter() {
                std::fs::create_dir_all(path).expect("Create directory failed.");
            }
        }
        new
    }

    /// Gradual means filler accounts will be added over the course of an epoch, during cache flush.
    /// This is in contrast to adding all the filler accounts immediately before the validator starts.
    fn init_gradual_filler_accounts(&self, slots_per_epoch: Slot) {
        let count = self.filler_accounts_config.count;
        if count > 0 {
            // filler accounts are a debug only feature. integer division is fine here
            let accounts_per_slot = (count as u64) / slots_per_epoch;
            self.filler_accounts_per_slot
                .store(accounts_per_slot, Ordering::Release);
            self.filler_account_slots_remaining
                .store(slots_per_epoch, Ordering::Release);
        }
    }

    pub fn set_shrink_paths(&self, paths: Vec<PathBuf>) {
        assert!(!paths.is_empty());
        let mut shrink_paths = self.shrink_paths.write().unwrap();
        for path in &paths {
            std::fs::create_dir_all(path).expect("Create directory failed.");
        }
        *shrink_paths = Some(paths);
    }

    pub fn file_size(&self) -> u64 {
        self.file_size
    }

    pub fn new_single_for_tests() -> Self {
        AccountsDb::new_for_tests(Vec::new(), &ClusterType::Development)
    }

    pub fn new_single_for_tests_with_caching() -> Self {
        AccountsDb::new_for_tests_with_caching(Vec::new(), &ClusterType::Development)
    }

    fn next_id(&self) -> AppendVecId {
        let next_id = self.next_id.fetch_add(1, Ordering::AcqRel);
        assert!(next_id != AppendVecId::MAX, "We've run out of storage ids!");
        next_id
    }

    fn new_storage_entry(&self, slot: Slot, path: &Path, size: u64) -> AccountStorageEntry {
        AccountStorageEntry::new(path, slot, self.next_id(), size)
    }

    pub fn expected_cluster_type(&self) -> ClusterType {
        self.cluster_type
            .expect("Cluster type must be set at initialization")
    }

    /// Reclaim older states of accounts older than max_clean_root_inclusive for AccountsDb bloat mitigation.
    /// Any accounts which are removed from the accounts index are returned in PubkeysRemovedFromAccountsIndex.
    /// These should NOT be unref'd later from the accounts index.
    fn clean_accounts_older_than_root(
        &self,
        purges: Vec<Pubkey>,
        max_clean_root_inclusive: Option<Slot>,
        ancient_account_cleans: &AtomicU64,
    ) -> (ReclaimResult, PubkeysRemovedFromAccountsIndex) {
        let pubkeys_removed_from_accounts_index = HashSet::default();
        if purges.is_empty() {
            return (
                ReclaimResult::default(),
                pubkeys_removed_from_accounts_index,
            );
        }
        // This number isn't carefully chosen; just guessed randomly such that
        // the hot loop will be the order of ~Xms.
        const INDEX_CLEAN_BULK_COUNT: usize = 4096;

        let one_epoch_old = self.get_accounts_hash_complete_one_epoch_old();
        let pubkeys_removed_from_accounts_index = Mutex::new(pubkeys_removed_from_accounts_index);

        let mut clean_rooted = Measure::start("clean_old_root-ms");
        let reclaim_vecs = purges
            .par_chunks(INDEX_CLEAN_BULK_COUNT)
            .filter_map(|pubkeys: &[Pubkey]| {
                let mut reclaims = Vec::new();
                for pubkey in pubkeys {
                    let removed_from_index = self.accounts_index.clean_rooted_entries(
                        pubkey,
                        &mut reclaims,
                        max_clean_root_inclusive,
                    );
                    if removed_from_index {
                        pubkeys_removed_from_accounts_index
                            .lock()
                            .unwrap()
                            .insert(*pubkey);
                    }
                }

                (!reclaims.is_empty()).then(|| {
                    // figure out how many ancient accounts have been reclaimed
                    let old_reclaims = reclaims
                        .iter()
                        .filter_map(|(slot, _)| (slot < &one_epoch_old).then_some(1))
                        .sum();
                    ancient_account_cleans.fetch_add(old_reclaims, Ordering::Relaxed);
                    reclaims
                })
            })
            .collect::<Vec<_>>();
        clean_rooted.stop();
        let pubkeys_removed_from_accounts_index =
            pubkeys_removed_from_accounts_index.into_inner().unwrap();
        self.clean_accounts_stats
            .clean_old_root_us
            .fetch_add(clean_rooted.as_us(), Ordering::Relaxed);

        let mut measure = Measure::start("clean_old_root_reclaims");

        // Don't reset from clean, since the pubkeys in those stores may need to be unref'ed
        // and those stores may be used for background hashing.
        let reset_accounts = false;

        let mut reclaim_result = ReclaimResult::default();
        self.handle_reclaims(
            (!reclaim_vecs.is_empty()).then(|| reclaim_vecs.iter().flatten()),
            None,
            Some((&self.clean_accounts_stats.purge_stats, &mut reclaim_result)),
            reset_accounts,
            &pubkeys_removed_from_accounts_index,
        );
        measure.stop();
        debug!("{} {}", clean_rooted, measure);
        self.clean_accounts_stats
            .clean_old_root_reclaim_us
            .fetch_add(measure.as_us(), Ordering::Relaxed);
        (reclaim_result, pubkeys_removed_from_accounts_index)
    }

    fn do_reset_uncleaned_roots(&self, max_clean_root: Option<Slot>) {
        let mut measure = Measure::start("reset");
        self.accounts_index.reset_uncleaned_roots(max_clean_root);
        measure.stop();
        self.clean_accounts_stats
            .reset_uncleaned_roots_us
            .fetch_add(measure.as_us(), Ordering::Relaxed);
    }

    /// increment store_counts to non-zero for all stores that can not be deleted.
    /// a store cannot be deleted if:
    /// 1. one of the pubkeys in the store has account info to a store whose store count is not going to zero
    /// 2. a pubkey we were planning to remove is not removing all stores that contain the account
    fn calc_delete_dependencies(
        purges: &HashMap<Pubkey, (SlotList<AccountInfo>, RefCount)>,
        store_counts: &mut HashMap<AppendVecId, (usize, HashSet<Pubkey>)>,
        min_store_id: Option<AppendVecId>,
    ) {
        // Another pass to check if there are some filtered accounts which
        // do not match the criteria of deleting all appendvecs which contain them
        // then increment their storage count.
        let mut already_counted = HashSet::new();
        for (pubkey, (account_infos, ref_count_from_storage)) in purges.iter() {
            let mut failed_store_id = None;
            let all_stores_being_deleted =
                account_infos.len() as RefCount == *ref_count_from_storage;
            if all_stores_being_deleted {
                let mut delete = true;
                for (_slot, account_info) in account_infos {
                    let store_id = account_info.store_id();
                    if let Some(count) = store_counts.get(&store_id).map(|s| s.0) {
                        debug!(
                            "calc_delete_dependencies()
                            storage id: {},
                            count len: {}",
                            store_id, count,
                        );
                        if count == 0 {
                            // this store CAN be removed
                            continue;
                        }
                    }
                    // One of the pubkeys in the store has account info to a store whose store count is not going to zero.
                    // If the store cannot be found, that also means store isn't being deleted.
                    failed_store_id = Some(store_id);
                    delete = false;
                    break;
                }
                if delete {
                    // this pubkey can be deleted from all stores it is in
                    continue;
                }
            } else {
                // a pubkey we were planning to remove is not removing all stores that contain the account
                debug!(
                    "calc_delete_dependencies(),
                    pubkey: {},
                    account_infos: {:?},
                    account_infos_len: {},
                    ref_count_from_storage: {}",
                    pubkey,
                    account_infos,
                    account_infos.len(),
                    ref_count_from_storage,
                );
            }

            // increment store_counts to non-zero for all stores that can not be deleted.
            let mut pending_store_ids = HashSet::new();
            for (_slot, account_info) in account_infos {
                if !already_counted.contains(&account_info.store_id()) {
                    pending_store_ids.insert(account_info.store_id());
                }
            }
            while !pending_store_ids.is_empty() {
                let id = pending_store_ids.iter().next().cloned().unwrap();
                if Some(id) == min_store_id {
                    if let Some(failed_store_id) = failed_store_id.take() {
                        info!("calc_delete_dependencies, oldest store is not able to be deleted because of {pubkey} in store {failed_store_id}");
                    } else {
                        info!("calc_delete_dependencies, oldest store is not able to be deleted because of {pubkey}, account infos len: {}, ref count: {ref_count_from_storage}", account_infos.len());
                    }
                }

                pending_store_ids.remove(&id);
                if !already_counted.insert(id) {
                    continue;
                }
                // the point of all this code: remove the store count for all stores we cannot remove
                if let Some(store_count) = store_counts.remove(&id) {
                    // all pubkeys in this store also cannot be removed from all stores they are in
                    let affected_pubkeys = &store_count.1;
                    for key in affected_pubkeys {
                        for (_slot, account_info) in &purges.get(key).unwrap().0 {
                            if !already_counted.contains(&account_info.store_id()) {
                                pending_store_ids.insert(account_info.store_id());
                            }
                        }
                    }
                }
            }
        }
    }

    fn background_hasher(receiver: Receiver<CachedAccount>) {
        loop {
            let result = receiver.recv();
            match result {
                Ok(account) => {
                    // if we hold the only ref, then this account doesn't need to be hashed, we ignore this account and it will disappear
                    if Arc::strong_count(&account) > 1 {
                        // this will cause the hash to be calculated and store inside account if it needs to be calculated
                        let _ = (*account).hash();
                    };
                }
                Err(_) => {
                    break;
                }
            }
        }
    }

    fn start_background_hasher(&mut self) {
        let (sender, receiver) = unbounded();
        Builder::new()
            .name("solDbStoreHashr".to_string())
            .spawn(move || {
                Self::background_hasher(receiver);
            })
            .unwrap();
        self.sender_bg_hasher = Some(sender);
    }

    #[must_use]
    pub(crate) fn purge_keys_exact<'a, C: 'a>(
        &'a self,
        pubkey_to_slot_set: impl Iterator<Item = &'a (Pubkey, C)>,
    ) -> (Vec<(Slot, AccountInfo)>, PubkeysRemovedFromAccountsIndex)
    where
        C: Contains<'a, Slot>,
    {
        let mut reclaims = Vec::new();
        let mut dead_keys = Vec::new();

        for (pubkey, slots_set) in pubkey_to_slot_set {
            let is_empty = self
                .accounts_index
                .purge_exact(pubkey, slots_set, &mut reclaims);
            if is_empty {
                dead_keys.push(pubkey);
            }
        }

        let pubkeys_removed_from_accounts_index = self
            .accounts_index
            .handle_dead_keys(&dead_keys, &self.account_indexes);
        (reclaims, pubkeys_removed_from_accounts_index)
    }

    fn max_clean_root(&self, proposed_clean_root: Option<Slot>) -> Option<Slot> {
        match (
            self.accounts_index.min_ongoing_scan_root(),
            proposed_clean_root,
        ) {
            (None, None) => None,
            (Some(min_scan_root), None) => Some(min_scan_root),
            (None, Some(proposed_clean_root)) => Some(proposed_clean_root),
            (Some(min_scan_root), Some(proposed_clean_root)) => {
                Some(std::cmp::min(min_scan_root, proposed_clean_root))
            }
        }
    }

    /// return 'slot' - slots_in_epoch
    fn get_slot_one_epoch_prior(slot: Slot, epoch_schedule: &EpochSchedule) -> Slot {
        // would like to use:
        // slot.saturating_sub(epoch_schedule.get_slots_in_epoch(epoch_schedule.get_epoch(slot)))
        // but there are problems with warmup and such on tests and probably test clusters.
        // So, just use the maximum below (epoch_schedule.slots_per_epoch)
        slot.saturating_sub(epoch_schedule.slots_per_epoch)
    }

    /// hash calc is completed as of 'slot'
    /// so, any process that wants to take action on really old slots can now proceed up to 'completed_slot'-slots per epoch
    pub fn notify_accounts_hash_calculated_complete(
        &self,
        completed_slot: Slot,
        epoch_schedule: &EpochSchedule,
    ) {
        let one_epoch_old_slot = Self::get_slot_one_epoch_prior(completed_slot, epoch_schedule);
        let mut accounts_hash_complete_one_epoch_old =
            self.accounts_hash_complete_one_epoch_old.write().unwrap();
        *accounts_hash_complete_one_epoch_old =
            std::cmp::max(*accounts_hash_complete_one_epoch_old, one_epoch_old_slot);
        let accounts_hash_complete_one_epoch_old = *accounts_hash_complete_one_epoch_old;

        // now that accounts hash calculation is complete, we can remove old historical roots
        self.remove_old_historical_roots(accounts_hash_complete_one_epoch_old);
    }

    /// get the slot that is one epoch older than the highest slot that has been used for hash calculation
    fn get_accounts_hash_complete_one_epoch_old(&self) -> Slot {
        *self.accounts_hash_complete_one_epoch_old.read().unwrap()
    }

    /// Collect all the uncleaned slots, up to a max slot
    ///
    /// Search through the uncleaned Pubkeys and return all the slots, up to a maximum slot.
    fn collect_uncleaned_slots_up_to_slot(&self, max_slot_inclusive: Slot) -> Vec<Slot> {
        self.uncleaned_pubkeys
            .iter()
            .filter_map(|entry| {
                let slot = *entry.key();
                (slot <= max_slot_inclusive).then_some(slot)
            })
            .collect()
    }

    /// Remove `slots` from `uncleaned_pubkeys` and collect all pubkeys
    ///
    /// For each slot in the list of uncleaned slots, remove it from the `uncleaned_pubkeys` Map
    /// and collect all the pubkeys to return.
    fn remove_uncleaned_slots_and_collect_pubkeys(
        &self,
        uncleaned_slots: Vec<Slot>,
    ) -> Vec<Vec<Pubkey>> {
        uncleaned_slots
            .into_iter()
            .filter_map(|uncleaned_slot| {
                self.uncleaned_pubkeys
                    .remove(&uncleaned_slot)
                    .map(|(_removed_slot, removed_pubkeys)| removed_pubkeys)
            })
            .collect()
    }

    /// Remove uncleaned slots, up to a maximum slot, and return the collected pubkeys
    ///
    fn remove_uncleaned_slots_and_collect_pubkeys_up_to_slot(
        &self,
        max_slot_inclusive: Slot,
    ) -> Vec<Vec<Pubkey>> {
        let uncleaned_slots = self.collect_uncleaned_slots_up_to_slot(max_slot_inclusive);
        self.remove_uncleaned_slots_and_collect_pubkeys(uncleaned_slots)
    }

    // Construct a vec of pubkeys for cleaning from:
    //   uncleaned_pubkeys - the delta set of updated pubkeys in rooted slots from the last clean
    //   dirty_stores - set of stores which had accounts removed or recently rooted
    fn construct_candidate_clean_keys(
        &self,
        max_clean_root_inclusive: Option<Slot>,
        is_startup: bool,
        last_full_snapshot_slot: Option<Slot>,
        timings: &mut CleanKeyTimings,
    ) -> (Vec<Pubkey>, Option<AppendVecId>) {
        let mut dirty_store_processing_time = Measure::start("dirty_store_processing");
        let max_slot_inclusive =
            max_clean_root_inclusive.unwrap_or_else(|| self.accounts_index.max_root_inclusive());
        let mut dirty_stores = Vec::with_capacity(self.dirty_stores.len());
        // find the oldest append vec older than one epoch old
        // we'll add logging if that append vec cannot be marked dead
        let mut min_dirty_slot = self.get_accounts_hash_complete_one_epoch_old();
        let mut min_dirty_store_id = None;
        self.dirty_stores.retain(|(slot, store_id), store| {
            if *slot > max_slot_inclusive {
                true
            } else {
                if *slot < min_dirty_slot {
                    min_dirty_slot = *slot;
                    min_dirty_store_id = Some(*store_id);
                }
                dirty_stores.push((*slot, store.clone()));
                false
            }
        });
        let dirty_stores_len = dirty_stores.len();
        let pubkeys = DashSet::new();
        let dirty_ancient_stores = AtomicUsize::default();
        let mut dirty_store_routine = || {
            let chunk_size = 1.max(dirty_stores_len.saturating_div(rayon::current_num_threads()));
            let oldest_dirty_slots: Vec<u64> = dirty_stores
                .par_chunks(chunk_size)
                .map(|dirty_store_chunk| {
                    let mut oldest_dirty_slot = max_slot_inclusive.saturating_add(1);
                    dirty_store_chunk.iter().for_each(|(slot, store)| {
                        if is_ancient(&store.accounts) {
                            dirty_ancient_stores.fetch_add(1, Ordering::Relaxed);
                        }
                        oldest_dirty_slot = oldest_dirty_slot.min(*slot);
                        store.accounts.account_iter().for_each(|account| {
                            pubkeys.insert(*account.pubkey());
                        });
                    });
                    oldest_dirty_slot
                })
                .collect();
            timings.oldest_dirty_slot = *oldest_dirty_slots
                .iter()
                .min()
                .unwrap_or(&max_slot_inclusive.saturating_add(1));
        };

        if is_startup {
            // Free to consume all the cores during startup
            dirty_store_routine();
        } else {
            self.thread_pool_clean.install(|| {
                dirty_store_routine();
            });
        }
        trace!(
            "dirty_stores.len: {} pubkeys.len: {}",
            dirty_stores_len,
            pubkeys.len()
        );
        timings.dirty_pubkeys_count = pubkeys.len() as u64;
        dirty_store_processing_time.stop();
        timings.dirty_store_processing_us += dirty_store_processing_time.as_us();
        timings.dirty_ancient_stores = dirty_ancient_stores.load(Ordering::Relaxed);

        let mut collect_delta_keys = Measure::start("key_create");
        let delta_keys =
            self.remove_uncleaned_slots_and_collect_pubkeys_up_to_slot(max_slot_inclusive);
        collect_delta_keys.stop();
        timings.collect_delta_keys_us += collect_delta_keys.as_us();

        let mut delta_insert = Measure::start("delta_insert");
        self.thread_pool_clean.install(|| {
            delta_keys.par_iter().for_each(|keys| {
                for key in keys {
                    pubkeys.insert(*key);
                }
            });
        });
        delta_insert.stop();
        timings.delta_insert_us += delta_insert.as_us();

        timings.delta_key_count = pubkeys.len() as u64;

        let mut hashset_to_vec = Measure::start("flat_map");
        let mut pubkeys: Vec<Pubkey> = pubkeys.into_iter().collect();
        hashset_to_vec.stop();
        timings.hashset_to_vec_us += hashset_to_vec.as_us();

        // Check if we should purge any of the zero_lamport_accounts_to_purge_later, based on the
        // last_full_snapshot_slot.
        assert!(
            last_full_snapshot_slot.is_some() || self.zero_lamport_accounts_to_purge_after_full_snapshot.is_empty(),
            "if snapshots are disabled, then zero_lamport_accounts_to_purge_later should always be empty"
        );
        if let Some(last_full_snapshot_slot) = last_full_snapshot_slot {
            self.zero_lamport_accounts_to_purge_after_full_snapshot
                .retain(|(slot, pubkey)| {
                    let is_candidate_for_clean =
                        max_slot_inclusive >= *slot && last_full_snapshot_slot >= *slot;
                    if is_candidate_for_clean {
                        pubkeys.push(*pubkey);
                    }
                    !is_candidate_for_clean
                });
        }

        (pubkeys, min_dirty_store_id)
    }

    /// Call clean_accounts() with the common parameters that tests/benches use.
    pub fn clean_accounts_for_tests(&self) {
        self.clean_accounts(None, false, None)
    }

    /// called with cli argument to verify refcounts are correct on all accounts
    /// this is very slow
    fn exhaustively_verify_refcounts(&self, max_slot_inclusive: Option<Slot>) {
        let max_slot_inclusive =
            max_slot_inclusive.unwrap_or_else(|| self.accounts_index.max_root_inclusive());
        info!("exhaustively verifying refcounts as of slot: {max_slot_inclusive}");
        let pubkey_refcount = DashMap::<Pubkey, Vec<Slot>>::default();
        let slots = self.storage.all_slots();
        // populate
        slots.into_par_iter().for_each(|slot| {
            if slot > max_slot_inclusive {
                return;
            }
            if let Some(storage) = self.storage.get_slot_storage_entry(slot) {
                storage.all_accounts().iter().for_each(|account| {
                    let pk = account.pubkey();
                    match pubkey_refcount.entry(*pk) {
                        dashmap::mapref::entry::Entry::Occupied(mut occupied_entry) => {
                            if !occupied_entry.get().iter().any(|s| s == &slot) {
                                occupied_entry.get_mut().push(slot);
                            }
                        }
                        dashmap::mapref::entry::Entry::Vacant(vacant_entry) => {
                            vacant_entry.insert(vec![slot]);
                        }
                    }
                });
            }
        });
        let total = pubkey_refcount.len();
        let failed = AtomicBool::default();
        let threads = quarter_thread_count();
        let per_batch = total / threads;
        (0..=threads).into_par_iter().for_each(|attempt| {
                pubkey_refcount.iter().skip(attempt * per_batch).take(per_batch).for_each(|entry| {
                    if failed.load(Ordering::Relaxed) {
                        return;
                    }
                    if let Some(idx) = self.accounts_index.get_account_read_entry(entry.key()) {
                        match (idx.ref_count() as usize).cmp(&entry.value().len()) {
                            std::cmp::Ordering::Greater => {
                            let list = idx.slot_list();
                            let too_new = list.iter().filter_map(|(slot, _)| (slot > &max_slot_inclusive).then_some(())).count();

                            if ((idx.ref_count() as usize) - too_new) > entry.value().len() {
                                failed.store(true, Ordering::Relaxed);
                                error!("exhaustively_verify_refcounts: {} refcount too large: {}, should be: {}, {:?}, {:?}, original: {:?}, too_new: {too_new}", entry.key(), idx.ref_count(), entry.value().len(), *entry.value(), list, idx.slot_list());
                            }
                        }
                        std::cmp::Ordering::Less => {
                            error!("exhaustively_verify_refcounts: {} refcount too small: {}, should be: {}, {:?}, {:?}", entry.key(), idx.ref_count(), entry.value().len(), *entry.value(), idx.slot_list());
                        }
                        _ => {}
                    }
                    }
                });
            });
        if failed.load(Ordering::Relaxed) {
            panic!("exhaustively_verify_refcounts failed");
        }
    }

    // Purge zero lamport accounts and older rooted account states as garbage
    // collection
    // Only remove those accounts where the entire rooted history of the account
    // can be purged because there are no live append vecs in the ancestors
    pub fn clean_accounts(
        &self,
        max_clean_root_inclusive: Option<Slot>,
        is_startup: bool,
        last_full_snapshot_slot: Option<Slot>,
    ) {
        if self.exhaustively_verify_refcounts {
            self.exhaustively_verify_refcounts(max_clean_root_inclusive);
        }

        let _guard = self.active_stats.activate(ActiveStatItem::Clean);

        let ancient_account_cleans = AtomicU64::default();

        let mut measure_all = Measure::start("clean_accounts");
        let max_clean_root_inclusive = self.max_clean_root(max_clean_root_inclusive);

        self.report_store_stats();

        let mut key_timings = CleanKeyTimings::default();
        let (mut pubkeys, min_dirty_store_id) = self.construct_candidate_clean_keys(
            max_clean_root_inclusive,
            is_startup,
            last_full_snapshot_slot,
            &mut key_timings,
        );

        let mut sort = Measure::start("sort");
        if is_startup {
            pubkeys.par_sort_unstable();
        } else {
            self.thread_pool_clean
                .install(|| pubkeys.par_sort_unstable());
        }
        sort.stop();

        let total_keys_count = pubkeys.len();
        let mut accounts_scan = Measure::start("accounts_scan");
        let uncleaned_roots = self.accounts_index.clone_uncleaned_roots();
        let found_not_zero_accum = AtomicU64::new(0);
        let not_found_on_fork_accum = AtomicU64::new(0);
        let missing_accum = AtomicU64::new(0);
        let useful_accum = AtomicU64::new(0);

        // parallel scan the index.
        let (mut purges_zero_lamports, purges_old_accounts) = {
            let do_clean_scan = || {
                pubkeys
                    .par_chunks(4096)
                    .map(|pubkeys: &[Pubkey]| {
                        let mut purges_zero_lamports = HashMap::new();
                        let mut purges_old_accounts = Vec::new();
                        let mut found_not_zero = 0;
                        let mut not_found_on_fork = 0;
                        let mut missing = 0;
                        let mut useful = 0;
                        self.accounts_index.scan(
                            pubkeys.iter(),
                            |pubkey, slots_refs| {
                                let mut useless = true;
                                if let Some((slot_list, ref_count)) = slots_refs {
                                    let index_in_slot_list = self.accounts_index.latest_slot(
                                        None,
                                        slot_list,
                                        max_clean_root_inclusive,
                                    );

                                    match index_in_slot_list {
                                        Some(index_in_slot_list) => {
                                            // found info relative to max_clean_root
                                            let (slot, account_info) =
                                                &slot_list[index_in_slot_list];
                                            if account_info.is_zero_lamport() {
                                                useless = false;
                                                purges_zero_lamports.insert(
                                                    *pubkey,
                                                    (
                                                        self.accounts_index.get_rooted_entries(
                                                            slot_list,
                                                            max_clean_root_inclusive,
                                                        ),
                                                        ref_count,
                                                    ),
                                                );
                                            } else {
                                                found_not_zero += 1;
                                            }
                                            if uncleaned_roots.contains(slot) {
                                                // Assertion enforced by `accounts_index.get()`, the latest slot
                                                // will not be greater than the given `max_clean_root`
                                                if let Some(max_clean_root_inclusive) =
                                                    max_clean_root_inclusive
                                                {
                                                    assert!(slot <= &max_clean_root_inclusive);
                                                }
                                                purges_old_accounts.push(*pubkey);
                                                useless = false;
                                            }
                                        }
                                        None => {
                                            // This pubkey is in the index but not in a root slot, so clean
                                            // it up by adding it to the to-be-purged list.
                                            //
                                            // Also, this pubkey must have been touched by some slot since
                                            // it was in the dirty list, so we assume that the slot it was
                                            // touched in must be unrooted.
                                            not_found_on_fork += 1;
                                            useless = false;
                                            purges_old_accounts.push(*pubkey);
                                        }
                                    }
                                } else {
                                    missing += 1;
                                }
                                if !useless {
                                    useful += 1;
                                }
                                if useless {
                                    AccountsIndexScanResult::None
                                } else {
                                    AccountsIndexScanResult::KeepInMemory
                                }
                            },
                            None,
                        );
                        found_not_zero_accum.fetch_add(found_not_zero, Ordering::Relaxed);
                        not_found_on_fork_accum.fetch_add(not_found_on_fork, Ordering::Relaxed);
                        missing_accum.fetch_add(missing, Ordering::Relaxed);
                        useful_accum.fetch_add(useful, Ordering::Relaxed);
                        (purges_zero_lamports, purges_old_accounts)
                    })
                    .reduce(
                        || (HashMap::new(), Vec::new()),
                        |mut m1, m2| {
                            // Collapse down the hashmaps/vecs into one.
                            m1.0.extend(m2.0);
                            m1.1.extend(m2.1);
                            m1
                        },
                    )
            };
            if is_startup {
                do_clean_scan()
            } else {
                self.thread_pool_clean.install(do_clean_scan)
            }
        };
        accounts_scan.stop();

        let mut clean_old_rooted = Measure::start("clean_old_roots");
        let ((purged_account_slots, removed_accounts), mut pubkeys_removed_from_accounts_index) =
            self.clean_accounts_older_than_root(
                purges_old_accounts,
                max_clean_root_inclusive,
                &ancient_account_cleans,
            );

        self.do_reset_uncleaned_roots(max_clean_root_inclusive);
        clean_old_rooted.stop();

        let mut store_counts_time = Measure::start("store_counts");

        // Calculate store counts as if everything was purged
        // Then purge if we can
        let mut store_counts: HashMap<AppendVecId, (usize, HashSet<Pubkey>)> = HashMap::new();
        for (key, (account_infos, ref_count)) in purges_zero_lamports.iter_mut() {
            if purged_account_slots.contains_key(key) {
                *ref_count = self.accounts_index.ref_count_from_storage(key);
            }
            account_infos.retain(|(slot, account_info)| {
                let was_slot_purged = purged_account_slots
                    .get(key)
                    .map(|slots_removed| slots_removed.contains(slot))
                    .unwrap_or(false);
                if was_slot_purged {
                    // No need to look up the slot storage below if the entire
                    // slot was purged
                    return false;
                }
                // Check if this update in `slot` to the account with `key` was reclaimed earlier by
                // `clean_accounts_older_than_root()`
                let was_reclaimed = removed_accounts
                    .get(&account_info.store_id())
                    .map(|store_removed| store_removed.contains(&account_info.offset()))
                    .unwrap_or(false);
                if was_reclaimed {
                    return false;
                }
                if let Some(store_count) = store_counts.get_mut(&account_info.store_id()) {
                    store_count.0 -= 1;
                    store_count.1.insert(*key);
                } else {
                    let mut key_set = HashSet::new();
                    key_set.insert(*key);
                    assert!(
                        !account_info.is_cached(),
                        "The Accounts Cache must be flushed first for this account info. pubkey: {}, slot: {}",
                        *key,
                        *slot
                    );
                    let count = self
                        .storage
                        .get_account_storage_entry(*slot, account_info.store_id())
                        .map(|store| store.count())
                        .unwrap()
                        - 1;
                    debug!(
                        "store_counts, inserting slot: {}, store id: {}, count: {}",
                        slot, account_info.store_id(), count
                    );
                    store_counts.insert(account_info.store_id(), (count, key_set));
                }
                true
            });
        }
        store_counts_time.stop();

        let mut calc_deps_time = Measure::start("calc_deps");
        Self::calc_delete_dependencies(
            &purges_zero_lamports,
            &mut store_counts,
            min_dirty_store_id,
        );
        calc_deps_time.stop();

        let mut purge_filter = Measure::start("purge_filter");
        self.filter_zero_lamport_clean_for_incremental_snapshots(
            max_clean_root_inclusive,
            last_full_snapshot_slot,
            &store_counts,
            &mut purges_zero_lamports,
        );
        purge_filter.stop();

        let mut reclaims_time = Measure::start("reclaims");
        // Recalculate reclaims with new purge set
        let pubkey_to_slot_set: Vec<_> = purges_zero_lamports
            .into_iter()
            .map(|(key, (slots_list, _ref_count))| {
                (
                    key,
                    slots_list
                        .into_iter()
                        .map(|(slot, _)| slot)
                        .collect::<HashSet<Slot>>(),
                )
            })
            .collect();

        let (reclaims, pubkeys_removed_from_accounts_index2) =
            self.purge_keys_exact(pubkey_to_slot_set.iter());
        pubkeys_removed_from_accounts_index
            .extend(pubkeys_removed_from_accounts_index2.into_iter());

        // Don't reset from clean, since the pubkeys in those stores may need to be unref'ed
        // and those stores may be used for background hashing.
        let reset_accounts = false;
        let mut reclaim_result = ReclaimResult::default();
        self.handle_reclaims(
            (!reclaims.is_empty()).then(|| reclaims.iter()),
            None,
            Some((&self.clean_accounts_stats.purge_stats, &mut reclaim_result)),
            reset_accounts,
            &pubkeys_removed_from_accounts_index,
        );

        reclaims_time.stop();
        measure_all.stop();

        self.clean_accounts_stats.report();
        datapoint_info!(
            "clean_accounts",
            ("total_us", measure_all.as_us(), i64),
            (
                "collect_delta_keys_us",
                key_timings.collect_delta_keys_us,
                i64
            ),
            ("oldest_dirty_slot", key_timings.oldest_dirty_slot, i64),
            (
                "pubkeys_removed_from_accounts_index",
                pubkeys_removed_from_accounts_index.len(),
                i64
            ),
            (
                "dirty_ancient_stores",
                key_timings.dirty_ancient_stores,
                i64
            ),
            (
                "dirty_store_processing_us",
                key_timings.dirty_store_processing_us,
                i64
            ),
            ("accounts_scan", accounts_scan.as_us() as i64, i64),
            ("clean_old_rooted", clean_old_rooted.as_us() as i64, i64),
            ("store_counts", store_counts_time.as_us() as i64, i64),
            ("purge_filter", purge_filter.as_us() as i64, i64),
            ("calc_deps", calc_deps_time.as_us() as i64, i64),
            ("reclaims", reclaims_time.as_us() as i64, i64),
            ("delta_insert_us", key_timings.delta_insert_us, i64),
            ("delta_key_count", key_timings.delta_key_count, i64),
            ("dirty_pubkeys_count", key_timings.dirty_pubkeys_count, i64),
            ("sort_us", sort.as_us(), i64),
            ("useful_keys", useful_accum.load(Ordering::Relaxed), i64),
            ("total_keys_count", total_keys_count, i64),
            (
                "scan_found_not_zero",
                found_not_zero_accum.load(Ordering::Relaxed),
                i64
            ),
            (
                "scan_not_found_on_fork",
                not_found_on_fork_accum.load(Ordering::Relaxed),
                i64
            ),
            ("scan_missing", missing_accum.load(Ordering::Relaxed), i64),
            ("uncleaned_roots_len", uncleaned_roots.len(), i64),
            (
                "clean_old_root_us",
                self.clean_accounts_stats
                    .clean_old_root_us
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "clean_old_root_reclaim_us",
                self.clean_accounts_stats
                    .clean_old_root_reclaim_us
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "reset_uncleaned_roots_us",
                self.clean_accounts_stats
                    .reset_uncleaned_roots_us
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "remove_dead_accounts_remove_us",
                self.clean_accounts_stats
                    .remove_dead_accounts_remove_us
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "remove_dead_accounts_shrink_us",
                self.clean_accounts_stats
                    .remove_dead_accounts_shrink_us
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "clean_stored_dead_slots_us",
                self.clean_accounts_stats
                    .clean_stored_dead_slots_us
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "roots_added",
                self.accounts_index.roots_added.swap(0, Ordering::Relaxed) as i64,
                i64
            ),
            (
                "roots_removed",
                self.accounts_index.roots_removed.swap(0, Ordering::Relaxed) as i64,
                i64
            ),
            (
                "active_scans",
                self.accounts_index.active_scans.load(Ordering::Relaxed) as i64,
                i64
            ),
            (
                "max_distance_to_min_scan_slot",
                self.accounts_index
                    .max_distance_to_min_scan_slot
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "ancient_account_cleans",
                ancient_account_cleans.load(Ordering::Relaxed),
                i64
            ),
            ("next_store_id", self.next_id.load(Ordering::Relaxed), i64),
        );
    }

    /// Removes the accounts in the input `reclaims` from the tracked "count" of
    /// their corresponding  storage entries. Note this does not actually free
    /// the memory from the storage entries until all the storage entries for
    /// a given slot `S` are empty, at which point `process_dead_slots` will
    /// remove all the storage entries for `S`.
    ///
    /// # Arguments
    /// * `reclaims` - The accounts to remove from storage entries' "count". Note here
    ///    that we should not remove cache entries, only entries for accounts actually
    ///    stored in a storage entry.
    ///
    /// * `expected_single_dead_slot` - A correctness assertion. If this is equal to `Some(S)`,
    ///    then the function will check that the only slot being cleaned up in `reclaims`
    ///    is the slot == `S`. This is true for instance when `handle_reclaims` is called
    ///    from store or slot shrinking, as those should only touch the slot they are
    ///    currently storing to or shrinking.
    ///
    /// * `purge_stats_and_reclaim_result` - Option containing `purge_stats` and `reclaim_result`.
    ///    `purge_stats`. `purge_stats` are stats used to track performance of purging dead slots.
    ///    `reclaim_result` contains information about accounts that were removed from storage,
    ///    does not include accounts that were removed from the cache.
    ///    If `purge_stats_and_reclaim_result.is_none()`, this implies there can be no dead slots
    ///    that happen as a result of this call, and the function will check that no slots are
    ///    cleaned up/removed via `process_dead_slots`. For instance, on store, no slots should
    ///    be cleaned up, but during the background clean accounts purges accounts from old rooted
    ///    slots, so outdated slots may be removed.
    ///
    /// * `reset_accounts` - Reset the append_vec store when the store is dead (count==0)
    ///    From the clean and shrink paths it should be false since there may be an in-progress
    ///    hash operation and the stores may hold accounts that need to be unref'ed.
    /// * `pubkeys_removed_from_accounts_index` - These keys have already been removed from the accounts index
    ///    and should not be unref'd. If they exist in the accounts index, they are NEW.
    fn handle_reclaims<'a, I>(
        &'a self,
        reclaims: Option<I>,
        expected_single_dead_slot: Option<Slot>,
        purge_stats_and_reclaim_result: Option<(&PurgeStats, &mut ReclaimResult)>,
        reset_accounts: bool,
        pubkeys_removed_from_accounts_index: &PubkeysRemovedFromAccountsIndex,
    ) where
        I: Iterator<Item = &'a (Slot, AccountInfo)>,
    {
        if let Some(reclaims) = reclaims {
            let (purge_stats, purged_account_slots, reclaimed_offsets) = if let Some((
                purge_stats,
                (ref mut purged_account_slots, ref mut reclaimed_offsets),
            )) =
                purge_stats_and_reclaim_result
            {
                (
                    Some(purge_stats),
                    Some(purged_account_slots),
                    Some(reclaimed_offsets),
                )
            } else {
                (None, None, None)
            };

            let dead_slots = self.remove_dead_accounts(
                reclaims,
                expected_single_dead_slot,
                reclaimed_offsets,
                reset_accounts,
            );

            if let Some(purge_stats) = purge_stats {
                if let Some(expected_single_dead_slot) = expected_single_dead_slot {
                    assert!(dead_slots.len() <= 1);
                    if dead_slots.len() == 1 {
                        assert!(dead_slots.contains(&expected_single_dead_slot));
                    }
                }

                self.process_dead_slots(
                    &dead_slots,
                    purged_account_slots,
                    purge_stats,
                    pubkeys_removed_from_accounts_index,
                );
            } else {
                assert!(dead_slots.is_empty());
            }
        }
    }

    /// During clean, some zero-lamport accounts that are marked for purge should *not* actually
    /// get purged.  Filter out those accounts here by removing them from 'purges_zero_lamports'
    ///
    /// When using incremental snapshots, do not purge zero-lamport accounts if the slot is higher
    /// than the last full snapshot slot.  This is to protect against the following scenario:
    ///
    ///   ```text
    ///   A full snapshot is taken, including account 'alpha' with a non-zero balance.  In a later slot,
    ///   alpha's lamports go to zero.  Eventually, cleaning runs.  Without this change,
    ///   alpha would be cleaned up and removed completely. Finally, an incremental snapshot is taken.
    ///
    ///   Later, the incremental and full snapshots are used to rebuild the bank and accounts
    ///   database (e.x. if the node restarts).  The full snapshot _does_ contain alpha
    ///   and its balance is non-zero.  However, since alpha was cleaned up in a slot after the full
    ///   snapshot slot (due to having zero lamports), the incremental snapshot would not contain alpha.
    ///   Thus, the accounts database will contain the old, incorrect info for alpha with a non-zero
    ///   balance.  Very bad!
    ///   ```
    ///
    /// This filtering step can be skipped if there is no `last_full_snapshot_slot`, or if the
    /// `max_clean_root_inclusive` is less-than-or-equal-to the `last_full_snapshot_slot`.
    fn filter_zero_lamport_clean_for_incremental_snapshots(
        &self,
        max_clean_root_inclusive: Option<Slot>,
        last_full_snapshot_slot: Option<Slot>,
        store_counts: &HashMap<AppendVecId, (usize, HashSet<Pubkey>)>,
        purges_zero_lamports: &mut HashMap<Pubkey, (SlotList<AccountInfo>, RefCount)>,
    ) {
        let should_filter_for_incremental_snapshots = max_clean_root_inclusive.unwrap_or(Slot::MAX)
            > last_full_snapshot_slot.unwrap_or(Slot::MAX);
        assert!(
            last_full_snapshot_slot.is_some() || !should_filter_for_incremental_snapshots,
            "if filtering for incremental snapshots, then snapshots should be enabled",
        );

        purges_zero_lamports.retain(|pubkey, (slot_account_infos, _ref_count)| {
            // Only keep purges_zero_lamports where the entire history of the account in the root set
            // can be purged. All AppendVecs for those updates are dead.
            for (_slot, account_info) in slot_account_infos.iter() {
                if let Some(store_count) = store_counts.get(&account_info.store_id()) {
                    if store_count.0 != 0 {
                        // one store this pubkey is in is not being removed, so this pubkey cannot be removed at all
                        return false;
                    }
                } else {
                    // store is not being removed, so this pubkey cannot be removed at all
                    return false;
                }
            }

            // Exit early if not filtering more for incremental snapshots
            if !should_filter_for_incremental_snapshots {
                return true;
            }

            let slot_account_info_at_highest_slot = slot_account_infos
                .iter()
                .max_by_key(|(slot, _account_info)| slot);

            slot_account_info_at_highest_slot.map_or(true, |(slot, account_info)| {
                // Do *not* purge zero-lamport accounts if the slot is greater than the last full
                // snapshot slot.  Since we're `retain`ing the accounts-to-purge, I felt creating
                // the `cannot_purge` variable made this easier to understand.  Accounts that do
                // not get purged here are added to a list so they be considered for purging later
                // (i.e. after the next full snapshot).
                assert!(account_info.is_zero_lamport());
                let cannot_purge = *slot > last_full_snapshot_slot.unwrap();
                if cannot_purge {
                    self.zero_lamport_accounts_to_purge_after_full_snapshot
                        .insert((*slot, *pubkey));
                }
                !cannot_purge
            })
        });
    }

    // Must be kept private!, does sensitive cleanup that should only be called from
    // supported pipelines in AccountsDb
    /// pubkeys_removed_from_accounts_index - These keys have already been removed from the accounts index
    ///    and should not be unref'd. If they exist in the accounts index, they are NEW.
    fn process_dead_slots(
        &self,
        dead_slots: &HashSet<Slot>,
        purged_account_slots: Option<&mut AccountSlots>,
        purge_stats: &PurgeStats,
        pubkeys_removed_from_accounts_index: &PubkeysRemovedFromAccountsIndex,
    ) {
        if dead_slots.is_empty() {
            return;
        }
        let mut clean_dead_slots = Measure::start("reclaims::clean_dead_slots");
        self.clean_stored_dead_slots(
            dead_slots,
            purged_account_slots,
            pubkeys_removed_from_accounts_index,
        );
        clean_dead_slots.stop();

        let mut purge_removed_slots = Measure::start("reclaims::purge_removed_slots");
        self.purge_dead_slots_from_storage(dead_slots.iter(), purge_stats);
        purge_removed_slots.stop();

        // If the slot is dead, remove the need to shrink the storages as
        // the storage entries will be purged.
        {
            let mut list = self.shrink_candidate_slots.lock().unwrap();
            for slot in dead_slots {
                list.remove(slot);
            }
        }

        debug!(
            "process_dead_slots({}): {} {} {:?}",
            dead_slots.len(),
            clean_dead_slots,
            purge_removed_slots,
            dead_slots,
        );
    }

    /// load the account index entry for the first `count` items in `accounts`
    /// store a reference to all alive accounts in `alive_accounts`
    /// unref and optionally store a reference to all pubkeys that are in the index, but dead in `unrefed_pubkeys`
    /// return sum of account size for all alive accounts
    fn load_accounts_index_for_shrink<'a>(
        &'a self,
        accounts: &'a [FoundStoredAccount<'a>],
        stats: &ShrinkStats,
    ) -> LoadAccountsIndexForShrink<'a> {
        let count = accounts.len();
        let mut alive_accounts = Vec::with_capacity(count);
        let mut unrefed_pubkeys = Vec::with_capacity(count);

        let mut alive_total_bytes = 0;

        let mut alive = 0;
        let mut dead = 0;
        let mut index = 0;
        let mut all_are_zero_lamports = true;
        self.accounts_index.scan(
            accounts.iter().map(|account| account.pubkey()),
            |pubkey, slots_refs| {
                let mut result = AccountsIndexScanResult::None;
                if let Some((slot_list, _ref_count)) = slots_refs {
                    let stored_account = &accounts[index];
                    let is_alive = slot_list.iter().any(|(_slot, acct_info)| {
                        acct_info.matches_storage_location(
                            stored_account.store_id,
                            stored_account.account.offset,
                        )
                    });
                    if !is_alive {
                        // This pubkey was found in the storage, but no longer exists in the index.
                        // It would have had a ref to the storage from the initial store, but it will
                        // not exist in the re-written slot. Unref it to keep the index consistent with
                        // rewriting the storage entries.
                        unrefed_pubkeys.push(pubkey);
                        result = AccountsIndexScanResult::Unref;
                        dead += 1;
                    } else {
                        all_are_zero_lamports &= stored_account.account.lamports() == 0;
                        alive_accounts.push(stored_account);
                        alive_total_bytes += stored_account.account.stored_size;
                        alive += 1;
                    }
                }
                index += 1;
                result
            },
            None,
        );
        assert_eq!(index, std::cmp::min(accounts.len(), count));
        stats.alive_accounts.fetch_add(alive, Ordering::Relaxed);
        stats.dead_accounts.fetch_add(dead, Ordering::Relaxed);

        LoadAccountsIndexForShrink {
            alive_total_bytes,
            alive_accounts,
            unrefed_pubkeys,
            all_are_zero_lamports,
        }
    }

    /// get all accounts in all the storages passed in
    /// for duplicate pubkeys, the account with the highest write_value is returned
    pub(crate) fn get_unique_accounts_from_storages<'a, I>(
        &'a self,
        stores: I,
    ) -> GetUniqueAccountsResult<'a>
    where
        I: Iterator<Item = &'a Arc<AccountStorageEntry>>,
    {
        let mut stored_accounts: HashMap<Pubkey, FoundStoredAccount> = HashMap::new();
        let mut original_bytes = 0;
        let mut count = 0;
        stores.into_iter().for_each(|store| {
            count += 1;
            assert!(count < 2, "there should be a max of 1 append vec per slot");
            original_bytes += store.total_bytes();
            let store_id = store.append_vec_id();
            store.accounts.account_iter().for_each(|account| {
                let new_entry = FoundStoredAccount { account, store_id };
                match stored_accounts.entry(*new_entry.account.pubkey()) {
                    Entry::Occupied(mut occupied_entry) => {
                        assert!(
                            new_entry.account.meta.write_version_obsolete
                                > occupied_entry.get().account.meta.write_version_obsolete
                        );
                        occupied_entry.insert(new_entry);
                    }
                    Entry::Vacant(vacant_entry) => {
                        vacant_entry.insert(new_entry);
                    }
                }
            });
        });

        // sort by pubkey to keep account index lookups close
        let mut stored_accounts = stored_accounts.drain().map(|(_k, v)| v).collect::<Vec<_>>();
        stored_accounts.sort_unstable_by(|a, b| a.pubkey().cmp(b.pubkey()));

        GetUniqueAccountsResult {
            stored_accounts,
            original_bytes,
        }
    }

    /// shared code for shrinking normal slots and combining into ancient append vecs
    /// note 'stored_accounts' is passed by ref so we can return references to data within it, avoiding self-references
    fn shrink_collect<'a: 'b, 'b, I>(
        &'a self,
        stores: I,
        stored_accounts: &'b mut Vec<FoundStoredAccount<'b>>,
        stats: &ShrinkStats,
    ) -> ShrinkCollect<'b>
    where
        I: Iterator<Item = &'a Arc<AccountStorageEntry>>,
    {
        let (
            GetUniqueAccountsResult {
                stored_accounts: stored_accounts_temp,
                original_bytes,
            },
            storage_read_elapsed,
        ) = measure!(self.get_unique_accounts_from_storages(stores));
        stats
            .storage_read_elapsed
            .fetch_add(storage_read_elapsed.as_us(), Ordering::Relaxed);
        *stored_accounts = stored_accounts_temp;

        let mut index_read_elapsed = Measure::start("index_read_elapsed");
        let alive_total_bytes_collect = AtomicUsize::new(0);

        let len = stored_accounts.len();
        let alive_accounts_collect = Mutex::new(Vec::with_capacity(len));
        let unrefed_pubkeys_collect = Mutex::new(Vec::with_capacity(len));
        stats
            .accounts_loaded
            .fetch_add(len as u64, Ordering::Relaxed);
        let all_are_zero_lamports_collect = Mutex::new(true);
        self.thread_pool_clean.install(|| {
            stored_accounts
                .par_chunks(SHRINK_COLLECT_CHUNK_SIZE)
                .for_each(|stored_accounts| {
                    let LoadAccountsIndexForShrink {
                        alive_total_bytes,
                        mut alive_accounts,
                        mut unrefed_pubkeys,
                        all_are_zero_lamports,
                    } = self.load_accounts_index_for_shrink(stored_accounts, stats);

                    // collect
                    alive_accounts_collect
                        .lock()
                        .unwrap()
                        .append(&mut alive_accounts);
                    unrefed_pubkeys_collect
                        .lock()
                        .unwrap()
                        .append(&mut unrefed_pubkeys);
                    alive_total_bytes_collect.fetch_add(alive_total_bytes, Ordering::Relaxed);
                    if !all_are_zero_lamports {
                        *all_are_zero_lamports_collect.lock().unwrap() = false;
                    }
                });
        });

        let alive_accounts = alive_accounts_collect.into_inner().unwrap();
        let unrefed_pubkeys = unrefed_pubkeys_collect.into_inner().unwrap();
        let alive_total_bytes = alive_total_bytes_collect.load(Ordering::Relaxed);

        index_read_elapsed.stop();
        stats
            .index_read_elapsed
            .fetch_add(index_read_elapsed.as_us(), Ordering::Relaxed);

        let aligned_total_bytes: u64 = Self::page_align(alive_total_bytes as u64);

        stats
            .accounts_removed
            .fetch_add(len - alive_accounts.len(), Ordering::Relaxed);
        stats.bytes_removed.fetch_add(
            original_bytes.saturating_sub(aligned_total_bytes),
            Ordering::Relaxed,
        );
        stats
            .bytes_written
            .fetch_add(aligned_total_bytes, Ordering::Relaxed);

        ShrinkCollect {
            original_bytes,
            aligned_total_bytes,
            unrefed_pubkeys,
            alive_accounts,
            alive_total_bytes,
            total_starting_accounts: len,
            all_are_zero_lamports: all_are_zero_lamports_collect.into_inner().unwrap(),
        }
    }

    /// common code from shrink and combine_ancient_slots
    /// get rid of all original store_ids in the slot
    /// returns remaining stores
    fn remove_old_stores_shrink(
        &self,
        shrink_collect: &ShrinkCollect,
        slot: Slot,
        stats: &ShrinkStats,
        shrink_in_progress: Option<ShrinkInProgress>,
    ) -> usize {
        // Purge old, overwritten storage entries
        let (remaining_stores, dead_storages) = self.mark_dirty_dead_stores(
            slot,
            // If all accounts are zero lamports, then we want to mark the entire OLD append vec as dirty.
            // otherwise, we'll call 'add_uncleaned_pubkeys_after_shrink' just on the unref'd keys below.
            shrink_collect.all_are_zero_lamports,
            shrink_in_progress,
        );

        if !shrink_collect.all_are_zero_lamports {
            self.add_uncleaned_pubkeys_after_shrink(
                slot,
                shrink_collect.unrefed_pubkeys.iter().cloned().cloned(),
            );
        }

        self.drop_or_recycle_stores(dead_storages, stats);
        remaining_stores
    }

    fn do_shrink_slot_stores<'a, I>(&'a self, slot: Slot, stores: I) -> usize
    where
        I: Iterator<Item = &'a Arc<AccountStorageEntry>>,
    {
        let mut stored_accounts = Vec::default();
        debug!("do_shrink_slot_stores: slot: {}", slot);
        let shrink_collect = self.shrink_collect(stores, &mut stored_accounts, &self.shrink_stats);

        // This shouldn't happen if alive_bytes/approx_stored_count are accurate
        if Self::should_not_shrink(
            shrink_collect.aligned_total_bytes,
            shrink_collect.original_bytes,
        ) {
            self.shrink_stats
                .skipped_shrink
                .fetch_add(1, Ordering::Relaxed);
            for pubkey in shrink_collect.unrefed_pubkeys {
                if let Some(locked_entry) = self.accounts_index.get_account_read_entry(pubkey) {
                    locked_entry.addref();
                }
            }
            return 0;
        }

        let total_accounts_after_shrink = shrink_collect.alive_accounts.len();
        debug!(
            "shrinking: slot: {}, accounts: ({} => {}) bytes: ({} ; aligned to: {}) original: {}",
            slot,
            shrink_collect.total_starting_accounts,
            total_accounts_after_shrink,
            shrink_collect.alive_total_bytes,
            shrink_collect.aligned_total_bytes,
            shrink_collect.original_bytes,
        );

        let mut rewrite_elapsed = Measure::start("rewrite_elapsed");
        let mut create_and_insert_store_elapsed_us = 0;
        let mut remove_old_stores_shrink_us = 0;
        let mut store_accounts_timing = StoreAccountsTiming::default();
        if shrink_collect.aligned_total_bytes > 0 {
            let (shrink_in_progress, time) =
                measure!(self.get_store_for_shrink(slot, shrink_collect.aligned_total_bytes));
            create_and_insert_store_elapsed_us = time.as_us();

            // here, we're writing back alive_accounts. That should be an atomic operation
            // without use of rather wide locks in this whole function, because we're
            // mutating rooted slots; There should be no writers to them.
            store_accounts_timing = self.store_accounts_frozen(
                (
                    slot,
                    &shrink_collect.alive_accounts[..],
                    INCLUDE_SLOT_IN_HASH_IRRELEVANT_APPEND_VEC_OPERATION,
                ),
                None::<Vec<&Hash>>,
                Some(shrink_in_progress.new_storage()),
                None,
                StoreReclaims::Ignore,
            );

            rewrite_elapsed.stop();

            // `store_accounts_frozen()` above may have purged accounts from some
            // other storage entries (the ones that were just overwritten by this
            // new storage entry). This means some of those stores might have caused
            // this slot to be read to `self.shrink_candidate_slots`, so delete
            // those here
            self.shrink_candidate_slots.lock().unwrap().remove(&slot);

            let (remaining_stores, remove_old_stores_shrink) = measure!(self
                .remove_old_stores_shrink(
                    &shrink_collect,
                    slot,
                    &self.shrink_stats,
                    Some(shrink_in_progress)
                ));
            remove_old_stores_shrink_us = remove_old_stores_shrink.as_us();
            if remaining_stores > 1 {
                inc_new_counter_info!("accounts_db_shrink_extra_stores", 1);
                info!(
                    "after shrink, slot has extra stores: {}, {}",
                    slot, remaining_stores
                );
            }
        }

        Self::update_shrink_stats(
            &self.shrink_stats,
            Measure::start("ignored"), // find_alive_elapsed
            create_and_insert_store_elapsed_us,
            store_accounts_timing,
            rewrite_elapsed,
            remove_old_stores_shrink_us,
        );
        self.shrink_stats.report();

        total_accounts_after_shrink
    }

    #[allow(clippy::too_many_arguments)]
    fn update_shrink_stats(
        shrink_stats: &ShrinkStats,
        find_alive_elapsed: Measure,
        create_and_insert_store_elapsed_us: u64,
        store_accounts_timing: StoreAccountsTiming,
        rewrite_elapsed: Measure,
        remove_old_stores_shrink_us: u64,
    ) {
        shrink_stats
            .num_slots_shrunk
            .fetch_add(1, Ordering::Relaxed);
        shrink_stats
            .find_alive_elapsed
            .fetch_add(find_alive_elapsed.as_us(), Ordering::Relaxed);
        shrink_stats
            .create_and_insert_store_elapsed
            .fetch_add(create_and_insert_store_elapsed_us, Ordering::Relaxed);
        shrink_stats.store_accounts_elapsed.fetch_add(
            store_accounts_timing.store_accounts_elapsed,
            Ordering::Relaxed,
        );
        shrink_stats.update_index_elapsed.fetch_add(
            store_accounts_timing.update_index_elapsed,
            Ordering::Relaxed,
        );
        shrink_stats.handle_reclaims_elapsed.fetch_add(
            store_accounts_timing.handle_reclaims_elapsed,
            Ordering::Relaxed,
        );
        shrink_stats
            .remove_old_stores_shrink_us
            .fetch_add(remove_old_stores_shrink_us, Ordering::Relaxed);
        shrink_stats
            .rewrite_elapsed
            .fetch_add(rewrite_elapsed.as_us(), Ordering::Relaxed);
    }

    /// get stores for 'slot'
    /// Drop 'shrink_in_progress', which will cause the old store to be removed from the storage map.
    /// For 'shrink_in_progress'.'old_storage' which is not retained, insert in 'dead_storages' and optionally 'dirty_stores'
    /// This is the end of the life cycle of `shrink_in_progress`.
    /// returns: (# of remaining stores for this slot, dead storages)
    pub(crate) fn mark_dirty_dead_stores(
        &self,
        slot: Slot,
        add_dirty_stores: bool,
        shrink_in_progress: Option<ShrinkInProgress>,
    ) -> (usize, SnapshotStorage) {
        let mut dead_storages = Vec::default();

        let mut not_retaining_store = |store: &Arc<AccountStorageEntry>| {
            if add_dirty_stores {
                self.dirty_stores
                    .insert((slot, store.append_vec_id()), store.clone());
            }
            dead_storages.push(store.clone());
        };

        let remaining_stores = if let Some(slot_stores) = self.storage.get_slot_stores(slot) {
            if let Some(shrink_in_progress) = shrink_in_progress {
                // shrink is in progress, so 1 new append vec to keep, 1 old one to throw away
                let store = shrink_in_progress.old_storage();
                not_retaining_store(store);
                // drop removes the old append vec that was being shrunk from db's storage
                drop(shrink_in_progress);
                slot_stores.read().unwrap().len()
            } else {
                // no shrink in progress, so all append vecs in this slot are dead
                let mut list = slot_stores.write().unwrap();
                list.drain().for_each(|(_key, store)| {
                    not_retaining_store(&store);
                });
                0
            }
        } else {
            0
        };
        (remaining_stores, dead_storages)
    }

    pub(crate) fn drop_or_recycle_stores(
        &self,
        dead_storages: SnapshotStorage,
        stats: &ShrinkStats,
    ) {
        let mut recycle_stores_write_elapsed = Measure::start("recycle_stores_write_time");
        let mut recycle_stores = self.recycle_stores.write().unwrap();
        recycle_stores_write_elapsed.stop();

        let mut drop_storage_entries_elapsed = Measure::start("drop_storage_entries_elapsed");
        if recycle_stores.entry_count() < MAX_RECYCLE_STORES {
            recycle_stores.add_entries(dead_storages);
            drop(recycle_stores);
        } else {
            self.stats
                .dropped_stores
                .fetch_add(dead_storages.len() as u64, Ordering::Relaxed);
            drop(recycle_stores);
            drop(dead_storages);
        }
        drop_storage_entries_elapsed.stop();
        stats
            .drop_storage_entries_elapsed
            .fetch_add(drop_storage_entries_elapsed.as_us(), Ordering::Relaxed);
        stats
            .recycle_stores_write_elapsed
            .fetch_add(recycle_stores_write_elapsed.as_us(), Ordering::Relaxed);
    }

    /// return a store that can contain 'aligned_total' bytes
    pub(crate) fn get_store_for_shrink(
        &self,
        slot: Slot,
        aligned_total: u64,
    ) -> ShrinkInProgress<'_> {
        let shrunken_store = self
            .try_recycle_store(slot, aligned_total, aligned_total + 1024)
            .unwrap_or_else(|| {
                let maybe_shrink_paths = self.shrink_paths.read().unwrap();
                let (shrink_paths, from) = maybe_shrink_paths
                    .as_ref()
                    .map(|paths| (paths, "shrink-w-path"))
                    .unwrap_or_else(|| (&self.paths, "shrink"));
                self.create_store(slot, aligned_total, from, shrink_paths)
            });
        self.storage.shrinking_in_progress(slot, shrunken_store)
    }

    // Reads all accounts in given slot's AppendVecs and filter only to alive,
    // then create a minimum AppendVec filled with the alive.
    fn shrink_slot_forced(&self, slot: Slot) -> usize {
        debug!("shrink_slot_forced: slot: {}", slot);

        if let Some(store) = self.storage.get_slot_storage_entry(slot) {
            if !Self::is_shrinking_productive(slot, &store) {
                return 0;
            }
            self.do_shrink_slot_stores(slot, std::iter::once(&store))
        } else {
            0
        }
    }

    fn all_slots_in_storage(&self) -> Vec<Slot> {
        self.storage.all_slots()
    }

    /// Given the input `ShrinkCandidates`, this function sorts the stores by their alive ratio
    /// in increasing order with the most sparse entries in the front. It will then simulate the
    /// shrinking by working on the most sparse entries first and if the overall alive ratio is
    /// achieved, it will stop and return the filtered-down candidates and the candidates which
    /// are skipped in this round and might be eligible for the future shrink.
    fn select_candidates_by_total_usage(
        shrink_slots: &ShrinkCandidates,
        shrink_ratio: f64,
    ) -> (ShrinkCandidates, ShrinkCandidates) {
        struct StoreUsageInfo {
            slot: Slot,
            alive_ratio: f64,
            store: Arc<AccountStorageEntry>,
        }
        let mut measure = Measure::start("select_top_sparse_storage_entries-ms");
        let mut store_usage: Vec<StoreUsageInfo> = Vec::with_capacity(shrink_slots.len());
        let mut total_alive_bytes: u64 = 0;
        let mut candidates_count: usize = 0;
        let mut total_bytes: u64 = 0;
        let mut total_candidate_stores: usize = 0;
        for (slot, slot_shrink_candidates) in shrink_slots {
            candidates_count += slot_shrink_candidates.len();
            for store in slot_shrink_candidates.values() {
                total_alive_bytes += Self::page_align(store.alive_bytes() as u64);
                total_bytes += store.total_bytes();
                let alive_ratio = Self::page_align(store.alive_bytes() as u64) as f64
                    / store.total_bytes() as f64;
                store_usage.push(StoreUsageInfo {
                    slot: *slot,
                    alive_ratio,
                    store: store.clone(),
                });
                total_candidate_stores += 1;
            }
        }
        store_usage.sort_by(|a, b| {
            a.alive_ratio
                .partial_cmp(&b.alive_ratio)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Working from the beginning of store_usage which are the most sparse and see when we can stop
        // shrinking while still achieving the overall goals.
        let mut shrink_slots: ShrinkCandidates = HashMap::new();
        let mut shrink_slots_next_batch: ShrinkCandidates = HashMap::new();
        for usage in &store_usage {
            let store = &usage.store;
            let alive_ratio = (total_alive_bytes as f64) / (total_bytes as f64);
            debug!("alive_ratio: {:?} store_id: {:?}, store_ratio: {:?} requirment: {:?}, total_bytes: {:?} total_alive_bytes: {:?}",
                alive_ratio, usage.store.append_vec_id(), usage.alive_ratio, shrink_ratio, total_bytes, total_alive_bytes);
            if alive_ratio > shrink_ratio {
                // we have reached our goal, stop
                debug!(
                    "Shrinking goal can be achieved at slot {:?}, total_alive_bytes: {:?} \
                    total_bytes: {:?}, alive_ratio: {:}, shrink_ratio: {:?}",
                    usage.slot, total_alive_bytes, total_bytes, alive_ratio, shrink_ratio
                );
                if usage.alive_ratio < shrink_ratio {
                    shrink_slots_next_batch
                        .entry(usage.slot)
                        .or_default()
                        .insert(store.append_vec_id(), store.clone());
                } else {
                    break;
                }
            } else {
                let current_store_size = store.total_bytes();
                let after_shrink_size = Self::page_align(store.alive_bytes() as u64);
                let bytes_saved = current_store_size.saturating_sub(after_shrink_size);
                total_bytes -= bytes_saved;
                shrink_slots
                    .entry(usage.slot)
                    .or_default()
                    .insert(store.append_vec_id(), store.clone());
            }
        }
        measure.stop();
        inc_new_counter_debug!(
            "shrink_select_top_sparse_storage_entries-ms",
            measure.as_ms() as usize
        );
        inc_new_counter_debug!(
            "shrink_select_top_sparse_storage_entries-seeds",
            candidates_count
        );
        inc_new_counter_debug!(
            "shrink_total_preliminary_candidate_stores",
            total_candidate_stores
        );

        (shrink_slots, shrink_slots_next_batch)
    }

    fn get_roots_less_than(&self, slot: Slot) -> Vec<Slot> {
        self.accounts_index
            .roots_tracker
            .read()
            .unwrap()
            .alive_roots
            .get_all_less_than(slot)
    }

    fn get_prior_root(&self, slot: Slot) -> Option<Slot> {
        self.accounts_index
            .roots_tracker
            .read()
            .unwrap()
            .alive_roots
            .get_prior(slot)
    }

    /// return all slots that are more than one epoch old and thus could already be an ancient append vec
    /// or which could need to be combined into a new or existing ancient append vec
    /// offset is used to combine newer slots than we normally would. This is designed to be used for testing.
    fn get_sorted_potential_ancient_slots(&self) -> Vec<Slot> {
        let mut reference_slot = self.get_accounts_hash_complete_one_epoch_old();
        if let Some(offset) = self.ancient_append_vec_offset {
            reference_slot = reference_slot.saturating_add(offset);
        }
        let mut old_slots = self.get_roots_less_than(reference_slot);
        old_slots.sort_unstable();
        old_slots
    }

    /// get a sorted list of slots older than an epoch
    /// squash those slots into ancient append vecs
    fn shrink_ancient_slots(&self) {
        if self.ancient_append_vec_offset.is_none() {
            return;
        }

        let can_randomly_shrink = true;
        self.combine_ancient_slots(
            self.get_sorted_potential_ancient_slots(),
            can_randomly_shrink,
        );
    }

    /// create and return new ancient append vec
    fn create_ancient_append_vec(&self, slot: Slot) -> ShrinkInProgress<'_> {
        let shrink_in_progress = self.get_store_for_shrink(slot, get_ancient_append_vec_capacity());
        info!(
            "ancient_append_vec: creating initial ancient append vec: {}, size: {}, id: {}",
            slot,
            get_ancient_append_vec_capacity(),
            shrink_in_progress.new_storage().append_vec_id(),
        );
        shrink_in_progress
    }

    #[cfg(test)]
    pub(crate) fn sizes_of_accounts_in_storage_for_tests(&self, slot: Slot) -> Vec<usize> {
        self.storage
            .get_slot_storage_entry(slot)
            .map(|storage| {
                storage
                    .accounts
                    .account_iter()
                    .map(|account| account.stored_size)
                    .collect()
            })
            .unwrap_or_default()
    }

    #[cfg(test)]
    fn get_storages_for_slot(&self, slot: Slot) -> Option<SnapshotStorage> {
        self.storage
            .get_slot_storage_entry(slot)
            .map(|storage| vec![storage])
    }

    /// 'accounts' that exist in the current slot we are combining into a different ancient slot
    /// 'existing_ancient_pubkeys': pubkeys that exist currently in the ancient append vec slot
    /// returns the pubkeys that are in 'accounts' that are already in 'existing_ancient_pubkeys'
    /// Also updated 'existing_ancient_pubkeys' to include all pubkeys in 'accounts' since they will soon be written into the ancient slot.
    fn get_keys_to_unref_ancient<'a>(
        accounts: &'a [&FoundStoredAccount<'_>],
        existing_ancient_pubkeys: &mut HashSet<Pubkey>,
    ) -> HashSet<&'a Pubkey> {
        let mut unref = HashSet::<&Pubkey>::default();
        // for each key that we're about to add that already exists in this storage, we need to unref. The account was in a different storage.
        // Now it is being put into an ancient storage again, but it is already there, so maintain max of 1 ref per storage in the accounts index.
        // The slot that currently references the account is going away, so unref to maintain # slots that reference the pubkey = refcount.
        accounts.iter().for_each(|account| {
            let key = account.pubkey();
            if !existing_ancient_pubkeys.insert(*key) {
                // this key exists BOTH in 'accounts' and already in the ancient append vec, so we need to unref it
                unref.insert(key);
            }
        });
        unref
    }

    /// 'accounts' are about to be appended to an ancient append vec. That ancient append vec may already have some accounts.
    /// Unref each account in 'accounts' that already exists in 'existing_ancient_pubkeys'.
    /// As a side effect, on exit, 'existing_ancient_pubkeys' will now contain all pubkeys in 'accounts'.
    fn unref_accounts_already_in_storage(
        &self,
        accounts: &[&FoundStoredAccount<'_>],
        existing_ancient_pubkeys: &mut HashSet<Pubkey>,
    ) {
        let unref = Self::get_keys_to_unref_ancient(accounts, existing_ancient_pubkeys);

        self.unref_pubkeys(
            unref.iter().cloned(),
            unref.len(),
            &PubkeysRemovedFromAccountsIndex::default(),
        );
    }

    /// get the storages from 'slot' to squash
    /// or None if this slot should be skipped
    /// side effect could be updating 'current_ancient'
    fn get_storages_to_move_to_ancient_append_vec(
        &self,
        slot: Slot,
        current_ancient: &mut CurrentAncientAppendVec,
        can_randomly_shrink: bool,
    ) -> Option<Arc<AccountStorageEntry>> {
        self.storage
            .get_slot_storage_entry(slot)
            .and_then(|storage| {
                self.should_move_to_ancient_append_vec(
                    &storage,
                    current_ancient,
                    slot,
                    can_randomly_shrink,
                )
                .then_some(storage)
            })
    }

    /// return true if the accounts in this slot should be moved to an ancient append vec
    /// otherwise, return false and the caller can skip this slot
    /// side effect could be updating 'current_ancient'
    /// can_randomly_shrink: true if ancient append vecs that otherwise don't qualify to be shrunk can be randomly shrunk
    ///  this is convenient for a running system
    ///  this is not useful for testing
    fn should_move_to_ancient_append_vec(
        &self,
        storage: &Arc<AccountStorageEntry>,
        current_ancient: &mut CurrentAncientAppendVec,
        slot: Slot,
        can_randomly_shrink: bool,
    ) -> bool {
        let accounts = &storage.accounts;

        self.shrink_ancient_stats
            .slots_considered
            .fetch_add(1, Ordering::Relaxed);

        if is_ancient(accounts) {
            self.shrink_ancient_stats
                .ancient_scanned
                .fetch_add(1, Ordering::Relaxed);

            // randomly shrink ancient slots
            // this exercises the ancient shrink code more often
            let written_bytes = storage.written_bytes();
            let mut alive_ratio = 0;
            let is_candidate = if written_bytes > 0 {
                alive_ratio = (storage.alive_bytes() as u64) * 100 / written_bytes;
                alive_ratio < 90
            } else {
                false
            };
            if is_candidate || (can_randomly_shrink && thread_rng().gen_range(0, 10000) == 0) {
                // we are a candidate for shrink, so either append us to the previous append vec
                // or recreate us as a new append vec and eliminate the dead accounts
                info!(
                    "ancient_append_vec: shrinking full ancient: {}, random: {}, alive_ratio: {}",
                    slot, !is_candidate, alive_ratio
                );
                if !is_candidate {
                    self.shrink_ancient_stats
                        .random_shrink
                        .fetch_add(1, Ordering::Relaxed);
                }
                self.shrink_ancient_stats
                    .ancient_append_vecs_shrunk
                    .fetch_add(1, Ordering::Relaxed);
                return true;
            }
            // this slot is ancient and can become the 'current' ancient for other slots to be squashed into
            *current_ancient = CurrentAncientAppendVec::new(slot, Arc::clone(storage));
            return false; // we're done with this slot - this slot IS the ancient append vec
        }

        // otherwise, yes, squash this slot into the current ancient append vec or create one at this slot
        true
    }

    /// Combine all account data from storages in 'sorted_slots' into ancient append vecs.
    /// This keeps us from accumulating append vecs for each slot older than an epoch.
    fn combine_ancient_slots(&self, sorted_slots: Vec<Slot>, can_randomly_shrink: bool) {
        let mut total = Measure::start("combine_ancient_slots");
        if sorted_slots.is_empty() {
            return;
        }
        let mut guard = None;

        // the ancient append vec currently being written to
        let mut current_ancient = CurrentAncientAppendVec::default();
        let mut dropped_roots = vec![];

        // we have to keep track of what pubkeys exist in the current ancient append vec so we can unref correctly
        let mut ancient_slot_pubkeys = AncientSlotPubkeys::default();

        let len = sorted_slots.len();
        for slot in sorted_slots {
            let old_storage = match self.get_storages_to_move_to_ancient_append_vec(
                slot,
                &mut current_ancient,
                can_randomly_shrink,
            ) {
                Some(old_storages) => old_storages,
                None => {
                    // nothing to squash for this slot
                    continue;
                }
            };

            if guard.is_none() {
                // we are now doing interesting work in squashing ancient
                guard = Some(self.active_stats.activate(ActiveStatItem::SquashAncient));
                info!(
                    "ancient_append_vec: combine_ancient_slots first slot: {}, num_roots: {}",
                    slot, len
                );
            }

            let old_storages = [old_storage];
            self.combine_one_store_into_ancient(
                slot,
                &old_storages,
                &mut current_ancient,
                &mut ancient_slot_pubkeys,
                &mut dropped_roots,
            );
        }

        self.handle_dropped_roots_for_ancient(dropped_roots);

        total.stop();
        self.shrink_ancient_stats
            .total_us
            .fetch_add(total.as_us(), Ordering::Relaxed);

        // only log when we moved some accounts to ancient append vecs or we've exceeded 100ms
        // results will continue to accumulate otherwise
        if guard.is_some() || self.shrink_ancient_stats.total_us.load(Ordering::Relaxed) > 100_000 {
            self.shrink_ancient_stats.report();
        }
    }

    /// put entire alive contents of 'old_storages' into the current ancient append vec or a newly created ancient append vec
    fn combine_one_store_into_ancient(
        &self,
        slot: Slot,
        old_storages: &[Arc<AccountStorageEntry>],
        current_ancient: &mut CurrentAncientAppendVec,
        ancient_slot_pubkeys: &mut AncientSlotPubkeys,
        dropped_roots: &mut Vec<Slot>,
    ) {
        let mut stored_accounts = Vec::default();
        let shrink_collect = self.shrink_collect(
            old_storages.iter(),
            &mut stored_accounts,
            &self.shrink_ancient_stats.shrink_stats,
        );

        // could follow what shrink does more closely
        if shrink_collect.total_starting_accounts == 0 {
            return; // skipping slot with no useful accounts to write
        }

        let (mut shrink_in_progress, time) =
            measure!(current_ancient.create_if_necessary(slot, self));
        let mut create_and_insert_store_elapsed_us = time.as_us();
        let available_bytes = current_ancient.append_vec().accounts.remaining_bytes();
        // split accounts in 'slot' into:
        // 'Primary', which can fit in 'current_ancient'
        // 'Overflow', which will have to go into a new ancient append vec at 'slot'
        let (to_store, find_alive_elapsed) = measure!(AccountsToStore::new(
            available_bytes,
            &shrink_collect.alive_accounts,
            shrink_collect.alive_total_bytes,
            slot
        ));

        ancient_slot_pubkeys.maybe_unref_accounts_already_in_ancient(
            slot,
            self,
            current_ancient,
            &to_store,
        );

        let mut rewrite_elapsed = Measure::start("rewrite_elapsed");
        // write what we can to the current ancient storage
        let mut store_accounts_timing =
            current_ancient.store_ancient_accounts(self, &to_store, StorageSelector::Primary);

        // handle accounts from 'slot' which did not fit into the current ancient append vec
        if to_store.has_overflow() {
            // We need a new ancient append vec at this slot.
            // Assert: it cannot be the case that we already had an ancient append vec at this slot and
            // yet that ancient append vec does not have room for the accounts stored at this slot currently
            assert_ne!(slot, current_ancient.slot());
            let (shrink_in_progress_overflow, time) =
                measure!(current_ancient.create_ancient_append_vec(slot, self));
            create_and_insert_store_elapsed_us += time.as_us();
            // We cannot possibly be shrinking the original slot that created an ancient append vec
            // AND not have enough room in the ancient append vec at that slot
            // to hold all the contents of that slot.
            // We need this new 'shrink_in_progress' to be used in 'remove_old_stores_shrink' below.
            // All non-overflow accounts were put in a prior slot's ancient append vec. All overflow accounts
            // are essentially being shrunk into a new ancient append vec in 'slot'.
            assert!(shrink_in_progress.is_none());
            shrink_in_progress = Some(shrink_in_progress_overflow);

            // write the overflow accounts to the next ancient storage
            let timing =
                current_ancient.store_ancient_accounts(self, &to_store, StorageSelector::Overflow);
            store_accounts_timing.accumulate(&timing);
        }
        rewrite_elapsed.stop();

        if slot != current_ancient.slot() {
            // all append vecs in this slot have been combined into an ancient append vec
            dropped_roots.push(slot);
        }

        let (_remaining_stores, remove_old_stores_shrink) = measure!(self
            .remove_old_stores_shrink(
                &shrink_collect,
                slot,
                &self.shrink_ancient_stats.shrink_stats,
                shrink_in_progress,
            ));

        // we should not try to shrink any of the stores from this slot anymore. All shrinking for this slot is now handled by ancient append vec code.
        self.shrink_candidate_slots.lock().unwrap().remove(&slot);

        Self::update_shrink_stats(
            &self.shrink_ancient_stats.shrink_stats,
            find_alive_elapsed,
            create_and_insert_store_elapsed_us,
            store_accounts_timing,
            rewrite_elapsed,
            remove_old_stores_shrink.as_us(),
        );
    }

    /// each slot in 'dropped_roots' has been combined into an ancient append vec.
    /// We are done with the slot now forever.
    fn handle_dropped_roots_for_ancient(&self, dropped_roots: Vec<Slot>) {
        if !dropped_roots.is_empty() {
            dropped_roots.iter().for_each(|slot| {
                self.accounts_index
                    .clean_dead_slot(*slot, &mut AccountsIndexRootsStats::default());
                self.bank_hashes.write().unwrap().remove(slot);
                // all storages have been removed from this slot and recycled or dropped
                assert!(self
                    .storage
                    .remove(slot)
                    .unwrap()
                    .1
                    .read()
                    .unwrap()
                    .is_empty());
            });
        }
    }

    /// add all 'pubkeys' into the set of pubkeys that are 'uncleaned', associated with 'slot'
    /// clean will visit these pubkeys next time it runs
    fn add_uncleaned_pubkeys_after_shrink(
        &self,
        slot: Slot,
        pubkeys: impl Iterator<Item = Pubkey>,
    ) {
        /*
        This is only called during 'shrink'-type operations.
        Original accounts were separated into 'accounts' and 'unrefed_pubkeys'.
        These sets correspond to 'alive' and 'dead'.
        'alive' means this account in this slot is in the accounts index.
        'dead' means this account in this slot is NOT in the accounts index.
        If dead, nobody will care if this version of this account is not written into the newly shrunk append vec for this slot.
        For all dead accounts, they were already unrefed and are now absent in the new append vec.
        This means that another version of this pubkey could possibly now be cleaned since this one is now gone.
        For example, a zero lamport account in a later slot can be removed if we just removed the only non-zero lamport account for that pubkey in this slot.
        So, for all unrefed accounts, send them to clean to be revisited next time clean runs.
        If an account is alive, then its status has not changed. It was previously alive in this slot. It is still alive in this slot.
        Clean doesn't care about alive accounts that remain alive.
        Except... A slightly different case is if ALL the alive accounts in this slot are zero lamport accounts, then it is possible that
        this slot can be marked dead. So, if all alive accounts are zero lamports, we send the entire OLD/pre-shrunk append vec
        to clean so that all the pubkeys are visited.
        It is a performance optimization to not send the ENTIRE old/pre-shrunk append vec to clean in the normal case.
        */

        let mut uncleaned_pubkeys = self
            .uncleaned_pubkeys
            .entry(slot)
            .or_insert_with(Vec::default);
        uncleaned_pubkeys.extend(pubkeys);
    }

    pub fn shrink_candidate_slots(&self) -> usize {
        if !self.shrink_candidate_slots.lock().unwrap().is_empty() {
            // this can affect 'shrink_candidate_slots', so don't 'take' it until after this completes
            self.shrink_ancient_slots();
        }

        let shrink_candidates_slots =
            std::mem::take(&mut *self.shrink_candidate_slots.lock().unwrap());

        let (shrink_slots, shrink_slots_next_batch) = {
            if let AccountShrinkThreshold::TotalSpace { shrink_ratio } = self.shrink_ratio {
                let (shrink_slots, shrink_slots_next_batch) =
                    Self::select_candidates_by_total_usage(&shrink_candidates_slots, shrink_ratio);
                (shrink_slots, Some(shrink_slots_next_batch))
            } else {
                (shrink_candidates_slots, None)
            }
        };

        if shrink_slots.is_empty()
            && shrink_slots_next_batch
                .as_ref()
                .map(|s| s.is_empty())
                .unwrap_or(true)
        {
            return 0;
        }

        let _guard = self.active_stats.activate(ActiveStatItem::Shrink);

        let mut measure_shrink_all_candidates = Measure::start("shrink_all_candidate_slots-ms");
        let num_candidates = shrink_slots.len();
        let shrink_candidates_count: usize = self.thread_pool_clean.install(|| {
            shrink_slots
                .into_par_iter()
                .map(|(slot, slot_shrink_candidates)| {
                    let mut measure = Measure::start("shrink_candidate_slots-ms");
                    self.do_shrink_slot_stores(slot, slot_shrink_candidates.values());
                    measure.stop();
                    inc_new_counter_info!("shrink_candidate_slots-ms", measure.as_ms() as usize);
                    slot_shrink_candidates.len()
                })
                .sum()
        });
        measure_shrink_all_candidates.stop();
        inc_new_counter_info!(
            "shrink_all_candidate_slots-ms",
            measure_shrink_all_candidates.as_ms() as usize
        );
        inc_new_counter_info!("shrink_all_candidate_slots-count", shrink_candidates_count);
        let mut pended_counts: usize = 0;
        if let Some(shrink_slots_next_batch) = shrink_slots_next_batch {
            let mut shrink_slots = self.shrink_candidate_slots.lock().unwrap();
            for (slot, stores) in shrink_slots_next_batch {
                pended_counts += stores.len();
                shrink_slots.entry(slot).or_default().extend(stores);
            }
        }
        inc_new_counter_info!("shrink_pended_stores-count", pended_counts);

        num_candidates
    }

    pub fn shrink_all_slots(&self, is_startup: bool, last_full_snapshot_slot: Option<Slot>) {
        let _guard = self.active_stats.activate(ActiveStatItem::Shrink);
        const DIRTY_STORES_CLEANING_THRESHOLD: usize = 10_000;
        const OUTER_CHUNK_SIZE: usize = 2000;
        if is_startup {
            let slots = self.all_slots_in_storage();
            let threads = num_cpus::get();
            let inner_chunk_size = std::cmp::max(OUTER_CHUNK_SIZE / threads, 1);
            slots.chunks(OUTER_CHUNK_SIZE).for_each(|chunk| {
                chunk.par_chunks(inner_chunk_size).for_each(|slots| {
                    for slot in slots {
                        self.shrink_slot_forced(*slot);
                    }
                });
                if self.dirty_stores.len() > DIRTY_STORES_CLEANING_THRESHOLD {
                    self.clean_accounts(None, is_startup, last_full_snapshot_slot);
                }
            });
        } else {
            for slot in self.all_slots_in_storage() {
                self.shrink_slot_forced(slot);
                if self.dirty_stores.len() > DIRTY_STORES_CLEANING_THRESHOLD {
                    self.clean_accounts(None, is_startup, last_full_snapshot_slot);
                }
            }
        }
    }

    pub fn scan_accounts<F>(
        &self,
        ancestors: &Ancestors,
        bank_id: BankId,
        mut scan_func: F,
        config: &ScanConfig,
    ) -> ScanResult<()>
    where
        F: FnMut(Option<(&Pubkey, AccountSharedData, Slot)>),
    {
        // This can error out if the slots being scanned over are aborted
        self.accounts_index.scan_accounts(
            ancestors,
            bank_id,
            |pubkey, (account_info, slot)| {
                let account_slot = self
                    .get_account_accessor(slot, pubkey, &account_info.storage_location())
                    .get_loaded_account()
                    .map(|loaded_account| (pubkey, loaded_account.take_account(), slot));
                scan_func(account_slot)
            },
            config,
        )?;

        Ok(())
    }

    pub fn unchecked_scan_accounts<F>(
        &self,
        metric_name: &'static str,
        ancestors: &Ancestors,
        mut scan_func: F,
        config: &ScanConfig,
    ) where
        F: FnMut(&Pubkey, LoadedAccount, Slot),
    {
        self.accounts_index.unchecked_scan_accounts(
            metric_name,
            ancestors,
            |pubkey, (account_info, slot)| {
                if let Some(loaded_account) = self
                    .get_account_accessor(slot, pubkey, &account_info.storage_location())
                    .get_loaded_account()
                {
                    scan_func(pubkey, loaded_account, slot);
                }
            },
            config,
        );
    }

    /// Only guaranteed to be safe when called from rent collection
    pub fn range_scan_accounts<F, R>(
        &self,
        metric_name: &'static str,
        ancestors: &Ancestors,
        range: R,
        config: &ScanConfig,
        mut scan_func: F,
    ) where
        F: FnMut(Option<(&Pubkey, AccountSharedData, Slot)>),
        R: RangeBounds<Pubkey> + std::fmt::Debug,
    {
        self.accounts_index.range_scan_accounts(
            metric_name,
            ancestors,
            range,
            config,
            |pubkey, (account_info, slot)| {
                // unlike other scan fns, this is called from Bank::collect_rent_eagerly(),
                // which is on-consensus processing in the banking/replaying stage.
                // This requires infallible and consistent account loading.
                // So, we unwrap Option<LoadedAccount> from get_loaded_account() here.
                // This is safe because this closure is invoked with the account_info,
                // while we lock the index entry at AccountsIndex::do_scan_accounts() ultimately,
                // meaning no other subsystems can invalidate the account_info before making their
                // changes to the index entry.
                // For details, see the comment in retry_to_get_account_accessor()
                if let Some(account_slot) = self
                    .get_account_accessor(slot, pubkey, &account_info.storage_location())
                    .get_loaded_account()
                    .map(|loaded_account| (pubkey, loaded_account.take_account(), slot))
                {
                    scan_func(Some(account_slot))
                }
            },
        );
    }

    pub fn index_scan_accounts<F>(
        &self,
        ancestors: &Ancestors,
        bank_id: BankId,
        index_key: IndexKey,
        mut scan_func: F,
        config: &ScanConfig,
    ) -> ScanResult<bool>
    where
        F: FnMut(Option<(&Pubkey, AccountSharedData, Slot)>),
    {
        let key = match &index_key {
            IndexKey::ProgramId(key) => key,
            IndexKey::SplTokenMint(key) => key,
            IndexKey::SplTokenOwner(key) => key,
        };
        if !self.account_indexes.include_key(key) {
            // the requested key was not indexed in the secondary index, so do a normal scan
            let used_index = false;
            self.scan_accounts(ancestors, bank_id, scan_func, config)?;
            return Ok(used_index);
        }

        self.accounts_index.index_scan_accounts(
            ancestors,
            bank_id,
            index_key,
            |pubkey, (account_info, slot)| {
                let account_slot = self
                    .get_account_accessor(slot, pubkey, &account_info.storage_location())
                    .get_loaded_account()
                    .map(|loaded_account| (pubkey, loaded_account.take_account(), slot));
                scan_func(account_slot)
            },
            config,
        )?;
        let used_index = true;
        Ok(used_index)
    }

    /// Scan a specific slot through all the account storage
    pub fn scan_account_storage<R, B>(
        &self,
        slot: Slot,
        cache_map_func: impl Fn(LoadedAccount) -> Option<R> + Sync,
        storage_scan_func: impl Fn(&B, LoadedAccount) + Sync,
    ) -> ScanStorageResult<R, B>
    where
        R: Send,
        B: Send + Default + Sync,
    {
        if let Some(slot_cache) = self.accounts_cache.slot_cache(slot) {
            // If we see the slot in the cache, then all the account information
            // is in this cached slot
            if slot_cache.len() > SCAN_SLOT_PAR_ITER_THRESHOLD {
                ScanStorageResult::Cached(self.thread_pool.install(|| {
                    slot_cache
                        .par_iter()
                        .filter_map(|cached_account| {
                            cache_map_func(LoadedAccount::Cached(Cow::Borrowed(
                                cached_account.value(),
                            )))
                        })
                        .collect()
                }))
            } else {
                ScanStorageResult::Cached(
                    slot_cache
                        .iter()
                        .filter_map(|cached_account| {
                            cache_map_func(LoadedAccount::Cached(Cow::Borrowed(
                                cached_account.value(),
                            )))
                        })
                        .collect(),
                )
            }
        } else {
            let retval = B::default();
            // If the slot is not in the cache, then all the account information must have
            // been flushed. This is guaranteed because we only remove the rooted slot from
            // the cache *after* we've finished flushing in `flush_slot_cache`.
            if let Some(storage) = self.storage.get_slot_storage_entry(slot) {
                storage
                    .accounts
                    .account_iter()
                    .for_each(|account| storage_scan_func(&retval, LoadedAccount::Stored(account)));
            }

            ScanStorageResult::Stored(retval)
        }
    }

    /// Insert a new bank hash for `slot`
    ///
    /// The new bank hash is empty/default except for the slot.  This fn is called when creating a
    /// new bank from parent.  The bank hash for this slot is updated with real values later.
    pub fn insert_default_bank_hash(&self, slot: Slot, parent_slot: Slot) {
        let mut bank_hashes = self.bank_hashes.write().unwrap();
        if bank_hashes.get(&slot).is_some() {
            error!(
                "set_hash: already exists; multiple forks with shared slot {} as child (parent: {})!?",
                slot, parent_slot,
            );
            return;
        }

        let new_hash_info = BankHashInfo::default();
        bank_hashes.insert(slot, new_hash_info);
    }

    pub fn load(
        &self,
        ancestors: &Ancestors,
        pubkey: &Pubkey,
        load_hint: LoadHint,
    ) -> Option<(AccountSharedData, Slot)> {
        self.do_load(ancestors, pubkey, None, load_hint, LoadZeroLamports::None)
    }

    pub fn load_account_into_read_cache(&self, ancestors: &Ancestors, pubkey: &Pubkey) {
        self.do_load_with_populate_read_cache(
            ancestors,
            pubkey,
            None,
            LoadHint::Unspecified,
            true,
            // no return from this function, so irrelevant
            LoadZeroLamports::None,
        );
    }

    /// note this returns None for accounts with zero lamports
    pub fn load_with_fixed_root(
        &self,
        ancestors: &Ancestors,
        pubkey: &Pubkey,
    ) -> Option<(AccountSharedData, Slot)> {
        self.load(ancestors, pubkey, LoadHint::FixedMaxRoot)
    }

    fn read_index_for_accessor_or_load_slow<'a>(
        &'a self,
        ancestors: &Ancestors,
        pubkey: &'a Pubkey,
        max_root: Option<Slot>,
        clone_in_lock: bool,
    ) -> Option<(Slot, StorageLocation, Option<LoadedAccountAccessor<'a>>)> {
        let (lock, index) = match self.accounts_index.get(pubkey, Some(ancestors), max_root) {
            AccountIndexGetResult::Found(lock, index) => (lock, index),
            // we bail out pretty early for missing.
            AccountIndexGetResult::NotFound => {
                return None;
            }
        };

        let slot_list = lock.slot_list();
        let (slot, info) = slot_list[index];
        let storage_location = info.storage_location();
        let some_from_slow_path = if clone_in_lock {
            // the fast path must have failed.... so take the slower approach
            // of copying potentially large Account::data inside the lock.

            // calling check_and_get_loaded_account is safe as long as we're guaranteed to hold
            // the lock during the time and there should be no purge thanks to alive ancestors
            // held by our caller.
            Some(self.get_account_accessor(slot, pubkey, &storage_location))
        } else {
            None
        };

        Some((slot, storage_location, some_from_slow_path))
        // `lock` is dropped here rather pretty quickly with clone_in_lock = false,
        // so the entry could be raced for mutation by other subsystems,
        // before we actually provision an account data for caller's use from now on.
        // This is traded for less contention and resultant performance, introducing fair amount of
        // delicate handling in retry_to_get_account_accessor() below ;)
        // you're warned!
    }

    fn retry_to_get_account_accessor<'a>(
        &'a self,
        mut slot: Slot,
        mut storage_location: StorageLocation,
        ancestors: &'a Ancestors,
        pubkey: &'a Pubkey,
        max_root: Option<Slot>,
        load_hint: LoadHint,
    ) -> Option<(LoadedAccountAccessor<'a>, Slot)> {
        // Happy drawing time! :)
        //
        // Reader                               | Accessed data source for cached/stored
        // -------------------------------------+----------------------------------
        // R1 read_index_for_accessor_or_load_slow()| cached/stored: index
        //          |                           |
        //        <(store_id, offset, ..)>      |
        //          V                           |
        // R2 retry_to_get_account_accessor()/  | cached: map of caches & entry for (slot, pubkey)
        //        get_account_accessor()        | stored: map of stores
        //          |                           |
        //        <Accessor>                    |
        //          V                           |
        // R3 check_and_get_loaded_account()/   | cached: N/A (note: basically noop unwrap)
        //        get_loaded_account()          | stored: store's entry for slot
        //          |                           |
        //        <LoadedAccount>               |
        //          V                           |
        // R4 take_account()                    | cached/stored: entry of cache/storage for (slot, pubkey)
        //          |                           |
        //        <AccountSharedData>           |
        //          V                           |
        //    Account!!                         V
        //
        // Flusher                              | Accessed data source for cached/stored
        // -------------------------------------+----------------------------------
        // F1 flush_slot_cache()                | N/A
        //          |                           |
        //          V                           |
        // F2 store_accounts_frozen()/          | map of stores (creates new entry)
        //        write_accounts_to_storage()   |
        //          |                           |
        //          V                           |
        // F3 store_accounts_frozen()/          | index
        //        update_index()                | (replaces existing store_id, offset in caches)
        //          |                           |
        //          V                           |
        // F4 accounts_cache.remove_slot()      | map of caches (removes old entry)
        //                                      V
        //
        // Remarks for flusher: So, for any reading operations, it's a race condition where F4 happens
        // between R1 and R2. In that case, retrying from R1 is safu because F3 should have
        // been occurred.
        //
        // Shrinker                             | Accessed data source for stored
        // -------------------------------------+----------------------------------
        // S1 do_shrink_slot_stores()           | N/A
        //          |                           |
        //          V                           |
        // S2 store_accounts_frozen()/          | map of stores (creates new entry)
        //        write_accounts_to_storage()   |
        //          |                           |
        //          V                           |
        // S3 store_accounts_frozen()/          | index
        //        update_index()                | (replaces existing store_id, offset in stores)
        //          |                           |
        //          V                           |
        // S4 do_shrink_slot_stores()/          | map of stores (removes old entry)
        //        dead_storages
        //
        // Remarks for shrinker: So, for any reading operations, it's a race condition
        // where S4 happens between R1 and R2. In that case, retrying from R1 is safu because S3 should have
        // been occurred, and S3 atomically replaced the index accordingly.
        //
        // Cleaner                              | Accessed data source for stored
        // -------------------------------------+----------------------------------
        // C1 clean_accounts()                  | N/A
        //          |                           |
        //          V                           |
        // C2 clean_accounts()/                 | index
        //        purge_keys_exact()            | (removes existing store_id, offset for stores)
        //          |                           |
        //          V                           |
        // C3 clean_accounts()/                 | map of stores (removes old entry)
        //        handle_reclaims()             |
        //
        // Remarks for cleaner: So, for any reading operations, it's a race condition
        // where C3 happens between R1 and R2. In that case, retrying from R1 is safu.
        // In that case, None would be returned while bailing out at R1.
        //
        // Purger                                 | Accessed data source for cached/stored
        // ---------------------------------------+----------------------------------
        // P1 purge_slot()                        | N/A
        //          |                             |
        //          V                             |
        // P2 purge_slots_from_cache_and_store()  | map of caches/stores (removes old entry)
        //          |                             |
        //          V                             |
        // P3 purge_slots_from_cache_and_store()/ | index
        //       purge_slot_cache()/              |
        //          purge_slot_cache_pubkeys()    | (removes existing store_id, offset for cache)
        //       purge_slot_storage()/            |
        //          purge_keys_exact()            | (removes accounts index entries)
        //          handle_reclaims()             | (removes storage entries)
        //      OR                                |
        //    clean_accounts()/                   |
        //        clean_accounts_older_than_root()| (removes existing store_id, offset for stores)
        //                                        V
        //
        // Remarks for purger: So, for any reading operations, it's a race condition
        // where P2 happens between R1 and R2. In that case, retrying from R1 is safu.
        // In that case, we may bail at index read retry when P3 hasn't been run

        #[cfg(test)]
        {
            // Give some time for cache flushing to occur here for unit tests
            sleep(Duration::from_millis(self.load_delay));
        }

        // Failsafe for potential race conditions with other subsystems
        let mut num_acceptable_failed_iterations = 0;
        loop {
            let account_accessor = self.get_account_accessor(slot, pubkey, &storage_location);
            match account_accessor {
                LoadedAccountAccessor::Cached(Some(_)) | LoadedAccountAccessor::Stored(Some(_)) => {
                    // Great! There was no race, just return :) This is the most usual situation
                    return Some((account_accessor, slot));
                }
                LoadedAccountAccessor::Cached(None) => {
                    num_acceptable_failed_iterations += 1;
                    // Cache was flushed in between checking the index and retrieving from the cache,
                    // so retry. This works because in accounts cache flush, an account is written to
                    // storage *before* it is removed from the cache
                    match load_hint {
                        LoadHint::FixedMaxRoot => {
                            // it's impossible for this to fail for transaction loads from
                            // replaying/banking more than once.
                            // This is because:
                            // 1) For a slot `X` that's being replayed, there is only one
                            // latest ancestor containing the latest update for the account, and this
                            // ancestor can only be flushed once.
                            // 2) The root cannot move while replaying, so the index cannot continually
                            // find more up to date entries than the current `slot`
                            assert!(num_acceptable_failed_iterations <= 1);
                        }
                        LoadHint::Unspecified => {
                            // Because newer root can be added to the index (= not fixed),
                            // multiple flush race conditions can be observed under very rare
                            // condition, at least theoretically
                        }
                    }
                }
                LoadedAccountAccessor::Stored(None) => {
                    match load_hint {
                        LoadHint::FixedMaxRoot => {
                            // When running replay on the validator, or banking stage on the leader,
                            // it should be very rare that the storage entry doesn't exist if the
                            // entry in the accounts index is the latest version of this account.
                            //
                            // There are only a few places where the storage entry may not exist
                            // after reading the index:
                            // 1) Shrink has removed the old storage entry and rewritten to
                            // a newer storage entry
                            // 2) The `pubkey` asked for in this function is a zero-lamport account,
                            // and the storage entry holding this account qualified for zero-lamport clean.
                            //
                            // In both these cases, it should be safe to retry and recheck the accounts
                            // index indefinitely, without incrementing num_acceptable_failed_iterations.
                            // That's because if the root is fixed, there should be a bounded number
                            // of pending cleans/shrinks (depends how far behind the AccountsBackgroundService
                            // is), termination to the desired condition is guaranteed.
                            //
                            // Also note that in both cases, if we do find the storage entry,
                            // we can guarantee that the storage entry is safe to read from because
                            // we grabbed a reference to the storage entry while it was still in the
                            // storage map. This means even if the storage entry is removed from the storage
                            // map after we grabbed the storage entry, the recycler should not reset the
                            // storage entry until we drop the reference to the storage entry.
                            //
                            // eh, no code in this arm? yes!
                        }
                        LoadHint::Unspecified => {
                            // RPC get_account() may have fetched an old root from the index that was
                            // either:
                            // 1) Cleaned up by clean_accounts(), so the accounts index has been updated
                            // and the storage entries have been removed.
                            // 2) Dropped by purge_slots() because the slot was on a minor fork, which
                            // removes the slots' storage entries but doesn't purge from the accounts index
                            // (account index cleanup is left to clean for stored slots). Note that
                            // this generally is impossible to occur in the wild because the RPC
                            // should hold the slot's bank, preventing it from being purged() to
                            // begin with.
                            num_acceptable_failed_iterations += 1;
                        }
                    }
                }
            }
            #[cfg(not(test))]
            let load_limit = ABSURD_CONSECUTIVE_FAILED_ITERATIONS;

            #[cfg(test)]
            let load_limit = self.load_limit.load(Ordering::Relaxed);

            let fallback_to_slow_path = if num_acceptable_failed_iterations >= load_limit {
                // The latest version of the account existed in the index, but could not be
                // fetched from storage. This means a race occurred between this function and clean
                // accounts/purge_slots
                let message = format!(
                    "do_load() failed to get key: {pubkey} from storage, latest attempt was for \
                     slot: {slot}, storage_location: {storage_location:?}, load_hint: {load_hint:?}",
                );
                datapoint_warn!("accounts_db-do_load_warn", ("warn", message, String));
                true
            } else {
                false
            };

            // Because reading from the cache/storage failed, retry from the index read
            let (new_slot, new_storage_location, maybe_account_accessor) = self
                .read_index_for_accessor_or_load_slow(
                    ancestors,
                    pubkey,
                    max_root,
                    fallback_to_slow_path,
                )?;
            // Notice the subtle `?` at previous line, we bail out pretty early if missing.

            if new_slot == slot && new_storage_location.is_store_id_equal(&storage_location) {
                inc_new_counter_info!("retry_to_get_account_accessor-panic", 1);
                let message = format!(
                    "Bad index entry detected ({}, {}, {:?}, {:?}, {:?}, {:?})",
                    pubkey,
                    slot,
                    storage_location,
                    load_hint,
                    new_storage_location,
                    self.accounts_index.get_account_read_entry(pubkey)
                );
                // Considering that we've failed to get accessor above and further that
                // the index still returned the same (slot, store_id) tuple, offset must be same
                // too.
                assert!(
                    new_storage_location.is_offset_equal(&storage_location),
                    "{message}"
                );

                // If the entry was missing from the cache, that means it must have been flushed,
                // and the accounts index is always updated before cache flush, so store_id must
                // not indicate being cached at this point.
                assert!(!new_storage_location.is_cached(), "{message}");

                // If this is not a cache entry, then this was a minor fork slot
                // that had its storage entries cleaned up by purge_slots() but hasn't been
                // cleaned yet. That means this must be rpc access and not replay/banking at the
                // very least. Note that purge shouldn't occur even for RPC as caller must hold all
                // of ancestor slots..
                assert_eq!(load_hint, LoadHint::Unspecified, "{message}");

                // Everything being assert!()-ed, let's panic!() here as it's an error condition
                // after all....
                // That reasoning is based on the fact all of code-path reaching this fn
                // retry_to_get_account_accessor() must outlive the Arc<Bank> (and its all
                // ancestors) over this fn invocation, guaranteeing the prevention of being purged,
                // first of all.
                // For details, see the comment in AccountIndex::do_checked_scan_accounts(),
                // which is referring back here.
                panic!("{message}");
            } else if fallback_to_slow_path {
                // the above bad-index-entry check must had been checked first to retain the same
                // behavior
                return Some((
                    maybe_account_accessor.expect("must be some if clone_in_lock=true"),
                    new_slot,
                ));
            }

            slot = new_slot;
            storage_location = new_storage_location;
        }
    }

    fn do_load(
        &self,
        ancestors: &Ancestors,
        pubkey: &Pubkey,
        max_root: Option<Slot>,
        load_hint: LoadHint,
        load_zero_lamports: LoadZeroLamports,
    ) -> Option<(AccountSharedData, Slot)> {
        self.do_load_with_populate_read_cache(
            ancestors,
            pubkey,
            max_root,
            load_hint,
            false,
            load_zero_lamports,
        )
    }

    /// remove all entries from the read only accounts cache
    /// useful for benches/tests
    pub fn flush_read_only_cache_for_tests(&self) {
        self.read_only_accounts_cache.reset_for_tests();
    }

    /// if 'load_into_read_cache_only', then return value is meaningless.
    ///   The goal is to get the account into the read-only cache.
    fn do_load_with_populate_read_cache(
        &self,
        ancestors: &Ancestors,
        pubkey: &Pubkey,
        max_root: Option<Slot>,
        load_hint: LoadHint,
        load_into_read_cache_only: bool,
        load_zero_lamports: LoadZeroLamports,
    ) -> Option<(AccountSharedData, Slot)> {
        #[cfg(not(test))]
        assert!(max_root.is_none());

        let (slot, storage_location, _maybe_account_accesor) =
            self.read_index_for_accessor_or_load_slow(ancestors, pubkey, max_root, false)?;
        // Notice the subtle `?` at previous line, we bail out pretty early if missing.

        let in_write_cache = storage_location.is_cached();
        if !load_into_read_cache_only {
            if !in_write_cache {
                let result = self.read_only_accounts_cache.load(*pubkey, slot);
                if let Some(account) = result {
                    if matches!(load_zero_lamports, LoadZeroLamports::None)
                        && account.is_zero_lamport()
                    {
                        return None;
                    }
                    return Some((account, slot));
                }
            }
        } else {
            // goal is to load into read cache
            if in_write_cache {
                // no reason to load in read cache. already in write cache
                return None;
            }
            if self.read_only_accounts_cache.in_cache(pubkey, slot) {
                // already in read cache
                return None;
            }
        }

        let (mut account_accessor, slot) = self.retry_to_get_account_accessor(
            slot,
            storage_location,
            ancestors,
            pubkey,
            max_root,
            load_hint,
        )?;
        let loaded_account = account_accessor.check_and_get_loaded_account();
        let is_cached = loaded_account.is_cached();
        let account = loaded_account.take_account();
        if matches!(load_zero_lamports, LoadZeroLamports::None) && account.is_zero_lamport() {
            return None;
        }

        if !is_cached {
            /*
            We show this store into the read-only cache for account 'A' and future loads of 'A' from the read-only cache are
            safe/reflect 'A''s latest state on this fork.
            This safety holds if during replay of slot 'S', we show we only read 'A' from the write cache,
            not the read-only cache, after it's been updated in replay of slot 'S'.
            Assume for contradiction this is not true, and we read 'A' from the read-only cache *after* it had been updated in 'S'.
            This means an entry '(S, A)' was added to the read-only cache after 'A' had been updated in 'S'.
            Now when '(S, A)' was being added to the read-only cache, it must have been true that  'is_cache == false',
            which means '(S', A)' does not exist in the write cache yet.
            However, by the assumption for contradiction above ,  'A' has already been updated in 'S' which means '(S, A)'
            must exist in the write cache, which is a contradiction.
            */
            self.read_only_accounts_cache
                .store(*pubkey, slot, account.clone());
        }
        Some((account, slot))
    }

    pub fn load_account_hash(
        &self,
        ancestors: &Ancestors,
        pubkey: &Pubkey,
        max_root: Option<Slot>,
        load_hint: LoadHint,
    ) -> Option<Hash> {
        let (slot, storage_location, _maybe_account_accesor) =
            self.read_index_for_accessor_or_load_slow(ancestors, pubkey, max_root, false)?;
        // Notice the subtle `?` at previous line, we bail out pretty early if missing.

        let (mut account_accessor, _) = self.retry_to_get_account_accessor(
            slot,
            storage_location,
            ancestors,
            pubkey,
            max_root,
            load_hint,
        )?;
        let loaded_account = account_accessor.check_and_get_loaded_account();
        Some(loaded_account.loaded_hash())
    }

    fn get_account_accessor<'a>(
        &'a self,
        slot: Slot,
        pubkey: &'a Pubkey,
        storage_location: &StorageLocation,
    ) -> LoadedAccountAccessor<'a> {
        match storage_location {
            StorageLocation::Cached => {
                let maybe_cached_account = self.accounts_cache.load(slot, pubkey).map(Cow::Owned);
                LoadedAccountAccessor::Cached(maybe_cached_account)
            }
            StorageLocation::AppendVec(store_id, offset) => {
                let maybe_storage_entry = self
                    .storage
                    .get_account_storage_entry(slot, *store_id)
                    .map(|account_storage_entry| (account_storage_entry, *offset));
                LoadedAccountAccessor::Stored(maybe_storage_entry)
            }
        }
    }

    fn try_recycle_and_insert_store(
        &self,
        slot: Slot,
        min_size: u64,
        max_size: u64,
    ) -> Option<Arc<AccountStorageEntry>> {
        let store = self.try_recycle_store(slot, min_size, max_size)?;
        self.insert_store(slot, store.clone());
        Some(store)
    }

    fn try_recycle_store(
        &self,
        slot: Slot,
        min_size: u64,
        max_size: u64,
    ) -> Option<Arc<AccountStorageEntry>> {
        let mut max = 0;
        let mut min = std::u64::MAX;
        let mut avail = 0;
        let mut recycle_stores = self.recycle_stores.write().unwrap();
        for (i, (_recycled_time, store)) in recycle_stores.iter().enumerate() {
            if Arc::strong_count(store) == 1 {
                max = std::cmp::max(store.accounts.capacity(), max);
                min = std::cmp::min(store.accounts.capacity(), min);
                avail += 1;

                if store.accounts.capacity() >= min_size && store.accounts.capacity() < max_size {
                    let ret = recycle_stores.remove_entry(i);
                    drop(recycle_stores);
                    let old_id = ret.append_vec_id();
                    ret.recycle(slot, self.next_id());
                    debug!(
                        "recycling store: {} {:?} old_id: {}",
                        ret.append_vec_id(),
                        ret.get_path(),
                        old_id
                    );
                    self.stats
                        .recycle_store_count
                        .fetch_add(1, Ordering::Relaxed);
                    return Some(ret);
                }
            }
        }
        debug!(
            "no recycle stores max: {} min: {} len: {} looking: {}, {} avail: {}",
            max,
            min,
            recycle_stores.entry_count(),
            min_size,
            max_size,
            avail,
        );
        None
    }

    fn find_storage_candidate(&self, slot: Slot, size: usize) -> Arc<AccountStorageEntry> {
        let mut get_slot_stores = Measure::start("get_slot_stores");
        let slot_stores_lock = self.storage.get_slot_stores(slot);
        get_slot_stores.stop();
        self.stats
            .store_get_slot_store
            .fetch_add(get_slot_stores.as_us(), Ordering::Relaxed);
        let mut find_existing = Measure::start("find_existing");
        if let Some(slot_stores_lock) = slot_stores_lock {
            let slot_stores = slot_stores_lock.read().unwrap();
            if !slot_stores.is_empty() {
                // pick an available store at random by iterating from a random point
                let to_skip = thread_rng().gen_range(0, slot_stores.len());

                for (i, store) in slot_stores.values().cycle().skip(to_skip).enumerate() {
                    if store.try_available() {
                        let ret = store.clone();
                        drop(slot_stores);
                        find_existing.stop();
                        self.stats
                            .store_find_existing
                            .fetch_add(find_existing.as_us(), Ordering::Relaxed);
                        return ret;
                    }
                    // looked at every store, bail...
                    if i == slot_stores.len() {
                        break;
                    }
                }
            }
        }
        find_existing.stop();
        self.stats
            .store_find_existing
            .fetch_add(find_existing.as_us(), Ordering::Relaxed);

        let store = if let Some(store) = self.try_recycle_store(slot, size as u64, std::u64::MAX) {
            store
        } else {
            self.create_store(slot, self.file_size, "store", &self.paths)
        };

        // try_available is like taking a lock on the store,
        // preventing other threads from using it.
        // It must succeed here and happen before insert,
        // otherwise another thread could also grab it from the index.
        assert!(store.try_available());
        self.insert_store(slot, store.clone());
        store
    }

    pub(crate) fn page_align(size: u64) -> u64 {
        (size + (PAGE_SIZE - 1)) & !(PAGE_SIZE - 1)
    }

    fn has_space_available(&self, slot: Slot, size: u64) -> bool {
        let slot_storage = self.storage.get_slot_stores(slot).unwrap();
        let slot_storage_r = slot_storage.read().unwrap();
        for (_id, store) in slot_storage_r.iter() {
            if store.status() == AccountStorageStatus::Available
                && (store.accounts.capacity() - store.accounts.len() as u64) > size
            {
                return true;
            }
        }
        false
    }

    fn create_store(
        &self,
        slot: Slot,
        size: u64,
        from: &str,
        paths: &[PathBuf],
    ) -> Arc<AccountStorageEntry> {
        self.stats
            .create_store_count
            .fetch_add(1, Ordering::Relaxed);
        let path_index = thread_rng().gen_range(0, paths.len());
        let store = Arc::new(self.new_storage_entry(
            slot,
            Path::new(&paths[path_index]),
            Self::page_align(size),
        ));

        debug!(
            "creating store: {} slot: {} len: {} size: {} from: {} path: {:?}",
            store.append_vec_id(),
            slot,
            store.accounts.len(),
            store.accounts.capacity(),
            from,
            store.accounts.get_path()
        );

        store
    }

    fn create_and_insert_store(
        &self,
        slot: Slot,
        size: u64,
        from: &str,
    ) -> Arc<AccountStorageEntry> {
        self.create_and_insert_store_with_paths(slot, size, from, &self.paths)
    }

    fn create_and_insert_store_with_paths(
        &self,
        slot: Slot,
        size: u64,
        from: &str,
        paths: &[PathBuf],
    ) -> Arc<AccountStorageEntry> {
        let store = self.create_store(slot, size, from, paths);
        let store_for_index = store.clone();

        self.insert_store(slot, store_for_index);
        store
    }

    fn insert_store(&self, slot: Slot, store: Arc<AccountStorageEntry>) {
        self.storage.insert(slot, store)
    }

    pub fn create_drop_bank_callback(
        &self,
        pruned_banks_sender: DroppedSlotsSender,
    ) -> SendDroppedBankCallback {
        self.is_bank_drop_callback_enabled
            .store(true, Ordering::Release);
        SendDroppedBankCallback::new(pruned_banks_sender)
    }

    /// This should only be called after the `Bank::drop()` runs in bank.rs, See BANK_DROP_SAFETY
    /// comment below for more explanation.
    ///   * `is_serialized_with_abs` - indicates whehter this call runs sequentially with all other
    ///        accounts_db relevant calls, such as shrinking, purging etc., in account background
    ///        service.
    pub fn purge_slot(&self, slot: Slot, bank_id: BankId, is_serialized_with_abs: bool) {
        if self.is_bank_drop_callback_enabled.load(Ordering::Acquire) && !is_serialized_with_abs {
            panic!(
                "bad drop callpath detected; Bank::drop() must run serially with other logic in
                ABS like clean_accounts()"
            )
        }

        // BANK_DROP_SAFETY: Because this function only runs once the bank is dropped,
        // we know that there are no longer any ongoing scans on this bank, because scans require
        // and hold a reference to the bank at the tip of the fork they're scanning. Hence it's
        // safe to remove this bank_id from the `removed_bank_ids` list at this point.
        if self
            .accounts_index
            .removed_bank_ids
            .lock()
            .unwrap()
            .remove(&bank_id)
        {
            // If this slot was already cleaned up, no need to do any further cleans
            return;
        }

        self.purge_slots(std::iter::once(&slot));
    }

    fn recycle_slot_stores(
        &self,
        total_removed_storage_entries: usize,
        slot_stores: &[SlotStores],
    ) -> u64 {
        let mut recycled_count = 0;

        let mut recycle_stores_write_elapsed = Measure::start("recycle_stores_write_elapsed");
        let mut recycle_stores = self.recycle_stores.write().unwrap();
        recycle_stores_write_elapsed.stop();

        for slot_entries in slot_stores {
            let entry = slot_entries.read().unwrap();
            for (_store_id, stores) in entry.iter() {
                if recycle_stores.entry_count() > MAX_RECYCLE_STORES {
                    let dropped_count = total_removed_storage_entries - recycled_count;
                    self.stats
                        .dropped_stores
                        .fetch_add(dropped_count as u64, Ordering::Relaxed);
                    return recycle_stores_write_elapsed.as_us();
                }
                recycle_stores.add_entry(stores.clone());
                recycled_count += 1;
            }
        }
        recycle_stores_write_elapsed.as_us()
    }

    /// Purges every slot in `removed_slots` from both the cache and storage. This includes
    /// entries in the accounts index, cache entries, and any backing storage entries.
    pub(crate) fn purge_slots_from_cache_and_store<'a>(
        &self,
        removed_slots: impl Iterator<Item = &'a Slot> + Clone,
        purge_stats: &PurgeStats,
        log_accounts: bool,
    ) {
        let mut remove_cache_elapsed_across_slots = 0;
        let mut num_cached_slots_removed = 0;
        let mut total_removed_cached_bytes = 0;
        if log_accounts {
            if let Some(min) = removed_slots.clone().min() {
                info!(
                    "purge_slots_from_cache_and_store: {:?}",
                    self.get_pubkey_hash_for_slot(*min).0
                );
            }
        }
        for remove_slot in removed_slots {
            // This function is only currently safe with respect to `flush_slot_cache()` because
            // both functions run serially in AccountsBackgroundService.
            let mut remove_cache_elapsed = Measure::start("remove_cache_elapsed");
            // Note: we cannot remove this slot from the slot cache until we've removed its
            // entries from the accounts index first. This is because `scan_accounts()` relies on
            // holding the index lock, finding the index entry, and then looking up the entry
            // in the cache. If it fails to find that entry, it will panic in `get_loaded_account()`
            if let Some(slot_cache) = self.accounts_cache.slot_cache(*remove_slot) {
                // If the slot is still in the cache, remove the backing storages for
                // the slot and from the Accounts Index
                num_cached_slots_removed += 1;
                total_removed_cached_bytes += slot_cache.total_bytes();
                self.purge_slot_cache(*remove_slot, slot_cache);
                remove_cache_elapsed.stop();
                remove_cache_elapsed_across_slots += remove_cache_elapsed.as_us();
                // Nobody else shoud have removed the slot cache entry yet
                assert!(self.accounts_cache.remove_slot(*remove_slot).is_some());
            } else {
                self.purge_slot_storage(*remove_slot, purge_stats);
            }
            // It should not be possible that a slot is neither in the cache or storage. Even in
            // a slot with all ticks, `Bank::new_from_parent()` immediately stores some sysvars
            // on bank creation.
        }

        purge_stats
            .remove_cache_elapsed
            .fetch_add(remove_cache_elapsed_across_slots, Ordering::Relaxed);
        purge_stats
            .num_cached_slots_removed
            .fetch_add(num_cached_slots_removed, Ordering::Relaxed);
        purge_stats
            .total_removed_cached_bytes
            .fetch_add(total_removed_cached_bytes, Ordering::Relaxed);
    }

    /// Purge the backing storage entries for the given slot, does not purge from
    /// the cache!
    fn purge_dead_slots_from_storage<'a>(
        &'a self,
        removed_slots: impl Iterator<Item = &'a Slot> + Clone,
        purge_stats: &PurgeStats,
    ) {
        // Check all slots `removed_slots` are no longer "relevant" roots.
        // Note that the slots here could have been rooted slots, but if they're passed here
        // for removal it means:
        // 1) All updates in that old root have been outdated by updates in newer roots
        // 2) Those slots/roots should have already been purged from the accounts index root
        // tracking metadata via `accounts_index.clean_dead_slot()`.
        let mut safety_checks_elapsed = Measure::start("safety_checks_elapsed");
        assert!(self
            .accounts_index
            .get_rooted_from_list(removed_slots.clone())
            .is_empty());
        safety_checks_elapsed.stop();
        purge_stats
            .safety_checks_elapsed
            .fetch_add(safety_checks_elapsed.as_us(), Ordering::Relaxed);

        let mut total_removed_storage_entries = 0;
        let mut total_removed_stored_bytes = 0;
        let mut all_removed_slot_storages = vec![];

        let mut remove_storage_entries_elapsed = Measure::start("remove_storage_entries_elapsed");
        for remove_slot in removed_slots {
            // Remove the storage entries and collect some metrics
            if let Some((_, slot_storages_to_be_removed)) = self.storage.remove(remove_slot) {
                {
                    let r_slot_removed_storages = slot_storages_to_be_removed.read().unwrap();
                    total_removed_storage_entries += r_slot_removed_storages.len();
                    total_removed_stored_bytes += r_slot_removed_storages
                        .values()
                        .map(|i| i.accounts.capacity())
                        .sum::<u64>();
                }
                all_removed_slot_storages.push(slot_storages_to_be_removed.clone());
            }
        }
        remove_storage_entries_elapsed.stop();
        let num_stored_slots_removed = all_removed_slot_storages.len();

        let recycle_stores_write_elapsed =
            self.recycle_slot_stores(total_removed_storage_entries, &all_removed_slot_storages);

        let mut drop_storage_entries_elapsed = Measure::start("drop_storage_entries_elapsed");
        // Backing mmaps for removed storages entries explicitly dropped here outside
        // of any locks
        drop(all_removed_slot_storages);
        drop_storage_entries_elapsed.stop();
        purge_stats
            .remove_storage_entries_elapsed
            .fetch_add(remove_storage_entries_elapsed.as_us(), Ordering::Relaxed);
        purge_stats
            .drop_storage_entries_elapsed
            .fetch_add(drop_storage_entries_elapsed.as_us(), Ordering::Relaxed);
        purge_stats
            .num_stored_slots_removed
            .fetch_add(num_stored_slots_removed, Ordering::Relaxed);
        purge_stats
            .total_removed_storage_entries
            .fetch_add(total_removed_storage_entries, Ordering::Relaxed);
        purge_stats
            .total_removed_stored_bytes
            .fetch_add(total_removed_stored_bytes, Ordering::Relaxed);
        purge_stats
            .recycle_stores_write_elapsed
            .fetch_add(recycle_stores_write_elapsed, Ordering::Relaxed);
    }

    fn purge_slot_cache(&self, purged_slot: Slot, slot_cache: SlotCache) {
        let mut purged_slot_pubkeys: HashSet<(Slot, Pubkey)> = HashSet::new();
        let pubkey_to_slot_set: Vec<(Pubkey, Slot)> = slot_cache
            .iter()
            .map(|account| {
                purged_slot_pubkeys.insert((purged_slot, *account.key()));
                (*account.key(), purged_slot)
            })
            .collect();
        self.purge_slot_cache_pubkeys(
            purged_slot,
            purged_slot_pubkeys,
            pubkey_to_slot_set,
            true,
            &HashSet::default(),
        );
    }

    fn purge_slot_cache_pubkeys(
        &self,
        purged_slot: Slot,
        purged_slot_pubkeys: HashSet<(Slot, Pubkey)>,
        pubkey_to_slot_set: Vec<(Pubkey, Slot)>,
        is_dead: bool,
        pubkeys_removed_from_accounts_index: &PubkeysRemovedFromAccountsIndex,
    ) {
        // Slot purged from cache should not exist in the backing store
        assert!(self.storage.get_slot_stores(purged_slot).is_none());
        let num_purged_keys = pubkey_to_slot_set.len();
        let (reclaims, _) = self.purge_keys_exact(pubkey_to_slot_set.iter());
        assert_eq!(reclaims.len(), num_purged_keys);
        if is_dead {
            self.remove_dead_slots_metadata(
                std::iter::once(&purged_slot),
                purged_slot_pubkeys,
                None,
                pubkeys_removed_from_accounts_index,
            );
        }
    }

    fn purge_slot_storage(&self, remove_slot: Slot, purge_stats: &PurgeStats) {
        // Because AccountsBackgroundService synchronously flushes from the accounts cache
        // and handles all Bank::drop() (the cleanup function that leads to this
        // function call), then we don't need to worry above an overlapping cache flush
        // with this function call. This means, if we get into this case, we can be
        // confident that the entire state for this slot has been flushed to the storage
        // already.
        let mut scan_storages_elasped = Measure::start("scan_storages_elasped");
        type ScanResult = ScanStorageResult<Pubkey, Arc<Mutex<HashSet<(Pubkey, Slot)>>>>;
        let scan_result: ScanResult = self.scan_account_storage(
            remove_slot,
            |loaded_account: LoadedAccount| Some(*loaded_account.pubkey()),
            |accum: &Arc<Mutex<HashSet<(Pubkey, Slot)>>>, loaded_account: LoadedAccount| {
                accum
                    .lock()
                    .unwrap()
                    .insert((*loaded_account.pubkey(), remove_slot));
            },
        );
        scan_storages_elasped.stop();
        purge_stats
            .scan_storages_elapsed
            .fetch_add(scan_storages_elasped.as_us(), Ordering::Relaxed);

        let mut purge_accounts_index_elapsed = Measure::start("purge_accounts_index_elapsed");
        let (reclaims, pubkeys_removed_from_accounts_index) = match scan_result {
            ScanStorageResult::Cached(_) => {
                panic!("Should not see cached keys in this `else` branch, since we checked this slot did not exist in the cache above");
            }
            ScanStorageResult::Stored(stored_keys) => {
                // Purge this slot from the accounts index
                self.purge_keys_exact(stored_keys.lock().unwrap().iter())
            }
        };
        purge_accounts_index_elapsed.stop();
        purge_stats
            .purge_accounts_index_elapsed
            .fetch_add(purge_accounts_index_elapsed.as_us(), Ordering::Relaxed);

        // `handle_reclaims()` should remove all the account index entries and
        // storage entries
        let mut handle_reclaims_elapsed = Measure::start("handle_reclaims_elapsed");
        // Slot should be dead after removing all its account entries
        let expected_dead_slot = Some(remove_slot);
        self.handle_reclaims(
            (!reclaims.is_empty()).then(|| reclaims.iter()),
            expected_dead_slot,
            Some((purge_stats, &mut ReclaimResult::default())),
            false,
            &pubkeys_removed_from_accounts_index,
        );
        handle_reclaims_elapsed.stop();
        purge_stats
            .handle_reclaims_elapsed
            .fetch_add(handle_reclaims_elapsed.as_us(), Ordering::Relaxed);
        // After handling the reclaimed entries, this slot's
        // storage entries should be purged from self.storage
        assert!(
            self.storage.get_slot_stores(remove_slot).is_none(),
            "slot {remove_slot} is not none"
        );
    }

    #[allow(clippy::needless_collect)]
    fn purge_slots<'a>(&self, slots: impl Iterator<Item = &'a Slot> + Clone) {
        // `add_root()` should be called first
        let mut safety_checks_elapsed = Measure::start("safety_checks_elapsed");
        let non_roots = slots
            // Only safe to check when there are duplicate versions of a slot
            // because ReplayStage will not make new roots before dumping the
            // duplicate slots first. Thus we will not be in a case where we
            // root slot `S`, then try to dump some other version of slot `S`, the
            // dumping has to finish first
            //
            // Also note roots are never removed via `remove_unrooted_slot()`, so
            // it's safe to filter them out here as they won't need deletion from
            // self.accounts_index.removed_bank_ids in `purge_slots_from_cache_and_store()`.
            .filter(|slot| !self.accounts_index.is_alive_root(**slot));
        safety_checks_elapsed.stop();
        self.external_purge_slots_stats
            .safety_checks_elapsed
            .fetch_add(safety_checks_elapsed.as_us(), Ordering::Relaxed);
        self.purge_slots_from_cache_and_store(non_roots, &self.external_purge_slots_stats, false);
        self.external_purge_slots_stats
            .report("external_purge_slots_stats", Some(1000));
    }

    pub fn remove_unrooted_slots(&self, remove_slots: &[(Slot, BankId)]) {
        let rooted_slots = self
            .accounts_index
            .get_rooted_from_list(remove_slots.iter().map(|(slot, _)| slot));
        assert!(
            rooted_slots.is_empty(),
            "Trying to remove accounts for rooted slots {rooted_slots:?}"
        );

        let RemoveUnrootedSlotsSynchronization {
            slots_under_contention,
            signal,
        } = &self.remove_unrooted_slots_synchronization;

        {
            // Slots that are currently being flushed by flush_slot_cache()

            let mut currently_contended_slots = slots_under_contention.lock().unwrap();

            // Slots that are currently being flushed by flush_slot_cache() AND
            // we want to remove in this function
            let mut remaining_contended_flush_slots: Vec<Slot> = remove_slots
                .iter()
                .filter_map(|(remove_slot, _)| {
                    // Reserve the slots that we want to purge that aren't currently
                    // being flushed to prevent cache from flushing those slots in
                    // the future.
                    //
                    // Note that the single replay thread has to remove a specific slot `N`
                    // before another version of the same slot can be replayed. This means
                    // multiple threads should not call `remove_unrooted_slots()` simultaneously
                    // with the same slot.
                    let is_being_flushed = !currently_contended_slots.insert(*remove_slot);
                    // If the cache is currently flushing this slot, add it to the list
                    is_being_flushed.then_some(remove_slot)
                })
                .cloned()
                .collect();

            // Wait for cache flushes to finish
            loop {
                if !remaining_contended_flush_slots.is_empty() {
                    // Wait for the signal that the cache has finished flushing a slot
                    //
                    // Don't wait if the remaining_contended_flush_slots is empty, otherwise
                    // we may never get a signal since there's no cache flush thread to
                    // do the signaling
                    currently_contended_slots = signal.wait(currently_contended_slots).unwrap();
                } else {
                    // There are no slots being flushed to wait on, so it's safe to continue
                    // to purging the slots we want to purge!
                    break;
                }

                // For each slot the cache flush has finished, mark that we're about to start
                // purging these slots by reserving it in `currently_contended_slots`.
                remaining_contended_flush_slots.retain(|flush_slot| {
                    // returns true if slot was already in set. This means slot is being flushed
                    !currently_contended_slots.insert(*flush_slot)
                });
            }
        }

        // Mark down these slots are about to be purged so that new attempts to scan these
        // banks fail, and any ongoing scans over these slots will detect that they should abort
        // their results
        {
            let mut locked_removed_bank_ids = self.accounts_index.removed_bank_ids.lock().unwrap();
            for (_slot, remove_bank_id) in remove_slots.iter() {
                locked_removed_bank_ids.insert(*remove_bank_id);
            }
        }

        let remove_unrooted_purge_stats = PurgeStats::default();
        self.purge_slots_from_cache_and_store(
            remove_slots.iter().map(|(slot, _)| slot),
            &remove_unrooted_purge_stats,
            true,
        );
        remove_unrooted_purge_stats.report("remove_unrooted_slots_purge_slots_stats", Some(0));

        let mut currently_contended_slots = slots_under_contention.lock().unwrap();
        for (remove_slot, _) in remove_slots {
            assert!(currently_contended_slots.remove(remove_slot));
        }
    }

    pub fn hash_account<T: ReadableAccount>(
        slot: Slot,
        account: &T,
        pubkey: &Pubkey,
        include_slot: IncludeSlotInHash,
    ) -> Hash {
        Self::hash_account_data(
            slot,
            account.lamports(),
            account.owner(),
            account.executable(),
            account.rent_epoch(),
            account.data(),
            pubkey,
            include_slot,
        )
    }

    fn hash_account_data(
        slot: Slot,
        lamports: u64,
        owner: &Pubkey,
        executable: bool,
        rent_epoch: Epoch,
        data: &[u8],
        pubkey: &Pubkey,
        include_slot: IncludeSlotInHash,
    ) -> Hash {
        if lamports == 0 {
            return Hash::default();
        }

        let mut hasher = blake3::Hasher::new();

        hasher.update(&lamports.to_le_bytes());

        match include_slot {
            IncludeSlotInHash::IncludeSlot => {
                // upon feature activation, stop including slot# in the account hash
                hasher.update(&slot.to_le_bytes());
            }
            IncludeSlotInHash::RemoveSlot => {}
            IncludeSlotInHash::IrrelevantAssertOnUse => {
                panic!("IncludeSlotInHash is irrelevant, but we are calculating hash");
            }
        }

        hasher.update(&rent_epoch.to_le_bytes());

        hasher.update(data);

        if executable {
            hasher.update(&[1u8; 1]);
        } else {
            hasher.update(&[0u8; 1]);
        }

        hasher.update(owner.as_ref());
        hasher.update(pubkey.as_ref());

        Hash::new_from_array(
            <[u8; solana_sdk::hash::HASH_BYTES]>::try_from(hasher.finalize().as_slice()).unwrap(),
        )
    }

    fn bulk_assign_write_version(&self, count: usize) -> StoredMetaWriteVersion {
        self.write_version
            .fetch_add(count as StoredMetaWriteVersion, Ordering::AcqRel)
    }

    fn write_accounts_to_storage<
        'a,
        'b,
        F: FnMut(Slot, usize) -> Arc<AccountStorageEntry>,
        T: ReadableAccount + Sync,
        U: StorableAccounts<'a, T>,
        V: Borrow<Hash>,
    >(
        &self,
        slot: Slot,
        mut storage_finder: F,
        accounts_and_meta_to_store: &StorableAccountsWithHashesAndWriteVersions<'a, 'b, T, U, V>,
    ) -> Vec<AccountInfo> {
        let mut infos: Vec<AccountInfo> = Vec::with_capacity(accounts_and_meta_to_store.len());
        let mut total_append_accounts_us = 0;
        let mut total_storage_find_us = 0;
        while infos.len() < accounts_and_meta_to_store.len() {
            let mut storage_find = Measure::start("storage_finder");
            let account = accounts_and_meta_to_store.account(infos.len());
            let data_len = account
                .map(|account| account.data().len())
                .unwrap_or_default();
            // ok if storage can't hold data + alignment at end - we just need the unaligned data to fit
            let storage = storage_finder(slot, data_len + STORE_META_OVERHEAD);
            storage_find.stop();
            total_storage_find_us += storage_find.as_us();
            let mut append_accounts = Measure::start("append_accounts");
            let rvs = storage
                .accounts
                .append_accounts(accounts_and_meta_to_store, infos.len());
            append_accounts.stop();
            total_append_accounts_us += append_accounts.as_us();
            if rvs.is_none() {
                storage.set_status(AccountStorageStatus::Full);

                // See if an account overflows the append vecs in the slot.
                let data_len = (data_len + STORE_META_OVERHEAD) as u64;
                if !self.has_space_available(slot, data_len) {
                    let special_store_size = std::cmp::max(data_len * 2, self.file_size);
                    if self
                        .try_recycle_and_insert_store(slot, special_store_size, std::u64::MAX)
                        .is_none()
                    {
                        self.create_and_insert_store(slot, special_store_size, "large create");
                    }
                }
                continue;
            }

            for (i, offsets) in rvs.unwrap().windows(2).enumerate() {
                let stored_size = offsets[1] - offsets[0];
                storage.add_account(stored_size);

                infos.push(AccountInfo::new(
                    StorageLocation::AppendVec(storage.append_vec_id(), offsets[0]),
                    stored_size as StoredSize, // stored_size should never exceed StoredSize::MAX because of max data len const
                    accounts_and_meta_to_store
                        .account(i)
                        .map(|account| account.lamports())
                        .unwrap_or_default(),
                ));
            }
            // restore the state to available
            storage.set_status(AccountStorageStatus::Available);
        }

        self.stats
            .store_append_accounts
            .fetch_add(total_append_accounts_us, Ordering::Relaxed);
        self.stats
            .store_find_store
            .fetch_add(total_storage_find_us, Ordering::Relaxed);

        infos
    }

    pub fn mark_slot_frozen(&self, slot: Slot) {
        if let Some(slot_cache) = self.accounts_cache.slot_cache(slot) {
            slot_cache.mark_slot_frozen();
            slot_cache.report_slot_store_metrics();
        }
        self.accounts_cache.report_size();
    }

    pub fn expire_old_recycle_stores(&self) {
        let mut recycle_stores_write_elapsed = Measure::start("recycle_stores_write_time");
        let recycle_stores = self.recycle_stores.write().unwrap().expire_old_entries();
        recycle_stores_write_elapsed.stop();

        let mut drop_storage_entries_elapsed = Measure::start("drop_storage_entries_elapsed");
        drop(recycle_stores);
        drop_storage_entries_elapsed.stop();

        self.clean_accounts_stats
            .purge_stats
            .drop_storage_entries_elapsed
            .fetch_add(drop_storage_entries_elapsed.as_us(), Ordering::Relaxed);
        self.clean_accounts_stats
            .purge_stats
            .recycle_stores_write_elapsed
            .fetch_add(recycle_stores_write_elapsed.as_us(), Ordering::Relaxed);
    }

    #[cfg(test)]
    pub(crate) fn flush_accounts_cache_slot_for_tests(&self, slot: Slot) {
        self.flush_slot_cache(slot);
    }

    /// true if write cache is too big
    fn should_aggressively_flush_cache(&self) -> bool {
        self.write_cache_limit_bytes
            .unwrap_or(WRITE_CACHE_LIMIT_BYTES_DEFAULT)
            < self.accounts_cache.size()
    }

    // `force_flush` flushes all the cached roots `<= requested_flush_root`. It also then
    // flushes:
    // 1) excess remaining roots or unrooted slots while 'should_aggressively_flush_cache' is true
    pub fn flush_accounts_cache(&self, force_flush: bool, requested_flush_root: Option<Slot>) {
        #[cfg(not(test))]
        assert!(requested_flush_root.is_some());

        if !force_flush && !self.should_aggressively_flush_cache() {
            return;
        }

        // Flush only the roots <= requested_flush_root, so that snapshotting has all
        // the relevant roots in storage.
        let mut flush_roots_elapsed = Measure::start("flush_roots_elapsed");
        let mut account_bytes_saved = 0;
        let mut num_accounts_saved = 0;

        let _guard = self.active_stats.activate(ActiveStatItem::Flush);

        // Note even if force_flush is false, we will still flush all roots <= the
        // given `requested_flush_root`, even if some of the later roots cannot be used for
        // cleaning due to an ongoing scan
        let (total_new_cleaned_roots, num_cleaned_roots_flushed) = self
            .flush_rooted_accounts_cache(
                requested_flush_root,
                Some((&mut account_bytes_saved, &mut num_accounts_saved)),
            );
        flush_roots_elapsed.stop();

        // Note we don't purge unrooted slots here because there may be ongoing scans/references
        // for those slot, let the Bank::drop() implementation do cleanup instead on dead
        // banks

        // If 'should_aggressively_flush_cache', then flush the excess ones to storage
        let (total_new_excess_roots, num_excess_roots_flushed) =
            if self.should_aggressively_flush_cache() {
                // Start by flushing the roots
                //
                // Cannot do any cleaning on roots past `requested_flush_root` because future
                // snapshots may need updates from those later slots, hence we pass `None`
                // for `should_clean`.
                self.flush_rooted_accounts_cache(None, None)
            } else {
                (0, 0)
            };

        let mut excess_slot_count = 0;
        let mut unflushable_unrooted_slot_count = 0;
        let max_flushed_root = self.accounts_cache.fetch_max_flush_root();
        if self.should_aggressively_flush_cache() {
            let old_slots = self.accounts_cache.cached_frozen_slots();
            excess_slot_count = old_slots.len();
            let mut flush_stats = FlushStats::default();
            old_slots.into_iter().for_each(|old_slot| {
                // Don't flush slots that are known to be unrooted
                if old_slot > max_flushed_root {
                    if self.should_aggressively_flush_cache() {
                        if let Some(stats) = self.flush_slot_cache(old_slot) {
                            flush_stats.num_flushed += stats.num_flushed;
                            flush_stats.num_purged += stats.num_purged;
                            flush_stats.total_size += stats.total_size;
                        }
                    }
                } else {
                    unflushable_unrooted_slot_count += 1;
                }
            });
            datapoint_info!(
                "accounts_db-flush_accounts_cache_aggressively",
                ("num_flushed", flush_stats.num_flushed, i64),
                ("num_purged", flush_stats.num_purged, i64),
                ("total_flush_size", flush_stats.total_size, i64),
                ("total_cache_size", self.accounts_cache.size(), i64),
                ("total_frozen_slots", excess_slot_count, i64),
                ("total_slots", self.accounts_cache.num_slots(), i64),
            );
        }

        datapoint_info!(
            "accounts_db-flush_accounts_cache",
            ("total_new_cleaned_roots", total_new_cleaned_roots, i64),
            ("num_cleaned_roots_flushed", num_cleaned_roots_flushed, i64),
            ("total_new_excess_roots", total_new_excess_roots, i64),
            ("num_excess_roots_flushed", num_excess_roots_flushed, i64),
            ("excess_slot_count", excess_slot_count, i64),
            (
                "unflushable_unrooted_slot_count",
                unflushable_unrooted_slot_count,
                i64
            ),
            (
                "flush_roots_elapsed",
                flush_roots_elapsed.as_us() as i64,
                i64
            ),
            ("account_bytes_saved", account_bytes_saved, i64),
            ("num_accounts_saved", num_accounts_saved, i64),
        );
    }

    fn flush_rooted_accounts_cache(
        &self,
        requested_flush_root: Option<Slot>,
        should_clean: Option<(&mut usize, &mut usize)>,
    ) -> (usize, usize) {
        let max_clean_root = should_clean.as_ref().and_then(|_| {
            // If there is a long running scan going on, this could prevent any cleaning
            // based on updates from slots > `max_clean_root`.
            self.max_clean_root(requested_flush_root)
        });

        let mut written_accounts = HashSet::new();

        // If `should_clean` is None, then`should_flush_f` is also None, which will cause
        // `flush_slot_cache` to flush all accounts to storage without cleaning any accounts.
        let mut should_flush_f = should_clean.map(|(account_bytes_saved, num_accounts_saved)| {
            move |&pubkey: &Pubkey, account: &AccountSharedData| {
                // if not in hashset, then not flushed previously, so flush it
                let should_flush = written_accounts.insert(pubkey);
                if !should_flush {
                    *account_bytes_saved += account.data().len();
                    *num_accounts_saved += 1;
                    // If a later root already wrote this account, no point
                    // in flushing it
                }
                should_flush
            }
        });

        // Always flush up to `requested_flush_root`, which is necessary for things like snapshotting.
        let cached_roots: BTreeSet<Slot> = self.accounts_cache.clear_roots(requested_flush_root);

        // Iterate from highest to lowest so that we don't need to flush earlier
        // outdated updates in earlier roots
        let mut num_roots_flushed = 0;
        for &root in cached_roots.iter().rev() {
            if self
                .flush_slot_cache_with_clean(&[root], should_flush_f.as_mut(), max_clean_root)
                .is_some()
            {
                num_roots_flushed += 1;
            }

            // Regardless of whether this slot was *just* flushed from the cache by the above
            // `flush_slot_cache()`, we should update the `max_flush_root`.
            // This is because some rooted slots may be flushed to storage *before* they are marked as root.
            // This can occur for instance when
            //  the cache is overwhelmed, we flushed some yet to be rooted frozen slots
            // These slots may then *later* be marked as root, so we still need to handle updating the
            // `max_flush_root` in the accounts cache.
            self.accounts_cache.set_max_flush_root(root);
        }

        // Only add to the uncleaned roots set *after* we've flushed the previous roots,
        // so that clean will actually be able to clean the slots.
        let num_new_roots = cached_roots.len();
        self.accounts_index.add_uncleaned_roots(cached_roots);
        (num_new_roots, num_roots_flushed)
    }

    fn do_flush_slot_cache(
        &self,
        slot: Slot,
        slot_cache: &SlotCache,
        mut should_flush_f: Option<&mut impl FnMut(&Pubkey, &AccountSharedData) -> bool>,
        max_clean_root: Option<Slot>,
    ) -> FlushStats {
        let mut num_purged = 0;
        let mut total_size = 0;
        let mut num_flushed = 0;
        let iter_items: Vec<_> = slot_cache.iter().collect();
        let mut purged_slot_pubkeys: HashSet<(Slot, Pubkey)> = HashSet::new();
        let mut pubkey_to_slot_set: Vec<(Pubkey, Slot)> = vec![];
        if should_flush_f.is_some() {
            if let Some(max_clean_root) = max_clean_root {
                if slot > max_clean_root {
                    // Only if the root is greater than the `max_clean_root` do we
                    // have to prevent cleaning, otherwise, just default to `should_flush_f`
                    // for any slots <= `max_clean_root`
                    should_flush_f = None;
                }
            }
        }

        let mut filler_accounts = 0;
        if self.filler_accounts_enabled() {
            let slots_remaining = self.filler_account_slots_remaining.load(Ordering::Acquire);
            if slots_remaining > 0 {
                // figure out
                let pr = self.get_prior_root(slot);

                if let Some(prior_root) = pr {
                    let filler_account_slots =
                        std::cmp::min(slot.saturating_sub(prior_root), slots_remaining);
                    self.filler_account_slots_remaining
                        .fetch_sub(filler_account_slots, Ordering::Release);
                    let filler_accounts_per_slot =
                        self.filler_accounts_per_slot.load(Ordering::Acquire);
                    filler_accounts = filler_account_slots * filler_accounts_per_slot;

                    // keep space for filler accounts
                    let addl_size = filler_accounts
                        * (aligned_stored_size(self.filler_accounts_config.size) as u64);
                    total_size += addl_size;
                }
            }
        }

        let (accounts, hashes): (Vec<(&Pubkey, &AccountSharedData)>, Vec<Hash>) = iter_items
            .iter()
            .filter_map(|iter_item| {
                let key = iter_item.key();
                let account = &iter_item.value().account;
                let should_flush = should_flush_f
                    .as_mut()
                    .map(|should_flush_f| should_flush_f(key, account))
                    .unwrap_or(true);
                if should_flush {
                    let hash = iter_item.value().hash();
                    total_size += aligned_stored_size(account.data().len()) as u64;
                    num_flushed += 1;
                    Some(((key, account), hash))
                } else {
                    // If we don't flush, we have to remove the entry from the
                    // index, since it's equivalent to purging
                    purged_slot_pubkeys.insert((slot, *key));
                    pubkey_to_slot_set.push((*key, slot));
                    num_purged += 1;
                    None
                }
            })
            .unzip();

        let is_dead_slot = accounts.is_empty();
        // Remove the account index entries from earlier roots that are outdated by later roots.
        // Safe because queries to the index will be reading updates from later roots.
        self.purge_slot_cache_pubkeys(
            slot,
            purged_slot_pubkeys,
            pubkey_to_slot_set,
            is_dead_slot,
            &HashSet::default(),
        );

        if !is_dead_slot {
            let aligned_total_size = Self::page_align(total_size);
            // This ensures that all updates are written to an AppendVec, before any
            // updates to the index happen, so anybody that sees a real entry in the index,
            // will be able to find the account in storage
            let flushed_store =
                self.create_and_insert_store(slot, aligned_total_size, "flush_slot_cache");
            // irrelevant - account will already be hashed since it was used in bank hash previously
            let include_slot_in_hash = IncludeSlotInHash::IrrelevantAssertOnUse;
            self.store_accounts_frozen(
                (slot, &accounts[..], include_slot_in_hash),
                Some(hashes),
                Some(&flushed_store),
                None,
                StoreReclaims::Default,
            );

            if filler_accounts > 0 {
                // add extra filler accounts at the end of the append vec
                let (account, hash) = self.get_filler_account(&Rent::default());
                let mut accounts = Vec::with_capacity(filler_accounts as usize);
                let mut hashes = Vec::with_capacity(filler_accounts as usize);
                let pubkeys = self.get_filler_account_pubkeys(filler_accounts as usize);
                pubkeys.iter().for_each(|key| {
                    accounts.push((key, &account));
                    hashes.push(hash);
                });
                self.store_accounts_frozen(
                    (slot, &accounts[..], include_slot_in_hash),
                    Some(hashes),
                    Some(&flushed_store),
                    None,
                    StoreReclaims::Ignore,
                );
            }

            // If the above sizing function is correct, just one AppendVec is enough to hold
            // all the data for the slot
            assert_eq!(
                self.storage
                    .get_slot_stores(slot)
                    .unwrap()
                    .read()
                    .unwrap()
                    .len(),
                1
            );
        }

        // Remove this slot from the cache, which will to AccountsDb's new readers should look like an
        // atomic switch from the cache to storage.
        // There is some racy condition for existing readers who just has read exactly while
        // flushing. That case is handled by retry_to_get_account_accessor()
        assert!(self.accounts_cache.remove_slot(slot).is_some());
        FlushStats {
            num_flushed,
            num_purged,
            total_size,
        }
    }

    /// flush all accounts in this slot
    fn flush_slot_cache(&self, slot: Slot) -> Option<FlushStats> {
        self.flush_slot_cache_with_clean(&[slot], None::<&mut fn(&_, &_) -> bool>, None)
    }

    /// `should_flush_f` is an optional closure that determines whether a given
    /// account should be flushed. Passing `None` will by default flush all
    /// accounts
    fn flush_slot_cache_with_clean(
        &self,
        slots: &[Slot],
        should_flush_f: Option<&mut impl FnMut(&Pubkey, &AccountSharedData) -> bool>,
        max_clean_root: Option<Slot>,
    ) -> Option<FlushStats> {
        assert_eq!(1, slots.len());
        let slot = slots[0];
        if self
            .remove_unrooted_slots_synchronization
            .slots_under_contention
            .lock()
            .unwrap()
            .insert(slot)
        {
            // We have not seen this slot, flush it.
            let flush_stats = self.accounts_cache.slot_cache(slot).map(|slot_cache| {
                #[cfg(test)]
                {
                    // Give some time for cache flushing to occur here for unit tests
                    sleep(Duration::from_millis(self.load_delay));
                }
                // Since we added the slot to `slots_under_contention` AND this slot
                // still exists in the cache, we know the slot cannot be removed
                // by any other threads past this point. We are now responsible for
                // flushing this slot.
                self.do_flush_slot_cache(slot, &slot_cache, should_flush_f, max_clean_root)
            });

            // Nobody else should have been purging this slot, so should not have been removed
            // from `self.remove_unrooted_slots_synchronization`.

            slots.iter().for_each(|slot| {
                assert!(self
                    .remove_unrooted_slots_synchronization
                    .slots_under_contention
                    .lock()
                    .unwrap()
                    .remove(slot));
            });

            // Signal to any threads blocked on `remove_unrooted_slots(slot)` that we have finished
            // flushing
            self.remove_unrooted_slots_synchronization
                .signal
                .notify_all();
            flush_stats
        } else {
            // We have already seen this slot. It is already under flushing. Skip.
            None
        }
    }

    fn write_accounts_to_cache<'a, 'b, T: ReadableAccount + Sync, P>(
        &self,
        slot: Slot,
        accounts_and_meta_to_store: &impl StorableAccounts<'b, T>,
        txn_signatures_iter: Box<dyn std::iter::Iterator<Item = &Option<&Signature>> + 'a>,
        include_slot_in_hash: IncludeSlotInHash,
        mut write_version_producer: P,
    ) -> Vec<AccountInfo>
    where
        P: Iterator<Item = u64>,
    {
        txn_signatures_iter
            .enumerate()
            .map(|(i, signature)| {
                let account = accounts_and_meta_to_store
                    .account_default_if_zero_lamport(i)
                    .map(|account| account.to_account_shared_data())
                    .unwrap_or_default();
                let account_info = AccountInfo::new(
                    StorageLocation::Cached,
                    CACHE_VIRTUAL_STORED_SIZE,
                    account.lamports(),
                );

                self.notify_account_at_accounts_update(
                    slot,
                    &account,
                    signature,
                    accounts_and_meta_to_store.pubkey(i),
                    &mut write_version_producer,
                );

                let cached_account = self.accounts_cache.store(
                    slot,
                    accounts_and_meta_to_store.pubkey(i),
                    account,
                    None::<&Hash>,
                    include_slot_in_hash,
                );
                // hash this account in the bg
                match &self.sender_bg_hasher {
                    Some(ref sender) => {
                        let _ = sender.send(cached_account);
                    }
                    None => (),
                };
                account_info
            })
            .collect()
    }

    fn store_accounts_to<
        'a: 'c,
        'b,
        'c,
        F: FnMut(Slot, usize) -> Arc<AccountStorageEntry>,
        P: Iterator<Item = u64>,
        T: ReadableAccount + Sync + ZeroLamport + 'b,
    >(
        &self,
        accounts: &'c impl StorableAccounts<'b, T>,
        hashes: Option<Vec<impl Borrow<Hash>>>,
        storage_finder: F,
        mut write_version_producer: P,
        is_cached_store: bool,
        txn_signatures: Option<&[Option<&'a Signature>]>,
    ) -> Vec<AccountInfo> {
        let mut calc_stored_meta_time = Measure::start("calc_stored_meta");
        let slot = accounts.target_slot();
        (0..accounts.len()).for_each(|index| {
            let pubkey = accounts.pubkey(index);
            self.read_only_accounts_cache.remove(*pubkey, slot);
        });
        calc_stored_meta_time.stop();
        self.stats
            .calc_stored_meta
            .fetch_add(calc_stored_meta_time.as_us(), Ordering::Relaxed);

        if is_cached_store {
            let signature_iter: Box<dyn std::iter::Iterator<Item = &Option<&Signature>>> =
                match txn_signatures {
                    Some(txn_signatures) => {
                        assert_eq!(txn_signatures.len(), accounts.len());
                        Box::new(txn_signatures.iter())
                    }
                    None => Box::new(std::iter::repeat(&None).take(accounts.len())),
                };

            self.write_accounts_to_cache(
                slot,
                accounts,
                signature_iter,
                accounts.include_slot_in_hash(),
                write_version_producer,
            )
        } else if accounts.has_hash_and_write_version() {
            self.write_accounts_to_storage(
                slot,
                storage_finder,
                &StorableAccountsWithHashesAndWriteVersions::<'_, '_, _, _, &Hash>::new(accounts),
            )
        } else {
            let write_versions = (0..accounts.len())
                .map(|_| write_version_producer.next().unwrap())
                .collect::<Vec<_>>();
            match hashes {
                Some(hashes) => self.write_accounts_to_storage(
                    slot,
                    storage_finder,
                    &StorableAccountsWithHashesAndWriteVersions::new_with_hashes_and_write_versions(
                        accounts,
                        hashes,
                        write_versions,
                    ),
                ),
                None => {
                    // hash any accounts where we were lazy in calculating the hash
                    let mut hash_time = Measure::start("hash_accounts");
                    let len = accounts.len();
                    let mut hashes = Vec::with_capacity(len);
                    for index in 0..accounts.len() {
                        let (pubkey, account) = (accounts.pubkey(index), accounts.account(index));
                        let hash = Self::hash_account(
                            slot,
                            account,
                            pubkey,
                            accounts.include_slot_in_hash(),
                        );
                        hashes.push(hash);
                    }
                    hash_time.stop();
                    self.stats
                        .store_hash_accounts
                        .fetch_add(hash_time.as_us(), Ordering::Relaxed);

                    self.write_accounts_to_storage(
                            slot,
                            storage_finder,
                            &StorableAccountsWithHashesAndWriteVersions::new_with_hashes_and_write_versions(accounts, hashes, write_versions),
                        )
                }
            }
        }
    }

    fn report_store_stats(&self) {
        let mut total_count = 0;
        let mut min = std::usize::MAX;
        let mut min_slot = 0;
        let mut max = 0;
        let mut max_slot = 0;
        let mut newest_slot = 0;
        let mut oldest_slot = std::u64::MAX;
        let mut total_bytes = 0;
        let mut total_alive_bytes = 0;
        for iter_item in self.storage.iter() {
            let slot = iter_item.key();
            let slot_stores = iter_item.value().read().unwrap();
            total_count += slot_stores.len();
            if slot_stores.len() < min {
                min = slot_stores.len();
                min_slot = *slot;
            }

            if slot_stores.len() > max {
                max = slot_stores.len();
                max_slot = *slot;
            }
            if *slot > newest_slot {
                newest_slot = *slot;
            }

            if *slot < oldest_slot {
                oldest_slot = *slot;
            }

            for store in slot_stores.values() {
                total_alive_bytes += Self::page_align(store.alive_bytes() as u64);
                total_bytes += store.total_bytes();
            }
        }
        info!("total_stores: {}, newest_slot: {}, oldest_slot: {}, max_slot: {} (num={}), min_slot: {} (num={})",
              total_count, newest_slot, oldest_slot, max_slot, max, min_slot, min);

        let total_alive_ratio = if total_bytes > 0 {
            total_alive_bytes as f64 / total_bytes as f64
        } else {
            0.
        };

        datapoint_info!(
            "accounts_db-stores",
            ("total_count", total_count, i64),
            (
                "recycle_count",
                self.recycle_stores.read().unwrap().entry_count() as u64,
                i64
            ),
            ("total_bytes", total_bytes, i64),
            ("total_alive_bytes", total_alive_bytes, i64),
            ("total_alive_ratio", total_alive_ratio, f64),
        );
        datapoint_info!(
            "accounts_db-perf-stats",
            (
                "delta_hash_num",
                self.stats.delta_hash_num.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "delta_hash_scan_us",
                self.stats
                    .delta_hash_scan_time_total_us
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "delta_hash_accumulate_us",
                self.stats
                    .delta_hash_accumulate_time_total_us
                    .swap(0, Ordering::Relaxed),
                i64
            ),
        );
    }

    /// find slot >= 'slot' which is a root or in 'ancestors'
    pub fn find_unskipped_slot(&self, slot: Slot, ancestors: Option<&Ancestors>) -> Option<Slot> {
        self.accounts_index.get_next_original_root(slot, ancestors)
    }

    pub fn checked_iterative_sum_for_capitalization(total_cap: u64, new_cap: u64) -> u64 {
        let new_total = total_cap as u128 + new_cap as u128;
        AccountsHasher::checked_cast_for_capitalization(new_total)
    }

    pub fn checked_sum_for_capitalization<T: Iterator<Item = u64>>(balances: T) -> u64 {
        AccountsHasher::checked_cast_for_capitalization(balances.map(|b| b as u128).sum::<u128>())
    }

    pub fn calculate_accounts_hash_from_index(
        &self,
        max_slot: Slot,
        config: &CalcAccountsHashConfig<'_>,
    ) -> Result<(AccountsHash, u64), BankHashVerificationError> {
        use BankHashVerificationError::*;
        let mut collect = Measure::start("collect");
        let keys: Vec<_> = self
            .accounts_index
            .account_maps
            .iter()
            .flat_map(|map| {
                let mut keys = map.keys();
                keys.sort_unstable(); // hashmap is not ordered, but bins are relative to each other
                keys
            })
            .collect();
        collect.stop();

        let mut scan = Measure::start("scan");
        let mismatch_found = AtomicU64::new(0);
        // Pick a chunk size big enough to allow us to produce output vectors that are smaller than the overall size.
        // We'll also accumulate the lamports within each chunk and fewer chunks results in less contention to accumulate the sum.
        let chunks = crate::accounts_hash::MERKLE_FANOUT.pow(4);
        let total_lamports = Mutex::<u64>::new(0);
        let stats = HashStats::default();

        let get_hashes = || {
            keys.par_chunks(chunks)
                .map(|pubkeys| {
                    let mut sum = 0u128;
                    let result: Vec<Hash> = pubkeys
                        .iter()
                        .filter_map(|pubkey| {
                            if self.is_filler_account(pubkey) {
                                return None;
                            }
                            if let AccountIndexGetResult::Found(lock, index) =
                                self.accounts_index.get(pubkey, config.ancestors, Some(max_slot))
                            {
                                let (slot, account_info) = &lock.slot_list()[index];
                                if !account_info.is_zero_lamport() {
                                    // Because we're keeping the `lock' here, there is no need
                                    // to use retry_to_get_account_accessor()
                                    // In other words, flusher/shrinker/cleaner is blocked to
                                    // cause any Accessor(None) situation.
                                    // Anyway this race condition concern is currently a moot
                                    // point because calculate_accounts_hash() should not
                                    // currently race with clean/shrink because the full hash
                                    // is synchronous with clean/shrink in
                                    // AccountsBackgroundService
                                    self.get_account_accessor(
                                        *slot,
                                        pubkey,
                                        &account_info.storage_location(),
                                    )
                                    .get_loaded_account()
                                    .and_then(
                                        |loaded_account| {
                                            let loaded_hash = loaded_account.loaded_hash();
                                            let balance = loaded_account.lamports();
                                            if config.check_hash && !self.is_filler_account(pubkey) {  // this will not be supported anymore
                                                let computed_hash =
                                                    loaded_account.compute_hash(*slot, pubkey, INCLUDE_SLOT_IN_HASH_IRRELEVANT_CHECK_HASH);
                                                if computed_hash != loaded_hash {
                                                    info!("hash mismatch found: computed: {}, loaded: {}, pubkey: {}", computed_hash, loaded_hash, pubkey);
                                                    mismatch_found
                                                        .fetch_add(1, Ordering::Relaxed);
                                                    return None;
                                                }
                                            }

                                            sum += balance as u128;
                                            Some(loaded_hash)
                                        },
                                    )
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        })
                        .collect();
                    let mut total = total_lamports.lock().unwrap();
                    *total =
                        AccountsHasher::checked_cast_for_capitalization(*total as u128 + sum);
                    result
                }).collect()
        };

        let hashes: Vec<Vec<Hash>> = if config.check_hash {
            get_hashes()
        } else {
            self.thread_pool_clean.install(get_hashes)
        };
        if mismatch_found.load(Ordering::Relaxed) > 0 {
            warn!(
                "{} mismatched account hash(es) found",
                mismatch_found.load(Ordering::Relaxed)
            );
            return Err(MismatchedAccountHash);
        }

        scan.stop();
        let total_lamports = *total_lamports.lock().unwrap();

        let mut hash_time = Measure::start("hash");
        let (accumulated_hash, hash_total) = AccountsHasher::calculate_hash(hashes);
        hash_time.stop();
        datapoint_info!(
            "calculate_accounts_hash_from_index",
            ("accounts_scan", scan.as_us(), i64),
            ("hash", hash_time.as_us(), i64),
            ("hash_total", hash_total, i64),
            ("collect", collect.as_us(), i64),
            (
                "rehashed_rewrites",
                stats.rehash_required.load(Ordering::Relaxed),
                i64
            ),
            (
                "rehashed_rewrites_unnecessary",
                stats.rehash_unnecessary.load(Ordering::Relaxed),
                i64
            ),
        );
        self.assert_safe_squashing_accounts_hash(max_slot, config.epoch_schedule);

        let accounts_hash = AccountsHash(accumulated_hash);
        Ok((accounts_hash, total_lamports))
    }

    pub fn get_accounts_hash(&self, slot: Slot) -> AccountsHash {
        let bank_hashes = self.bank_hashes.read().unwrap();
        let bank_hash_info = bank_hashes.get(&slot).unwrap();
        bank_hash_info.accounts_hash
    }

    pub fn update_accounts_hash_for_tests(
        &self,
        slot: Slot,
        ancestors: &Ancestors,
        debug_verify: bool,
        is_startup: bool,
    ) -> (AccountsHash, u64) {
        self.update_accounts_hash(
            CalcAccountsHashDataSource::IndexForTests,
            debug_verify,
            slot,
            ancestors,
            None,
            &EpochSchedule::default(),
            &RentCollector::default(),
            is_startup,
        )
    }

    fn scan_multiple_account_storages_one_slot<S>(
        storages: &[Arc<AccountStorageEntry>],
        scanner: &mut S,
    ) where
        S: AppendVecScan,
    {
        let mut len = storages.len();
        if len == 1 {
            // only 1 storage, so no need to interleave between multiple storages based on write_version
            storages[0].accounts.account_iter().for_each(|account| {
                if scanner.filter(account.pubkey()) {
                    scanner.found_account(&LoadedAccount::Stored(account))
                }
            });
        } else {
            // we have to call the scan_func in order of write_version within a slot if there are multiple storages per slot
            let mut progress = Vec::with_capacity(len);
            let mut current =
                Vec::<(StoredMetaWriteVersion, Option<StoredAccountMeta<'_>>)>::with_capacity(len);
            for storage in storages {
                let mut iterator = storage.accounts.account_iter();
                if let Some(item) = iterator.next().map(|stored_account| {
                    (
                        stored_account.meta.write_version_obsolete,
                        Some(stored_account),
                    )
                }) {
                    current.push(item);
                    progress.push(iterator);
                }
            }
            while !progress.is_empty() {
                let mut min = current[0].0;
                let mut min_index = 0;
                for (i, (item, _)) in current.iter().enumerate().take(len).skip(1) {
                    if item < &min {
                        min_index = i;
                        min = *item;
                    }
                }
                let found_account = &mut current[min_index];
                if scanner.filter(
                    found_account
                        .1
                        .as_ref()
                        .map(|stored_account| stored_account.pubkey())
                        .unwrap(), // will always be 'Some'
                ) {
                    let account = std::mem::take(found_account);
                    scanner.found_account(&LoadedAccount::Stored(account.1.unwrap()));
                }
                let next = progress[min_index].next().map(|stored_account| {
                    (
                        stored_account.meta.write_version_obsolete,
                        Some(stored_account),
                    )
                });
                match next {
                    Some(item) => {
                        current[min_index] = item;
                    }
                    None => {
                        current.remove(min_index);
                        progress.remove(min_index);
                        len -= 1;
                    }
                }
            }
        }
    }

    fn update_old_slot_stats(&self, stats: &HashStats, sub_storages: Option<&SnapshotStorage>) {
        if let Some(sub_storages) = sub_storages {
            stats.roots_older_than_epoch.fetch_add(1, Ordering::Relaxed);
            let mut ancients = 0;
            let num_accounts = sub_storages
                .iter()
                .map(|storage| {
                    if is_ancient(&storage.accounts) {
                        ancients += 1;
                    }
                    storage.count()
                })
                .sum();
            let sizes = sub_storages
                .iter()
                .map(|storage| storage.total_bytes())
                .sum::<u64>();
            stats
                .append_vec_sizes_older_than_epoch
                .fetch_add(sizes as usize, Ordering::Relaxed);
            stats
                .accounts_in_roots_older_than_epoch
                .fetch_add(num_accounts, Ordering::Relaxed);
            stats
                .ancient_append_vecs
                .fetch_add(ancients, Ordering::Relaxed);
        }
    }

    /// if ancient append vecs are enabled, return a slot 'max_slot_inclusive' - (slots_per_epoch - `self.ancient_append_vec_offset`)
    /// otherwise, return 0
    fn get_one_epoch_old_slot_for_hash_calc_scan(
        &self,
        max_slot_inclusive: Slot,
        config: &CalcAccountsHashConfig<'_>,
    ) -> Slot {
        if let Some(offset) = self.ancient_append_vec_offset {
            // we are going to use a fixed slots per epoch here.
            // We are mainly interested in the network at steady state.
            let slots_in_epoch = config.epoch_schedule.slots_per_epoch;
            // For performance, this is required when ancient appendvecs are enabled
            max_slot_inclusive
                .saturating_sub(slots_in_epoch)
                .saturating_add(offset)
        } else {
            // This causes the entire range to be chunked together, treating older append vecs just like new ones.
            // This performs well if there are many old append vecs that haven't been cleaned yet.
            // 0 will have the effect of causing ALL older append vecs to be chunked together, just like every other append vec.
            0
        }
    }

    /// hash info about 'storages' into 'hasher'
    /// return true iff storages are valid for loading from cache
    fn hash_storage_info(
        hasher: &mut impl StdHasher,
        storages: Option<&SnapshotStorage>,
        slot: Slot,
    ) -> bool {
        if let Some(sub_storages) = storages {
            if sub_storages.len() > 1 {
                // Having > 1 appendvecs per slot is not expected. If we have that, we just fail to load from the cache for this slot.
                return false;
            }
            // hash info about this storage
            let append_vec = sub_storages.first().unwrap();
            append_vec.written_bytes().hash(hasher);
            let storage_file = append_vec.accounts.get_path();
            slot.hash(hasher);
            storage_file.hash(hasher);
            let amod = std::fs::metadata(storage_file);
            if amod.is_err() {
                return false;
            }
            let amod = amod.unwrap().modified();
            if amod.is_err() {
                return false;
            }
            let amod = amod
                .unwrap()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            amod.hash(hasher);
        }
        // if we made it here, we have hashed info and we should try to load from the cache
        true
    }

    /// Scan through all the account storage in parallel.
    /// Returns a Vec of open/mmapped files.
    /// Each file has serialized hash info, sorted by pubkey and then slot, from scanning the append vecs.
    ///   A single pubkey could be in multiple entries. The pubkey found in the latest entry is the one to use.
    fn scan_account_storage_no_bank<S>(
        &self,
        cache_hash_data: &CacheHashData,
        config: &CalcAccountsHashConfig<'_>,
        snapshot_storages: &SortedStorages,
        scanner: S,
        bin_range: &Range<usize>,
        stats: &HashStats,
    ) -> Vec<CacheHashDataFile>
    where
        S: AppendVecScan,
    {
        let splitter = SplitAncientStorages::new(
            self.get_one_epoch_old_slot_for_hash_calc_scan(
                snapshot_storages.max_slot_inclusive(),
                config,
            ),
            snapshot_storages,
        );

        (0..splitter.chunk_count)
            .into_par_iter()
            .map(|chunk| {
                let mut scanner = scanner.clone();

                let range_this_chunk = splitter.get_slot_range(chunk)?;

                let slots_per_epoch = config
                    .rent_collector
                    .epoch_schedule
                    .get_slots_in_epoch(config.rent_collector.epoch);
                let one_epoch_old = snapshot_storages
                    .range()
                    .end
                    .saturating_sub(slots_per_epoch);

                let file_name = {
                    let mut load_from_cache = true;
                    let mut hasher = std::collections::hash_map::DefaultHasher::new();
                    bin_range.start.hash(&mut hasher);
                    bin_range.end.hash(&mut hasher);
                    let is_first_scan_pass = bin_range.start == 0;

                    // calculate hash representing all storages in this chunk
                    for (slot, sub_storages) in snapshot_storages.iter_range(&range_this_chunk) {
                        if is_first_scan_pass && slot < one_epoch_old {
                            self.update_old_slot_stats(stats, sub_storages);
                        }
                        if !Self::hash_storage_info(&mut hasher, sub_storages, slot) {
                            load_from_cache = false;
                            break;
                        }
                    }
                    // we have a hash value for all the storages in this slot
                    // so, build a file name:
                    let hash = hasher.finish();
                    let file_name = format!(
                        "{}.{}.{}.{}.{}",
                        range_this_chunk.start,
                        range_this_chunk.end,
                        bin_range.start,
                        bin_range.end,
                        hash
                    );
                    if load_from_cache {
                        if let Ok(mapped_file) = cache_hash_data.load_map(&file_name) {
                            return Some(mapped_file);
                        }
                    }

                    // fall through and load normally - we failed to load from a cache file
                    file_name
                };

                let mut init_accum = true;
                // load from cache failed, so create the cache file for this chunk
                for (slot, sub_storages) in snapshot_storages.iter_range(&range_this_chunk) {
                    let mut ancient = false;
                    let (_, scan) = measure!(if let Some(sub_storages) = sub_storages {
                        if let Some(storage) = sub_storages.first() {
                            ancient = is_ancient(&storage.accounts);
                        }
                        if init_accum {
                            let range = bin_range.end - bin_range.start;
                            scanner.init_accum(range);
                            init_accum = false;
                        }
                        scanner.set_slot(slot);

                        Self::scan_multiple_account_storages_one_slot(sub_storages, &mut scanner);
                    });
                    if ancient {
                        stats
                            .sum_ancient_scans_us
                            .fetch_add(scan.as_us(), Ordering::Relaxed);
                        stats.count_ancient_scans.fetch_add(1, Ordering::Relaxed);
                        stats
                            .longest_ancient_scan_us
                            .fetch_max(scan.as_us(), Ordering::Relaxed);
                    }
                }
                (!init_accum)
                    .then(|| {
                        let r = scanner.scanning_complete();
                        assert!(!file_name.is_empty());
                        (!r.is_empty() && r.iter().any(|b| !b.is_empty())).then(|| {
                            // error if we can't write this
                            cache_hash_data.save(&file_name, &r).unwrap();
                            cache_hash_data.load_map(&file_name).unwrap()
                        })
                    })
                    .flatten()
            })
            .filter_map(|x| x)
            .collect()
    }

    /// storages are sorted by slot and have range info.
    /// add all stores older than slots_per_epoch to dirty_stores so clean visits these slots
    fn mark_old_slots_as_dirty(
        &self,
        storages: &SortedStorages,
        slots_per_epoch: Slot,
        mut stats: &mut crate::accounts_hash::HashStats,
    ) {
        let mut mark_time = Measure::start("mark_time");
        let mut num_dirty_slots: usize = 0;
        let max = storages.max_slot_inclusive();
        let acceptable_straggler_slot_count = 100; // do nothing special for these old stores which will likely get cleaned up shortly
        let sub = slots_per_epoch + acceptable_straggler_slot_count;
        let in_epoch_range_start = max.saturating_sub(sub);
        for (slot, storages) in storages.iter_range(&(..in_epoch_range_start)) {
            if let Some(storages) = storages {
                storages.iter().for_each(|store| {
                    if !is_ancient(&store.accounts) {
                        // ancient stores are managed separately - we expect them to be old and keeping accounts
                        // We can expect the normal processes will keep them cleaned.
                        // If we included them here then ALL accounts in ALL ancient append vecs will be visited by clean each time.
                        self.dirty_stores
                            .insert((slot, store.append_vec_id()), store.clone());
                        num_dirty_slots += 1;
                    }
                });
            }
        }
        mark_time.stop();
        stats.mark_time_us = mark_time.as_us();
        stats.num_dirty_slots = num_dirty_slots;
    }

    pub(crate) fn calculate_accounts_hash(
        &self,
        data_source: CalcAccountsHashDataSource,
        slot: Slot,
        config: &CalcAccountsHashConfig<'_>,
    ) -> Result<(AccountsHash, u64), BankHashVerificationError> {
        match data_source {
            CalcAccountsHashDataSource::Storages => {
                if self.accounts_cache.contains_any_slots(slot) {
                    // this indicates a race condition
                    inc_new_counter_info!("accounts_hash_items_in_write_cache", 1);
                }

                let mut collect_time = Measure::start("collect");
                let (combined_maps, slots) = self.get_snapshot_storages(..=slot, config.ancestors);
                collect_time.stop();

                let mut sort_time = Measure::start("sort_storages");
                let min_root = self.accounts_index.min_alive_root();
                let storages = SortedStorages::new_with_slots(
                    combined_maps.iter().zip(slots.into_iter()),
                    min_root,
                    Some(slot),
                );
                sort_time.stop();

                let mut timings = HashStats {
                    collect_snapshots_us: collect_time.as_us(),
                    storage_sort_us: sort_time.as_us(),
                    ..HashStats::default()
                };
                timings.calc_storage_size_quartiles(&combined_maps);

                self.calculate_accounts_hash_from_storages(config, &storages, timings)
            }
            CalcAccountsHashDataSource::IndexForTests => {
                self.calculate_accounts_hash_from_index(slot, config)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn calculate_accounts_hash_with_verify(
        &self,
        data_source: CalcAccountsHashDataSource,
        debug_verify: bool,
        slot: Slot,
        config: CalcAccountsHashConfig<'_>,
        expected_capitalization: Option<u64>,
    ) -> Result<(AccountsHash, u64), BankHashVerificationError> {
        let (accounts_hash, total_lamports) =
            self.calculate_accounts_hash(data_source, slot, &config)?;
        if debug_verify {
            // calculate the other way (store or non-store) and verify results match.
            let data_source_other = match data_source {
                CalcAccountsHashDataSource::IndexForTests => CalcAccountsHashDataSource::Storages,
                CalcAccountsHashDataSource::Storages => CalcAccountsHashDataSource::IndexForTests,
            };
            let (accounts_hash_other, total_lamports_other) =
                self.calculate_accounts_hash(data_source_other, slot, &config)?;

            let success = accounts_hash == accounts_hash_other
                && total_lamports == total_lamports_other
                && total_lamports == expected_capitalization.unwrap_or(total_lamports);
            assert!(success, "calculate_accounts_hash_with_verify mismatch. hashes: {}, {}; lamports: {}, {}; expected lamports: {:?}, data source: {:?}, slot: {}", accounts_hash.0, accounts_hash_other.0, total_lamports, total_lamports_other, expected_capitalization, data_source, slot);
        }
        Ok((accounts_hash, total_lamports))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_accounts_hash(
        &self,
        data_source: CalcAccountsHashDataSource,
        debug_verify: bool,
        slot: Slot,
        ancestors: &Ancestors,
        expected_capitalization: Option<u64>,
        epoch_schedule: &EpochSchedule,
        rent_collector: &RentCollector,
        is_startup: bool,
    ) -> (AccountsHash, u64) {
        let check_hash = false;
        let (accounts_hash, total_lamports) = self
            .calculate_accounts_hash_with_verify(
                data_source,
                debug_verify,
                slot,
                CalcAccountsHashConfig {
                    use_bg_thread_pool: !is_startup,
                    check_hash,
                    ancestors: Some(ancestors),
                    epoch_schedule,
                    rent_collector,
                    store_detailed_debug_info_on_failure: false,
                    full_snapshot: None,
                },
                expected_capitalization,
            )
            .unwrap(); // unwrap here will never fail since check_hash = false
        self.set_accounts_hash(slot, accounts_hash);
        (accounts_hash, total_lamports)
    }

    /// update hash for this slot in the 'bank_hashes' map
    pub(crate) fn set_accounts_hash(&self, slot: Slot, accounts_hash: AccountsHash) {
        let mut bank_hashes = self.bank_hashes.write().unwrap();
        let mut bank_hash_info = bank_hashes.get_mut(&slot).unwrap();
        bank_hash_info.accounts_hash = accounts_hash;
    }

    /// scan 'storages', return a vec of 'CacheHashDataFile', one per pass
    fn scan_snapshot_stores_with_cache(
        &self,
        cache_hash_data: &CacheHashData,
        storages: &SortedStorages,
        mut stats: &mut crate::accounts_hash::HashStats,
        bins: usize,
        bin_range: &Range<usize>,
        config: &CalcAccountsHashConfig<'_>,
        filler_account_suffix: Option<&Pubkey>,
    ) -> Result<Vec<CacheHashDataFile>, BankHashVerificationError> {
        let bin_calculator = PubkeyBinCalculator24::new(bins);
        assert!(bin_range.start < bins && bin_range.end <= bins && bin_range.start < bin_range.end);
        let mut time = Measure::start("scan all accounts");
        stats.num_snapshot_storage = storages.storage_count();
        stats.num_slots = storages.slot_count();
        let mismatch_found = Arc::new(AtomicU64::new(0));
        let range = bin_range.end - bin_range.start;
        let sort_time = Arc::new(AtomicU64::new(0));

        let scanner = ScanState {
            current_slot: Slot::default(),
            accum: BinnedHashData::default(),
            bin_calculator: &bin_calculator,
            config,
            mismatch_found: mismatch_found.clone(),
            filler_account_suffix,
            range,
            bin_range,
            sort_time: sort_time.clone(),
            pubkey_to_bin_index: 0,
        };

        let result = self.scan_account_storage_no_bank(
            cache_hash_data,
            config,
            storages,
            scanner,
            bin_range,
            stats,
        );

        stats.sort_time_total_us += sort_time.load(Ordering::Relaxed);

        if config.check_hash && mismatch_found.load(Ordering::Relaxed) > 0 {
            warn!(
                "{} mismatched account hash(es) found",
                mismatch_found.load(Ordering::Relaxed)
            );
            return Err(BankHashVerificationError::MismatchedAccountHash);
        }

        time.stop();
        stats.scan_time_total_us += time.as_us();

        Ok(result)
    }

    fn sort_slot_storage_scan(accum: BinnedHashData) -> (BinnedHashData, u64) {
        let time = AtomicU64::new(0);
        (
            accum
                .into_iter()
                .map(|mut items| {
                    let mut sort_time = Measure::start("sort");
                    {
                        // sort_by vs unstable because slot and write_version are already in order
                        items.sort_by(AccountsHasher::compare_two_hash_entries);
                    }
                    sort_time.stop();
                    time.fetch_add(sort_time.as_us(), Ordering::Relaxed);
                    items
                })
                .collect(),
            time.load(Ordering::Relaxed),
        )
    }

    /// if we ever try to calc hash where there are squashed append vecs within the last epoch, we will fail
    fn assert_safe_squashing_accounts_hash(&self, slot: Slot, epoch_schedule: &EpochSchedule) {
        let previous = self.get_accounts_hash_complete_one_epoch_old();
        let current = Self::get_slot_one_epoch_prior(slot, epoch_schedule);
        assert!(
            previous <= current,
            "get_accounts_hash_complete_one_epoch_old: {previous}, get_slot_one_epoch_prior: {current}, slot: {slot}"
        );
    }

    /// normal code path returns the common cache path
    /// when called after a failure has been detected, redirect the cache storage to a separate folder for debugging later
    fn get_cache_hash_data(
        &self,
        config: &CalcAccountsHashConfig<'_>,
        slot: Slot,
    ) -> CacheHashData {
        if !config.store_detailed_debug_info_on_failure {
            CacheHashData::new(&self.accounts_hash_cache_path)
        } else {
            // this path executes when we are failing with a hash mismatch
            let mut new = self.accounts_hash_cache_path.clone();
            new.push("failed_calculate_accounts_hash_cache");
            new.push(slot.to_string());
            let _ = std::fs::remove_dir_all(&new);
            CacheHashData::new(&new)
        }
    }

    // modeled after get_accounts_delta_hash
    // intended to be faster than calculate_accounts_hash
    pub fn calculate_accounts_hash_from_storages(
        &self,
        config: &CalcAccountsHashConfig<'_>,
        storages: &SortedStorages<'_>,
        mut stats: HashStats,
    ) -> Result<(AccountsHash, u64), BankHashVerificationError> {
        let _guard = self.active_stats.activate(ActiveStatItem::Hash);
        stats.oldest_root = storages.range().start;

        self.mark_old_slots_as_dirty(storages, config.epoch_schedule.slots_per_epoch, &mut stats);

        let use_bg_thread_pool = config.use_bg_thread_pool;
        let mut scan_and_hash = || {
            let cache_hash_data = self.get_cache_hash_data(config, storages.max_slot_inclusive());

            let bounds = Range {
                start: 0,
                end: PUBKEY_BINS_FOR_CALCULATING_HASHES,
            };

            let hash = AccountsHasher {
                filler_account_suffix: if self.filler_accounts_config.count > 0 {
                    self.filler_account_suffix
                } else {
                    None
                },
            };

            // get raw data by scanning
            let result = self.scan_snapshot_stores_with_cache(
                &cache_hash_data,
                storages,
                &mut stats,
                PUBKEY_BINS_FOR_CALCULATING_HASHES,
                &bounds,
                config,
                hash.filler_account_suffix.as_ref(),
            )?;

            // convert mmapped cache files into slices of data
            let slices = result
                .iter()
                .map(|d| d.get_cache_hash_data())
                .collect::<Vec<_>>();

            // rework slices of data into bins for parallel processing and to match data shape expected by 'rest_of_hash_calculation'
            let result = AccountsHasher::get_binned_data(
                &slices,
                PUBKEY_BINS_FOR_CALCULATING_HASHES,
                &bounds,
            );

            // turn raw data into merkle tree hashes and sum of lamports
            let final_result = hash.rest_of_hash_calculation(result, &mut stats);

            info!(
                "calculate_accounts_hash_from_storages: slot: {} {:?}",
                storages.max_slot_inclusive(),
                final_result
            );
            let final_result = (AccountsHash(final_result.0), final_result.1);
            Ok(final_result)
        };

        let result = if use_bg_thread_pool {
            self.thread_pool_clean.install(scan_and_hash)
        } else {
            scan_and_hash()
        };
        self.assert_safe_squashing_accounts_hash(
            storages.max_slot_inclusive(),
            config.epoch_schedule,
        );
        stats.log();
        result
    }

    /// return alive roots to retain, even though they are ancient
    fn calc_alive_ancient_historical_roots(&self, min_root: Slot) -> HashSet<Slot> {
        let mut ancient_alive_roots = HashSet::default();
        {
            let all_roots = self.accounts_index.roots_tracker.read().unwrap();

            if let Some(min) = all_roots.historical_roots.min() {
                for slot in min..min_root {
                    if all_roots.alive_roots.contains(&slot) {
                        // there was a storage for this root, so it counts as a root
                        ancient_alive_roots.insert(slot);
                    }
                }
            }
        }
        ancient_alive_roots
    }

    /// get rid of historical roots that are older than 'min_root'.
    /// These will be older than an epoch from a current root.
    fn remove_old_historical_roots(&self, min_root: Slot) {
        let alive_roots = self.calc_alive_ancient_historical_roots(min_root);
        self.accounts_index
            .remove_old_historical_roots(min_root, &alive_roots);
    }

    /// Only called from startup or test code.
    pub fn verify_bank_hash_and_lamports(
        &self,
        slot: Slot,
        ancestors: &Ancestors,
        total_lamports: u64,
        test_hash_calculation: bool,
        epoch_schedule: &EpochSchedule,
        rent_collector: &RentCollector,
        use_bg_thread_pool: bool,
    ) -> Result<(), BankHashVerificationError> {
        self.verify_bank_hash_and_lamports_new(
            slot,
            ancestors,
            total_lamports,
            test_hash_calculation,
            epoch_schedule,
            rent_collector,
            false,
            false,
            use_bg_thread_pool,
        )
    }

    /// Only called from startup or test code.
    #[allow(clippy::too_many_arguments)]
    pub fn verify_bank_hash_and_lamports_new(
        &self,
        slot: Slot,
        ancestors: &Ancestors,
        total_lamports: u64,
        test_hash_calculation: bool,
        epoch_schedule: &EpochSchedule,
        rent_collector: &RentCollector,
        ignore_mismatch: bool,
        store_hash_raw_data_for_debug: bool,
        use_bg_thread_pool: bool,
    ) -> Result<(), BankHashVerificationError> {
        use BankHashVerificationError::*;

        let check_hash = false; // this will not be supported anymore
        let (calculated_accounts_hash, calculated_lamports) = self
            .calculate_accounts_hash_with_verify(
                CalcAccountsHashDataSource::Storages,
                test_hash_calculation,
                slot,
                CalcAccountsHashConfig {
                    use_bg_thread_pool,
                    check_hash,
                    ancestors: Some(ancestors),
                    epoch_schedule,
                    rent_collector,
                    store_detailed_debug_info_on_failure: store_hash_raw_data_for_debug,
                    full_snapshot: None,
                },
                None,
            )?;

        if calculated_lamports != total_lamports {
            warn!(
                "Mismatched total lamports: {} calculated: {}",
                total_lamports, calculated_lamports
            );
            return Err(MismatchedTotalLamports(calculated_lamports, total_lamports));
        }

        if ignore_mismatch {
            Ok(())
        } else {
            let bank_hashes = self.bank_hashes.read().unwrap();
            if let Some(found_hash_info) = bank_hashes.get(&slot) {
                if calculated_accounts_hash == found_hash_info.accounts_hash {
                    Ok(())
                } else {
                    warn!(
                        "mismatched bank hash for slot {}: {:?} (calculated) != {:?} (expected)",
                        slot, calculated_accounts_hash, found_hash_info.accounts_hash,
                    );
                    Err(MismatchedBankHash)
                }
            } else {
                Err(MissingBankHash)
            }
        }
    }

    /// helper to return
    /// 1. pubkey, hash pairs for the slot
    /// 2. us spent scanning
    /// 3. Measure started when we began accumulating
    fn get_pubkey_hash_for_slot(&self, slot: Slot) -> (Vec<(Pubkey, Hash)>, u64, Measure) {
        let mut scan = Measure::start("scan");

        let scan_result: ScanStorageResult<(Pubkey, Hash), DashMapVersionHash> = self
            .scan_account_storage(
                slot,
                |loaded_account: LoadedAccount| {
                    // Cache only has one version per key, don't need to worry about versioning
                    Some((*loaded_account.pubkey(), loaded_account.loaded_hash()))
                },
                |accum: &DashMap<Pubkey, (u64, Hash)>, loaded_account: LoadedAccount| {
                    let loaded_write_version = loaded_account.write_version();
                    let loaded_hash = loaded_account.loaded_hash();
                    // keep the latest write version for each pubkey
                    match accum.entry(*loaded_account.pubkey()) {
                        Occupied(mut occupied_entry) => {
                            if loaded_write_version > occupied_entry.get().version() {
                                occupied_entry.insert((loaded_write_version, loaded_hash));
                            }
                        }

                        Vacant(vacant_entry) => {
                            vacant_entry.insert((loaded_write_version, loaded_hash));
                        }
                    }
                },
            );
        scan.stop();

        let accumulate = Measure::start("accumulate");
        let hashes: Vec<_> = match scan_result {
            ScanStorageResult::Cached(cached_result) => cached_result,
            ScanStorageResult::Stored(stored_result) => stored_result
                .into_iter()
                .map(|(pubkey, (_latest_write_version, hash))| (pubkey, hash))
                .collect(),
        };
        (hashes, scan.as_us(), accumulate)
    }

    pub fn get_accounts_delta_hash(&self, slot: Slot) -> Hash {
        let (mut hashes, scan_us, mut accumulate) = self.get_pubkey_hash_for_slot(slot);
        let dirty_keys = hashes.iter().map(|(pubkey, _hash)| *pubkey).collect();

        if self.filler_accounts_enabled() {
            // filler accounts must be added to 'dirty_keys' above but cannot be used to calculate hash
            hashes.retain(|(pubkey, _hash)| !self.is_filler_account(pubkey));
        }

        let ret = AccountsHasher::accumulate_account_hashes(hashes);
        accumulate.stop();
        let mut uncleaned_time = Measure::start("uncleaned_index");
        self.uncleaned_pubkeys.insert(slot, dirty_keys);
        uncleaned_time.stop();
        self.stats
            .store_uncleaned_update
            .fetch_add(uncleaned_time.as_us(), Ordering::Relaxed);

        self.stats
            .delta_hash_scan_time_total_us
            .fetch_add(scan_us, Ordering::Relaxed);
        self.stats
            .delta_hash_accumulate_time_total_us
            .fetch_add(accumulate.as_us(), Ordering::Relaxed);
        self.stats.delta_hash_num.fetch_add(1, Ordering::Relaxed);
        ret
    }

    fn update_index<'a, T: ReadableAccount + Sync>(
        &self,
        infos: Vec<AccountInfo>,
        accounts: &impl StorableAccounts<'a, T>,
        reclaim: UpsertReclaim,
    ) -> SlotList<AccountInfo> {
        let target_slot = accounts.target_slot();
        // using a thread pool here results in deadlock panics from bank_hashes.write()
        // so, instead we limit how many threads will be created to the same size as the bg thread pool
        let len = std::cmp::min(accounts.len(), infos.len());
        let threshold = 1;
        let update = |start, end| {
            let mut reclaims = Vec::with_capacity((end - start) / 2);

            (start..end).for_each(|i| {
                let info = infos[i];
                let pubkey_account = (accounts.pubkey(i), accounts.account(i));
                let pubkey = pubkey_account.0;
                let old_slot = accounts.slot(i);
                self.accounts_index.upsert(
                    target_slot,
                    old_slot,
                    pubkey,
                    pubkey_account.1,
                    &self.account_indexes,
                    info,
                    &mut reclaims,
                    reclaim,
                );
            });
            reclaims
        };
        if len > threshold {
            let chunk_size = std::cmp::max(1, len / quarter_thread_count()); // # pubkeys/thread
            let batches = 1 + len / chunk_size;
            (0..batches)
                .into_par_iter()
                .map(|batch| {
                    let start = batch * chunk_size;
                    let end = std::cmp::min(start + chunk_size, len);
                    update(start, end)
                })
                .flatten()
                .collect::<Vec<_>>()
        } else {
            update(0, len)
        }
    }

    fn should_not_shrink(aligned_bytes: u64, total_bytes: u64) -> bool {
        aligned_bytes + PAGE_SIZE > total_bytes
    }

    fn is_shrinking_productive(slot: Slot, store: &Arc<AccountStorageEntry>) -> bool {
        let alive_count = store.count();
        let stored_count = store.approx_stored_count();
        let alive_bytes = store.alive_bytes();
        let total_bytes = store.total_bytes();

        let aligned_bytes = Self::page_align(alive_bytes as u64);
        if Self::should_not_shrink(aligned_bytes, total_bytes) {
            trace!(
                "shrink_slot_forced ({}): not able to shrink at all: alive/stored: ({} / {}) ({}b / {}b) save: {}",
                slot,
                alive_count,
                stored_count,
                aligned_bytes,
                total_bytes,
                total_bytes.saturating_sub(aligned_bytes),
            );
            return false;
        }

        true
    }

    fn is_candidate_for_shrink(
        &self,
        store: &Arc<AccountStorageEntry>,
        allow_shrink_ancient: bool,
    ) -> bool {
        let total_bytes = if is_ancient(&store.accounts) {
            if !allow_shrink_ancient {
                return false;
            }

            store.written_bytes()
        } else {
            store.total_bytes()
        };
        match self.shrink_ratio {
            AccountShrinkThreshold::TotalSpace { shrink_ratio: _ } => {
                Self::page_align(store.alive_bytes() as u64) < total_bytes
            }
            AccountShrinkThreshold::IndividualStore { shrink_ratio } => {
                (Self::page_align(store.alive_bytes() as u64) as f64 / total_bytes as f64)
                    < shrink_ratio
            }
        }
    }

    fn remove_dead_accounts<'a, I>(
        &'a self,
        reclaims: I,
        expected_slot: Option<Slot>,
        mut reclaimed_offsets: Option<&mut AppendVecOffsets>,
        reset_accounts: bool,
    ) -> HashSet<Slot>
    where
        I: Iterator<Item = &'a (Slot, AccountInfo)>,
    {
        let mut dead_slots = HashSet::new();
        let mut new_shrink_candidates: ShrinkCandidates = HashMap::new();
        let mut measure = Measure::start("remove");
        for (slot, account_info) in reclaims {
            // No cached accounts should make it here
            assert!(!account_info.is_cached());
            if let Some(ref mut reclaimed_offsets) = reclaimed_offsets {
                reclaimed_offsets
                    .entry(account_info.store_id())
                    .or_default()
                    .insert(account_info.offset());
            }
            if let Some(expected_slot) = expected_slot {
                assert_eq!(*slot, expected_slot);
            }
            if let Some(store) = self
                .storage
                .get_account_storage_entry(*slot, account_info.store_id())
            {
                assert_eq!(
                    *slot, store.slot(),
                    "AccountsDB::accounts_index corrupted. Storage pointed to: {}, expected: {}, should only point to one slot",
                    store.slot(), *slot
                );
                let count =
                    store.remove_account(account_info.stored_size() as usize, reset_accounts);
                if count == 0 {
                    self.dirty_stores
                        .insert((*slot, store.append_vec_id()), store.clone());
                    dead_slots.insert(*slot);
                } else if Self::is_shrinking_productive(*slot, &store)
                    && self.is_candidate_for_shrink(&store, false)
                {
                    // Checking that this single storage entry is ready for shrinking,
                    // should be a sufficient indication that the slot is ready to be shrunk
                    // because slots should only have one storage entry, namely the one that was
                    // created by `flush_slot_cache()`.
                    {
                        new_shrink_candidates
                            .entry(*slot)
                            .or_default()
                            .insert(store.append_vec_id(), store);
                    }
                }
            }
        }
        measure.stop();
        self.clean_accounts_stats
            .remove_dead_accounts_remove_us
            .fetch_add(measure.as_us(), Ordering::Relaxed);

        let mut measure = Measure::start("shrink");
        let mut shrink_candidate_slots = self.shrink_candidate_slots.lock().unwrap();
        for (slot, slot_shrink_candidates) in new_shrink_candidates {
            for (store_id, store) in slot_shrink_candidates {
                // count could be == 0 if multiple accounts are removed
                // at once
                if store.count() != 0 {
                    debug!(
                        "adding: {} {} to shrink candidates: count: {}/{} bytes: {}/{}",
                        store_id,
                        slot,
                        store.approx_stored_count(),
                        store.count(),
                        store.alive_bytes(),
                        store.total_bytes()
                    );

                    shrink_candidate_slots
                        .entry(slot)
                        .or_default()
                        .insert(store_id, store);
                }
            }
            measure.stop();
            self.clean_accounts_stats
                .remove_dead_accounts_shrink_us
                .fetch_add(measure.as_us(), Ordering::Relaxed);
        }

        dead_slots.retain(|slot| {
            if let Some(slot_stores) = self.storage.get_slot_stores(*slot) {
                for x in slot_stores.read().unwrap().values() {
                    if x.count() != 0 {
                        return false;
                    }
                }
            }
            true
        });

        dead_slots
    }

    /// pubkeys_removed_from_accounts_index - These keys have already been removed from the accounts index
    ///    and should not be unref'd. If they exist in the accounts index, they are NEW.
    fn remove_dead_slots_metadata<'a>(
        &'a self,
        dead_slots_iter: impl Iterator<Item = &'a Slot> + Clone,
        purged_slot_pubkeys: HashSet<(Slot, Pubkey)>,
        // Should only be `Some` for non-cached slots
        purged_stored_account_slots: Option<&mut AccountSlots>,
        pubkeys_removed_from_accounts_index: &PubkeysRemovedFromAccountsIndex,
    ) {
        let mut measure = Measure::start("remove_dead_slots_metadata-ms");
        self.clean_dead_slots_from_accounts_index(
            dead_slots_iter.clone(),
            purged_slot_pubkeys,
            purged_stored_account_slots,
            pubkeys_removed_from_accounts_index,
        );
        {
            let mut bank_hashes = self.bank_hashes.write().unwrap();
            for slot in dead_slots_iter {
                bank_hashes.remove(slot);
            }
        }
        measure.stop();
        inc_new_counter_info!("remove_dead_slots_metadata-ms", measure.as_ms() as usize);
    }

    /// lookup each pubkey in 'pubkeys' and unref it in the accounts index
    /// skip pubkeys that are in 'pubkeys_removed_from_accounts_index'
    fn unref_pubkeys<'a>(
        &'a self,
        pubkeys: impl Iterator<Item = &'a Pubkey> + Clone + Send + Sync,
        num_pubkeys: usize,
        pubkeys_removed_from_accounts_index: &'a PubkeysRemovedFromAccountsIndex,
    ) {
        let batches = 1 + (num_pubkeys / UNREF_ACCOUNTS_BATCH_SIZE);
        self.thread_pool_clean.install(|| {
            (0..batches).into_par_iter().for_each(|batch| {
                let skip = batch * UNREF_ACCOUNTS_BATCH_SIZE;
                self.accounts_index.scan(
                    pubkeys
                        .clone()
                        .skip(skip)
                        .take(UNREF_ACCOUNTS_BATCH_SIZE)
                        .filter(|pubkey| {
                            // filter out pubkeys that have already been removed from the accounts index in a previous step
                            let already_removed =
                                pubkeys_removed_from_accounts_index.contains(pubkey);
                            !already_removed
                        }),
                    |_pubkey, _slots_refs| {
                        /* unused */
                        AccountsIndexScanResult::Unref
                    },
                    Some(AccountsIndexScanResult::Unref),
                )
            });
        });
    }

    /// lookup each pubkey in 'purged_slot_pubkeys' and unref it in the accounts index
    /// populate 'purged_stored_account_slots' by grouping 'purged_slot_pubkeys' by pubkey
    /// pubkeys_removed_from_accounts_index - These keys have already been removed from the accounts index
    ///    and should not be unref'd. If they exist in the accounts index, they are NEW.
    fn unref_accounts(
        &self,
        purged_slot_pubkeys: HashSet<(Slot, Pubkey)>,
        purged_stored_account_slots: &mut AccountSlots,
        pubkeys_removed_from_accounts_index: &PubkeysRemovedFromAccountsIndex,
    ) {
        self.unref_pubkeys(
            purged_slot_pubkeys.iter().map(|(_slot, pubkey)| pubkey),
            purged_slot_pubkeys.len(),
            pubkeys_removed_from_accounts_index,
        );
        for (slot, pubkey) in purged_slot_pubkeys {
            purged_stored_account_slots
                .entry(pubkey)
                .or_default()
                .insert(slot);
        }
    }

    /// pubkeys_removed_from_accounts_index - These keys have already been removed from the accounts index
    ///    and should not be unref'd. If they exist in the accounts index, they are NEW.
    fn clean_dead_slots_from_accounts_index<'a>(
        &'a self,
        dead_slots_iter: impl Iterator<Item = &'a Slot> + Clone,
        purged_slot_pubkeys: HashSet<(Slot, Pubkey)>,
        // Should only be `Some` for non-cached slots
        purged_stored_account_slots: Option<&mut AccountSlots>,
        pubkeys_removed_from_accounts_index: &PubkeysRemovedFromAccountsIndex,
    ) {
        let mut accounts_index_root_stats = AccountsIndexRootsStats::default();
        let mut measure = Measure::start("unref_from_storage");
        if let Some(purged_stored_account_slots) = purged_stored_account_slots {
            self.unref_accounts(
                purged_slot_pubkeys,
                purged_stored_account_slots,
                pubkeys_removed_from_accounts_index,
            );
        }
        measure.stop();
        accounts_index_root_stats.clean_unref_from_storage_us += measure.as_us();

        let mut measure = Measure::start("clean_dead_slot");
        let mut rooted_cleaned_count = 0;
        let mut unrooted_cleaned_count = 0;
        let dead_slots: Vec<_> = dead_slots_iter
            .map(|slot| {
                if self
                    .accounts_index
                    .clean_dead_slot(*slot, &mut accounts_index_root_stats)
                {
                    rooted_cleaned_count += 1;
                } else {
                    unrooted_cleaned_count += 1;
                }
                *slot
            })
            .collect();
        measure.stop();
        accounts_index_root_stats.clean_dead_slot_us += measure.as_us();
        if self.log_dead_slots.load(Ordering::Relaxed) {
            info!(
                "remove_dead_slots_metadata: {} dead slots",
                dead_slots.len()
            );
            trace!("remove_dead_slots_metadata: dead_slots: {:?}", dead_slots);
        }

        accounts_index_root_stats.rooted_cleaned_count += rooted_cleaned_count;
        accounts_index_root_stats.unrooted_cleaned_count += unrooted_cleaned_count;

        self.clean_accounts_stats
            .latest_accounts_index_roots_stats
            .update(&accounts_index_root_stats);
    }

    /// pubkeys_removed_from_accounts_index - These keys have already been removed from the accounts index
    ///    and should not be unref'd. If they exist in the accounts index, they are NEW.
    fn clean_stored_dead_slots(
        &self,
        dead_slots: &HashSet<Slot>,
        purged_account_slots: Option<&mut AccountSlots>,
        pubkeys_removed_from_accounts_index: &PubkeysRemovedFromAccountsIndex,
    ) {
        let mut measure = Measure::start("clean_stored_dead_slots-ms");
        let mut stores = vec![];
        // get all stores in a vec so we can iterate in parallel
        for slot in dead_slots.iter() {
            if let Some(slot_storage) = self.storage.get_slot_stores(*slot) {
                for store in slot_storage.read().unwrap().values() {
                    stores.push(store.clone());
                }
            }
        }
        // get all pubkeys in all dead slots
        let purged_slot_pubkeys: HashSet<(Slot, Pubkey)> = {
            self.thread_pool_clean.install(|| {
                stores
                    .into_par_iter()
                    .map(|store| {
                        let slot = store.slot();
                        store
                            .accounts
                            .account_iter()
                            .map(|account| (slot, *account.pubkey()))
                            .collect::<Vec<(Slot, Pubkey)>>()
                    })
                    .flatten()
                    .collect::<HashSet<_>>()
            })
        };
        self.remove_dead_slots_metadata(
            dead_slots.iter(),
            purged_slot_pubkeys,
            purged_account_slots,
            pubkeys_removed_from_accounts_index,
        );
        measure.stop();
        inc_new_counter_info!("clean_stored_dead_slots-ms", measure.as_ms() as usize);
        self.clean_accounts_stats
            .clean_stored_dead_slots_us
            .fetch_add(measure.as_us(), Ordering::Relaxed);
    }

    pub fn store_cached<'a, T: ReadableAccount + Sync + ZeroLamport + 'a>(
        &self,
        accounts: impl StorableAccounts<'a, T>,
        txn_signatures: Option<&'a [Option<&'a Signature>]>,
    ) {
        self.store(accounts, true, txn_signatures, StoreReclaims::Default);
    }

    /// Store the account update.
    /// only called by tests
    pub fn store_uncached(&self, slot: Slot, accounts: &[(&Pubkey, &AccountSharedData)]) {
        self.store(
            (slot, accounts, INCLUDE_SLOT_IN_HASH_TESTS),
            false,
            None,
            StoreReclaims::Default,
        );
    }

    fn store<'a, T: ReadableAccount + Sync + ZeroLamport + 'a>(
        &self,
        accounts: impl StorableAccounts<'a, T>,
        is_cached_store: bool,
        txn_signatures: Option<&'a [Option<&'a Signature>]>,
        reclaim: StoreReclaims,
    ) {
        // If all transactions in a batch are errored,
        // it's possible to get a store with no accounts.
        if accounts.is_empty() {
            return;
        }

        let mut stats = BankHashStats::default();
        let mut total_data = 0;
        (0..accounts.len()).for_each(|index| {
            let account = accounts.account(index);
            total_data += account.data().len();
            stats.update(account);
        });

        self.stats
            .store_total_data
            .fetch_add(total_data as u64, Ordering::Relaxed);

        {
            // we need to drop bank_hashes to prevent deadlocks
            let mut bank_hashes = self.bank_hashes.write().unwrap();
            let slot_info = bank_hashes
                .entry(accounts.target_slot())
                .or_insert_with(BankHashInfo::default);
            slot_info.stats.accumulate(&stats);
        }

        // we use default hashes for now since the same account may be stored to the cache multiple times
        self.store_accounts_unfrozen(
            accounts,
            None::<Vec<Hash>>,
            is_cached_store,
            txn_signatures,
            reclaim,
        );
        self.report_store_timings();
    }

    fn report_store_timings(&self) {
        if self.stats.last_store_report.should_update(1000) {
            let (read_only_cache_hits, read_only_cache_misses, read_only_cache_evicts) =
                self.read_only_accounts_cache.get_and_reset_stats();
            datapoint_info!(
                "accounts_db_store_timings",
                (
                    "hash_accounts",
                    self.stats.store_hash_accounts.swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "store_accounts",
                    self.stats.store_accounts.swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "update_index",
                    self.stats.store_update_index.swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "handle_reclaims",
                    self.stats.store_handle_reclaims.swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "append_accounts",
                    self.stats.store_append_accounts.swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "stakes_cache_check_and_store_us",
                    self.stats
                        .stakes_cache_check_and_store_us
                        .swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "find_storage",
                    self.stats.store_find_store.swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "num_accounts",
                    self.stats.store_num_accounts.swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "total_data",
                    self.stats.store_total_data.swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "read_only_accounts_cache_entries",
                    self.read_only_accounts_cache.cache_len(),
                    i64
                ),
                (
                    "read_only_accounts_cache_data_size",
                    self.read_only_accounts_cache.data_size(),
                    i64
                ),
                ("read_only_accounts_cache_hits", read_only_cache_hits, i64),
                (
                    "read_only_accounts_cache_misses",
                    read_only_cache_misses,
                    i64
                ),
                (
                    "read_only_accounts_cache_evicts",
                    read_only_cache_evicts,
                    i64
                ),
                (
                    "calc_stored_meta_us",
                    self.stats.calc_stored_meta.swap(0, Ordering::Relaxed),
                    i64
                ),
            );

            let recycle_stores = self.recycle_stores.read().unwrap();
            datapoint_info!(
                "accounts_db_store_timings2",
                (
                    "recycle_store_count",
                    self.stats.recycle_store_count.swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "current_recycle_store_count",
                    recycle_stores.entry_count(),
                    i64
                ),
                (
                    "current_recycle_store_bytes",
                    recycle_stores.total_bytes(),
                    i64
                ),
                (
                    "create_store_count",
                    self.stats.create_store_count.swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "store_get_slot_store",
                    self.stats.store_get_slot_store.swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "store_find_existing",
                    self.stats.store_find_existing.swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "dropped_stores",
                    self.stats.dropped_stores.swap(0, Ordering::Relaxed),
                    i64
                ),
            );
        }
    }

    fn store_accounts_unfrozen<'a, T: ReadableAccount + Sync + ZeroLamport + 'a>(
        &self,
        accounts: impl StorableAccounts<'a, T>,
        hashes: Option<Vec<impl Borrow<Hash>>>,
        is_cached_store: bool,
        txn_signatures: Option<&'a [Option<&'a Signature>]>,
        reclaim: StoreReclaims,
    ) {
        // This path comes from a store to a non-frozen slot.
        // If a store is dead here, then a newer update for
        // each pubkey in the store must exist in another
        // store in the slot. Thus it is safe to reset the store and
        // re-use it for a future store op. The pubkey ref counts should still
        // hold just 1 ref from this slot.
        let reset_accounts = true;

        self.store_accounts_custom(
            accounts,
            hashes,
            None,
            None::<Box<dyn Iterator<Item = u64>>>,
            is_cached_store,
            reset_accounts,
            txn_signatures,
            reclaim,
        );
    }

    pub(crate) fn store_accounts_frozen<'a, T: ReadableAccount + Sync + ZeroLamport + 'a>(
        &'a self,
        accounts: impl StorableAccounts<'a, T>,
        hashes: Option<Vec<impl Borrow<Hash>>>,
        storage: Option<&'a Arc<AccountStorageEntry>>,
        write_version_producer: Option<Box<dyn Iterator<Item = StoredMetaWriteVersion>>>,
        reclaim: StoreReclaims,
    ) -> StoreAccountsTiming {
        // stores on a frozen slot should not reset
        // the append vec so that hashing could happen on the store
        // and accounts in the append_vec can be unrefed correctly
        let reset_accounts = false;
        let is_cached_store = false;
        self.store_accounts_custom(
            accounts,
            hashes,
            storage,
            write_version_producer,
            is_cached_store,
            reset_accounts,
            None,
            reclaim,
        )
    }

    fn store_accounts_custom<'a, T: ReadableAccount + Sync + ZeroLamport + 'a>(
        &self,
        accounts: impl StorableAccounts<'a, T>,
        hashes: Option<Vec<impl Borrow<Hash>>>,
        storage: Option<&'a Arc<AccountStorageEntry>>,
        write_version_producer: Option<Box<dyn Iterator<Item = u64>>>,
        is_cached_store: bool,
        reset_accounts: bool,
        txn_signatures: Option<&[Option<&Signature>]>,
        reclaim: StoreReclaims,
    ) -> StoreAccountsTiming {
        let storage_finder = Box::new(move |slot, size| {
            storage
                .cloned()
                .unwrap_or_else(|| self.find_storage_candidate(slot, size))
        });

        let write_version_producer: Box<dyn Iterator<Item = u64>> = write_version_producer
            .unwrap_or_else(|| {
                let mut current_version = self.bulk_assign_write_version(accounts.len());
                Box::new(std::iter::from_fn(move || {
                    let ret = current_version;
                    current_version += 1;
                    Some(ret)
                }))
            });

        self.stats
            .store_num_accounts
            .fetch_add(accounts.len() as u64, Ordering::Relaxed);
        let mut store_accounts_time = Measure::start("store_accounts");
        let infos = self.store_accounts_to(
            &accounts,
            hashes,
            storage_finder,
            write_version_producer,
            is_cached_store,
            txn_signatures,
        );
        store_accounts_time.stop();
        self.stats
            .store_accounts
            .fetch_add(store_accounts_time.as_us(), Ordering::Relaxed);
        let mut update_index_time = Measure::start("update_index");

        let reclaim = if matches!(reclaim, StoreReclaims::Ignore) {
            UpsertReclaim::IgnoreReclaims
        } else if is_cached_store {
            UpsertReclaim::PreviousSlotEntryWasCached
        } else {
            UpsertReclaim::PopulateReclaims
        };

        // if we are squashing a single slot, then we can expect a single dead slot
        let expected_single_dead_slot =
            (!accounts.contains_multiple_slots()).then(|| accounts.target_slot());

        // If the cache was flushed, then because `update_index` occurs
        // after the account are stored by the above `store_accounts_to`
        // call and all the accounts are stored, all reads after this point
        // will know to not check the cache anymore
        let mut reclaims = self.update_index(infos, &accounts, reclaim);

        // For each updated account, `reclaims` should only have at most one
        // item (if the account was previously updated in this slot).
        // filter out the cached reclaims as those don't actually map
        // to anything that needs to be cleaned in the backing storage
        // entries
        reclaims.retain(|(_, r)| !r.is_cached());

        if is_cached_store {
            assert!(reclaims.is_empty());
        }

        update_index_time.stop();
        self.stats
            .store_update_index
            .fetch_add(update_index_time.as_us(), Ordering::Relaxed);

        // A store for a single slot should:
        // 1) Only make "reclaims" for the same slot
        // 2) Should not cause any slots to be removed from the storage
        // database because
        //    a) this slot  has at least one account (the one being stored),
        //    b)From 1) we know no other slots are included in the "reclaims"
        //
        // From 1) and 2) we guarantee passing `no_purge_stats` == None, which is
        // equivalent to asserting there will be no dead slots, is safe.
        let mut handle_reclaims_time = Measure::start("handle_reclaims");
        self.handle_reclaims(
            (!reclaims.is_empty()).then(|| reclaims.iter()),
            expected_single_dead_slot,
            None,
            reset_accounts,
            &HashSet::default(),
        );
        handle_reclaims_time.stop();
        self.stats
            .store_handle_reclaims
            .fetch_add(handle_reclaims_time.as_us(), Ordering::Relaxed);

        StoreAccountsTiming {
            store_accounts_elapsed: store_accounts_time.as_us(),
            update_index_elapsed: update_index_time.as_us(),
            handle_reclaims_elapsed: handle_reclaims_time.as_us(),
        }
    }

    pub fn add_root(&self, slot: Slot) -> AccountsAddRootTiming {
        let mut index_time = Measure::start("index_add_root");
        self.accounts_index.add_root(slot);
        index_time.stop();
        let mut cache_time = Measure::start("cache_add_root");
        self.accounts_cache.add_root(slot);
        cache_time.stop();
        let mut store_time = Measure::start("store_add_root");
        if let Some(slot_stores) = self.storage.get_slot_stores(slot) {
            for (store_id, store) in slot_stores.read().unwrap().iter() {
                self.dirty_stores.insert((slot, *store_id), store.clone());
            }
        }
        store_time.stop();

        AccountsAddRootTiming {
            index_us: index_time.as_us(),
            cache_us: cache_time.as_us(),
            store_us: store_time.as_us(),
        }
    }

    /// Get storages to use for snapshots, for the requested slots
    pub fn get_snapshot_storages(
        &self,
        requested_slots: impl RangeBounds<Slot> + Sync,
        ancestors: Option<&Ancestors>,
    ) -> (SnapshotStorages, Vec<Slot>) {
        let mut m = Measure::start("get slots");
        let slots_and_storages = self
            .storage
            .iter()
            .filter_map(|entry| {
                let slot = *entry.key() as Slot;
                requested_slots
                    .contains(&slot)
                    .then_some((slot, Arc::clone(entry.value())))
            })
            .collect::<Vec<_>>();
        m.stop();
        let mut m2 = Measure::start("filter");

        let chunk_size = 5_000;
        let wide = self.thread_pool_clean.install(|| {
            slots_and_storages
                .par_chunks(chunk_size)
                .map(|slots_and_storages| {
                    slots_and_storages
                        .iter()
                        .filter(|(slot, _)| {
                            self.accounts_index.is_alive_root(*slot)
                                || ancestors
                                    .map(|ancestors| ancestors.contains_key(slot))
                                    .unwrap_or_default()
                        })
                        .filter_map(|(slot, storages)| {
                            let storages = storages
                                .read()
                                .unwrap()
                                .values()
                                .filter(|x| x.has_accounts())
                                .cloned()
                                .collect::<Vec<_>>();
                            (!storages.is_empty()).then_some((storages, *slot))
                        })
                        .collect::<Vec<(SnapshotStorage, Slot)>>()
                })
                .collect::<Vec<_>>()
        });
        m2.stop();
        let mut m3 = Measure::start("flatten");
        // some slots we found above may not have been a root or met the slot # constraint.
        // So the resulting 'slots' vector we return will be a subset of the raw keys we got initially.
        let mut slots = Vec::with_capacity(slots_and_storages.len());
        let result = wide
            .into_iter()
            .flatten()
            .map(|(storage, slot)| {
                slots.push(slot);
                storage
            })
            .collect::<Vec<_>>();
        m3.stop();

        debug!(
            "hash_total: get slots: {}, filter: {}, flatten: {}",
            m.as_us(),
            m2.as_us(),
            m3.as_us()
        );
        (result, slots)
    }

    fn process_storage_slot<'a>(
        &self,
        storage: &'a Arc<AccountStorageEntry>,
    ) -> GenerateIndexAccountsMap<'a> {
        let num_accounts = storage.approx_stored_count();
        let mut accounts_map = GenerateIndexAccountsMap::with_capacity(num_accounts);
        storage.accounts.account_iter().for_each(|stored_account| {
            let this_version = stored_account.meta.write_version_obsolete;
            let pubkey = stored_account.pubkey();
            assert!(!self.is_filler_account(pubkey));
            match accounts_map.entry(*pubkey) {
                Entry::Vacant(entry) => {
                    entry.insert(IndexAccountMapEntry {
                        write_version: this_version,
                        store_id: storage.append_vec_id(),
                        stored_account,
                    });
                }
                Entry::Occupied(mut entry) => {
                    let occupied_version = entry.get().write_version;
                    assert!(occupied_version < this_version);
                    entry.insert(IndexAccountMapEntry {
                        write_version: this_version,
                        store_id: storage.append_vec_id(),
                        stored_account,
                    });
                }
            }
        });
        accounts_map
    }

    /// return Some(lamports_to_top_off) if 'account' would collect rent
    fn stats_for_rent_payers<T: ReadableAccount>(
        pubkey: &Pubkey,
        account: &T,
        rent_collector: &RentCollector,
    ) -> Option<u64> {
        if account.lamports() == 0 {
            return None;
        }
        (rent_collector.should_collect_rent(pubkey, account)
            && !rent_collector.get_rent_due(account).is_exempt())
        .then(|| {
            let min_balance = rent_collector.rent.minimum_balance(account.data().len());
            // return lamports required to top off this account to make it rent exempt
            min_balance.saturating_sub(account.lamports())
        })
    }

    fn generate_index_for_slot(
        &self,
        accounts_map: GenerateIndexAccountsMap<'_>,
        slot: &Slot,
        rent_collector: &RentCollector,
    ) -> SlotIndexGenerationInfo {
        if accounts_map.is_empty() {
            return SlotIndexGenerationInfo::default();
        }

        let secondary = !self.account_indexes.is_empty();

        let mut rent_paying_accounts_by_partition = Vec::default();
        let mut accounts_data_len = 0;
        let mut num_accounts_rent_paying = 0;
        let num_accounts = accounts_map.len();
        let mut amount_to_top_off_rent = 0;
        let items = accounts_map.into_iter().map(
            |(
                pubkey,
                IndexAccountMapEntry {
                    write_version: _write_version,
                    store_id,
                    stored_account,
                },
            )| {
                if secondary {
                    self.accounts_index.update_secondary_indexes(
                        &pubkey,
                        &stored_account,
                        &self.account_indexes,
                    );
                }
                if !stored_account.is_zero_lamport() {
                    accounts_data_len += stored_account.data().len() as u64;
                }

                if let Some(amount_to_top_off_rent_this_account) =
                    Self::stats_for_rent_payers(&pubkey, &stored_account, rent_collector)
                {
                    amount_to_top_off_rent += amount_to_top_off_rent_this_account;
                    num_accounts_rent_paying += 1;
                    // remember this rent-paying account pubkey
                    rent_paying_accounts_by_partition.push(pubkey);
                }

                (
                    pubkey,
                    AccountInfo::new(
                        StorageLocation::AppendVec(store_id, stored_account.offset), // will never be cached
                        stored_account.stored_size as StoredSize, // stored_size should never exceed StoredSize::MAX because of max data len const
                        stored_account.account_meta.lamports,
                    ),
                )
            },
        );

        let (dirty_pubkeys, insert_time_us) = self
            .accounts_index
            .insert_new_if_missing_into_primary_index(*slot, num_accounts, items);

        // dirty_pubkeys will contain a pubkey if an item has multiple rooted entries for
        // a given pubkey. If there is just a single item, there is no cleaning to
        // be done on that pubkey. Use only those pubkeys with multiple updates.
        if !dirty_pubkeys.is_empty() {
            self.uncleaned_pubkeys.insert(*slot, dirty_pubkeys);
        }
        SlotIndexGenerationInfo {
            insert_time_us,
            num_accounts: num_accounts as u64,
            num_accounts_rent_paying,
            accounts_data_len,
            amount_to_top_off_rent,
            rent_paying_accounts_by_partition,
        }
    }

    fn filler_unique_id_bytes() -> usize {
        std::mem::size_of::<u32>()
    }

    fn filler_rent_partition_prefix_bytes() -> usize {
        std::mem::size_of::<u64>()
    }

    fn filler_prefix_bytes() -> usize {
        Self::filler_unique_id_bytes() + Self::filler_rent_partition_prefix_bytes()
    }

    pub fn is_filler_account_helper(
        pubkey: &Pubkey,
        filler_account_suffix: Option<&Pubkey>,
    ) -> bool {
        let offset = Self::filler_prefix_bytes();
        filler_account_suffix
            .as_ref()
            .map(|filler_account_suffix| {
                pubkey.as_ref()[offset..] == filler_account_suffix.as_ref()[offset..]
            })
            .unwrap_or_default()
    }

    /// true if 'pubkey' is a filler account
    pub fn is_filler_account(&self, pubkey: &Pubkey) -> bool {
        Self::is_filler_account_helper(pubkey, self.filler_account_suffix.as_ref())
    }

    /// true if it is possible that there are filler accounts present
    pub fn filler_accounts_enabled(&self) -> bool {
        self.filler_account_suffix.is_some()
    }

    /// retain slots in 'roots' that are > (max(roots) - slots_per_epoch)
    fn retain_roots_within_one_epoch_range(roots: &mut Vec<Slot>, slots_per_epoch: SlotCount) {
        if let Some(max) = roots.iter().max() {
            let min = max - slots_per_epoch;
            roots.retain(|slot| slot > &min);
        }
    }

    /// return 'AccountSharedData' and a hash for a filler account
    fn get_filler_account(&self, rent: &Rent) -> (AccountSharedData, Hash) {
        let string = "FiLLERACCoUNTooooooooooooooooooooooooooooooo";
        let hash = Hash::from_str(string).unwrap();
        let owner = Pubkey::from_str(string).unwrap();
        let space = self.filler_accounts_config.size;
        let rent_exempt_reserve = rent.minimum_balance(space);
        let lamports = rent_exempt_reserve;
        let mut account = AccountSharedData::new(lamports, space, &owner);
        // just non-zero rent epoch. filler accounts are rent-exempt
        let dummy_rent_epoch = 2;
        account.set_rent_epoch(dummy_rent_epoch);
        (account, hash)
    }

    fn get_filler_account_pubkeys(&self, count: usize) -> Vec<Pubkey> {
        (0..count)
            .map(|_| {
                let subrange = solana_sdk::pubkey::new_rand();
                self.get_filler_account_pubkey(&subrange)
            })
            .collect()
    }

    fn get_filler_account_pubkey(&self, subrange: &Pubkey) -> Pubkey {
        // pubkey begins life as entire filler 'suffix' pubkey
        let mut key = self.filler_account_suffix.unwrap();
        let rent_prefix_bytes = Self::filler_rent_partition_prefix_bytes();
        // first bytes are replaced with rent partition range: filler_rent_partition_prefix_bytes
        key.as_mut()[0..rent_prefix_bytes]
            .copy_from_slice(&subrange.as_ref()[0..rent_prefix_bytes]);
        key
    }

    /// filler accounts are space-holding accounts which are ignored by hash calculations and rent.
    /// They are designed to allow a validator to run against a network successfully while simulating having many more accounts present.
    /// All filler accounts share a common pubkey suffix. The suffix is randomly generated per validator on startup.
    /// The filler accounts are added to each slot in the snapshot after index generation.
    /// The accounts added in a slot are setup to have pubkeys such that rent will be collected from them before (or when?) their slot becomes an epoch old.
    /// Thus, the filler accounts are rewritten by rent and the old slot can be thrown away successfully.
    pub fn maybe_add_filler_accounts(
        &self,
        epoch_schedule: &EpochSchedule,
        rent: &Rent,
        slot: Slot,
    ) {
        if self.filler_accounts_config.count == 0 {
            return;
        }

        if ADD_FILLER_ACCOUNTS_GRADUALLY {
            self.init_gradual_filler_accounts(
                epoch_schedule.get_slots_in_epoch(epoch_schedule.get_epoch(slot)),
            );
            return;
        }

        let max_root_inclusive = self.accounts_index.max_root_inclusive();
        let epoch = epoch_schedule.get_epoch(max_root_inclusive);

        info!(
            "adding {} filler accounts with size {}",
            self.filler_accounts_config.count, self.filler_accounts_config.size,
        );
        // break this up to force the accounts out of memory after each pass
        let passes = 100;
        let mut roots = self.storage.all_slots();
        Self::retain_roots_within_one_epoch_range(
            &mut roots,
            epoch_schedule.get_slots_in_epoch(epoch),
        );
        let root_count = roots.len();
        let per_pass = std::cmp::max(1, root_count / passes);
        let overall_index = AtomicUsize::new(0);
        let (account, hash) = self.get_filler_account(rent);
        let added = AtomicU32::default();
        let rent_prefix_bytes = Self::filler_rent_partition_prefix_bytes();
        for pass in 0..=passes {
            self.accounts_index
                .set_startup(Startup::StartupWithExtraThreads);
            let roots_in_this_pass = roots
                .iter()
                .skip(pass * per_pass)
                .take(per_pass)
                .collect::<Vec<_>>();
            roots_in_this_pass.into_par_iter().for_each(|slot| {
                if self.storage.is_empty(*slot) {
                    return;
                }

                let partition = crate::bank::Bank::variable_cycle_partition_from_previous_slot(
                    epoch_schedule,
                    *slot,
                );
                let subrange = crate::bank::Bank::pubkey_range_from_partition(partition);

                let idx = overall_index.fetch_add(1, Ordering::Relaxed);
                let filler_entries = (idx + 1) * self.filler_accounts_config.count / root_count
                    - idx * self.filler_accounts_config.count / root_count;
                let accounts = (0..filler_entries)
                    .map(|_| {
                        let my_id = added.fetch_add(1, Ordering::Relaxed);
                        let mut key = self.get_filler_account_pubkey(subrange.start());
                        // next bytes are replaced with my_id: filler_unique_id_bytes
                        let my_id_bytes = u32::to_be_bytes(my_id);
                        key.as_mut()[rent_prefix_bytes
                            ..(rent_prefix_bytes + Self::filler_unique_id_bytes())]
                            .copy_from_slice(&my_id_bytes);
                        key
                    })
                    .collect::<Vec<_>>();
                let add = accounts
                    .iter()
                    .map(|key| (key, &account))
                    .collect::<Vec<_>>();
                let hashes = (0..filler_entries).map(|_| hash).collect::<Vec<_>>();
                self.maybe_throttle_index_generation();
                // filler accounts are debug only and their hash is irrelevant anyway, so any value is ok here.
                let include_slot_in_hash = INCLUDE_SLOT_IN_HASH_TESTS;
                self.store_accounts_frozen(
                    (*slot, &add[..], include_slot_in_hash),
                    Some(hashes),
                    None,
                    None,
                    StoreReclaims::Ignore,
                );
            });
            self.accounts_index.set_startup(Startup::Normal);
        }
        info!("added {} filler accounts", added.load(Ordering::Relaxed));
    }

    #[allow(clippy::needless_collect)]
    pub fn generate_index(
        &self,
        limit_load_slot_count_from_snapshot: Option<usize>,
        verify: bool,
        genesis_config: &GenesisConfig,
    ) -> IndexGenerationInfo {
        let mut slots = self.storage.all_slots();
        #[allow(clippy::stable_sort_primitive)]
        slots.sort();
        if let Some(limit) = limit_load_slot_count_from_snapshot {
            slots.truncate(limit); // get rid of the newer slots and keep just the older
        }
        let max_slot = slots.last().cloned().unwrap_or_default();
        let schedule = genesis_config.epoch_schedule;
        let rent_collector = RentCollector::new(
            schedule.get_epoch(max_slot),
            schedule,
            genesis_config.slots_per_year(),
            genesis_config.rent,
        );
        let accounts_data_len = AtomicU64::new(0);

        let rent_paying_accounts_by_partition =
            Mutex::new(RentPayingAccountsByPartition::new(&schedule));

        // pass == 0 always runs and generates the index
        // pass == 1 only runs if verify == true.
        // verify checks that all the expected items are in the accounts index and measures how long it takes to look them all up
        let passes = if verify { 2 } else { 1 };
        for pass in 0..passes {
            if pass == 0 {
                self.accounts_index
                    .set_startup(Startup::StartupWithExtraThreads);
            }
            let storage_info = StorageSizeAndCountMap::default();
            let total_processed_slots_across_all_threads = AtomicU64::new(0);
            let outer_slots_len = slots.len();
            let threads = if self.accounts_index.is_disk_index_enabled() {
                // these write directly to disk, so the more threads, the better
                num_cpus::get()
            } else {
                // seems to be a good hueristic given varying # cpus for in-mem disk index
                8
            };
            let chunk_size = (outer_slots_len / (std::cmp::max(1, threads.saturating_sub(1)))) + 1; // approximately 400k slots in a snapshot
            let mut index_time = Measure::start("index");
            let insertion_time_us = AtomicU64::new(0);
            let rent_paying = AtomicUsize::new(0);
            let amount_to_top_off_rent = AtomicU64::new(0);
            let total_duplicates = AtomicU64::new(0);
            let storage_info_timings = Mutex::new(GenerateIndexTimings::default());
            let scan_time: u64 = slots
                .par_chunks(chunk_size)
                .map(|slots| {
                    let mut log_status = MultiThreadProgress::new(
                        &total_processed_slots_across_all_threads,
                        2,
                        outer_slots_len as u64,
                    );
                    let mut scan_time_sum = 0;
                    for (index, slot) in slots.iter().enumerate() {
                        let mut scan_time = Measure::start("scan");
                        log_status.report(index as u64);
                        let storage = self.storage.get_slot_storage_entry(*slot);
                        let accounts_map = storage
                            .as_ref()
                            .map(|storage| self.process_storage_slot(storage))
                            .unwrap_or_default();

                        scan_time.stop();
                        scan_time_sum += scan_time.as_us();
                        Self::update_storage_info(
                            &storage_info,
                            &accounts_map,
                            &storage_info_timings,
                        );

                        let insert_us = if pass == 0 {
                            // generate index
                            self.maybe_throttle_index_generation();
                            let SlotIndexGenerationInfo {
                                insert_time_us: insert_us,
                                num_accounts: total_this_slot,
                                num_accounts_rent_paying: rent_paying_this_slot,
                                accounts_data_len: accounts_data_len_this_slot,
                                amount_to_top_off_rent: amount_to_top_off_rent_this_slot,
                                rent_paying_accounts_by_partition:
                                    rent_paying_accounts_by_partition_this_slot,
                            } = self.generate_index_for_slot(accounts_map, slot, &rent_collector);
                            rent_paying.fetch_add(rent_paying_this_slot, Ordering::Relaxed);
                            amount_to_top_off_rent
                                .fetch_add(amount_to_top_off_rent_this_slot, Ordering::Relaxed);
                            total_duplicates.fetch_add(total_this_slot, Ordering::Relaxed);
                            accounts_data_len
                                .fetch_add(accounts_data_len_this_slot, Ordering::Relaxed);
                            let mut rent_paying_accounts_by_partition =
                                rent_paying_accounts_by_partition.lock().unwrap();
                            rent_paying_accounts_by_partition_this_slot
                                .iter()
                                .for_each(|k| {
                                    rent_paying_accounts_by_partition.add_account(k);
                                });

                            insert_us
                        } else {
                            // verify index matches expected and measure the time to get all items
                            assert!(verify);
                            let mut lookup_time = Measure::start("lookup_time");
                            for account in accounts_map.into_iter() {
                                let (key, account_info) = account;
                                let lock = self.accounts_index.get_bin(&key);
                                let x = lock.get(&key).unwrap();
                                let sl = x.slot_list.read().unwrap();
                                let mut count = 0;
                                for (slot2, account_info2) in sl.iter() {
                                    if slot2 == slot {
                                        count += 1;
                                        let ai = AccountInfo::new(
                                            StorageLocation::AppendVec(
                                                account_info.store_id,
                                                account_info.stored_account.offset,
                                            ), // will never be cached
                                            account_info.stored_account.stored_size as StoredSize, // stored_size should never exceed StoredSize::MAX because of max data len const
                                            account_info.stored_account.account_meta.lamports,
                                        );
                                        assert_eq!(&ai, account_info2);
                                    }
                                }
                                assert_eq!(1, count);
                            }
                            lookup_time.stop();
                            lookup_time.as_us()
                        };
                        insertion_time_us.fetch_add(insert_us, Ordering::Relaxed);
                    }
                    scan_time_sum
                })
                .sum();
            index_time.stop();

            info!("rent_collector: {:?}", rent_collector);
            let mut min_bin_size = usize::MAX;
            let mut max_bin_size = usize::MIN;
            let total_items = self
                .accounts_index
                .account_maps
                .iter()
                .map(|map_bin| {
                    let len = map_bin.len_for_stats();
                    min_bin_size = std::cmp::min(min_bin_size, len);
                    max_bin_size = std::cmp::max(max_bin_size, len);
                    len
                })
                .sum();

            let mut index_flush_us = 0;
            if pass == 0 {
                // tell accounts index we are done adding the initial accounts at startup
                let mut m = Measure::start("accounts_index_idle_us");
                self.accounts_index.set_startup(Startup::Normal);
                m.stop();
                index_flush_us = m.as_us();

                // this has to happen before visit_duplicate_pubkeys_during_startup below
                // get duplicate keys from acct idx. We have to wait until we've finished flushing.
                for (slot, key) in self
                    .accounts_index
                    .retrieve_duplicate_keys_from_startup()
                    .into_iter()
                    .flatten()
                {
                    match self.uncleaned_pubkeys.entry(slot) {
                        Occupied(mut occupied) => occupied.get_mut().push(key),
                        Vacant(vacant) => {
                            vacant.insert(vec![key]);
                        }
                    }
                }
            }

            let storage_info_timings = storage_info_timings.into_inner().unwrap();
            let mut timings = GenerateIndexTimings {
                index_flush_us,
                scan_time,
                index_time: index_time.as_us(),
                insertion_time_us: insertion_time_us.load(Ordering::Relaxed),
                min_bin_size,
                max_bin_size,
                total_items,
                rent_paying,
                amount_to_top_off_rent,
                total_duplicates: total_duplicates.load(Ordering::Relaxed),
                storage_size_accounts_map_us: storage_info_timings.storage_size_accounts_map_us,
                storage_size_accounts_map_flatten_us: storage_info_timings
                    .storage_size_accounts_map_flatten_us,
                ..GenerateIndexTimings::default()
            };

            // subtract data.len() from accounts_data_len for all old accounts that are in the index twice
            let mut accounts_data_len_dedup_timer =
                Measure::start("handle accounts data len duplicates");
            let uncleaned_roots = Mutex::new(HashSet::<Slot>::default());
            if pass == 0 {
                let mut unique_pubkeys = HashSet::<Pubkey>::default();
                self.uncleaned_pubkeys.iter().for_each(|entry| {
                    entry.value().iter().for_each(|pubkey| {
                        unique_pubkeys.insert(*pubkey);
                    })
                });
                let accounts_data_len_from_duplicates = unique_pubkeys
                    .into_iter()
                    .collect::<Vec<_>>()
                    .par_chunks(4096)
                    .map(|pubkeys| {
                        let (count, uncleaned_roots_this_group) = self
                            .visit_duplicate_pubkeys_during_startup(
                                pubkeys,
                                &rent_collector,
                                &timings,
                            );
                        let mut uncleaned_roots = uncleaned_roots.lock().unwrap();
                        uncleaned_roots_this_group.into_iter().for_each(|slot| {
                            uncleaned_roots.insert(slot);
                        });
                        count
                    })
                    .sum();
                accounts_data_len.fetch_sub(accounts_data_len_from_duplicates, Ordering::Relaxed);
                info!(
                    "accounts data len: {}",
                    accounts_data_len.load(Ordering::Relaxed)
                );
            }
            accounts_data_len_dedup_timer.stop();
            timings.accounts_data_len_dedup_time_us = accounts_data_len_dedup_timer.as_us();

            if pass == 0 {
                let uncleaned_roots = uncleaned_roots.into_inner().unwrap();
                // Need to add these last, otherwise older updates will be cleaned
                for root in &slots {
                    self.accounts_index.add_root(*root);
                }
                self.accounts_index
                    .add_uncleaned_roots(uncleaned_roots.into_iter());

                self.set_storage_count_and_alive_bytes(storage_info, &mut timings);
            }
            timings.report();
        }

        self.accounts_index.log_secondary_indexes();

        IndexGenerationInfo {
            accounts_data_len: accounts_data_len.load(Ordering::Relaxed),
            rent_paying_accounts_by_partition: rent_paying_accounts_by_partition
                .into_inner()
                .unwrap(),
        }
    }

    /// Startup processes can consume large amounts of memory while inserting accounts into the index as fast as possible.
    /// Calling this can slow down the insertion process to allow flushing to disk to keep pace.
    fn maybe_throttle_index_generation(&self) {
        // This number is chosen to keep the initial ram usage sufficiently small
        // The process of generating the index is goverened entirely by how fast the disk index can be populated.
        // 10M accounts is sufficiently small that it will never have memory usage. It seems sufficiently large that it will provide sufficient performance.
        // Performance is measured by total time to generate the index.
        // Just estimating - 150M accounts can easily be held in memory in the accounts index on a 256G machine. 2-300M are also likely 'fine' during startup.
        // 550M was straining a 384G machine at startup.
        // This is a tunable parameter that just needs to be small enough to keep the generation threads from overwhelming RAM and oom at startup.
        const LIMIT: usize = 10_000_000;
        while self
            .accounts_index
            .get_startup_remaining_items_to_flush_estimate()
            > LIMIT
        {
            // 10 ms is long enough to allow some flushing to occur before insertion is resumed.
            // callers of this are typically run in parallel, so many threads will be sleeping at different starting intervals, waiting to resume insertion.
            sleep(Duration::from_millis(10));
        }
    }

    /// Used during generate_index() to:
    /// 1. get the _duplicate_ accounts data len from the given pubkeys
    /// 2. get the slots that contained duplicate pubkeys
    /// 3. update rent stats
    /// Note this should only be used when ALL entries in the accounts index are roots.
    /// returns (data len sum of all older duplicates, slots that contained duplicate pubkeys)
    fn visit_duplicate_pubkeys_during_startup(
        &self,
        pubkeys: &[Pubkey],
        rent_collector: &RentCollector,
        timings: &GenerateIndexTimings,
    ) -> (u64, HashSet<Slot>) {
        let mut accounts_data_len_from_duplicates = 0;
        let mut uncleaned_slots = HashSet::<Slot>::default();
        let mut removed_rent_paying = 0;
        let mut removed_top_off = 0;
        pubkeys.iter().for_each(|pubkey| {
            if let Some(entry) = self.accounts_index.get_account_read_entry(pubkey) {
                let slot_list = entry.slot_list();
                if slot_list.len() < 2 {
                    return;
                }
                // Only the account data len in the highest slot should be used, and the rest are
                // duplicates.  So find the max slot to keep.
                // Then sum up the remaining data len, which are the duplicates.
                // All of the slots need to go in the 'uncleaned_slots' list. For clean to work properly,
                // the slot where duplicate accounts are found in the index need to be in 'uncleaned_slots' list, too.
                let max = slot_list.iter().map(|(slot, _)| slot).max().unwrap();
                slot_list.iter().for_each(|(slot, account_info)| {
                    uncleaned_slots.insert(*slot);
                    if slot == max {
                        // the info in 'max' is the most recent, current info for this pubkey
                        return;
                    }
                    let maybe_storage_entry = self
                        .storage
                        .get_account_storage_entry(*slot, account_info.store_id());
                    let mut accessor = LoadedAccountAccessor::Stored(
                        maybe_storage_entry.map(|entry| (entry, account_info.offset())),
                    );
                    let loaded_account = accessor.check_and_get_loaded_account();
                    accounts_data_len_from_duplicates += loaded_account.data().len();
                    if let Some(lamports_to_top_off) =
                        Self::stats_for_rent_payers(pubkey, &loaded_account, rent_collector)
                    {
                        removed_rent_paying += 1;
                        removed_top_off += lamports_to_top_off;
                    }
                });
            }
        });
        timings
            .rent_paying
            .fetch_sub(removed_rent_paying, Ordering::Relaxed);
        timings
            .amount_to_top_off_rent
            .fetch_sub(removed_top_off, Ordering::Relaxed);
        (accounts_data_len_from_duplicates as u64, uncleaned_slots)
    }

    fn update_storage_info(
        storage_info: &StorageSizeAndCountMap,
        accounts_map: &GenerateIndexAccountsMap<'_>,
        timings: &Mutex<GenerateIndexTimings>,
    ) {
        let mut storage_size_accounts_map_time = Measure::start("storage_size_accounts_map");

        let mut storage_info_local = HashMap::<AppendVecId, StorageSizeAndCount>::default();
        // first collect into a local HashMap with no lock contention
        for (_, v) in accounts_map.iter() {
            let mut info = storage_info_local
                .entry(v.store_id)
                .or_insert_with(StorageSizeAndCount::default);
            info.stored_size += v.stored_account.stored_size;
            info.count += 1;
        }
        storage_size_accounts_map_time.stop();
        // second, collect into the shared DashMap once we've figured out all the info per store_id
        let mut storage_size_accounts_map_flatten_time =
            Measure::start("storage_size_accounts_map_flatten_time");
        for (store_id, v) in storage_info_local.into_iter() {
            let mut info = storage_info
                .entry(store_id)
                .or_insert_with(StorageSizeAndCount::default);
            info.stored_size += v.stored_size;
            info.count += v.count;
        }
        storage_size_accounts_map_flatten_time.stop();

        let mut timings = timings.lock().unwrap();
        timings.storage_size_accounts_map_us += storage_size_accounts_map_time.as_us();
        timings.storage_size_accounts_map_flatten_us +=
            storage_size_accounts_map_flatten_time.as_us();
    }
    fn set_storage_count_and_alive_bytes(
        &self,
        stored_sizes_and_counts: StorageSizeAndCountMap,
        timings: &mut GenerateIndexTimings,
    ) {
        // store count and size for each storage
        let mut storage_size_storages_time = Measure::start("storage_size_storages");
        for slot_stores in self.storage.iter() {
            for (id, store) in slot_stores.value().read().unwrap().iter() {
                // Should be default at this point
                assert_eq!(store.alive_bytes(), 0);
                if let Some(entry) = stored_sizes_and_counts.get(id) {
                    trace!(
                        "id: {} setting count: {} cur: {}",
                        id,
                        entry.count,
                        store.count(),
                    );
                    store.count_and_status.write().unwrap().0 = entry.count;
                    store.alive_bytes.store(entry.stored_size, Ordering::SeqCst);
                } else {
                    trace!("id: {} clearing count", id);
                    store.count_and_status.write().unwrap().0 = 0;
                }
            }
        }
        storage_size_storages_time.stop();
        timings.storage_size_storages_us = storage_size_storages_time.as_us();
    }

    pub(crate) fn print_accounts_stats(&self, label: &str) {
        self.print_index(label);
        self.print_count_and_status(label);
        info!("recycle_stores:");
        let recycle_stores = self.recycle_stores.read().unwrap();
        for (recycled_time, entry) in recycle_stores.iter() {
            info!(
                "  slot: {} id: {} count_and_status: {:?} approx_store_count: {} len: {} capacity: {} (recycled: {:?})",
                entry.slot(),
                entry.append_vec_id(),
                *entry.count_and_status.read().unwrap(),
                entry.approx_store_count.load(Ordering::Relaxed),
                entry.accounts.len(),
                entry.accounts.capacity(),
                recycled_time,
            );
        }
    }

    fn print_index(&self, label: &str) {
        let mut alive_roots: Vec<_> = self.accounts_index.all_alive_roots();
        #[allow(clippy::stable_sort_primitive)]
        alive_roots.sort();
        info!("{}: accounts_index alive_roots: {:?}", label, alive_roots,);
        let full_pubkey_range = Pubkey::new(&[0; 32])..=Pubkey::new(&[0xff; 32]);

        self.accounts_index.account_maps.iter().for_each(|map| {
            for (pubkey, account_entry) in map.items(&full_pubkey_range) {
                info!("  key: {} ref_count: {}", pubkey, account_entry.ref_count(),);
                info!(
                    "      slots: {:?}",
                    *account_entry.slot_list.read().unwrap()
                );
            }
        });
    }

    fn print_count_and_status(&self, label: &str) {
        let mut slots: Vec<_> = self.storage.all_slots();
        #[allow(clippy::stable_sort_primitive)]
        slots.sort();
        info!("{}: count_and status for {} slots:", label, slots.len());
        for slot in &slots {
            let slot_stores = self.storage.get_slot_stores(*slot).unwrap();
            let r_slot_stores = slot_stores.read().unwrap();
            let mut ids: Vec<_> = r_slot_stores.keys().cloned().collect();
            #[allow(clippy::stable_sort_primitive)]
            ids.sort();
            for id in &ids {
                let entry = r_slot_stores.get(id).unwrap();
                info!(
                    "  slot: {} id: {} count_and_status: {:?} approx_store_count: {} len: {} capacity: {}",
                    slot,
                    id,
                    *entry.count_and_status.read().unwrap(),
                    entry.approx_store_count.load(Ordering::Relaxed),
                    entry.accounts.len(),
                    entry.accounts.capacity(),
                );
            }
        }
    }
}

/// Specify the source of the accounts data when calculating the accounts hash
///
/// Using the Index is meant for testing the hash calculation itself and debugging;
/// not intended during normal validator operation.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum CalcAccountsHashDataSource {
    IndexForTests,
    Storages,
}

#[cfg(test)]
impl AccountsDb {
    pub fn new(paths: Vec<PathBuf>, cluster_type: &ClusterType) -> Self {
        Self::new_for_tests(paths, cluster_type)
    }

    pub fn new_with_config_for_tests(
        paths: Vec<PathBuf>,
        cluster_type: &ClusterType,
        account_indexes: AccountSecondaryIndexes,
        shrink_ratio: AccountShrinkThreshold,
    ) -> Self {
        Self::new_with_config(
            paths,
            cluster_type,
            account_indexes,
            shrink_ratio,
            Some(ACCOUNTS_DB_CONFIG_FOR_TESTING),
            None,
            &Arc::default(),
        )
    }

    pub fn new_sized(paths: Vec<PathBuf>, file_size: u64) -> Self {
        AccountsDb {
            file_size,
            ..AccountsDb::new(paths, &ClusterType::Development)
        }
    }

    pub fn new_sized_caching(paths: Vec<PathBuf>, file_size: u64) -> Self {
        AccountsDb {
            file_size,
            ..AccountsDb::new(paths, &ClusterType::Development)
        }
    }

    pub fn new_sized_no_extra_stores(paths: Vec<PathBuf>, file_size: u64) -> Self {
        AccountsDb {
            file_size,
            ..AccountsDb::new(paths, &ClusterType::Development)
        }
    }

    pub fn get_append_vec_id(&self, pubkey: &Pubkey, slot: Slot) -> Option<AppendVecId> {
        let ancestors = vec![(slot, 1)].into_iter().collect();
        let result = self.accounts_index.get(pubkey, Some(&ancestors), None);
        result.map(|(list, index)| list.slot_list()[index].1.store_id())
    }

    pub fn alive_account_count_in_slot(&self, slot: Slot) -> usize {
        self.storage
            .get_slot_stores(slot)
            .map(|storages| storages.read().unwrap().values().map(|s| s.count()).sum())
            .unwrap_or(0)
            .saturating_add(
                self.accounts_cache
                    .slot_cache(slot)
                    .map(|slot_cache| slot_cache.len())
                    .unwrap_or_default(),
            )
    }
}

/// A set of utility functions used for testing and benchmarking
pub mod test_utils {
    use {
        super::*,
        crate::{accounts::Accounts, append_vec::aligned_stored_size},
    };

    pub fn create_test_accounts(
        accounts: &Accounts,
        pubkeys: &mut Vec<Pubkey>,
        num: usize,
        slot: Slot,
    ) {
        let data_size = 0;
        if accounts.accounts_db.storage.get_slot_stores(slot).is_none() {
            let bytes_required = num * aligned_stored_size(data_size);
            // allocate an append vec for this slot that can hold all the test accounts. This prevents us from creating more than 1 append vec for this slot.
            _ = accounts.accounts_db.create_and_insert_store(
                slot,
                bytes_required as u64,
                "create_test_accounts",
            );
        }

        for t in 0..num {
            let pubkey = solana_sdk::pubkey::new_rand();
            let account = AccountSharedData::new(
                (t + 1) as u64,
                data_size,
                AccountSharedData::default().owner(),
            );
            accounts.store_slow_uncached(slot, &pubkey, &account);
            pubkeys.push(pubkey);
        }
    }

    // Only used by bench, not safe to call otherwise accounts can conflict with the
    // accounts cache!
    pub fn update_accounts_bench(accounts: &Accounts, pubkeys: &[Pubkey], slot: u64) {
        for pubkey in pubkeys {
            let amount = thread_rng().gen_range(0, 10);
            let account = AccountSharedData::new(amount, 0, AccountSharedData::default().owner());
            accounts.store_slow_uncached(slot, pubkey, &account);
        }
    }
}

#[cfg(test)]
pub mod tests {
    use {
        super::*,
        crate::{
            accounts_hash::MERKLE_FANOUT,
            accounts_index::{
                tests::*, AccountSecondaryIndexesIncludeExclude, ReadAccountMapEntry, RefCount,
            },
            append_vec::{test_utils::TempFile, AccountMeta, StoredMeta},
            cache_hash_data_stats::CacheHashDataStats,
            inline_spl_token,
        },
        assert_matches::assert_matches,
        itertools::Itertools,
        rand::{prelude::SliceRandom, thread_rng, Rng},
        solana_sdk::{
            account::{
                accounts_equal, Account, AccountSharedData, ReadableAccount, WritableAccount,
            },
            hash::HASH_BYTES,
            pubkey::PUBKEY_BYTES,
        },
        std::{
            iter::FromIterator,
            str::FromStr,
            sync::atomic::AtomicBool,
            thread::{self, Builder, JoinHandle},
        },
    };

    fn linear_ancestors(end_slot: u64) -> Ancestors {
        let mut ancestors: Ancestors = vec![(0, 0)].into_iter().collect();
        for i in 1..end_slot {
            ancestors.insert(i, (i - 1) as usize);
        }
        ancestors
    }

    fn empty_storages<'a>() -> SortedStorages<'a> {
        SortedStorages::new(&[])
    }

    impl AccountsDb {
        fn scan_snapshot_stores(
            &self,
            storage: &SortedStorages,
            stats: &mut crate::accounts_hash::HashStats,
            bins: usize,
            bin_range: &Range<usize>,
            check_hash: bool,
        ) -> Result<Vec<CacheHashDataFile>, BankHashVerificationError> {
            let temp_dir = TempDir::new().unwrap();
            let accounts_hash_cache_path = temp_dir.path();
            self.scan_snapshot_stores_with_cache(
                &CacheHashData::new(accounts_hash_cache_path),
                storage,
                stats,
                bins,
                bin_range,
                &CalcAccountsHashConfig {
                    check_hash,
                    ..CalcAccountsHashConfig::default()
                },
                None,
            )
        }

        fn load_without_fixed_root(
            &self,
            ancestors: &Ancestors,
            pubkey: &Pubkey,
        ) -> Option<(AccountSharedData, Slot)> {
            self.do_load(
                ancestors,
                pubkey,
                None,
                LoadHint::Unspecified,
                // callers of this expect zero lamport accounts that exist in the index to be returned as Some(empty)
                LoadZeroLamports::SomeWithZeroLamportAccountForTests,
            )
        }

        fn get_storage_for_slot(&self, slot: Slot) -> Option<Arc<AccountStorageEntry>> {
            self.storage.get_slot_storage_entry(slot)
        }
    }

    /// This impl exists until this feature is activated:
    ///  ignore slot when calculating an account hash #28420
    /// For now, all test code will continue to work thanks to this impl
    /// Tests will use INCLUDE_SLOT_IN_HASH_TESTS for 'include_slot_in_hash' calls.
    impl<'a, T: ReadableAccount + Sync> StorableAccounts<'a, T> for (Slot, &'a [(&'a Pubkey, &'a T)]) {
        fn pubkey(&self, index: usize) -> &Pubkey {
            self.1[index].0
        }
        fn account(&self, index: usize) -> &T {
            self.1[index].1
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
            INCLUDE_SLOT_IN_HASH_TESTS
        }
    }

    /// this tuple contains slot info PER account
    impl<'a, T: ReadableAccount + Sync> StorableAccounts<'a, T>
        for (Slot, &'a [(&'a Pubkey, &'a T, Slot)])
    {
        fn pubkey(&self, index: usize) -> &Pubkey {
            self.1[index].0
        }
        fn account(&self, index: usize) -> &T {
            self.1[index].1
        }
        fn slot(&self, index: usize) -> Slot {
            // note that this could be different than 'target_slot()' PER account
            self.1[index].2
        }
        fn target_slot(&self) -> Slot {
            self.0
        }
        fn len(&self) -> usize {
            self.1.len()
        }
        fn contains_multiple_slots(&self) -> bool {
            let len = self.len();
            if len > 0 {
                let slot = self.slot(0);
                // true if any item has a different slot than the first item
                (1..len).any(|i| slot != self.slot(i))
            } else {
                false
            }
        }
        fn include_slot_in_hash(&self) -> IncludeSlotInHash {
            INCLUDE_SLOT_IN_HASH_TESTS
        }
    }

    impl CurrentAncientAppendVec {
        /// note this requires that 'slot_and_append_vec' is Some
        fn append_vec_id(&self) -> AppendVecId {
            self.append_vec().append_vec_id()
        }
    }

    #[test]
    fn test_maybe_unref_accounts_already_in_ancient() {
        let db = AccountsDb::new_single_for_tests();
        let slot0 = 0;
        let slot1 = 1;
        let available_bytes = 1_000_000;
        let mut current_ancient = CurrentAncientAppendVec::default();

        // setup 'to_store'
        let pubkey = Pubkey::new(&[1; 32]);
        let store_id = AppendVecId::default();
        let account_size = 3;

        let account = AccountSharedData::default();

        let account_meta = AccountMeta {
            lamports: 1,
            owner: Pubkey::new(&[2; 32]),
            executable: false,
            rent_epoch: 0,
        };
        let offset = 3;
        let hash = Hash::new(&[2; 32]);
        let stored_meta = StoredMeta {
            /// global write version
            write_version_obsolete: 0,
            /// key for the account
            pubkey,
            data_len: 43,
        };
        let account = StoredAccountMeta {
            meta: &stored_meta,
            /// account data
            account_meta: &account_meta,
            data: account.data(),
            offset,
            stored_size: account_size,
            hash: &hash,
        };
        let found = FoundStoredAccount { account, store_id };
        let map = vec![&found];
        let alive_total_bytes = found.account.stored_size;
        let to_store = AccountsToStore::new(available_bytes, &map, alive_total_bytes, slot0);
        // Done: setup 'to_store'

        // there has to be an existing append vec at this slot for a new current ancient at the slot to make sense
        let _existing_append_vec = db.create_and_insert_store(slot0, 1000, "test");
        {
            let _shrink_in_progress = current_ancient.create_ancient_append_vec(slot0, &db);
        }
        let mut ancient_slot_pubkeys = AncientSlotPubkeys::default();
        assert!(ancient_slot_pubkeys.inner.is_none());
        // same slot as current_ancient, so no-op
        ancient_slot_pubkeys.maybe_unref_accounts_already_in_ancient(
            slot0,
            &db,
            &current_ancient,
            &to_store,
        );
        assert!(ancient_slot_pubkeys.inner.is_none());
        // different slot than current_ancient, so update 'ancient_slot_pubkeys'
        // there has to be an existing append vec at this slot for a new current ancient at the slot to make sense
        let _existing_append_vec = db.create_and_insert_store(slot1, 1000, "test");
        let _shrink_in_progress = current_ancient.create_ancient_append_vec(slot1, &db);
        let slot2 = 2;
        ancient_slot_pubkeys.maybe_unref_accounts_already_in_ancient(
            slot2,
            &db,
            &current_ancient,
            &to_store,
        );
        assert!(ancient_slot_pubkeys.inner.is_some());
        assert_eq!(ancient_slot_pubkeys.inner.as_ref().unwrap().slot, slot1);
        assert!(ancient_slot_pubkeys
            .inner
            .as_ref()
            .unwrap()
            .pubkeys
            .contains(&pubkey));
        assert_eq!(
            ancient_slot_pubkeys.inner.as_ref().unwrap().pubkeys.len(),
            1
        );
    }

    #[test]
    fn test_get_keys_to_unref_ancient() {
        let rent_epoch = 0;
        let lamports = 0;
        let executable = false;
        let owner = Pubkey::default();
        let data = Vec::new();

        let pubkey = solana_sdk::pubkey::new_rand();
        let pubkey2 = solana_sdk::pubkey::new_rand();
        let pubkey3 = solana_sdk::pubkey::new_rand();
        let pubkey4 = solana_sdk::pubkey::new_rand();

        let meta = StoredMeta {
            write_version_obsolete: 5,
            pubkey,
            data_len: 7,
        };
        let meta2 = StoredMeta {
            write_version_obsolete: 5,
            pubkey: pubkey2,
            data_len: 7,
        };
        let meta3 = StoredMeta {
            write_version_obsolete: 5,
            pubkey: pubkey3,
            data_len: 7,
        };
        let meta4 = StoredMeta {
            write_version_obsolete: 5,
            pubkey: pubkey4,
            data_len: 7,
        };
        let account_meta = AccountMeta {
            lamports,
            owner,
            executable,
            rent_epoch,
        };
        let offset = 99;
        let stored_size = 101;
        let hash = Hash::new_unique();
        let stored_account = StoredAccountMeta {
            meta: &meta,
            account_meta: &account_meta,
            data: &data,
            offset,
            stored_size,
            hash: &hash,
        };
        let stored_account2 = StoredAccountMeta {
            meta: &meta2,
            account_meta: &account_meta,
            data: &data,
            offset,
            stored_size,
            hash: &hash,
        };
        let stored_account3 = StoredAccountMeta {
            meta: &meta3,
            account_meta: &account_meta,
            data: &data,
            offset,
            stored_size,
            hash: &hash,
        };
        let stored_account4 = StoredAccountMeta {
            meta: &meta4,
            account_meta: &account_meta,
            data: &data,
            offset,
            stored_size,
            hash: &hash,
        };
        let store_id = 0;
        let found_account = FoundStoredAccount {
            account: stored_account,
            store_id,
        };
        let found_account2 = FoundStoredAccount {
            account: stored_account2,
            store_id,
        };
        let found_account3 = FoundStoredAccount {
            account: stored_account3,
            store_id,
        };
        let found_account4 = FoundStoredAccount {
            account: stored_account4,
            store_id,
        };
        let mut existing_ancient_pubkeys = HashSet::default();
        let accounts = [&found_account];
        // pubkey NOT in existing_ancient_pubkeys, so do NOT unref, but add to existing_ancient_pubkeys
        let unrefs =
            AccountsDb::get_keys_to_unref_ancient(&accounts, &mut existing_ancient_pubkeys);
        assert!(unrefs.is_empty());
        assert_eq!(
            existing_ancient_pubkeys.iter().collect::<Vec<_>>(),
            vec![&pubkey]
        );
        // pubkey already in existing_ancient_pubkeys, so DO unref
        let unrefs =
            AccountsDb::get_keys_to_unref_ancient(&accounts, &mut existing_ancient_pubkeys);
        assert_eq!(
            existing_ancient_pubkeys.iter().collect::<Vec<_>>(),
            vec![&pubkey]
        );
        assert_eq!(unrefs.iter().cloned().collect::<Vec<_>>(), vec![&pubkey]);
        // pubkey2 NOT in existing_ancient_pubkeys, so do NOT unref, but add to existing_ancient_pubkeys
        let accounts = [&found_account2];
        let unrefs =
            AccountsDb::get_keys_to_unref_ancient(&accounts, &mut existing_ancient_pubkeys);
        assert!(unrefs.is_empty());
        assert_eq!(
            existing_ancient_pubkeys.iter().sorted().collect::<Vec<_>>(),
            vec![&pubkey, &pubkey2]
                .into_iter()
                .sorted()
                .collect::<Vec<_>>()
        );
        // pubkey2 already in existing_ancient_pubkeys, so DO unref
        let unrefs =
            AccountsDb::get_keys_to_unref_ancient(&accounts, &mut existing_ancient_pubkeys);
        assert_eq!(
            existing_ancient_pubkeys.iter().sorted().collect::<Vec<_>>(),
            vec![&pubkey, &pubkey2]
                .into_iter()
                .sorted()
                .collect::<Vec<_>>()
        );
        assert_eq!(unrefs.iter().cloned().collect::<Vec<_>>(), vec![&pubkey2]);
        // pubkey3/4 NOT in existing_ancient_pubkeys, so do NOT unref, but add to existing_ancient_pubkeys
        let accounts = [&found_account3, &found_account4];
        let unrefs =
            AccountsDb::get_keys_to_unref_ancient(&accounts, &mut existing_ancient_pubkeys);
        assert!(unrefs.is_empty());
        assert_eq!(
            existing_ancient_pubkeys.iter().sorted().collect::<Vec<_>>(),
            vec![&pubkey, &pubkey2, &pubkey3, &pubkey4]
                .into_iter()
                .sorted()
                .collect::<Vec<_>>()
        );
        // pubkey3/4 already in existing_ancient_pubkeys, so DO unref
        let unrefs =
            AccountsDb::get_keys_to_unref_ancient(&accounts, &mut existing_ancient_pubkeys);
        assert_eq!(
            existing_ancient_pubkeys.iter().sorted().collect::<Vec<_>>(),
            vec![&pubkey, &pubkey2, &pubkey3, &pubkey4]
                .into_iter()
                .sorted()
                .collect::<Vec<_>>()
        );
        assert_eq!(
            unrefs.iter().cloned().sorted().collect::<Vec<_>>(),
            vec![&pubkey3, &pubkey4]
                .into_iter()
                .sorted()
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_retain_roots_within_one_epoch_range() {
        let mut roots = vec![0, 1, 2];
        let slots_per_epoch = 2;
        AccountsDb::retain_roots_within_one_epoch_range(&mut roots, slots_per_epoch);
        assert_eq!(&vec![1, 2], &roots);
    }

    #[test]
    #[should_panic(
        expected = "bin_range.start < bins && bin_range.end <= bins &&\\n    bin_range.start < bin_range.end"
    )]
    fn test_accountsdb_scan_snapshot_stores_illegal_range_start() {
        let mut stats = HashStats::default();
        let bounds = Range { start: 2, end: 2 };
        let accounts_db = AccountsDb::new_single_for_tests();

        accounts_db
            .scan_snapshot_stores(&empty_storages(), &mut stats, 2, &bounds, false)
            .unwrap();
    }
    #[test]
    #[should_panic(
        expected = "bin_range.start < bins && bin_range.end <= bins &&\\n    bin_range.start < bin_range.end"
    )]
    fn test_accountsdb_scan_snapshot_stores_illegal_range_end() {
        let mut stats = HashStats::default();
        let bounds = Range { start: 1, end: 3 };

        let accounts_db = AccountsDb::new_single_for_tests();
        accounts_db
            .scan_snapshot_stores(&empty_storages(), &mut stats, 2, &bounds, false)
            .unwrap();
    }

    #[test]
    #[should_panic(
        expected = "bin_range.start < bins && bin_range.end <= bins &&\\n    bin_range.start < bin_range.end"
    )]
    fn test_accountsdb_scan_snapshot_stores_illegal_range_inverse() {
        let mut stats = HashStats::default();
        let bounds = Range { start: 1, end: 0 };

        let accounts_db = AccountsDb::new_single_for_tests();
        accounts_db
            .scan_snapshot_stores(&empty_storages(), &mut stats, 2, &bounds, false)
            .unwrap();
    }

    fn sample_storages_and_account_in_slot(
        slot: Slot,
        accounts: &AccountsDb,
    ) -> (SnapshotStorages, Vec<CalculateHashIntermediate>) {
        let pubkey0 = Pubkey::new(&[0u8; 32]);
        let pubkey127 = Pubkey::new(&[0x7fu8; 32]);
        let pubkey128 = Pubkey::new(&[0x80u8; 32]);
        let pubkey255 = Pubkey::new(&[0xffu8; 32]);

        let mut raw_expected = vec![
            CalculateHashIntermediate::new(Hash::default(), 1, pubkey0),
            CalculateHashIntermediate::new(Hash::default(), 128, pubkey127),
            CalculateHashIntermediate::new(Hash::default(), 129, pubkey128),
            CalculateHashIntermediate::new(Hash::default(), 256, pubkey255),
        ];

        let expected_hashes = vec![
            Hash::from_str("5K3NW73xFHwgTWVe4LyCg4QfQda8f88uZj2ypDx2kmmH").unwrap(),
            Hash::from_str("84ozw83MZ8oeSF4hRAg7SeW1Tqs9LMXagX1BrDRjtZEx").unwrap(),
            Hash::from_str("5XqtnEJ41CG2JWNp7MAg9nxkRUAnyjLxfsKsdrLxQUbC").unwrap(),
            Hash::from_str("DpvwJcznzwULYh19Zu5CuAA4AT6WTBe4H6n15prATmqj").unwrap(),
        ];

        let mut raw_accounts = Vec::default();

        for i in 0..raw_expected.len() {
            raw_accounts.push(AccountSharedData::new(
                raw_expected[i].lamports,
                1,
                AccountSharedData::default().owner(),
            ));
            let hash = AccountsDb::hash_account(
                slot,
                &raw_accounts[i],
                &raw_expected[i].pubkey,
                INCLUDE_SLOT_IN_HASH_TESTS,
            );
            if slot == 1 {
                assert_eq!(hash, expected_hashes[i]);
            }
            raw_expected[i].hash = hash;
        }

        let to_store = raw_accounts
            .iter()
            .zip(raw_expected.iter())
            .map(|(account, intermediate)| (&intermediate.pubkey, account))
            .collect::<Vec<_>>();

        accounts.store_for_tests(slot, &to_store[..]);
        accounts.add_root_and_flush_write_cache(slot);

        let (storages, slots) = accounts.get_snapshot_storages(..=slot, None);
        assert_eq!(storages.len(), slots.len());
        storages
            .iter()
            .zip(slots.iter())
            .for_each(|(storages, slot)| {
                for storage in storages {
                    assert_eq!(&storage.slot(), slot);
                }
            });
        (storages, raw_expected)
    }

    fn sample_storages_and_accounts(
        accounts: &AccountsDb,
    ) -> (SnapshotStorages, Vec<CalculateHashIntermediate>) {
        sample_storages_and_account_in_slot(1, accounts)
    }

    fn get_storage_refs(input: &[SnapshotStorage]) -> SortedStorages {
        SortedStorages::new(input)
    }

    /// helper to compare expected binned data with scan result in cache files
    /// result: return from scanning
    /// expected: binned data expected
    /// bins: # bins total to divide pubkeys into
    /// start_bin_index: bin # that was the minimum # we were scanning for 0<=start_bin_index<bins
    /// bin_range: end_exclusive-start_bin_index passed to scan
    fn assert_scan(
        result: Vec<CacheHashDataFile>,
        expected: Vec<BinnedHashData>,
        bins: usize,
        start_bin_index: usize,
        bin_range: usize,
    ) {
        assert_eq!(expected.len(), result.len());

        for cache_file in &result {
            let mut result2 = (0..bin_range).map(|_| Vec::default()).collect::<Vec<_>>();
            cache_file.load_all(
                &mut result2,
                start_bin_index,
                &PubkeyBinCalculator24::new(bins),
                &mut CacheHashDataStats::default(),
            );
            assert_eq!(
                convert_to_slice(&[result2]),
                expected,
                "bins: {bins}, start_bin_index: {start_bin_index}"
            );
        }
    }

    #[test]
    fn test_accountsdb_scan_snapshot_stores() {
        solana_logger::setup();
        let accounts_db = AccountsDb::new_single_for_tests();
        let (storages, raw_expected) = sample_storages_and_accounts(&accounts_db);

        let bins = 1;
        let mut stats = HashStats::default();

        let result = accounts_db
            .scan_snapshot_stores(
                &get_storage_refs(&storages),
                &mut stats,
                bins,
                &Range {
                    start: 0,
                    end: bins,
                },
                false,
            )
            .unwrap();
        assert_scan(result, vec![vec![raw_expected.clone()]], bins, 0, bins);

        let bins = 2;
        let accounts_db = AccountsDb::new_single_for_tests();
        let result = accounts_db
            .scan_snapshot_stores(
                &get_storage_refs(&storages),
                &mut stats,
                bins,
                &Range {
                    start: 0,
                    end: bins,
                },
                false,
            )
            .unwrap();
        let mut expected = vec![Vec::new(); bins];
        expected[0].push(raw_expected[0].clone());
        expected[0].push(raw_expected[1].clone());
        expected[bins - 1].push(raw_expected[2].clone());
        expected[bins - 1].push(raw_expected[3].clone());
        assert_scan(result, vec![expected], bins, 0, bins);

        let bins = 4;
        let accounts_db = AccountsDb::new_single_for_tests();
        let result = accounts_db
            .scan_snapshot_stores(
                &get_storage_refs(&storages),
                &mut stats,
                bins,
                &Range {
                    start: 0,
                    end: bins,
                },
                false,
            )
            .unwrap();
        let mut expected = vec![Vec::new(); bins];
        expected[0].push(raw_expected[0].clone());
        expected[1].push(raw_expected[1].clone());
        expected[2].push(raw_expected[2].clone());
        expected[bins - 1].push(raw_expected[3].clone());
        assert_scan(result, vec![expected], bins, 0, bins);

        let bins = 256;
        let accounts_db = AccountsDb::new_single_for_tests();
        let result = accounts_db
            .scan_snapshot_stores(
                &get_storage_refs(&storages),
                &mut stats,
                bins,
                &Range {
                    start: 0,
                    end: bins,
                },
                false,
            )
            .unwrap();
        let mut expected = vec![Vec::new(); bins];
        expected[0].push(raw_expected[0].clone());
        expected[127].push(raw_expected[1].clone());
        expected[128].push(raw_expected[2].clone());
        expected[bins - 1].push(raw_expected.last().unwrap().clone());
        assert_scan(result, vec![expected], bins, 0, bins);
    }

    #[test]
    fn test_accountsdb_scan_snapshot_stores_2nd_chunk() {
        let accounts_db = AccountsDb::new_single_for_tests();
        // enough stores to get to 2nd chunk
        let bins = 1;
        let slot = MAX_ITEMS_PER_CHUNK as Slot;
        let (storages, raw_expected) = sample_storages_and_account_in_slot(slot, &accounts_db);
        let storage_data = vec![(&storages[0], slot)];

        let sorted_storages =
            SortedStorages::new_debug(&storage_data[..], 0, MAX_ITEMS_PER_CHUNK as usize + 1);

        let mut stats = HashStats::default();
        let result = accounts_db
            .scan_snapshot_stores(
                &sorted_storages,
                &mut stats,
                bins,
                &Range {
                    start: 0,
                    end: bins,
                },
                false,
            )
            .unwrap();

        assert_scan(result, vec![vec![raw_expected]], bins, 0, bins);
    }

    #[test]
    fn test_accountsdb_scan_snapshot_stores_binning() {
        let mut stats = HashStats::default();
        let accounts_db = AccountsDb::new_single_for_tests();
        let (storages, raw_expected) = sample_storages_and_accounts(&accounts_db);

        // just the first bin of 2
        let bins = 2;
        let half_bins = bins / 2;
        let result = accounts_db
            .scan_snapshot_stores(
                &get_storage_refs(&storages),
                &mut stats,
                bins,
                &Range {
                    start: 0,
                    end: half_bins,
                },
                false,
            )
            .unwrap();
        let mut expected = vec![Vec::new(); half_bins];
        expected[0].push(raw_expected[0].clone());
        expected[0].push(raw_expected[1].clone());
        assert_scan(result, vec![expected], bins, 0, half_bins);

        // just the second bin of 2
        let accounts_db = AccountsDb::new_single_for_tests();
        let result = accounts_db
            .scan_snapshot_stores(
                &get_storage_refs(&storages),
                &mut stats,
                bins,
                &Range {
                    start: 1,
                    end: bins,
                },
                false,
            )
            .unwrap();

        let mut expected = vec![Vec::new(); half_bins];
        let starting_bin_index = 0;
        expected[starting_bin_index].push(raw_expected[2].clone());
        expected[starting_bin_index].push(raw_expected[3].clone());
        assert_scan(result, vec![expected], bins, 1, bins - 1);

        // 1 bin at a time of 4
        let bins = 4;
        let accounts_db = AccountsDb::new_single_for_tests();

        for (bin, expected_item) in raw_expected.iter().enumerate().take(bins) {
            let result = accounts_db
                .scan_snapshot_stores(
                    &get_storage_refs(&storages),
                    &mut stats,
                    bins,
                    &Range {
                        start: bin,
                        end: bin + 1,
                    },
                    false,
                )
                .unwrap();
            let mut expected = vec![Vec::new(); 1];
            expected[0].push(expected_item.clone());
            assert_scan(result, vec![expected], bins, bin, 1);
        }

        let bins = 256;
        let bin_locations = vec![0, 127, 128, 255];
        let range = 1;
        for bin in 0..bins {
            let accounts_db = AccountsDb::new_single_for_tests();
            let result = accounts_db
                .scan_snapshot_stores(
                    &get_storage_refs(&storages),
                    &mut stats,
                    bins,
                    &Range {
                        start: bin,
                        end: bin + range,
                    },
                    false,
                )
                .unwrap();
            let mut expected = vec![];
            if let Some(index) = bin_locations.iter().position(|&r| r == bin) {
                expected = vec![Vec::new(); range];
                expected[0].push(raw_expected[index].clone());
            }
            let mut result2 = (0..range).map(|_| Vec::default()).collect::<Vec<_>>();
            if let Some(m) = result.get(0) {
                m.load_all(
                    &mut result2,
                    bin,
                    &PubkeyBinCalculator24::new(bins),
                    &mut CacheHashDataStats::default(),
                );
            } else {
                result2 = vec![];
            }

            assert_eq!(result2, expected);
        }
    }

    #[test]
    fn test_accountsdb_scan_snapshot_stores_binning_2nd_chunk() {
        let accounts_db = AccountsDb::new_single_for_tests();
        // enough stores to get to 2nd chunk
        // range is for only 1 bin out of 256.
        let bins = 256;
        let slot = MAX_ITEMS_PER_CHUNK as Slot;
        let (storages, raw_expected) = sample_storages_and_account_in_slot(slot, &accounts_db);
        let storage_data = vec![(&storages[0], slot)];

        let sorted_storages =
            SortedStorages::new_debug(&storage_data[..], 0, MAX_ITEMS_PER_CHUNK as usize + 1);

        let mut stats = HashStats::default();
        let range = 1;
        let start = 127;
        let result = accounts_db
            .scan_snapshot_stores(
                &sorted_storages,
                &mut stats,
                bins,
                &Range {
                    start,
                    end: start + range,
                },
                false,
            )
            .unwrap();
        assert_eq!(result.len(), 1); // 2 chunks, but 1 is empty so not included
        let mut expected = vec![Vec::new(); range];
        expected[0].push(raw_expected[1].clone());
        let mut result2 = (0..range).map(|_| Vec::default()).collect::<Vec<_>>();
        result[0].load_all(
            &mut result2,
            0,
            &PubkeyBinCalculator24::new(range),
            &mut CacheHashDataStats::default(),
        );
        assert_eq!(result2.len(), 1);
        assert_eq!(result2, expected);
    }

    #[test]
    fn test_accountsdb_calculate_accounts_hash_from_storages_simple() {
        solana_logger::setup();

        let (storages, _size, _slot_expected) = sample_storage();
        let db = AccountsDb::new(Vec::new(), &ClusterType::Development);
        let result = db
            .calculate_accounts_hash_from_storages(
                &CalcAccountsHashConfig::default(),
                &get_storage_refs(&storages),
                HashStats::default(),
            )
            .unwrap();
        let expected_hash = Hash::from_str("GKot5hBsd81kMupNCXHaqbhv3huEbxAFMLnpcX2hniwn").unwrap();
        let expected_accounts_hash = AccountsHash(expected_hash);
        assert_eq!(result, (expected_accounts_hash, 0));
    }

    #[test]
    fn test_accountsdb_calculate_accounts_hash_from_storages() {
        solana_logger::setup();

        let db = AccountsDb::new(Vec::new(), &ClusterType::Development);
        let (storages, raw_expected) = sample_storages_and_accounts(&db);
        let expected_hash =
            AccountsHasher::compute_merkle_root_loop(raw_expected.clone(), MERKLE_FANOUT, |item| {
                &item.hash
            });
        let sum = raw_expected.iter().map(|item| item.lamports).sum();
        let result = db
            .calculate_accounts_hash_from_storages(
                &CalcAccountsHashConfig::default(),
                &get_storage_refs(&storages),
                HashStats::default(),
            )
            .unwrap();

        let expected_accounts_hash = AccountsHash(expected_hash);
        assert_eq!(result, (expected_accounts_hash, sum));
    }

    fn sample_storage() -> (SnapshotStorages, usize, Slot) {
        let (_temp_dirs, paths) = get_temp_accounts_paths(1).unwrap();
        let slot_expected: Slot = 0;
        let size: usize = 123;
        let data = AccountStorageEntry::new(&paths[0], slot_expected, 0, size as u64);

        let arc = Arc::new(data);
        let storages = vec![vec![arc]];
        (storages, size, slot_expected)
    }

    #[derive(Clone)]
    struct TestScan {
        calls: Arc<AtomicU64>,
        pubkey: Pubkey,
        slot_expected: Slot,
        accum: BinnedHashData,
        current_slot: Slot,
        value_to_use_for_lamports: u64,
    }

    impl AppendVecScan for TestScan {
        fn filter(&mut self, _pubkey: &Pubkey) -> bool {
            true
        }
        fn set_slot(&mut self, slot: Slot) {
            self.current_slot = slot;
        }
        fn init_accum(&mut self, _count: usize) {}
        fn get_accum(&mut self) -> BinnedHashData {
            std::mem::take(&mut self.accum)
        }
        fn set_accum(&mut self, accum: BinnedHashData) {
            self.accum = accum;
        }
        fn found_account(&mut self, loaded_account: &LoadedAccount) {
            self.calls.fetch_add(1, Ordering::Relaxed);
            assert_eq!(loaded_account.pubkey(), &self.pubkey);
            assert_eq!(self.slot_expected, self.current_slot);
            self.accum.push(vec![CalculateHashIntermediate::new(
                Hash::default(),
                self.value_to_use_for_lamports,
                self.pubkey,
            )]);
        }
        fn scanning_complete(self) -> BinnedHashData {
            self.accum
        }
    }

    #[test]
    fn test_accountsdb_scan_account_storage_no_bank() {
        solana_logger::setup();

        let expected = 1;
        let tf = crate::append_vec::test_utils::get_append_vec_path(
            "test_accountsdb_scan_account_storage_no_bank",
        );
        let (_temp_dirs, paths) = get_temp_accounts_paths(1).unwrap();
        let slot_expected: Slot = 0;
        let size: usize = 123;
        let mut data = AccountStorageEntry::new(&paths[0], slot_expected, 0, size as u64);
        let av = AppendVec::new(&tf.path, true, 1024 * 1024);
        data.accounts = av;

        let arc = Arc::new(data);
        let storages = vec![vec![arc]];
        let pubkey = solana_sdk::pubkey::new_rand();
        let acc = AccountSharedData::new(1, 48, AccountSharedData::default().owner());
        append_single_account_with_default_hash(&storages[0][0].accounts, &pubkey, &acc, 1);

        let calls = Arc::new(AtomicU64::new(0));
        let temp_dir = TempDir::new().unwrap();
        let accounts_hash_cache_path = temp_dir.path();
        let accounts_db = AccountsDb::new_single_for_tests();

        let test_scan = TestScan {
            calls: calls.clone(),
            pubkey,
            slot_expected,
            accum: Vec::default(),
            current_slot: 0,
            value_to_use_for_lamports: expected,
        };

        let result = accounts_db.scan_account_storage_no_bank(
            &CacheHashData::new(accounts_hash_cache_path),
            &CalcAccountsHashConfig::default(),
            &get_storage_refs(&storages),
            test_scan,
            &Range { start: 0, end: 1 },
            &HashStats::default(),
        );
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert_scan(
            result,
            vec![vec![vec![CalculateHashIntermediate::new(
                Hash::default(),
                expected,
                pubkey,
            )]]],
            1,
            0,
            1,
        );
    }

    fn convert_to_slice(
        input: &[Vec<Vec<CalculateHashIntermediate>>],
    ) -> Vec<Vec<&[CalculateHashIntermediate]>> {
        input
            .iter()
            .map(|v| v.iter().map(|v| &v[..]).collect::<Vec<_>>())
            .collect::<Vec<_>>()
    }

    fn append_single_account_with_default_hash(
        vec: &AppendVec,
        pubkey: &Pubkey,
        account: &AccountSharedData,
        write_version: StoredMetaWriteVersion,
    ) {
        let slot_ignored = Slot::MAX;
        let accounts = [(pubkey, account)];
        let slice = &accounts[..];
        let account_data = (slot_ignored, slice);
        let hash = Hash::default();
        let storable_accounts =
            StorableAccountsWithHashesAndWriteVersions::new_with_hashes_and_write_versions(
                &account_data,
                vec![&hash],
                vec![write_version],
            );
        vec.append_accounts(&storable_accounts, 0);
    }

    #[test]
    fn test_accountsdb_scan_account_storage_no_bank_one_slot() {
        solana_logger::setup();

        let expected = 1;
        let tf = crate::append_vec::test_utils::get_append_vec_path(
            "test_accountsdb_scan_account_storage_no_bank",
        );
        let (_temp_dirs, paths) = get_temp_accounts_paths(1).unwrap();
        let slot_expected: Slot = 0;
        let size: usize = 123;
        let mut data = AccountStorageEntry::new(&paths[0], slot_expected, 0, size as u64);
        let av = AppendVec::new(&tf.path, true, 1024 * 1024);
        data.accounts = av;

        let arc = Arc::new(data);
        let storages = vec![vec![arc]];
        let pubkey = solana_sdk::pubkey::new_rand();
        let acc = AccountSharedData::new(1, 48, AccountSharedData::default().owner());
        append_single_account_with_default_hash(&storages[0][0].accounts, &pubkey, &acc, 1);

        let calls = Arc::new(AtomicU64::new(0));

        let mut test_scan = TestScan {
            calls: calls.clone(),
            pubkey,
            slot_expected,
            accum: Vec::default(),
            current_slot: 0,
            value_to_use_for_lamports: expected,
        };

        AccountsDb::scan_multiple_account_storages_one_slot(&storages[0], &mut test_scan);
        let accum = test_scan.scanning_complete();
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert_eq!(
            accum
                .iter()
                .flatten()
                .map(|a| a.lamports)
                .collect::<Vec<_>>(),
            vec![expected]
        );
    }

    fn append_sample_data_to_storage(
        storages: &SnapshotStorages,
        pubkey: &Pubkey,
        write_version: StoredMetaWriteVersion,
    ) {
        let acc = AccountSharedData::new(1, 48, AccountSharedData::default().owner());
        append_single_account_with_default_hash(
            &storages[0][0].accounts,
            pubkey,
            &acc,
            write_version,
        );
    }

    fn sample_storage_with_entries(
        tf: &TempFile,
        write_version: StoredMetaWriteVersion,
        slot: Slot,
        pubkey: &Pubkey,
    ) -> SnapshotStorages {
        sample_storage_with_entries_id(tf, write_version, slot, pubkey, 0)
    }

    fn sample_storage_with_entries_id(
        tf: &TempFile,
        write_version: StoredMetaWriteVersion,
        slot: Slot,
        pubkey: &Pubkey,
        id: AppendVecId,
    ) -> SnapshotStorages {
        let (_temp_dirs, paths) = get_temp_accounts_paths(1).unwrap();
        let size: usize = 123;
        let mut data = AccountStorageEntry::new(&paths[0], slot, id, size as u64);
        let av = AppendVec::new(&tf.path, true, 1024 * 1024);
        data.accounts = av;

        let arc = Arc::new(data);
        let storages = vec![vec![arc]];
        append_sample_data_to_storage(&storages, pubkey, write_version);
        storages
    }

    #[test]
    fn test_accountsdb_scan_multiple_account_storage_no_bank_one_slot() {
        solana_logger::setup();

        let slot_expected: Slot = 0;
        let tf = crate::append_vec::test_utils::get_append_vec_path(
            "test_accountsdb_scan_account_storage_no_bank",
        );
        let write_version1 = 0;
        let write_version2 = 1;
        let pubkey1 = solana_sdk::pubkey::new_rand();
        let pubkey2 = solana_sdk::pubkey::new_rand();
        for swap in [false, true].iter() {
            let mut storages = [
                sample_storage_with_entries(&tf, write_version1, slot_expected, &pubkey1)
                    .remove(0)
                    .remove(0),
                sample_storage_with_entries(&tf, write_version2, slot_expected, &pubkey2)
                    .remove(0)
                    .remove(0),
            ];
            if *swap {
                storages[..].swap(0, 1);
            }
            let calls = Arc::new(AtomicU64::new(0));
            let mut scanner = TestScanSimple {
                current_slot: 0,
                slot_expected,
                pubkey1,
                pubkey2,
                accum: Vec::default(),
                calls: calls.clone(),
                write_version1,
                write_version2,
            };
            AccountsDb::scan_multiple_account_storages_one_slot(&storages, &mut scanner);
            let accum = scanner.scanning_complete();
            assert_eq!(calls.load(Ordering::Relaxed), storages.len() as u64);
            assert_eq!(
                accum
                    .iter()
                    .flatten()
                    .map(|a| a.lamports)
                    .collect::<Vec<_>>(),
                vec![write_version1, write_version2]
            );
        }
    }

    #[derive(Clone)]
    struct TestScanSimple {
        current_slot: Slot,
        slot_expected: Slot,
        calls: Arc<AtomicU64>,
        accum: BinnedHashData,
        pubkey1: Pubkey,
        pubkey2: Pubkey,
        write_version1: u64,
        write_version2: u64,
    }

    impl AppendVecScan for TestScanSimple {
        fn set_slot(&mut self, slot: Slot) {
            self.current_slot = slot;
        }
        fn filter(&mut self, _pubkey: &Pubkey) -> bool {
            true
        }
        fn init_accum(&mut self, _count: usize) {}
        fn found_account(&mut self, loaded_account: &LoadedAccount) {
            self.calls.fetch_add(1, Ordering::Relaxed);
            let write_version = loaded_account.write_version();
            let first =
                loaded_account.pubkey() == &self.pubkey1 && write_version == self.write_version1;
            assert!(
                first
                    || loaded_account.pubkey() == &self.pubkey2
                        && write_version == self.write_version2
            );
            assert_eq!(self.slot_expected, self.current_slot);
            if first {
                assert!(self.accum.is_empty());
            } else {
                assert!(self.accum.len() == 1);
            }
            self.accum.push(vec![CalculateHashIntermediate {
                hash: Hash::default(),
                lamports: write_version,
                pubkey: Pubkey::default(),
            }]);
        }
        fn scanning_complete(self) -> BinnedHashData {
            self.accum
        }
        fn get_accum(&mut self) -> BinnedHashData {
            std::mem::take(&mut self.accum)
        }
        fn set_accum(&mut self, accum: BinnedHashData) {
            self.accum = accum;
        }
    }

    #[test]
    fn test_accountsdb_add_root() {
        solana_logger::setup();
        let db = AccountsDb::new(Vec::new(), &ClusterType::Development);
        let key = Pubkey::default();
        let account0 = AccountSharedData::new(1, 0, &key);

        db.store_for_tests(0, &[(&key, &account0)]);
        db.add_root(0);
        let ancestors = vec![(1, 1)].into_iter().collect();
        assert_eq!(
            db.load_without_fixed_root(&ancestors, &key),
            Some((account0, 0))
        );
    }

    #[test]
    fn test_accountsdb_latest_ancestor() {
        solana_logger::setup();
        let db = AccountsDb::new(Vec::new(), &ClusterType::Development);
        let key = Pubkey::default();
        let account0 = AccountSharedData::new(1, 0, &key);

        db.store_for_tests(0, &[(&key, &account0)]);

        let account1 = AccountSharedData::new(0, 0, &key);
        db.store_for_tests(1, &[(&key, &account1)]);

        let ancestors = vec![(1, 1)].into_iter().collect();
        assert_eq!(
            &db.load_without_fixed_root(&ancestors, &key).unwrap().0,
            &account1
        );

        let ancestors = vec![(1, 1), (0, 0)].into_iter().collect();
        assert_eq!(
            &db.load_without_fixed_root(&ancestors, &key).unwrap().0,
            &account1
        );

        let mut accounts = Vec::new();
        db.unchecked_scan_accounts(
            "",
            &ancestors,
            |_, account, _| {
                accounts.push(account.take_account());
            },
            &ScanConfig::default(),
        );
        assert_eq!(accounts, vec![account1]);
    }

    #[test]
    fn test_accountsdb_latest_ancestor_with_root() {
        solana_logger::setup();
        let db = AccountsDb::new(Vec::new(), &ClusterType::Development);
        let key = Pubkey::default();
        let account0 = AccountSharedData::new(1, 0, &key);

        db.store_for_tests(0, &[(&key, &account0)]);

        let account1 = AccountSharedData::new(0, 0, &key);
        db.store_for_tests(1, &[(&key, &account1)]);
        db.add_root(0);

        let ancestors = vec![(1, 1)].into_iter().collect();
        assert_eq!(
            &db.load_without_fixed_root(&ancestors, &key).unwrap().0,
            &account1
        );

        let ancestors = vec![(1, 1), (0, 0)].into_iter().collect();
        assert_eq!(
            &db.load_without_fixed_root(&ancestors, &key).unwrap().0,
            &account1
        );
    }

    #[test]
    fn test_accountsdb_root_one_slot() {
        solana_logger::setup();
        let db = AccountsDb::new(Vec::new(), &ClusterType::Development);

        let key = Pubkey::default();
        let account0 = AccountSharedData::new(1, 0, &key);

        // store value 1 in the "root", i.e. db zero
        db.store_for_tests(0, &[(&key, &account0)]);

        // now we have:
        //
        //                       root0 -> key.lamports==1
        //                        / \
        //                       /   \
        //  key.lamports==0 <- slot1    \
        //                             slot2 -> key.lamports==1
        //                                       (via root0)

        // store value 0 in one child
        let account1 = AccountSharedData::new(0, 0, &key);
        db.store_for_tests(1, &[(&key, &account1)]);

        // masking accounts is done at the Accounts level, at accountsDB we see
        // original account (but could also accept "None", which is implemented
        // at the Accounts level)
        let ancestors = vec![(0, 0), (1, 1)].into_iter().collect();
        assert_eq!(
            &db.load_without_fixed_root(&ancestors, &key).unwrap().0,
            &account1
        );

        // we should see 1 token in slot 2
        let ancestors = vec![(0, 0), (2, 2)].into_iter().collect();
        assert_eq!(
            &db.load_without_fixed_root(&ancestors, &key).unwrap().0,
            &account0
        );

        db.add_root(0);

        let ancestors = vec![(1, 1)].into_iter().collect();
        assert_eq!(
            db.load_without_fixed_root(&ancestors, &key),
            Some((account1, 1))
        );
        let ancestors = vec![(2, 2)].into_iter().collect();
        assert_eq!(
            db.load_without_fixed_root(&ancestors, &key),
            Some((account0, 0))
        ); // original value
    }

    #[test]
    fn test_accountsdb_add_root_many() {
        let db = AccountsDb::new(Vec::new(), &ClusterType::Development);

        let mut pubkeys: Vec<Pubkey> = vec![];
        create_account(&db, &mut pubkeys, 0, 100, 0, 0);
        for _ in 1..100 {
            let idx = thread_rng().gen_range(0, 99);
            let ancestors = vec![(0, 0)].into_iter().collect();
            let account = db
                .load_without_fixed_root(&ancestors, &pubkeys[idx])
                .unwrap();
            let default_account = AccountSharedData::from(Account {
                lamports: (idx + 1) as u64,
                ..Account::default()
            });
            assert_eq!((default_account, 0), account);
        }

        db.add_root(0);

        // check that all the accounts appear with a new root
        for _ in 1..100 {
            let idx = thread_rng().gen_range(0, 99);
            let ancestors = vec![(0, 0)].into_iter().collect();
            let account0 = db
                .load_without_fixed_root(&ancestors, &pubkeys[idx])
                .unwrap();
            let ancestors = vec![(1, 1)].into_iter().collect();
            let account1 = db
                .load_without_fixed_root(&ancestors, &pubkeys[idx])
                .unwrap();
            let default_account = AccountSharedData::from(Account {
                lamports: (idx + 1) as u64,
                ..Account::default()
            });
            assert_eq!(&default_account, &account0.0);
            assert_eq!(&default_account, &account1.0);
        }
    }

    #[test]
    fn test_accountsdb_count_stores() {
        solana_logger::setup();
        let db = AccountsDb::new_single_for_tests();

        let mut pubkeys: Vec<Pubkey> = vec![];
        create_account(&db, &mut pubkeys, 0, 2, DEFAULT_FILE_SIZE as usize / 3, 0);
        db.add_root_and_flush_write_cache(0);
        assert!(check_storage(&db, 0, 2));

        let pubkey = solana_sdk::pubkey::new_rand();
        let account = AccountSharedData::new(1, DEFAULT_FILE_SIZE as usize / 3, &pubkey);
        db.store_for_tests(1, &[(&pubkey, &account)]);
        db.store_for_tests(1, &[(&pubkeys[0], &account)]);
        // adding root doesn't change anything
        db.get_accounts_delta_hash(1);
        db.add_root_and_flush_write_cache(1);
        {
            let slot_0_stores = &db.storage.get_slot_stores(0).unwrap();
            let slot_1_stores = &db.storage.get_slot_stores(1).unwrap();
            let r_slot_0_stores = slot_0_stores.read().unwrap();
            let r_slot_1_stores = slot_1_stores.read().unwrap();
            assert_eq!(r_slot_0_stores.len(), 1);
            assert_eq!(r_slot_1_stores.len(), 1);
            assert_eq!(r_slot_0_stores.get(&0).unwrap().count(), 2);
            assert_eq!(r_slot_1_stores[&1].count(), 2);
            assert_eq!(r_slot_0_stores.get(&0).unwrap().approx_stored_count(), 2);
            assert_eq!(r_slot_1_stores[&1].approx_stored_count(), 2);
        }

        // overwrite old rooted account version; only the r_slot_0_stores.count() should be
        // decremented
        // slot 2 is not a root and should be ignored by clean
        db.store_for_tests(2, &[(&pubkeys[0], &account)]);
        db.clean_accounts_for_tests();
        {
            let slot_0_stores = &db.storage.get_slot_stores(0).unwrap();
            let slot_1_stores = &db.storage.get_slot_stores(1).unwrap();
            let r_slot_0_stores = slot_0_stores.read().unwrap();
            let r_slot_1_stores = slot_1_stores.read().unwrap();
            assert_eq!(r_slot_0_stores.len(), 1);
            assert_eq!(r_slot_1_stores.len(), 1);
            assert_eq!(r_slot_0_stores.get(&0).unwrap().count(), 1);
            assert_eq!(r_slot_1_stores[&1].count(), 2);
            assert_eq!(r_slot_0_stores.get(&0).unwrap().approx_stored_count(), 2);
            assert_eq!(r_slot_1_stores[&1].approx_stored_count(), 2);
        }
    }

    #[test]
    fn test_accounts_unsquashed() {
        let key = Pubkey::default();

        // 1 token in the "root", i.e. db zero
        let db0 = AccountsDb::new(Vec::new(), &ClusterType::Development);
        let account0 = AccountSharedData::new(1, 0, &key);
        db0.store_for_tests(0, &[(&key, &account0)]);

        // 0 lamports in the child
        let account1 = AccountSharedData::new(0, 0, &key);
        db0.store_for_tests(1, &[(&key, &account1)]);

        // masking accounts is done at the Accounts level, at accountsDB we see
        // original account
        let ancestors = vec![(0, 0), (1, 1)].into_iter().collect();
        assert_eq!(
            db0.load_without_fixed_root(&ancestors, &key),
            Some((account1, 1))
        );
        let ancestors = vec![(0, 0)].into_iter().collect();
        assert_eq!(
            db0.load_without_fixed_root(&ancestors, &key),
            Some((account0, 0))
        );
    }

    fn run_test_remove_unrooted_slot(is_cached: bool) {
        let unrooted_slot = 9;
        let unrooted_bank_id = 9;
        let db = AccountsDb::new(Vec::new(), &ClusterType::Development);
        let key = Pubkey::default();
        let account0 = AccountSharedData::new(1, 0, &key);
        let ancestors = vec![(unrooted_slot, 1)].into_iter().collect();
        if is_cached {
            db.store_cached((unrooted_slot, &[(&key, &account0)][..]), None);
        } else {
            db.store_for_tests(unrooted_slot, &[(&key, &account0)]);
        }
        db.bank_hashes
            .write()
            .unwrap()
            .insert(unrooted_slot, BankHashInfo::default());
        assert!(db
            .accounts_index
            .get(&key, Some(&ancestors), None)
            .is_some());
        assert_load_account(&db, unrooted_slot, key, 1);

        // Purge the slot
        db.remove_unrooted_slots(&[(unrooted_slot, unrooted_bank_id)]);
        assert!(db.load_without_fixed_root(&ancestors, &key).is_none());
        assert!(db.bank_hashes.read().unwrap().get(&unrooted_slot).is_none());
        assert!(db.accounts_cache.slot_cache(unrooted_slot).is_none());
        assert!(db.storage.get_slot_stores(unrooted_slot).is_none());
        assert!(db.accounts_index.get_account_read_entry(&key).is_none());
        assert!(db
            .accounts_index
            .get(&key, Some(&ancestors), None)
            .is_none());

        // Test we can store for the same slot again and get the right information
        let account0 = AccountSharedData::new(2, 0, &key);
        db.store_for_tests(unrooted_slot, &[(&key, &account0)]);
        assert_load_account(&db, unrooted_slot, key, 2);
    }

    #[test]
    fn test_remove_unrooted_slot_cached() {
        run_test_remove_unrooted_slot(true);
    }

    #[test]
    fn test_remove_unrooted_slot_storage() {
        run_test_remove_unrooted_slot(false);
    }

    #[test]
    fn test_remove_unrooted_slot_snapshot() {
        solana_logger::setup();
        let unrooted_slot = 9;
        let unrooted_bank_id = 9;
        let db = AccountsDb::new(Vec::new(), &ClusterType::Development);
        let key = solana_sdk::pubkey::new_rand();
        let account0 = AccountSharedData::new(1, 0, &key);
        db.store_for_tests(unrooted_slot, &[(&key, &account0)]);

        // Purge the slot
        db.remove_unrooted_slots(&[(unrooted_slot, unrooted_bank_id)]);

        // Add a new root
        let key2 = solana_sdk::pubkey::new_rand();
        let new_root = unrooted_slot + 1;
        db.store_for_tests(new_root, &[(&key2, &account0)]);
        db.add_root_and_flush_write_cache(new_root);

        // Simulate reconstruction from snapshot
        let db = reconstruct_accounts_db_via_serialization(&db, new_root);

        // Check root account exists
        assert_load_account(&db, new_root, key2, 1);

        // Check purged account stays gone
        let unrooted_slot_ancestors = vec![(unrooted_slot, 1)].into_iter().collect();
        assert!(db
            .load_without_fixed_root(&unrooted_slot_ancestors, &key)
            .is_none());
    }

    fn create_account(
        accounts: &AccountsDb,
        pubkeys: &mut Vec<Pubkey>,
        slot: Slot,
        num: usize,
        space: usize,
        num_vote: usize,
    ) {
        let ancestors = vec![(slot, 0)].into_iter().collect();
        for t in 0..num {
            let pubkey = solana_sdk::pubkey::new_rand();
            let account =
                AccountSharedData::new((t + 1) as u64, space, AccountSharedData::default().owner());
            pubkeys.push(pubkey);
            assert!(accounts
                .load_without_fixed_root(&ancestors, &pubkey)
                .is_none());
            accounts.store_for_tests(slot, &[(&pubkey, &account)]);
        }
        for t in 0..num_vote {
            let pubkey = solana_sdk::pubkey::new_rand();
            let account =
                AccountSharedData::new((num + t + 1) as u64, space, &solana_vote_program::id());
            pubkeys.push(pubkey);
            let ancestors = vec![(slot, 0)].into_iter().collect();
            assert!(accounts
                .load_without_fixed_root(&ancestors, &pubkey)
                .is_none());
            accounts.store_for_tests(slot, &[(&pubkey, &account)]);
        }
    }

    fn update_accounts(accounts: &AccountsDb, pubkeys: &[Pubkey], slot: Slot, range: usize) {
        for _ in 1..1000 {
            let idx = thread_rng().gen_range(0, range);
            let ancestors = vec![(slot, 0)].into_iter().collect();
            if let Some((mut account, _)) =
                accounts.load_without_fixed_root(&ancestors, &pubkeys[idx])
            {
                account.checked_add_lamports(1).unwrap();
                accounts.store_for_tests(slot, &[(&pubkeys[idx], &account)]);
                if account.is_zero_lamport() {
                    let ancestors = vec![(slot, 0)].into_iter().collect();
                    assert!(accounts
                        .load_without_fixed_root(&ancestors, &pubkeys[idx])
                        .is_none());
                } else {
                    let default_account = AccountSharedData::from(Account {
                        lamports: account.lamports(),
                        ..Account::default()
                    });
                    assert_eq!(default_account, account);
                }
            }
        }
    }

    fn check_storage(accounts: &AccountsDb, slot: Slot, count: usize) -> bool {
        assert_eq!(
            accounts
                .storage
                .get_slot_stores(slot)
                .unwrap()
                .read()
                .unwrap()
                .len(),
            1
        );
        let slot_storages = accounts.storage.get_slot_stores(slot).unwrap();
        let mut total_count: usize = 0;
        let r_slot_storages = slot_storages.read().unwrap();
        for store in r_slot_storages.values() {
            assert_eq!(store.status(), AccountStorageStatus::Available);
            total_count += store.count();
        }
        assert_eq!(total_count, count);
        let (expected_store_count, actual_store_count): (usize, usize) = (
            r_slot_storages
                .values()
                .map(|s| s.approx_stored_count())
                .sum(),
            r_slot_storages
                .values()
                .map(|s| s.all_accounts().len())
                .sum(),
        );
        assert_eq!(expected_store_count, actual_store_count);
        total_count == count
    }

    fn check_accounts(
        accounts: &AccountsDb,
        pubkeys: &[Pubkey],
        slot: Slot,
        num: usize,
        count: usize,
    ) {
        let ancestors = vec![(slot, 0)].into_iter().collect();
        for _ in 0..num {
            let idx = thread_rng().gen_range(0, num);
            let account = accounts.load_without_fixed_root(&ancestors, &pubkeys[idx]);
            let account1 = Some((
                AccountSharedData::new(
                    (idx + count) as u64,
                    0,
                    AccountSharedData::default().owner(),
                ),
                slot,
            ));
            assert_eq!(account, account1);
        }
    }

    #[allow(clippy::needless_range_loop)]
    fn modify_accounts(
        accounts: &AccountsDb,
        pubkeys: &[Pubkey],
        slot: Slot,
        num: usize,
        count: usize,
    ) {
        for idx in 0..num {
            let account = AccountSharedData::new(
                (idx + count) as u64,
                0,
                AccountSharedData::default().owner(),
            );
            accounts.store_for_tests(slot, &[(&pubkeys[idx], &account)]);
        }
    }

    #[test]
    fn test_account_one() {
        let (_accounts_dirs, paths) = get_temp_accounts_paths(1).unwrap();
        let db = AccountsDb::new(paths, &ClusterType::Development);
        let mut pubkeys: Vec<Pubkey> = vec![];
        create_account(&db, &mut pubkeys, 0, 1, 0, 0);
        let ancestors = vec![(0, 0)].into_iter().collect();
        let account = db.load_without_fixed_root(&ancestors, &pubkeys[0]).unwrap();
        let default_account = AccountSharedData::from(Account {
            lamports: 1,
            ..Account::default()
        });
        assert_eq!((default_account, 0), account);
    }

    #[test]
    fn test_account_many() {
        let (_accounts_dirs, paths) = get_temp_accounts_paths(2).unwrap();
        let db = AccountsDb::new(paths, &ClusterType::Development);
        let mut pubkeys: Vec<Pubkey> = vec![];
        create_account(&db, &mut pubkeys, 0, 100, 0, 0);
        check_accounts(&db, &pubkeys, 0, 100, 1);
    }

    #[test]
    fn test_account_update() {
        let accounts = AccountsDb::new_single_for_tests();
        let mut pubkeys: Vec<Pubkey> = vec![];
        create_account(&accounts, &mut pubkeys, 0, 100, 0, 0);
        update_accounts(&accounts, &pubkeys, 0, 99);
        accounts.add_root_and_flush_write_cache(0);
        assert!(check_storage(&accounts, 0, 100));
    }

    #[test]
    fn test_account_grow_many() {
        let (_accounts_dir, paths) = get_temp_accounts_paths(2).unwrap();
        let size = 4096;
        let accounts = AccountsDb::new_sized(paths, size);
        let mut keys = vec![];
        for i in 0..9 {
            let key = solana_sdk::pubkey::new_rand();
            let account = AccountSharedData::new(i + 1, size as usize / 4, &key);
            accounts.store_for_tests(0, &[(&key, &account)]);
            keys.push(key);
        }
        let ancestors = vec![(0, 0)].into_iter().collect();
        for (i, key) in keys.iter().enumerate() {
            assert_eq!(
                accounts
                    .load_without_fixed_root(&ancestors, key)
                    .unwrap()
                    .0
                    .lamports(),
                (i as u64) + 1
            );
        }

        let mut append_vec_histogram = HashMap::new();
        let mut all_storages = vec![];
        for slot_storage in accounts.storage.iter() {
            all_storages.extend(slot_storage.read().unwrap().values().cloned())
        }
        for storage in all_storages {
            *append_vec_histogram.entry(storage.slot()).or_insert(0) += 1;
        }
        for count in append_vec_histogram.values() {
            assert!(*count >= 2);
        }
    }

    #[test]
    fn test_account_grow() {
        for pass in 0..27 {
            let accounts = AccountsDb::new_single_for_tests();

            let status = [AccountStorageStatus::Available, AccountStorageStatus::Full];
            let pubkey1 = solana_sdk::pubkey::new_rand();
            let account1 = AccountSharedData::new(1, DEFAULT_FILE_SIZE as usize / 2, &pubkey1);
            accounts.store_for_tests(0, &[(&pubkey1, &account1)]);
            if pass == 0 {
                accounts.add_root_and_flush_write_cache(0);
                let stores = &accounts.storage.get_slot_stores(0).unwrap();
                let r_stores = stores.read().unwrap();
                assert_eq!(r_stores.len(), 1);
                assert_eq!(r_stores[&0].count(), 1);
                assert_eq!(r_stores[&0].status(), AccountStorageStatus::Available);
                continue;
            }

            let pubkey2 = solana_sdk::pubkey::new_rand();
            let account2 = AccountSharedData::new(1, DEFAULT_FILE_SIZE as usize / 2, &pubkey2);
            accounts.store_for_tests(0, &[(&pubkey2, &account2)]);

            if pass == 1 {
                accounts.add_root_and_flush_write_cache(0);
                assert_eq!(accounts.storage.len(), 1);
                let stores = &accounts.storage.get_slot_stores(0).unwrap();
                let r_stores = stores.read().unwrap();
                assert_eq!(r_stores.len(), 1);
                assert_eq!(r_stores[&0].count(), 2);
                assert_eq!(r_stores[&0].status(), AccountStorageStatus::Available);
                continue;
            }
            let ancestors = vec![(0, 0)].into_iter().collect();
            assert_eq!(
                accounts
                    .load_without_fixed_root(&ancestors, &pubkey1)
                    .unwrap()
                    .0,
                account1
            );
            assert_eq!(
                accounts
                    .load_without_fixed_root(&ancestors, &pubkey2)
                    .unwrap()
                    .0,
                account2
            );

            // lots of writes, but they are all duplicates
            for i in 0..25 {
                accounts.store_for_tests(0, &[(&pubkey1, &account1)]);
                let flush = pass == i + 2;
                if flush {
                    accounts.add_root_and_flush_write_cache(0);
                    assert_eq!(accounts.storage.len(), 1);
                    let stores = &accounts.storage.get_slot_stores(0).unwrap();
                    let r_stores = stores.read().unwrap();
                    assert_eq!(r_stores.len(), 1);
                    assert_eq!(r_stores[&0].status(), status[0]);
                }
                let ancestors = vec![(0, 0)].into_iter().collect();
                assert_eq!(
                    accounts
                        .load_without_fixed_root(&ancestors, &pubkey1)
                        .unwrap()
                        .0,
                    account1
                );
                assert_eq!(
                    accounts
                        .load_without_fixed_root(&ancestors, &pubkey2)
                        .unwrap()
                        .0,
                    account2
                );
                if flush {
                    break;
                }
            }
        }
    }

    #[test]
    fn test_lazy_gc_slot() {
        solana_logger::setup();
        //This test is pedantic
        //A slot is purged when a non root bank is cleaned up.  If a slot is behind root but it is
        //not root, it means we are retaining dead banks.
        let accounts = AccountsDb::new(Vec::new(), &ClusterType::Development);
        let pubkey = solana_sdk::pubkey::new_rand();
        let account = AccountSharedData::new(1, 0, AccountSharedData::default().owner());
        //store an account
        accounts.store_for_tests(0, &[(&pubkey, &account)]);
        accounts.add_root_and_flush_write_cache(0);

        let ancestors = vec![(0, 0)].into_iter().collect();
        let id = {
            let (lock, idx) = accounts
                .accounts_index
                .get_for_tests(&pubkey, Some(&ancestors), None)
                .unwrap();
            lock.slot_list()[idx].1.store_id()
        };
        accounts.get_accounts_delta_hash(0);

        //slot is still there, since gc is lazy
        assert!(accounts
            .storage
            .get_slot_stores(0)
            .unwrap()
            .read()
            .unwrap()
            .get(&id)
            .is_some());

        //store causes clean
        accounts.store_for_tests(1, &[(&pubkey, &account)]);

        // generate delta state for slot 1, so clean operates on it.
        accounts.get_accounts_delta_hash(1);

        //slot is gone
        accounts.print_accounts_stats("pre-clean");
        accounts.add_root_and_flush_write_cache(1);
        assert!(accounts.storage.get_slot_stores(0).is_some());
        accounts.clean_accounts_for_tests();
        assert!(accounts.storage.get_slot_stores(0).is_none());

        //new value is there
        let ancestors = vec![(1, 1)].into_iter().collect();
        assert_eq!(
            accounts.load_without_fixed_root(&ancestors, &pubkey),
            Some((account, 1))
        );
    }

    impl AccountsDb {
        fn all_account_count_in_append_vec(&self, slot: Slot) -> usize {
            let slot_storage = self.storage.get_slot_stores(slot);
            if let Some(slot_storage) = slot_storage {
                let r_slot_storage = slot_storage.read().unwrap();
                let count = r_slot_storage
                    .values()
                    .map(|store| store.all_accounts().len())
                    .sum();
                let stored_count: usize = r_slot_storage
                    .values()
                    .map(|store| store.approx_stored_count())
                    .sum();
                assert_eq!(stored_count, count);
                count
            } else {
                0
            }
        }

        pub fn ref_count_for_pubkey(&self, pubkey: &Pubkey) -> RefCount {
            self.accounts_index.ref_count_from_storage(pubkey)
        }
    }

    #[test]
    fn test_clean_zero_lamport_and_dead_slot() {
        solana_logger::setup();

        let accounts = AccountsDb::new(Vec::new(), &ClusterType::Development);
        let pubkey1 = solana_sdk::pubkey::new_rand();
        let pubkey2 = solana_sdk::pubkey::new_rand();
        let account = AccountSharedData::new(1, 1, AccountSharedData::default().owner());
        let zero_lamport_account =
            AccountSharedData::new(0, 0, AccountSharedData::default().owner());

        // Store two accounts
        accounts.store_for_tests(0, &[(&pubkey1, &account)]);
        accounts.store_for_tests(0, &[(&pubkey2, &account)]);

        // Make sure both accounts are in the same AppendVec in slot 0, which
        // will prevent pubkey1 from being cleaned up later even when it's a
        // zero-lamport account
        let ancestors = vec![(0, 1)].into_iter().collect();
        let (slot1, account_info1) = accounts
            .accounts_index
            .get_for_tests(&pubkey1, Some(&ancestors), None)
            .map(|(account_list1, index1)| account_list1.slot_list()[index1])
            .unwrap();
        let (slot2, account_info2) = accounts
            .accounts_index
            .get_for_tests(&pubkey2, Some(&ancestors), None)
            .map(|(account_list2, index2)| account_list2.slot_list()[index2])
            .unwrap();
        assert_eq!(slot1, 0);
        assert_eq!(slot1, slot2);
        assert_eq!(account_info1.storage_location(), StorageLocation::Cached);
        assert_eq!(
            account_info1.storage_location(),
            account_info2.storage_location()
        );

        // Update account 1 in slot 1
        accounts.store_for_tests(1, &[(&pubkey1, &account)]);

        // Update account 1 as  zero lamports account
        accounts.store_for_tests(2, &[(&pubkey1, &zero_lamport_account)]);

        // Pubkey 1 was the only account in slot 1, and it was updated in slot 2, so
        // slot 1 should be purged
        accounts.get_accounts_delta_hash(0);
        accounts.add_root_and_flush_write_cache(0);
        accounts.get_accounts_delta_hash(1);
        accounts.add_root_and_flush_write_cache(1);
        accounts.get_accounts_delta_hash(2);
        accounts.add_root_and_flush_write_cache(2);

        // Slot 1 should be removed, slot 0 cannot be removed because it still has
        // the latest update for pubkey 2
        accounts.clean_accounts_for_tests();
        assert!(accounts.storage.get_slot_stores(0).is_some());
        assert!(accounts.storage.get_slot_stores(1).is_none());

        // Slot 1 should be cleaned because all it's accounts are
        // zero lamports, and are not present in any other slot's
        // storage entries
        assert_eq!(accounts.alive_account_count_in_slot(1), 0);
    }

    #[test]
    fn test_clean_multiple_zero_lamport_decrements_index_ref_count() {
        solana_logger::setup();

        let accounts = AccountsDb::new(Vec::new(), &ClusterType::Development);
        let pubkey1 = solana_sdk::pubkey::new_rand();
        let pubkey2 = solana_sdk::pubkey::new_rand();
        let zero_lamport_account =
            AccountSharedData::new(0, 0, AccountSharedData::default().owner());

        // Store 2 accounts in slot 0, then update account 1 in two more slots
        accounts.store_for_tests(0, &[(&pubkey1, &zero_lamport_account)]);
        accounts.store_for_tests(0, &[(&pubkey2, &zero_lamport_account)]);
        accounts.store_for_tests(1, &[(&pubkey1, &zero_lamport_account)]);
        accounts.store_for_tests(2, &[(&pubkey1, &zero_lamport_account)]);
        // Root all slots
        accounts.get_accounts_delta_hash(0);
        accounts.add_root_and_flush_write_cache(0);
        accounts.get_accounts_delta_hash(1);
        accounts.add_root_and_flush_write_cache(1);
        accounts.get_accounts_delta_hash(2);
        accounts.add_root_and_flush_write_cache(2);

        // Account ref counts should match how many slots they were stored in
        // Account 1 = 3 slots; account 2 = 1 slot
        assert_eq!(accounts.accounts_index.ref_count_from_storage(&pubkey1), 3);
        assert_eq!(accounts.accounts_index.ref_count_from_storage(&pubkey2), 1);

        accounts.clean_accounts_for_tests();
        // Slots 0 and 1 should each have been cleaned because all of their
        // accounts are zero lamports
        assert!(accounts.storage.get_slot_stores(0).is_none());
        assert!(accounts.storage.get_slot_stores(1).is_none());
        // Slot 2 only has a zero lamport account as well. But, calc_delete_dependencies()
        // should exclude slot 2 from the clean due to changes in other slots
        assert!(accounts.storage.get_slot_stores(2).is_some());
        // Index ref counts should be consistent with the slot stores. Account 1 ref count
        // should be 1 since slot 2 is the only alive slot; account 2 should have a ref
        // count of 0 due to slot 0 being dead
        assert_eq!(accounts.accounts_index.ref_count_from_storage(&pubkey1), 1);
        assert_eq!(accounts.accounts_index.ref_count_from_storage(&pubkey2), 0);

        accounts.clean_accounts_for_tests();
        // Slot 2 will now be cleaned, which will leave account 1 with a ref count of 0
        assert!(accounts.storage.get_slot_stores(2).is_none());
        assert_eq!(accounts.accounts_index.ref_count_from_storage(&pubkey1), 0);
    }

    #[test]
    fn test_clean_zero_lamport_and_old_roots() {
        solana_logger::setup();

        let accounts = AccountsDb::new(Vec::new(), &ClusterType::Development);
        let pubkey = solana_sdk::pubkey::new_rand();
        let account = AccountSharedData::new(1, 0, AccountSharedData::default().owner());
        let zero_lamport_account =
            AccountSharedData::new(0, 0, AccountSharedData::default().owner());

        // Store a zero-lamport account
        accounts.store_for_tests(0, &[(&pubkey, &account)]);
        accounts.store_for_tests(1, &[(&pubkey, &zero_lamport_account)]);

        // Simulate rooting the zero-lamport account, should be a
        // candidate for cleaning
        accounts.get_accounts_delta_hash(0);
        accounts.add_root_and_flush_write_cache(0);
        accounts.get_accounts_delta_hash(1);
        accounts.add_root_and_flush_write_cache(1);

        // Slot 0 should be removed, and
        // zero-lamport account should be cleaned
        accounts.clean_accounts_for_tests();

        assert!(accounts.storage.get_slot_stores(0).is_none());
        assert!(accounts.storage.get_slot_stores(1).is_none());

        // Slot 0 should be cleaned because all it's accounts have been
        // updated in the rooted slot 1
        assert_eq!(accounts.alive_account_count_in_slot(0), 0);

        // Slot 1 should be cleaned because all it's accounts are
        // zero lamports, and are not present in any other slot's
        // storage entries
        assert_eq!(accounts.alive_account_count_in_slot(1), 0);

        // zero lamport account, should no longer exist in accounts index
        // because it has been removed
        assert!(accounts
            .accounts_index
            .get_for_tests(&pubkey, None, None)
            .is_none());
    }

    #[test]
    fn test_clean_old_with_normal_account() {
        solana_logger::setup();

        let accounts = AccountsDb::new(Vec::new(), &ClusterType::Development);
        let pubkey = solana_sdk::pubkey::new_rand();
        let account = AccountSharedData::new(1, 0, AccountSharedData::default().owner());
        //store an account
        accounts.store_for_tests(0, &[(&pubkey, &account)]);
        accounts.store_for_tests(1, &[(&pubkey, &account)]);

        // simulate slots are rooted after while
        accounts.get_accounts_delta_hash(0);
        accounts.add_root_and_flush_write_cache(0);
        accounts.get_accounts_delta_hash(1);
        accounts.add_root_and_flush_write_cache(1);

        //even if rooted, old state isn't cleaned up
        assert_eq!(accounts.alive_account_count_in_slot(0), 1);
        assert_eq!(accounts.alive_account_count_in_slot(1), 1);

        accounts.clean_accounts_for_tests();

        //now old state is cleaned up
        assert_eq!(accounts.alive_account_count_in_slot(0), 0);
        assert_eq!(accounts.alive_account_count_in_slot(1), 1);
    }

    #[test]
    fn test_clean_old_with_zero_lamport_account() {
        solana_logger::setup();

        let accounts = AccountsDb::new(Vec::new(), &ClusterType::Development);
        let pubkey1 = solana_sdk::pubkey::new_rand();
        let pubkey2 = solana_sdk::pubkey::new_rand();
        let normal_account = AccountSharedData::new(1, 0, AccountSharedData::default().owner());
        let zero_account = AccountSharedData::new(0, 0, AccountSharedData::default().owner());
        //store an account
        accounts.store_for_tests(0, &[(&pubkey1, &normal_account)]);
        accounts.store_for_tests(1, &[(&pubkey1, &zero_account)]);
        accounts.store_for_tests(0, &[(&pubkey2, &normal_account)]);
        accounts.store_for_tests(1, &[(&pubkey2, &normal_account)]);

        //simulate slots are rooted after while
        accounts.get_accounts_delta_hash(0);
        accounts.add_root_and_flush_write_cache(0);
        accounts.get_accounts_delta_hash(1);
        accounts.add_root_and_flush_write_cache(1);

        //even if rooted, old state isn't cleaned up
        assert_eq!(accounts.alive_account_count_in_slot(0), 2);
        assert_eq!(accounts.alive_account_count_in_slot(1), 2);

        accounts.print_accounts_stats("");

        accounts.clean_accounts_for_tests();

        //Old state behind zero-lamport account is cleaned up
        assert_eq!(accounts.alive_account_count_in_slot(0), 0);
        assert_eq!(accounts.alive_account_count_in_slot(1), 2);
    }

    #[test]
    fn test_clean_old_with_both_normal_and_zero_lamport_accounts() {
        solana_logger::setup();

        let mut accounts = AccountsDb::new_with_config_for_tests(
            Vec::new(),
            &ClusterType::Development,
            spl_token_mint_index_enabled(),
            AccountShrinkThreshold::default(),
        );
        let pubkey1 = solana_sdk::pubkey::new_rand();
        let pubkey2 = solana_sdk::pubkey::new_rand();

        // Set up account to be added to secondary index
        let mint_key = Pubkey::new_unique();
        let mut account_data_with_mint = vec![0; inline_spl_token::Account::get_packed_len()];
        account_data_with_mint[..PUBKEY_BYTES].clone_from_slice(&(mint_key.to_bytes()));

        let mut normal_account = AccountSharedData::new(1, 0, AccountSharedData::default().owner());
        normal_account.set_owner(inline_spl_token::id());
        normal_account.set_data(account_data_with_mint.clone());
        let mut zero_account = AccountSharedData::new(0, 0, AccountSharedData::default().owner());
        zero_account.set_owner(inline_spl_token::id());
        zero_account.set_data(account_data_with_mint);

        //store an account
        accounts.store_for_tests(0, &[(&pubkey1, &normal_account)]);
        accounts.store_for_tests(0, &[(&pubkey1, &normal_account)]);
        accounts.store_for_tests(1, &[(&pubkey1, &zero_account)]);
        accounts.store_for_tests(0, &[(&pubkey2, &normal_account)]);
        accounts.store_for_tests(2, &[(&pubkey2, &normal_account)]);

        //simulate slots are rooted after while
        accounts.get_accounts_delta_hash(0);
        accounts.add_root_and_flush_write_cache(0);
        accounts.get_accounts_delta_hash(1);
        accounts.add_root_and_flush_write_cache(1);
        accounts.get_accounts_delta_hash(2);
        accounts.add_root_and_flush_write_cache(2);

        //even if rooted, old state isn't cleaned up
        assert_eq!(accounts.alive_account_count_in_slot(0), 2);
        assert_eq!(accounts.alive_account_count_in_slot(1), 1);
        assert_eq!(accounts.alive_account_count_in_slot(2), 1);

        // Secondary index should still find both pubkeys
        let mut found_accounts = HashSet::new();
        let index_key = IndexKey::SplTokenMint(mint_key);
        let bank_id = 0;
        accounts
            .accounts_index
            .index_scan_accounts(
                &Ancestors::default(),
                bank_id,
                index_key,
                |key, _| {
                    found_accounts.insert(*key);
                },
                &ScanConfig::default(),
            )
            .unwrap();
        assert_eq!(found_accounts.len(), 2);
        assert!(found_accounts.contains(&pubkey1));
        assert!(found_accounts.contains(&pubkey2));

        {
            accounts.account_indexes.keys = Some(AccountSecondaryIndexesIncludeExclude {
                exclude: true,
                keys: [mint_key].iter().cloned().collect::<HashSet<Pubkey>>(),
            });
            // Secondary index can't be used - do normal scan: should still find both pubkeys
            let mut found_accounts = HashSet::new();
            let used_index = accounts
                .index_scan_accounts(
                    &Ancestors::default(),
                    bank_id,
                    index_key,
                    |account| {
                        found_accounts.insert(*account.unwrap().0);
                    },
                    &ScanConfig::default(),
                )
                .unwrap();
            assert!(!used_index);
            assert_eq!(found_accounts.len(), 2);
            assert!(found_accounts.contains(&pubkey1));
            assert!(found_accounts.contains(&pubkey2));

            accounts.account_indexes.keys = None;

            // Secondary index can now be used since it isn't marked as excluded
            let mut found_accounts = HashSet::new();
            let used_index = accounts
                .index_scan_accounts(
                    &Ancestors::default(),
                    bank_id,
                    index_key,
                    |account| {
                        found_accounts.insert(*account.unwrap().0);
                    },
                    &ScanConfig::default(),
                )
                .unwrap();
            assert!(used_index);
            assert_eq!(found_accounts.len(), 2);
            assert!(found_accounts.contains(&pubkey1));
            assert!(found_accounts.contains(&pubkey2));

            accounts.account_indexes.keys = None;
        }

        accounts.clean_accounts_for_tests();

        //both zero lamport and normal accounts are cleaned up
        assert_eq!(accounts.alive_account_count_in_slot(0), 0);
        // The only store to slot 1 was a zero lamport account, should
        // be purged by zero-lamport cleaning logic because slot 1 is
        // rooted
        assert_eq!(accounts.alive_account_count_in_slot(1), 0);
        assert_eq!(accounts.alive_account_count_in_slot(2), 1);

        // `pubkey1`, a zero lamport account, should no longer exist in accounts index
        // because it has been removed by the clean
        assert!(accounts
            .accounts_index
            .get_for_tests(&pubkey1, None, None)
            .is_none());

        // Secondary index should have purged `pubkey1` as well
        let mut found_accounts = vec![];
        accounts
            .accounts_index
            .index_scan_accounts(
                &Ancestors::default(),
                bank_id,
                IndexKey::SplTokenMint(mint_key),
                |key, _| found_accounts.push(*key),
                &ScanConfig::default(),
            )
            .unwrap();
        assert_eq!(found_accounts, vec![pubkey2]);
    }

    #[test]
    fn test_clean_max_slot_zero_lamport_account() {
        solana_logger::setup();

        let accounts = AccountsDb::new(Vec::new(), &ClusterType::Development);
        let pubkey = solana_sdk::pubkey::new_rand();
        let account = AccountSharedData::new(1, 0, AccountSharedData::default().owner());
        let zero_account = AccountSharedData::new(0, 0, AccountSharedData::default().owner());

        // store an account, make it a zero lamport account
        // in slot 1
        accounts.store_for_tests(0, &[(&pubkey, &account)]);
        accounts.store_for_tests(1, &[(&pubkey, &zero_account)]);

        // simulate slots are rooted after while
        accounts.get_accounts_delta_hash(0);
        accounts.add_root_and_flush_write_cache(0);
        accounts.get_accounts_delta_hash(1);
        accounts.add_root_and_flush_write_cache(1);

        // Only clean up to account 0, should not purge slot 0 based on
        // updates in later slots in slot 1
        assert_eq!(accounts.alive_account_count_in_slot(0), 1);
        assert_eq!(accounts.alive_account_count_in_slot(1), 1);
        accounts.clean_accounts(Some(0), false, None);
        assert_eq!(accounts.alive_account_count_in_slot(0), 1);
        assert_eq!(accounts.alive_account_count_in_slot(1), 1);
        assert!(accounts
            .accounts_index
            .get_for_tests(&pubkey, None, None)
            .is_some());

        // Now the account can be cleaned up
        accounts.clean_accounts(Some(1), false, None);
        assert_eq!(accounts.alive_account_count_in_slot(0), 0);
        assert_eq!(accounts.alive_account_count_in_slot(1), 0);

        // The zero lamport account, should no longer exist in accounts index
        // because it has been removed
        assert!(accounts
            .accounts_index
            .get_for_tests(&pubkey, None, None)
            .is_none());
    }

    #[test]
    fn test_uncleaned_roots_with_account() {
        solana_logger::setup();

        let accounts = AccountsDb::new(Vec::new(), &ClusterType::Development);
        let pubkey = solana_sdk::pubkey::new_rand();
        let account = AccountSharedData::new(1, 0, AccountSharedData::default().owner());
        //store an account
        accounts.store_for_tests(0, &[(&pubkey, &account)]);
        assert_eq!(accounts.accounts_index.uncleaned_roots_len(), 0);

        // simulate slots are rooted after while
        accounts.add_root_and_flush_write_cache(0);
        assert_eq!(accounts.accounts_index.uncleaned_roots_len(), 1);

        //now uncleaned roots are cleaned up
        accounts.clean_accounts_for_tests();
        assert_eq!(accounts.accounts_index.uncleaned_roots_len(), 0);
    }

    #[test]
    fn test_uncleaned_roots_with_no_account() {
        solana_logger::setup();

        let accounts = AccountsDb::new(Vec::new(), &ClusterType::Development);

        assert_eq!(accounts.accounts_index.uncleaned_roots_len(), 0);

        // simulate slots are rooted after while
        accounts.add_root_and_flush_write_cache(0);
        assert_eq!(accounts.accounts_index.uncleaned_roots_len(), 1);

        //now uncleaned roots are cleaned up
        accounts.clean_accounts_for_tests();
        assert_eq!(accounts.accounts_index.uncleaned_roots_len(), 0);
    }

    #[test]
    fn test_accounts_db_serialize1() {
        for pass in 0..2 {
            solana_logger::setup();
            let accounts = AccountsDb::new_single_for_tests();
            let mut pubkeys: Vec<Pubkey> = vec![];

            // Create 100 accounts in slot 0
            create_account(&accounts, &mut pubkeys, 0, 100, 0, 0);
            if pass == 0 {
                accounts.add_root_and_flush_write_cache(0);
                assert!(check_storage(&accounts, 0, 100));
                accounts.clean_accounts_for_tests();
                check_accounts(&accounts, &pubkeys, 0, 100, 1);
                // clean should have done nothing
                continue;
            }

            // do some updates to those accounts and re-check
            modify_accounts(&accounts, &pubkeys, 0, 100, 2);
            accounts.add_root_and_flush_write_cache(0);
            assert!(check_storage(&accounts, 0, 100));
            check_accounts(&accounts, &pubkeys, 0, 100, 2);
            accounts.get_accounts_delta_hash(0);

            let mut pubkeys1: Vec<Pubkey> = vec![];

            // CREATE SLOT 1
            let latest_slot = 1;

            // Modify the first 10 of the accounts from slot 0 in slot 1
            modify_accounts(&accounts, &pubkeys, latest_slot, 10, 3);
            // Overwrite account 30 from slot 0 with lamports=0 into slot 1.
            // Slot 1 should now have 10 + 1 = 11 accounts
            let account = AccountSharedData::new(0, 0, AccountSharedData::default().owner());
            accounts.store_for_tests(latest_slot, &[(&pubkeys[30], &account)]);

            // Create 10 new accounts in slot 1, should now have 11 + 10 = 21
            // accounts
            create_account(&accounts, &mut pubkeys1, latest_slot, 10, 0, 0);

            accounts.get_accounts_delta_hash(latest_slot);
            accounts.add_root_and_flush_write_cache(latest_slot);
            assert!(check_storage(&accounts, 1, 21));

            // CREATE SLOT 2
            let latest_slot = 2;
            let mut pubkeys2: Vec<Pubkey> = vec![];

            // Modify first 20 of the accounts from slot 0 in slot 2
            modify_accounts(&accounts, &pubkeys, latest_slot, 20, 4);
            accounts.clean_accounts_for_tests();
            // Overwrite account 31 from slot 0 with lamports=0 into slot 2.
            // Slot 2 should now have 20 + 1 = 21 accounts
            let account = AccountSharedData::new(0, 0, AccountSharedData::default().owner());
            accounts.store_for_tests(latest_slot, &[(&pubkeys[31], &account)]);

            // Create 10 new accounts in slot 2. Slot 2 should now have
            // 21 + 10 = 31 accounts
            create_account(&accounts, &mut pubkeys2, latest_slot, 10, 0, 0);

            accounts.get_accounts_delta_hash(latest_slot);
            accounts.add_root_and_flush_write_cache(latest_slot);
            assert!(check_storage(&accounts, 2, 31));

            accounts.clean_accounts_for_tests();
            // The first 20 accounts of slot 0 have been updated in slot 2, as well as
            // accounts 30 and  31 (overwritten with zero-lamport accounts in slot 1 and
            // slot 2 respectively), so only 78 accounts are left in slot 0's storage entries.
            assert!(check_storage(&accounts, 0, 78));
            // 10 of the 21 accounts have been modified in slot 2, so only 11
            // accounts left in slot 1.
            assert!(check_storage(&accounts, 1, 11));
            assert!(check_storage(&accounts, 2, 31));

            let daccounts = reconstruct_accounts_db_via_serialization(&accounts, latest_slot);

            assert_eq!(
                daccounts.write_version.load(Ordering::Acquire),
                accounts.write_version.load(Ordering::Acquire)
            );

            // Get the hash for the latest slot, which should be the only hash in the
            // bank_hashes map on the deserialized AccountsDb
            assert_eq!(daccounts.bank_hashes.read().unwrap().len(), 2);
            assert_eq!(
                daccounts.bank_hashes.read().unwrap().get(&latest_slot),
                accounts.bank_hashes.read().unwrap().get(&latest_slot)
            );

            daccounts.print_count_and_status("daccounts");

            // Don't check the first 35 accounts which have not been modified on slot 0
            check_accounts(&daccounts, &pubkeys[35..], 0, 65, 37);
            check_accounts(&daccounts, &pubkeys1, 1, 10, 1);
            assert!(check_storage(&daccounts, 0, 100));
            assert!(check_storage(&daccounts, 1, 21));
            assert!(check_storage(&daccounts, 2, 31));

            let ancestors = linear_ancestors(latest_slot);
            assert_eq!(
                daccounts.update_accounts_hash_for_tests(latest_slot, &ancestors, false, false,),
                accounts.update_accounts_hash_for_tests(latest_slot, &ancestors, false, false,)
            );
        }
    }

    fn assert_load_account(
        accounts: &AccountsDb,
        slot: Slot,
        pubkey: Pubkey,
        expected_lamports: u64,
    ) {
        let ancestors = vec![(slot, 0)].into_iter().collect();
        let (account, slot) = accounts
            .load_without_fixed_root(&ancestors, &pubkey)
            .unwrap();
        assert_eq!((account.lamports(), slot), (expected_lamports, slot));
    }

    fn assert_not_load_account(accounts: &AccountsDb, slot: Slot, pubkey: Pubkey) {
        let ancestors = vec![(slot, 0)].into_iter().collect();
        let load = accounts.load_without_fixed_root(&ancestors, &pubkey);
        assert!(load.is_none(), "{load:?}");
    }

    fn reconstruct_accounts_db_via_serialization(accounts: &AccountsDb, slot: Slot) -> AccountsDb {
        let daccounts =
            crate::serde_snapshot::reconstruct_accounts_db_via_serialization(accounts, slot);
        daccounts.print_count_and_status("daccounts");
        daccounts
    }

    fn assert_no_stores(accounts: &AccountsDb, slot: Slot) {
        let slot_stores = accounts.storage.get_slot_stores(slot);
        let r_slot_stores = slot_stores.as_ref().map(|slot_stores| {
            let r_slot_stores = slot_stores.read().unwrap();
            info!("{:?}", *r_slot_stores);
            r_slot_stores
        });
        assert!(r_slot_stores.is_none() || r_slot_stores.unwrap().is_empty());
    }

    #[test]
    fn test_accounts_db_purge_keep_live() {
        solana_logger::setup();
        let some_lamport = 223;
        let zero_lamport = 0;
        let no_data = 0;
        let owner = *AccountSharedData::default().owner();

        let account = AccountSharedData::new(some_lamport, no_data, &owner);
        let pubkey = solana_sdk::pubkey::new_rand();

        let account2 = AccountSharedData::new(some_lamport, no_data, &owner);
        let pubkey2 = solana_sdk::pubkey::new_rand();

        let zero_lamport_account = AccountSharedData::new(zero_lamport, no_data, &owner);

        let accounts = AccountsDb::new_single_for_tests();
        accounts.get_accounts_delta_hash(0);
        accounts.add_root_and_flush_write_cache(0);

        // Step A
        let mut current_slot = 1;
        accounts.store_for_tests(current_slot, &[(&pubkey, &account)]);
        // Store another live account to slot 1 which will prevent any purge
        // since the store count will not be zero
        accounts.store_for_tests(current_slot, &[(&pubkey2, &account2)]);
        accounts.get_accounts_delta_hash(current_slot);
        accounts.add_root_and_flush_write_cache(current_slot);
        let (slot1, account_info1) = accounts
            .accounts_index
            .get_for_tests(&pubkey, None, None)
            .map(|(account_list1, index1)| account_list1.slot_list()[index1])
            .unwrap();
        let (slot2, account_info2) = accounts
            .accounts_index
            .get_for_tests(&pubkey2, None, None)
            .map(|(account_list2, index2)| account_list2.slot_list()[index2])
            .unwrap();
        assert_eq!(slot1, current_slot);
        assert_eq!(slot1, slot2);
        assert_eq!(account_info1.store_id(), account_info2.store_id());

        // Step B
        current_slot += 1;
        let zero_lamport_slot = current_slot;
        accounts.store_for_tests(current_slot, &[(&pubkey, &zero_lamport_account)]);
        accounts.get_accounts_delta_hash(current_slot);
        accounts.add_root_and_flush_write_cache(current_slot);

        assert_load_account(&accounts, current_slot, pubkey, zero_lamport);

        current_slot += 1;
        accounts.get_accounts_delta_hash(current_slot);
        accounts.add_root_and_flush_write_cache(current_slot);

        accounts.print_accounts_stats("pre_purge");

        accounts.clean_accounts_for_tests();

        accounts.print_accounts_stats("post_purge");

        // The earlier entry for pubkey in the account index is purged,
        let (slot_list_len, index_slot) = {
            let account_entry = accounts
                .accounts_index
                .get_account_read_entry(&pubkey)
                .unwrap();
            let slot_list = account_entry.slot_list();
            (slot_list.len(), slot_list[0].0)
        };
        assert_eq!(slot_list_len, 1);
        // Zero lamport entry was not the one purged
        assert_eq!(index_slot, zero_lamport_slot);
        // The ref count should still be 2 because no slots were purged
        assert_eq!(accounts.ref_count_for_pubkey(&pubkey), 2);

        // storage for slot 1 had 2 accounts, now has 1 after pubkey 1
        // was reclaimed
        check_storage(&accounts, 1, 1);
        // storage for slot 2 had 1 accounts, now has 1
        check_storage(&accounts, 2, 1);
    }

    #[test]
    fn test_accounts_db_purge1() {
        solana_logger::setup();
        let some_lamport = 223;
        let zero_lamport = 0;
        let no_data = 0;
        let owner = *AccountSharedData::default().owner();

        let account = AccountSharedData::new(some_lamport, no_data, &owner);
        let pubkey = solana_sdk::pubkey::new_rand();

        let zero_lamport_account = AccountSharedData::new(zero_lamport, no_data, &owner);

        let accounts = AccountsDb::new_single_for_tests();
        accounts.add_root(0);

        let mut current_slot = 1;
        accounts.insert_default_bank_hash(current_slot, current_slot - 1);
        accounts.store_for_tests(current_slot, &[(&pubkey, &account)]);
        accounts.get_accounts_delta_hash(current_slot);
        accounts.add_root_and_flush_write_cache(current_slot);

        current_slot += 1;
        accounts.insert_default_bank_hash(current_slot, current_slot - 1);
        accounts.store_for_tests(current_slot, &[(&pubkey, &zero_lamport_account)]);
        accounts.get_accounts_delta_hash(current_slot);
        accounts.add_root_and_flush_write_cache(current_slot);

        assert_load_account(&accounts, current_slot, pubkey, zero_lamport);

        // Otherwise slot 2 will not be removed
        current_slot += 1;
        accounts.insert_default_bank_hash(current_slot, current_slot - 1);
        accounts.get_accounts_delta_hash(current_slot);
        accounts.add_root_and_flush_write_cache(current_slot);

        accounts.print_accounts_stats("pre_purge");

        let ancestors = linear_ancestors(current_slot);
        info!("ancestors: {:?}", ancestors);
        let hash = accounts.update_accounts_hash_for_tests(current_slot, &ancestors, true, true);

        accounts.clean_accounts_for_tests();

        assert_eq!(
            accounts.update_accounts_hash_for_tests(current_slot, &ancestors, true, true),
            hash
        );

        accounts.print_accounts_stats("post_purge");

        // Make sure the index is for pubkey cleared
        assert!(accounts
            .accounts_index
            .get_account_read_entry(&pubkey)
            .is_none());

        // slot 1 & 2 should not have any stores
        assert_no_stores(&accounts, 1);
        assert_no_stores(&accounts, 2);
    }

    #[test]
    fn test_accounts_db_serialize_zero_and_free() {
        solana_logger::setup();

        let some_lamport = 223;
        let zero_lamport = 0;
        let no_data = 0;
        let owner = *AccountSharedData::default().owner();

        let account = AccountSharedData::new(some_lamport, no_data, &owner);
        let pubkey = solana_sdk::pubkey::new_rand();
        let zero_lamport_account = AccountSharedData::new(zero_lamport, no_data, &owner);

        let account2 = AccountSharedData::new(some_lamport + 1, no_data, &owner);
        let pubkey2 = solana_sdk::pubkey::new_rand();

        let filler_account = AccountSharedData::new(some_lamport, no_data, &owner);
        let filler_account_pubkey = solana_sdk::pubkey::new_rand();

        let accounts = AccountsDb::new_single_for_tests();

        let mut current_slot = 1;
        accounts.store_for_tests(current_slot, &[(&pubkey, &account)]);
        accounts.add_root(current_slot);

        current_slot += 1;
        accounts.store_for_tests(current_slot, &[(&pubkey, &zero_lamport_account)]);
        accounts.store_for_tests(current_slot, &[(&pubkey2, &account2)]);

        // Store the account a few times.
        // use to be: store enough accounts such that an additional store for slot 2 is created.
        // but we use the write cache now
        for _ in 0..3 {
            accounts.store_for_tests(current_slot, &[(&filler_account_pubkey, &filler_account)]);
        }
        accounts.add_root_and_flush_write_cache(current_slot);

        assert_load_account(&accounts, current_slot, pubkey, zero_lamport);

        accounts.print_accounts_stats("accounts");

        accounts.clean_accounts_for_tests();

        accounts.print_accounts_stats("accounts_post_purge");
        let accounts = reconstruct_accounts_db_via_serialization(&accounts, current_slot);

        accounts.print_accounts_stats("reconstructed");

        assert_load_account(&accounts, current_slot, pubkey, zero_lamport);
    }

    fn with_chained_zero_lamport_accounts<F>(f: F)
    where
        F: Fn(AccountsDb, Slot) -> AccountsDb,
    {
        let some_lamport = 223;
        let zero_lamport = 0;
        let dummy_lamport = 999;
        let no_data = 0;
        let owner = *AccountSharedData::default().owner();

        let account = AccountSharedData::new(some_lamport, no_data, &owner);
        let account2 = AccountSharedData::new(some_lamport + 100_001, no_data, &owner);
        let account3 = AccountSharedData::new(some_lamport + 100_002, no_data, &owner);
        let zero_lamport_account = AccountSharedData::new(zero_lamport, no_data, &owner);

        let pubkey = solana_sdk::pubkey::new_rand();
        let purged_pubkey1 = solana_sdk::pubkey::new_rand();
        let purged_pubkey2 = solana_sdk::pubkey::new_rand();

        let dummy_account = AccountSharedData::new(dummy_lamport, no_data, &owner);
        let dummy_pubkey = Pubkey::default();

        let accounts = AccountsDb::new_single_for_tests();

        let mut current_slot = 1;
        accounts.store_for_tests(current_slot, &[(&pubkey, &account)]);
        accounts.store_for_tests(current_slot, &[(&purged_pubkey1, &account2)]);
        accounts.add_root_and_flush_write_cache(current_slot);

        current_slot += 1;
        accounts.store_for_tests(current_slot, &[(&purged_pubkey1, &zero_lamport_account)]);
        accounts.store_for_tests(current_slot, &[(&purged_pubkey2, &account3)]);
        accounts.add_root_and_flush_write_cache(current_slot);

        current_slot += 1;
        accounts.store_for_tests(current_slot, &[(&purged_pubkey2, &zero_lamport_account)]);
        accounts.add_root_and_flush_write_cache(current_slot);

        current_slot += 1;
        accounts.store_for_tests(current_slot, &[(&dummy_pubkey, &dummy_account)]);
        accounts.add_root_and_flush_write_cache(current_slot);

        accounts.print_accounts_stats("pre_f");
        accounts.update_accounts_hash_for_tests(4, &Ancestors::default(), false, false);

        let accounts = f(accounts, current_slot);

        accounts.print_accounts_stats("post_f");

        assert_load_account(&accounts, current_slot, pubkey, some_lamport);
        assert_load_account(&accounts, current_slot, purged_pubkey1, 0);
        assert_load_account(&accounts, current_slot, purged_pubkey2, 0);
        assert_load_account(&accounts, current_slot, dummy_pubkey, dummy_lamport);

        accounts
            .verify_bank_hash_and_lamports(
                4,
                &Ancestors::default(),
                1222,
                true,
                &EpochSchedule::default(),
                &RentCollector::default(),
                false,
            )
            .unwrap();
    }

    #[test]
    fn test_accounts_purge_chained_purge_before_snapshot_restore() {
        solana_logger::setup();
        with_chained_zero_lamport_accounts(|accounts, current_slot| {
            accounts.clean_accounts_for_tests();
            reconstruct_accounts_db_via_serialization(&accounts, current_slot)
        });
    }

    #[test]
    fn test_accounts_purge_chained_purge_after_snapshot_restore() {
        solana_logger::setup();
        with_chained_zero_lamport_accounts(|accounts, current_slot| {
            let accounts = reconstruct_accounts_db_via_serialization(&accounts, current_slot);
            accounts.print_accounts_stats("after_reconstruct");
            accounts.clean_accounts_for_tests();
            reconstruct_accounts_db_via_serialization(&accounts, current_slot)
        });
    }

    #[test]
    #[ignore]
    fn test_store_account_stress() {
        let slot = 42;
        let num_threads = 2;

        let min_file_bytes = std::mem::size_of::<StoredMeta>()
            + std::mem::size_of::<crate::append_vec::AccountMeta>();

        let db = Arc::new(AccountsDb::new_sized(Vec::new(), min_file_bytes as u64));

        db.add_root(slot);
        let thread_hdls: Vec<_> = (0..num_threads)
            .map(|_| {
                let db = db.clone();
                std::thread::Builder::new()
                    .name("account-writers".to_string())
                    .spawn(move || {
                        let pubkey = solana_sdk::pubkey::new_rand();
                        let mut account = AccountSharedData::new(1, 0, &pubkey);
                        let mut i = 0;
                        loop {
                            let account_bal = thread_rng().gen_range(1, 99);
                            account.set_lamports(account_bal);
                            db.store_for_tests(slot, &[(&pubkey, &account)]);

                            let (account, slot) = db
                                .load_without_fixed_root(&Ancestors::default(), &pubkey)
                                .unwrap_or_else(|| {
                                    panic!("Could not fetch stored account {pubkey}, iter {i}")
                                });
                            assert_eq!(slot, slot);
                            assert_eq!(account.lamports(), account_bal);
                            i += 1;
                        }
                    })
                    .unwrap()
            })
            .collect();

        for t in thread_hdls {
            t.join().unwrap();
        }
    }

    #[test]
    fn test_accountsdb_scan_accounts() {
        solana_logger::setup();
        let db = AccountsDb::new(Vec::new(), &ClusterType::Development);
        let key = Pubkey::default();
        let key0 = solana_sdk::pubkey::new_rand();
        let account0 = AccountSharedData::new(1, 0, &key);

        db.store_for_tests(0, &[(&key0, &account0)]);

        let key1 = solana_sdk::pubkey::new_rand();
        let account1 = AccountSharedData::new(2, 0, &key);
        db.store_for_tests(1, &[(&key1, &account1)]);

        let ancestors = vec![(0, 0)].into_iter().collect();
        let mut accounts = Vec::new();
        db.unchecked_scan_accounts(
            "",
            &ancestors,
            |_, account, _| {
                accounts.push(account.take_account());
            },
            &ScanConfig::default(),
        );
        assert_eq!(accounts, vec![account0]);

        let ancestors = vec![(1, 1), (0, 0)].into_iter().collect();
        let mut accounts = Vec::new();
        db.unchecked_scan_accounts(
            "",
            &ancestors,
            |_, account, _| {
                accounts.push(account.take_account());
            },
            &ScanConfig::default(),
        );
        assert_eq!(accounts.len(), 2);
    }

    #[test]
    fn test_cleanup_key_not_removed() {
        solana_logger::setup();
        let db = AccountsDb::new_single_for_tests();

        let key = Pubkey::default();
        let key0 = solana_sdk::pubkey::new_rand();
        let account0 = AccountSharedData::new(1, 0, &key);

        db.store_for_tests(0, &[(&key0, &account0)]);

        let key1 = solana_sdk::pubkey::new_rand();
        let account1 = AccountSharedData::new(2, 0, &key);
        db.store_for_tests(1, &[(&key1, &account1)]);

        db.print_accounts_stats("pre");

        let slots: HashSet<Slot> = vec![1].into_iter().collect();
        let purge_keys = vec![(key1, slots)];
        let _ = db.purge_keys_exact(purge_keys.iter());

        let account2 = AccountSharedData::new(3, 0, &key);
        db.store_for_tests(2, &[(&key1, &account2)]);

        db.print_accounts_stats("post");
        let ancestors = vec![(2, 0)].into_iter().collect();
        assert_eq!(
            db.load_without_fixed_root(&ancestors, &key1)
                .unwrap()
                .0
                .lamports(),
            3
        );
    }

    #[test]
    fn test_store_large_account() {
        solana_logger::setup();
        let db = AccountsDb::new(Vec::new(), &ClusterType::Development);

        let key = Pubkey::default();
        let data_len = DEFAULT_FILE_SIZE as usize + 7;
        let account = AccountSharedData::new(1, data_len, &key);

        db.store_for_tests(0, &[(&key, &account)]);

        let ancestors = vec![(0, 0)].into_iter().collect();
        let ret = db.load_without_fixed_root(&ancestors, &key).unwrap();
        assert_eq!(ret.0.data().len(), data_len);
    }

    #[test]
    fn test_stored_readable_account() {
        let lamports = 1;
        let owner = Pubkey::new_unique();
        let executable = true;
        let rent_epoch = 2;
        let meta = StoredMeta {
            write_version_obsolete: 5,
            pubkey: Pubkey::new_unique(),
            data_len: 7,
        };
        let account_meta = AccountMeta {
            lamports,
            owner,
            executable,
            rent_epoch,
        };
        let data = Vec::new();
        let account = Account {
            lamports,
            owner,
            executable,
            rent_epoch,
            data: data.clone(),
        };
        let offset = 99;
        let stored_size = 101;
        let hash = Hash::new_unique();
        let stored_account = StoredAccountMeta {
            meta: &meta,
            account_meta: &account_meta,
            data: &data,
            offset,
            stored_size,
            hash: &hash,
        };
        assert!(accounts_equal(&account, &stored_account));
    }

    #[test]
    fn test_hash_stored_account() {
        // This test uses some UNSAFE tricks to detect most of account's field
        // addition and deletion without changing the hash code

        const ACCOUNT_DATA_LEN: usize = 3;
        // the type of InputTuple elements must not contain references;
        // they should be simple scalars or data blobs
        type InputTuple = (
            Slot,
            StoredMeta,
            AccountMeta,
            [u8; ACCOUNT_DATA_LEN],
            usize, // for StoredAccountMeta::offset
            Hash,
        );
        const INPUT_LEN: usize = std::mem::size_of::<InputTuple>();
        type InputBlob = [u8; INPUT_LEN];
        let mut blob: InputBlob = [0u8; INPUT_LEN];

        // spray memory with decreasing counts so that, data layout can be detected.
        for (i, byte) in blob.iter_mut().enumerate() {
            *byte = (INPUT_LEN - i) as u8;
        }

        //UNSAFE: forcibly cast the special byte pattern to actual account fields.
        let (slot, meta, account_meta, data, offset, hash): InputTuple =
            unsafe { std::mem::transmute::<InputBlob, InputTuple>(blob) };

        let stored_account = StoredAccountMeta {
            meta: &meta,
            account_meta: &account_meta,
            data: &data,
            offset,
            stored_size: CACHE_VIRTUAL_STORED_SIZE as usize,
            hash: &hash,
        };
        let account = stored_account.clone_account();

        let expected_account_hash = if cfg!(debug_assertions) {
            Hash::from_str("4StuvYHFd7xuShVXB94uHHvpqGMCaacdZnYB74QQkPA1").unwrap()
        } else {
            Hash::from_str("33ruy7m3Xto7irYfsBSN74aAzQwCQxsfoZxXuZy2Rra3").unwrap()
        };

        assert_eq!(
            AccountsDb::hash_account(
                slot,
                &stored_account,
                stored_account.pubkey(),
                INCLUDE_SLOT_IN_HASH_TESTS
            ),
            expected_account_hash,
            "StoredAccountMeta's data layout might be changed; update hashing if needed."
        );
        assert_eq!(
            AccountsDb::hash_account(
                slot,
                &account,
                stored_account.pubkey(),
                INCLUDE_SLOT_IN_HASH_TESTS
            ),
            expected_account_hash,
            "Account-based hashing must be consistent with StoredAccountMeta-based one."
        );
    }

    #[test]
    fn test_bank_hash_stats() {
        solana_logger::setup();
        let db = AccountsDb::new(Vec::new(), &ClusterType::Development);

        let key = Pubkey::default();
        let some_data_len = 5;
        let some_slot: Slot = 0;
        let account = AccountSharedData::new(1, some_data_len, &key);
        let ancestors = vec![(some_slot, 0)].into_iter().collect();

        db.store_for_tests(some_slot, &[(&key, &account)]);
        let mut account = db.load_without_fixed_root(&ancestors, &key).unwrap().0;
        account.checked_sub_lamports(1).unwrap();
        account.set_executable(true);
        db.store_for_tests(some_slot, &[(&key, &account)]);
        db.add_root(some_slot);

        let bank_hashes = db.bank_hashes.read().unwrap();
        let bank_hash = bank_hashes.get(&some_slot).unwrap();
        assert_eq!(bank_hash.stats.num_updated_accounts, 1);
        assert_eq!(bank_hash.stats.num_removed_accounts, 1);
        assert_eq!(bank_hash.stats.num_lamports_stored, 1);
        assert_eq!(bank_hash.stats.total_data_len, 2 * some_data_len as u64);
        assert_eq!(bank_hash.stats.num_executable_accounts, 1);
    }

    // this test tests check_hash=true, which is unsupported behavior at the moment. It cannot be enabled by anything but these tests.
    #[ignore]
    #[test]
    fn test_calculate_accounts_hash_check_hash_mismatch() {
        solana_logger::setup();
        let db = AccountsDb::new(Vec::new(), &ClusterType::Development);

        let key = solana_sdk::pubkey::new_rand();
        let some_data_len = 0;
        let some_slot: Slot = 0;
        let account = AccountSharedData::new(1, some_data_len, &key);

        let ancestors = vec![(some_slot, 0)].into_iter().collect();

        // put wrong hash value in store so we get a mismatch
        db.store_accounts_unfrozen(
            (some_slot, &[(&key, &account)][..]),
            Some(vec![&Hash::default()]),
            false,
            None,
            StoreReclaims::Default,
        );
        db.add_root(some_slot);
        let check_hash = true;
        for data_source in [
            CalcAccountsHashDataSource::IndexForTests,
            CalcAccountsHashDataSource::Storages,
        ] {
            assert!(db
                .calculate_accounts_hash(
                    data_source,
                    some_slot,
                    &CalcAccountsHashConfig {
                        use_bg_thread_pool: true, // is_startup used to be false
                        check_hash,
                        ancestors: Some(&ancestors),
                        ..CalcAccountsHashConfig::default()
                    },
                )
                .is_err());
        }
    }

    // something we can get a ref to
    lazy_static! {
        pub static ref EPOCH_SCHEDULE: EpochSchedule = EpochSchedule::default();
        pub static ref RENT_COLLECTOR: RentCollector = RentCollector::default();
    }

    impl<'a> CalcAccountsHashConfig<'a> {
        fn default() -> Self {
            Self {
                use_bg_thread_pool: false,
                check_hash: false,
                ancestors: None,
                epoch_schedule: &EPOCH_SCHEDULE,
                rent_collector: &RENT_COLLECTOR,
                store_detailed_debug_info_on_failure: false,
                full_snapshot: None,
            }
        }
    }

    // this test tests check_hash=true, which is unsupported behavior at the moment. It cannot be enabled by anything but these tests.
    #[ignore]
    #[test]
    fn test_calculate_accounts_hash_check_hash() {
        solana_logger::setup();
        let db = AccountsDb::new(Vec::new(), &ClusterType::Development);

        let key = solana_sdk::pubkey::new_rand();
        let some_data_len = 0;
        let some_slot: Slot = 0;
        let account = AccountSharedData::new(1, some_data_len, &key);

        let ancestors = vec![(some_slot, 0)].into_iter().collect();

        db.store_for_tests(some_slot, &[(&key, &account)]);
        db.add_root(some_slot);
        let check_hash = true;
        assert_eq!(
            db.calculate_accounts_hash(
                CalcAccountsHashDataSource::Storages,
                some_slot,
                &CalcAccountsHashConfig {
                    use_bg_thread_pool: true, // is_startup used to be false
                    check_hash,
                    ancestors: Some(&ancestors),
                    ..CalcAccountsHashConfig::default()
                },
            )
            .unwrap(),
            db.calculate_accounts_hash(
                CalcAccountsHashDataSource::IndexForTests,
                some_slot,
                &CalcAccountsHashConfig {
                    use_bg_thread_pool: true, // is_startup used to be false
                    check_hash,
                    ancestors: Some(&ancestors),
                    ..CalcAccountsHashConfig::default()
                },
            )
            .unwrap(),
        );
    }

    #[test]
    fn test_verify_bank_hash() {
        use BankHashVerificationError::*;
        solana_logger::setup();
        let db = AccountsDb::new(Vec::new(), &ClusterType::Development);

        let key = solana_sdk::pubkey::new_rand();
        let some_data_len = 0;
        let some_slot: Slot = 0;
        let account = AccountSharedData::new(1, some_data_len, &key);
        let ancestors = vec![(some_slot, 0)].into_iter().collect();

        db.store_for_tests(some_slot, &[(&key, &account)]);
        db.add_root_and_flush_write_cache(some_slot);
        db.update_accounts_hash_for_tests(some_slot, &ancestors, true, true);
        assert_matches!(
            db.verify_bank_hash_and_lamports(
                some_slot,
                &ancestors,
                1,
                true,
                &EpochSchedule::default(),
                &RentCollector::default(),
                false,
            ),
            Ok(_)
        );

        db.bank_hashes.write().unwrap().remove(&some_slot).unwrap();
        assert_matches!(
            db.verify_bank_hash_and_lamports(
                some_slot,
                &ancestors,
                1,
                true,
                &EpochSchedule::default(),
                &RentCollector::default(),
                false,
            ),
            Err(MissingBankHash)
        );

        let some_bank_hash = Hash::new(&[0xca; HASH_BYTES]);
        let bank_hash_info = BankHashInfo {
            accounts_delta_hash: some_bank_hash,
            accounts_hash: AccountsHash(Hash::new(&[0xca; HASH_BYTES])),
            stats: BankHashStats::default(),
        };
        db.bank_hashes
            .write()
            .unwrap()
            .insert(some_slot, bank_hash_info);
        assert_matches!(
            db.verify_bank_hash_and_lamports(
                some_slot,
                &ancestors,
                1,
                true,
                &EpochSchedule::default(),
                &RentCollector::default(),
                false,
            ),
            Err(MismatchedBankHash)
        );
    }

    #[test]
    fn test_verify_bank_capitalization() {
        for pass in 0..2 {
            use BankHashVerificationError::*;
            solana_logger::setup();
            let db = AccountsDb::new(Vec::new(), &ClusterType::Development);

            let key = solana_sdk::pubkey::new_rand();
            let some_data_len = 0;
            let some_slot: Slot = 0;
            let account = AccountSharedData::new(1, some_data_len, &key);
            let ancestors = vec![(some_slot, 0)].into_iter().collect();

            db.store_for_tests(some_slot, &[(&key, &account)]);
            if pass == 0 {
                db.add_root_and_flush_write_cache(some_slot);
                db.update_accounts_hash_for_tests(some_slot, &ancestors, true, true);
                assert_matches!(
                    db.verify_bank_hash_and_lamports(
                        some_slot,
                        &ancestors,
                        1,
                        true,
                        &EpochSchedule::default(),
                        &RentCollector::default(),
                        false,
                    ),
                    Ok(_)
                );
                continue;
            }

            let native_account_pubkey = solana_sdk::pubkey::new_rand();
            db.store_for_tests(
                some_slot,
                &[(
                    &native_account_pubkey,
                    &solana_sdk::native_loader::create_loadable_account_for_test("foo"),
                )],
            );
            db.add_root_and_flush_write_cache(some_slot);
            db.update_accounts_hash_for_tests(some_slot, &ancestors, true, true);
            assert_matches!(
                db.verify_bank_hash_and_lamports(
                    some_slot,
                    &ancestors,
                    2,
                    true,
                    &EpochSchedule::default(),
                    &RentCollector::default(),
                    false,
                ),
                Ok(_)
            );

            assert_matches!(
                db.verify_bank_hash_and_lamports(some_slot, &ancestors, 10, true, &EpochSchedule::default(), &RentCollector::default(), false,),
                Err(MismatchedTotalLamports(expected, actual)) if expected == 2 && actual == 10
            );
        }
    }

    #[test]
    fn test_verify_bank_hash_no_account() {
        solana_logger::setup();
        let db = AccountsDb::new(Vec::new(), &ClusterType::Development);

        let some_slot: Slot = 0;
        let ancestors = vec![(some_slot, 0)].into_iter().collect();

        db.bank_hashes
            .write()
            .unwrap()
            .insert(some_slot, BankHashInfo::default());
        db.add_root(some_slot);
        db.update_accounts_hash_for_tests(some_slot, &ancestors, true, true);
        assert_matches!(
            db.verify_bank_hash_and_lamports(
                some_slot,
                &ancestors,
                0,
                true,
                &EpochSchedule::default(),
                &RentCollector::default(),
                false,
            ),
            Ok(_)
        );
    }

    #[test]
    fn test_verify_bank_hash_bad_account_hash() {
        use BankHashVerificationError::*;
        solana_logger::setup();
        let db = AccountsDb::new(Vec::new(), &ClusterType::Development);

        let key = Pubkey::default();
        let some_data_len = 0;
        let some_slot: Slot = 0;
        let account = AccountSharedData::new(1, some_data_len, &key);
        let ancestors = vec![(some_slot, 0)].into_iter().collect();

        let accounts = &[(&key, &account)][..];
        // update AccountsDb's bank hash
        {
            let mut bank_hashes = db.bank_hashes.write().unwrap();
            bank_hashes
                .entry(some_slot)
                .or_insert_with(BankHashInfo::default);
        }
        // provide bogus account hashes
        let some_hash = Hash::new(&[0xca; HASH_BYTES]);
        db.store_accounts_unfrozen(
            (some_slot, accounts),
            Some(vec![&some_hash]),
            false,
            None,
            StoreReclaims::Default,
        );
        db.add_root(some_slot);
        assert_matches!(
            db.verify_bank_hash_and_lamports(
                some_slot,
                &ancestors,
                1,
                true,
                &EpochSchedule::default(),
                &RentCollector::default(),
                false,
            ),
            Err(MismatchedBankHash)
        );
    }

    #[test]
    fn test_storage_finder() {
        solana_logger::setup();
        let db = AccountsDb::new_sized(Vec::new(), 16 * 1024);
        let key = solana_sdk::pubkey::new_rand();
        let lamports = 100;
        let data_len = 8190;
        let account = AccountSharedData::new(lamports, data_len, &solana_sdk::pubkey::new_rand());
        // pre-populate with a smaller empty store
        db.create_and_insert_store(1, 8192, "test_storage_finder");
        db.store_for_tests(1, &[(&key, &account)]);
    }

    #[test]
    fn test_get_snapshot_storages_empty() {
        let db = AccountsDb::new(Vec::new(), &ClusterType::Development);
        assert!(db.get_snapshot_storages(..=0, None).0.is_empty());
    }

    #[test]
    fn test_get_snapshot_storages_only_older_than_or_equal_to_snapshot_slot() {
        let db = AccountsDb::new(Vec::new(), &ClusterType::Development);

        let key = Pubkey::default();
        let account = AccountSharedData::new(1, 0, &key);
        let before_slot = 0;
        let base_slot = before_slot + 1;
        let after_slot = base_slot + 1;

        db.store_for_tests(base_slot, &[(&key, &account)]);
        db.add_root_and_flush_write_cache(base_slot);
        assert!(db.get_snapshot_storages(..=before_slot, None).0.is_empty());

        assert_eq!(1, db.get_snapshot_storages(..=base_slot, None).0.len());
        assert_eq!(1, db.get_snapshot_storages(..=after_slot, None).0.len());
    }

    #[test]
    fn test_get_snapshot_storages_only_non_empty() {
        for pass in 0..2 {
            let db = AccountsDb::new(Vec::new(), &ClusterType::Development);

            let key = Pubkey::default();
            let account = AccountSharedData::new(1, 0, &key);
            let base_slot = 0;
            let after_slot = base_slot + 1;

            db.store_for_tests(base_slot, &[(&key, &account)]);
            if pass == 0 {
                db.add_root_and_flush_write_cache(base_slot);
                db.storage
                    .get_slot_stores(base_slot)
                    .unwrap()
                    .write()
                    .unwrap()
                    .clear();
                assert!(db.get_snapshot_storages(..=after_slot, None).0.is_empty());
                continue;
            }

            db.store_for_tests(base_slot, &[(&key, &account)]);
            db.add_root_and_flush_write_cache(base_slot);
            assert_eq!(1, db.get_snapshot_storages(..=after_slot, None).0.len());
        }
    }

    #[test]
    fn test_get_snapshot_storages_only_roots() {
        let db = AccountsDb::new(Vec::new(), &ClusterType::Development);

        let key = Pubkey::default();
        let account = AccountSharedData::new(1, 0, &key);
        let base_slot = 0;
        let after_slot = base_slot + 1;

        db.store_for_tests(base_slot, &[(&key, &account)]);
        assert!(db.get_snapshot_storages(..=after_slot, None).0.is_empty());

        db.add_root_and_flush_write_cache(base_slot);
        assert_eq!(1, db.get_snapshot_storages(..=after_slot, None).0.len());
    }

    #[test]
    fn test_get_snapshot_storages_exclude_empty() {
        let db = AccountsDb::new(Vec::new(), &ClusterType::Development);

        let key = Pubkey::default();
        let account = AccountSharedData::new(1, 0, &key);
        let base_slot = 0;
        let after_slot = base_slot + 1;

        db.store_for_tests(base_slot, &[(&key, &account)]);
        db.add_root_and_flush_write_cache(base_slot);
        assert_eq!(1, db.get_snapshot_storages(..=after_slot, None).0.len());

        db.storage
            .get_slot_stores(0)
            .unwrap()
            .read()
            .unwrap()
            .values()
            .next()
            .unwrap()
            .remove_account(0, true);
        assert!(db.get_snapshot_storages(..=after_slot, None).0.is_empty());
    }

    #[test]
    fn test_get_snapshot_storages_with_base_slot() {
        let db = AccountsDb::new(Vec::new(), &ClusterType::Development);

        let key = Pubkey::default();
        let account = AccountSharedData::new(1, 0, &key);

        let slot = 10;
        db.store_for_tests(slot, &[(&key, &account)]);
        db.add_root_and_flush_write_cache(slot);
        assert_eq!(
            0,
            db.get_snapshot_storages(slot + 1..=slot + 1, None).0.len()
        );
        assert_eq!(1, db.get_snapshot_storages(slot..=slot + 1, None).0.len());
    }

    #[test]
    #[should_panic(expected = "double remove of account in slot: 0/store: 0!!")]
    fn test_storage_remove_account_double_remove() {
        let accounts = AccountsDb::new(Vec::new(), &ClusterType::Development);
        let pubkey = solana_sdk::pubkey::new_rand();
        let account = AccountSharedData::new(1, 0, AccountSharedData::default().owner());
        accounts.store_for_tests(0, &[(&pubkey, &account)]);
        accounts.add_root_and_flush_write_cache(0);
        let storage_entry = accounts
            .storage
            .get_slot_stores(0)
            .unwrap()
            .read()
            .unwrap()
            .values()
            .next()
            .unwrap()
            .clone();
        storage_entry.remove_account(0, true);
        storage_entry.remove_account(0, true);
    }

    #[test]
    fn test_accounts_purge_long_chained_after_snapshot_restore() {
        solana_logger::setup();
        let old_lamport = 223;
        let zero_lamport = 0;
        let no_data = 0;
        let owner = *AccountSharedData::default().owner();

        let account = AccountSharedData::new(old_lamport, no_data, &owner);
        let account2 = AccountSharedData::new(old_lamport + 100_001, no_data, &owner);
        let account3 = AccountSharedData::new(old_lamport + 100_002, no_data, &owner);
        let dummy_account = AccountSharedData::new(99_999_999, no_data, &owner);
        let zero_lamport_account = AccountSharedData::new(zero_lamport, no_data, &owner);

        let pubkey = solana_sdk::pubkey::new_rand();
        let dummy_pubkey = solana_sdk::pubkey::new_rand();
        let purged_pubkey1 = solana_sdk::pubkey::new_rand();
        let purged_pubkey2 = solana_sdk::pubkey::new_rand();

        let mut current_slot = 0;
        let accounts = AccountsDb::new_single_for_tests();

        // create intermediate updates to purged_pubkey1 so that
        // generate_index must add slots as root last at once
        current_slot += 1;
        accounts.store_for_tests(current_slot, &[(&pubkey, &account)]);
        accounts.store_for_tests(current_slot, &[(&purged_pubkey1, &account2)]);
        accounts.add_root_and_flush_write_cache(current_slot);

        current_slot += 1;
        accounts.store_for_tests(current_slot, &[(&purged_pubkey1, &account2)]);
        accounts.add_root_and_flush_write_cache(current_slot);

        current_slot += 1;
        accounts.store_for_tests(current_slot, &[(&purged_pubkey1, &account2)]);
        accounts.add_root_and_flush_write_cache(current_slot);

        current_slot += 1;
        accounts.store_for_tests(current_slot, &[(&purged_pubkey1, &zero_lamport_account)]);
        accounts.store_for_tests(current_slot, &[(&purged_pubkey2, &account3)]);
        accounts.add_root_and_flush_write_cache(current_slot);

        current_slot += 1;
        accounts.store_for_tests(current_slot, &[(&purged_pubkey2, &zero_lamport_account)]);
        accounts.add_root_and_flush_write_cache(current_slot);

        current_slot += 1;
        accounts.store_for_tests(current_slot, &[(&dummy_pubkey, &dummy_account)]);
        accounts.add_root_and_flush_write_cache(current_slot);

        accounts.print_count_and_status("before reconstruct");
        let accounts = reconstruct_accounts_db_via_serialization(&accounts, current_slot);
        accounts.print_count_and_status("before purge zero");
        accounts.clean_accounts_for_tests();
        accounts.print_count_and_status("after purge zero");

        assert_load_account(&accounts, current_slot, pubkey, old_lamport);
        assert_load_account(&accounts, current_slot, purged_pubkey1, 0);
        assert_load_account(&accounts, current_slot, purged_pubkey2, 0);
    }

    fn do_full_clean_refcount(store1_first: bool, store_size: u64) {
        let pubkey1 = Pubkey::from_str("My11111111111111111111111111111111111111111").unwrap();
        let pubkey2 = Pubkey::from_str("My22211111111111111111111111111111111111111").unwrap();
        let pubkey3 = Pubkey::from_str("My33311111111111111111111111111111111111111").unwrap();

        let old_lamport = 223;
        let zero_lamport = 0;
        let dummy_lamport = 999_999;

        // size data so only 1 fits in a 4k store
        let data_size = 2200;

        let owner = *AccountSharedData::default().owner();

        let account = AccountSharedData::new(old_lamport, data_size, &owner);
        let account2 = AccountSharedData::new(old_lamport + 100_001, data_size, &owner);
        let account3 = AccountSharedData::new(old_lamport + 100_002, data_size, &owner);
        let account4 = AccountSharedData::new(dummy_lamport, data_size, &owner);
        let zero_lamport_account = AccountSharedData::new(zero_lamport, data_size, &owner);

        let mut current_slot = 0;
        let accounts = AccountsDb::new_sized_no_extra_stores(Vec::new(), store_size);

        // A: Initialize AccountsDb with pubkey1 and pubkey2
        current_slot += 1;
        if store1_first {
            accounts.store_for_tests(current_slot, &[(&pubkey1, &account)]);
            accounts.store_for_tests(current_slot, &[(&pubkey2, &account)]);
        } else {
            accounts.store_for_tests(current_slot, &[(&pubkey2, &account)]);
            accounts.store_for_tests(current_slot, &[(&pubkey1, &account)]);
        }
        accounts.get_accounts_delta_hash(current_slot);
        accounts.add_root_and_flush_write_cache(current_slot);

        info!("post A");
        accounts.print_accounts_stats("Post-A");

        // B: Test multiple updates to pubkey1 in a single slot/storage
        current_slot += 1;
        assert_eq!(0, accounts.alive_account_count_in_slot(current_slot));
        assert_eq!(1, accounts.ref_count_for_pubkey(&pubkey1));
        accounts.store_for_tests(current_slot, &[(&pubkey1, &account2)]);
        accounts.store_for_tests(current_slot, &[(&pubkey1, &account2)]);
        accounts.add_root_and_flush_write_cache(current_slot);
        assert_eq!(1, accounts.alive_account_count_in_slot(current_slot));
        // Stores to same pubkey, same slot only count once towards the
        // ref count
        assert_eq!(2, accounts.ref_count_for_pubkey(&pubkey1));
        accounts.get_accounts_delta_hash(current_slot);
        accounts.add_root_and_flush_write_cache(current_slot);

        accounts.print_accounts_stats("Post-B pre-clean");

        accounts.clean_accounts_for_tests();

        info!("post B");
        accounts.print_accounts_stats("Post-B");

        // C: more updates to trigger clean of previous updates
        current_slot += 1;
        assert_eq!(2, accounts.ref_count_for_pubkey(&pubkey1));
        accounts.store_for_tests(current_slot, &[(&pubkey1, &account3)]);
        accounts.store_for_tests(current_slot, &[(&pubkey2, &account3)]);
        accounts.store_for_tests(current_slot, &[(&pubkey3, &account4)]);
        accounts.add_root_and_flush_write_cache(current_slot);
        assert_eq!(3, accounts.ref_count_for_pubkey(&pubkey1));
        accounts.get_accounts_delta_hash(current_slot);

        info!("post C");

        accounts.print_accounts_stats("Post-C");

        // D: Make all keys 0-lamport, cleans all keys
        current_slot += 1;
        assert_eq!(3, accounts.ref_count_for_pubkey(&pubkey1));
        accounts.store_for_tests(current_slot, &[(&pubkey1, &zero_lamport_account)]);
        accounts.store_for_tests(current_slot, &[(&pubkey2, &zero_lamport_account)]);
        accounts.store_for_tests(current_slot, &[(&pubkey3, &zero_lamport_account)]);

        let snapshot_stores = accounts.get_snapshot_storages(..=current_slot, None).0;
        let total_accounts: usize = snapshot_stores
            .iter()
            .flatten()
            .map(|s| s.all_accounts().len())
            .sum();
        assert!(!snapshot_stores.is_empty());
        assert!(total_accounts > 0);

        info!("post D");
        accounts.print_accounts_stats("Post-D");

        accounts.get_accounts_delta_hash(current_slot);
        accounts.add_root_and_flush_write_cache(current_slot);
        accounts.clean_accounts_for_tests();

        accounts.print_accounts_stats("Post-D clean");

        let total_accounts_post_clean: usize = snapshot_stores
            .iter()
            .flatten()
            .map(|s| s.all_accounts().len())
            .sum();
        assert_eq!(total_accounts, total_accounts_post_clean);

        // should clean all 3 pubkeys
        assert_eq!(accounts.ref_count_for_pubkey(&pubkey1), 0);
        assert_eq!(accounts.ref_count_for_pubkey(&pubkey2), 0);
        assert_eq!(accounts.ref_count_for_pubkey(&pubkey3), 0);
    }

    #[test]
    fn test_full_clean_refcount() {
        solana_logger::setup();

        // Setup 3 scenarios which try to differentiate between pubkey1 being in an
        // Available slot or a Full slot which would cause a different reset behavior
        // when pubkey1 is cleaned and therefore cause the ref count to be incorrect
        // preventing a removal of that key.
        //
        // do stores with a 4mb size so only 1 store is created per slot
        do_full_clean_refcount(false, 4 * 1024 * 1024);

        // do stores with a 4k size and store pubkey1 first
        do_full_clean_refcount(false, 4096);

        // do stores with a 4k size and store pubkey1 2nd
        do_full_clean_refcount(true, 4096);
    }

    #[test]
    fn test_accounts_clean_after_snapshot_restore_then_old_revives() {
        solana_logger::setup();
        let old_lamport = 223;
        let zero_lamport = 0;
        let no_data = 0;
        let dummy_lamport = 999_999;
        let owner = *AccountSharedData::default().owner();

        let account = AccountSharedData::new(old_lamport, no_data, &owner);
        let account2 = AccountSharedData::new(old_lamport + 100_001, no_data, &owner);
        let account3 = AccountSharedData::new(old_lamport + 100_002, no_data, &owner);
        let dummy_account = AccountSharedData::new(dummy_lamport, no_data, &owner);
        let zero_lamport_account = AccountSharedData::new(zero_lamport, no_data, &owner);

        let pubkey1 = solana_sdk::pubkey::new_rand();
        let pubkey2 = solana_sdk::pubkey::new_rand();
        let dummy_pubkey = solana_sdk::pubkey::new_rand();

        let mut current_slot = 0;
        let accounts = AccountsDb::new_single_for_tests();

        // A: Initialize AccountsDb with pubkey1 and pubkey2
        current_slot += 1;
        accounts.store_for_tests(current_slot, &[(&pubkey1, &account)]);
        accounts.store_for_tests(current_slot, &[(&pubkey2, &account)]);
        accounts.get_accounts_delta_hash(current_slot);
        accounts.add_root(current_slot);

        // B: Test multiple updates to pubkey1 in a single slot/storage
        current_slot += 1;
        assert_eq!(0, accounts.alive_account_count_in_slot(current_slot));
        accounts.add_root_and_flush_write_cache(current_slot - 1);
        assert_eq!(1, accounts.ref_count_for_pubkey(&pubkey1));
        accounts.store_for_tests(current_slot, &[(&pubkey1, &account2)]);
        accounts.store_for_tests(current_slot, &[(&pubkey1, &account2)]);
        accounts.add_root_and_flush_write_cache(current_slot);
        assert_eq!(1, accounts.alive_account_count_in_slot(current_slot));
        // Stores to same pubkey, same slot only count once towards the
        // ref count
        assert_eq!(2, accounts.ref_count_for_pubkey(&pubkey1));
        accounts.get_accounts_delta_hash(current_slot);

        // C: Yet more update to trigger lazy clean of step A
        current_slot += 1;
        assert_eq!(2, accounts.ref_count_for_pubkey(&pubkey1));
        accounts.store_for_tests(current_slot, &[(&pubkey1, &account3)]);
        accounts.add_root_and_flush_write_cache(current_slot);
        assert_eq!(3, accounts.ref_count_for_pubkey(&pubkey1));
        accounts.get_accounts_delta_hash(current_slot);
        accounts.add_root_and_flush_write_cache(current_slot);

        // D: Make pubkey1 0-lamport; also triggers clean of step B
        current_slot += 1;
        assert_eq!(3, accounts.ref_count_for_pubkey(&pubkey1));
        accounts.store_for_tests(current_slot, &[(&pubkey1, &zero_lamport_account)]);
        accounts.add_root_and_flush_write_cache(current_slot);
        // had to be a root to flush, but clean won't work as this test expects if it is a root
        // so, remove the root from alive_roots, then restore it after clean
        accounts
            .accounts_index
            .roots_tracker
            .write()
            .unwrap()
            .alive_roots
            .remove(&current_slot);
        accounts.clean_accounts_for_tests();
        accounts
            .accounts_index
            .roots_tracker
            .write()
            .unwrap()
            .alive_roots
            .insert(current_slot);

        assert_eq!(
            // Removed one reference from the dead slot (reference only counted once
            // even though there were two stores to the pubkey in that slot)
            3, /* == 3 - 1 + 1 */
            accounts.ref_count_for_pubkey(&pubkey1)
        );
        accounts.get_accounts_delta_hash(current_slot);
        accounts.add_root(current_slot);

        // E: Avoid missing bank hash error
        current_slot += 1;
        accounts.store_for_tests(current_slot, &[(&dummy_pubkey, &dummy_account)]);
        accounts.get_accounts_delta_hash(current_slot);
        accounts.add_root(current_slot);

        assert_load_account(&accounts, current_slot, pubkey1, zero_lamport);
        assert_load_account(&accounts, current_slot, pubkey2, old_lamport);
        assert_load_account(&accounts, current_slot, dummy_pubkey, dummy_lamport);

        // At this point, there is no index entries for A and B
        // If step C and step D should be purged, snapshot restore would cause
        // pubkey1 to be revived as the state of step A.
        // So, prevent that from happening by introducing refcount
        ((current_slot - 1)..=current_slot).for_each(|slot| accounts.flush_root_write_cache(slot));
        accounts.clean_accounts_for_tests();
        let accounts = reconstruct_accounts_db_via_serialization(&accounts, current_slot);
        accounts.clean_accounts_for_tests();

        info!("pubkey: {}", pubkey1);
        accounts.print_accounts_stats("pre_clean");
        assert_load_account(&accounts, current_slot, pubkey1, zero_lamport);
        assert_load_account(&accounts, current_slot, pubkey2, old_lamport);
        assert_load_account(&accounts, current_slot, dummy_pubkey, dummy_lamport);

        // F: Finally, make Step A cleanable
        current_slot += 1;
        accounts.store_for_tests(current_slot, &[(&pubkey2, &account)]);
        accounts.get_accounts_delta_hash(current_slot);
        accounts.add_root(current_slot);

        // Do clean
        accounts.flush_root_write_cache(current_slot);
        accounts.clean_accounts_for_tests();

        // 2nd clean needed to clean-up pubkey1
        accounts.clean_accounts_for_tests();

        // Ensure pubkey2 is cleaned from the index finally
        assert_not_load_account(&accounts, current_slot, pubkey1);
        assert_load_account(&accounts, current_slot, pubkey2, old_lamport);
        assert_load_account(&accounts, current_slot, dummy_pubkey, dummy_lamport);
    }

    #[test]
    fn test_clean_stored_dead_slots_empty() {
        let accounts = AccountsDb::new_single_for_tests();
        let mut dead_slots = HashSet::new();
        dead_slots.insert(10);
        accounts.clean_stored_dead_slots(&dead_slots, None, &HashSet::default());
    }

    #[test]
    fn test_shrink_all_slots_none() {
        for startup in &[false, true] {
            let accounts = AccountsDb::new_single_for_tests();

            for _ in 0..10 {
                accounts.shrink_candidate_slots();
            }

            accounts.shrink_all_slots(*startup, None);
        }
    }

    #[test]
    fn test_shrink_stale_slots_processed() {
        solana_logger::setup();

        for startup in &[false, true] {
            let accounts = AccountsDb::new_single_for_tests();

            let pubkey_count = 100;
            let pubkeys: Vec<_> = (0..pubkey_count)
                .map(|_| solana_sdk::pubkey::new_rand())
                .collect();

            let some_lamport = 223;
            let no_data = 0;
            let owner = *AccountSharedData::default().owner();

            let account = AccountSharedData::new(some_lamport, no_data, &owner);

            let mut current_slot = 0;

            current_slot += 1;
            for pubkey in &pubkeys {
                accounts.store_for_tests(current_slot, &[(pubkey, &account)]);
            }
            let shrink_slot = current_slot;
            accounts.get_accounts_delta_hash(current_slot);
            accounts.add_root_and_flush_write_cache(current_slot);

            current_slot += 1;
            let pubkey_count_after_shrink = 10;
            let updated_pubkeys = &pubkeys[0..pubkey_count - pubkey_count_after_shrink];

            for pubkey in updated_pubkeys {
                accounts.store_for_tests(current_slot, &[(pubkey, &account)]);
            }
            accounts.get_accounts_delta_hash(current_slot);
            accounts.add_root_and_flush_write_cache(current_slot);

            accounts.clean_accounts_for_tests();

            assert_eq!(
                pubkey_count,
                accounts.all_account_count_in_append_vec(shrink_slot)
            );
            accounts.shrink_all_slots(*startup, None);
            assert_eq!(
                pubkey_count_after_shrink,
                accounts.all_account_count_in_append_vec(shrink_slot)
            );

            let no_ancestors = Ancestors::default();
            accounts.update_accounts_hash_for_tests(current_slot, &no_ancestors, false, false);
            accounts
                .verify_bank_hash_and_lamports(
                    current_slot,
                    &no_ancestors,
                    22300,
                    true,
                    &EpochSchedule::default(),
                    &RentCollector::default(),
                    false,
                )
                .unwrap();

            let accounts = reconstruct_accounts_db_via_serialization(&accounts, current_slot);
            accounts
                .verify_bank_hash_and_lamports(
                    current_slot,
                    &no_ancestors,
                    22300,
                    true,
                    &EpochSchedule::default(),
                    &RentCollector::default(),
                    false,
                )
                .unwrap();

            // repeating should be no-op
            accounts.shrink_all_slots(*startup, None);
            assert_eq!(
                pubkey_count_after_shrink,
                accounts.all_account_count_in_append_vec(shrink_slot)
            );
        }
    }

    #[test]
    fn test_shrink_candidate_slots() {
        solana_logger::setup();

        let mut accounts = AccountsDb::new_single_for_tests();

        let pubkey_count = 30000;
        let pubkeys: Vec<_> = (0..pubkey_count)
            .map(|_| solana_sdk::pubkey::new_rand())
            .collect();

        let some_lamport = 223;
        let no_data = 0;
        let owner = *AccountSharedData::default().owner();

        let account = AccountSharedData::new(some_lamport, no_data, &owner);

        let mut current_slot = 0;

        current_slot += 1;
        for pubkey in &pubkeys {
            accounts.store_for_tests(current_slot, &[(pubkey, &account)]);
        }
        let shrink_slot = current_slot;
        accounts.get_accounts_delta_hash(current_slot);
        accounts.add_root_and_flush_write_cache(current_slot);

        current_slot += 1;
        let pubkey_count_after_shrink = 25000;
        let updated_pubkeys = &pubkeys[0..pubkey_count - pubkey_count_after_shrink];

        for pubkey in updated_pubkeys {
            accounts.store_for_tests(current_slot, &[(pubkey, &account)]);
        }
        accounts.get_accounts_delta_hash(current_slot);
        accounts.add_root_and_flush_write_cache(current_slot);
        accounts.clean_accounts_for_tests();

        assert_eq!(
            pubkey_count,
            accounts.all_account_count_in_append_vec(shrink_slot)
        );

        // Only, try to shrink stale slots, nothing happens because shrink ratio
        // is not small enough to do a shrink
        // Note this shrink ratio had to change because we are WAY over-allocating append vecs when we flush the write cache at the moment.
        accounts.shrink_ratio = AccountShrinkThreshold::TotalSpace { shrink_ratio: 0.4 };
        accounts.shrink_candidate_slots();
        assert_eq!(
            pubkey_count,
            accounts.all_account_count_in_append_vec(shrink_slot)
        );

        // Now, do full-shrink.
        accounts.shrink_all_slots(false, None);
        assert_eq!(
            pubkey_count_after_shrink,
            accounts.all_account_count_in_append_vec(shrink_slot)
        );
    }

    #[test]
    fn test_select_candidates_by_total_usage_no_candidates() {
        // no input candidates -- none should be selected
        solana_logger::setup();
        let candidates: ShrinkCandidates = HashMap::new();

        let (selected_candidates, next_candidates) = AccountsDb::select_candidates_by_total_usage(
            &candidates,
            DEFAULT_ACCOUNTS_SHRINK_RATIO,
        );

        assert_eq!(0, selected_candidates.len());
        assert_eq!(0, next_candidates.len());
    }

    #[test]
    fn test_select_candidates_by_total_usage_3_way_split_condition() {
        // three candidates, one selected for shrink, one is put back to the candidate list and one is ignored
        solana_logger::setup();
        let mut candidates: ShrinkCandidates = HashMap::new();

        let common_store_path = Path::new("");
        let common_slot_id = 12;
        let store_file_size = 2 * PAGE_SIZE;

        let store1_id = 22;
        let store1 = Arc::new(AccountStorageEntry::new(
            common_store_path,
            common_slot_id,
            store1_id,
            store_file_size,
        ));
        store1.alive_bytes.store(0, Ordering::Release);

        candidates
            .entry(common_slot_id)
            .or_default()
            .insert(store1.append_vec_id(), store1.clone());

        let store2_id = 44;
        let store2 = Arc::new(AccountStorageEntry::new(
            common_store_path,
            common_slot_id,
            store2_id,
            store_file_size,
        ));

        // The store2's alive_ratio is 0.5: as its page aligned alive size is 1 page.
        let store2_alive_bytes = (PAGE_SIZE - 1) as usize;
        store2
            .alive_bytes
            .store(store2_alive_bytes, Ordering::Release);
        candidates
            .entry(common_slot_id)
            .or_default()
            .insert(store2.append_vec_id(), store2.clone());

        let store3_id = 55;
        let entry3 = Arc::new(AccountStorageEntry::new(
            common_store_path,
            common_slot_id,
            store3_id,
            store_file_size,
        ));

        // The store3's alive ratio is 1.0 as its page-aligned alive size is 2 pages
        let store3_alive_bytes = (PAGE_SIZE + 1) as usize;
        entry3
            .alive_bytes
            .store(store3_alive_bytes, Ordering::Release);

        candidates
            .entry(common_slot_id)
            .or_default()
            .insert(entry3.append_vec_id(), entry3.clone());

        // Set the target alive ratio to 0.6 so that we can just get rid of store1, the remaining two stores
        // alive ratio can be > the target ratio: the actual ratio is 0.75 because of 3 alive pages / 4 total pages.
        // The target ratio is also set to larger than store2's alive ratio: 0.5 so that it would be added
        // to the candidates list for next round.
        let target_alive_ratio = 0.6;
        let (selected_candidates, next_candidates) =
            AccountsDb::select_candidates_by_total_usage(&candidates, target_alive_ratio);
        assert_eq!(1, selected_candidates.len());
        assert_eq!(1, selected_candidates[&common_slot_id].len());
        assert!(selected_candidates[&common_slot_id].contains(&store1.append_vec_id()));
        assert_eq!(1, next_candidates.len());
        assert!(next_candidates[&common_slot_id].contains(&store2.append_vec_id()));
    }

    #[test]
    fn test_select_candidates_by_total_usage_2_way_split_condition() {
        // three candidates, 2 are selected for shrink, one is ignored
        solana_logger::setup();
        let mut candidates: ShrinkCandidates = HashMap::new();

        let common_store_path = Path::new("");
        let common_slot_id = 12;
        let store_file_size = 2 * PAGE_SIZE;

        let store1_id = 22;
        let store1 = Arc::new(AccountStorageEntry::new(
            common_store_path,
            common_slot_id,
            store1_id,
            store_file_size,
        ));
        store1.alive_bytes.store(0, Ordering::Release);

        candidates
            .entry(common_slot_id)
            .or_default()
            .insert(store1.append_vec_id(), store1.clone());

        let store2_id = 44;
        let store2 = Arc::new(AccountStorageEntry::new(
            common_store_path,
            common_slot_id,
            store2_id,
            store_file_size,
        ));

        // The store2's alive_ratio is 0.5: as its page aligned alive size is 1 page.
        let store2_alive_bytes = (PAGE_SIZE - 1) as usize;
        store2
            .alive_bytes
            .store(store2_alive_bytes, Ordering::Release);
        candidates
            .entry(common_slot_id)
            .or_default()
            .insert(store2.append_vec_id(), store2.clone());

        let store3_id = 55;
        let entry3 = Arc::new(AccountStorageEntry::new(
            common_store_path,
            common_slot_id,
            store3_id,
            store_file_size,
        ));

        // The store3's alive ratio is 1.0 as its page-aligned alive size is 2 pages
        let store3_alive_bytes = (PAGE_SIZE + 1) as usize;
        entry3
            .alive_bytes
            .store(store3_alive_bytes, Ordering::Release);

        candidates
            .entry(common_slot_id)
            .or_default()
            .insert(entry3.append_vec_id(), entry3.clone());

        // Set the target ratio to default (0.8), both store1 and store2 must be selected and store3 is ignored.
        let target_alive_ratio = DEFAULT_ACCOUNTS_SHRINK_RATIO;
        let (selected_candidates, next_candidates) =
            AccountsDb::select_candidates_by_total_usage(&candidates, target_alive_ratio);
        assert_eq!(1, selected_candidates.len());
        assert_eq!(2, selected_candidates[&common_slot_id].len());
        assert!(selected_candidates[&common_slot_id].contains(&store1.append_vec_id()));
        assert!(selected_candidates[&common_slot_id].contains(&store2.append_vec_id()));
        assert_eq!(0, next_candidates.len());
    }

    #[test]
    fn test_select_candidates_by_total_usage_all_clean() {
        // 2 candidates, they must be selected to achieve the target alive ratio
        solana_logger::setup();
        let mut candidates: ShrinkCandidates = HashMap::new();

        let slot1 = 12;
        let common_store_path = Path::new("");

        let store_file_size = 4 * PAGE_SIZE;
        let store1_id = 22;
        let store1 = Arc::new(AccountStorageEntry::new(
            common_store_path,
            slot1,
            store1_id,
            store_file_size,
        ));

        // store1 has 1 page-aligned alive bytes, its alive ratio is 1/4: 0.25
        let store1_alive_bytes = (PAGE_SIZE - 1) as usize;
        store1
            .alive_bytes
            .store(store1_alive_bytes, Ordering::Release);

        candidates
            .entry(slot1)
            .or_default()
            .insert(store1.append_vec_id(), store1.clone());

        let store2_id = 44;
        let slot2 = 44;
        let store2 = Arc::new(AccountStorageEntry::new(
            common_store_path,
            slot2,
            store2_id,
            store_file_size,
        ));

        // store2 has 2 page-aligned bytes, its alive ratio is 2/4: 0.5
        let store2_alive_bytes = (PAGE_SIZE + 1) as usize;
        store2
            .alive_bytes
            .store(store2_alive_bytes, Ordering::Release);

        candidates
            .entry(slot2)
            .or_default()
            .insert(store2.append_vec_id(), store2.clone());

        // Set the target ratio to default (0.8), both stores from the two different slots must be selected.
        let target_alive_ratio = DEFAULT_ACCOUNTS_SHRINK_RATIO;
        let (selected_candidates, next_candidates) =
            AccountsDb::select_candidates_by_total_usage(&candidates, target_alive_ratio);
        assert_eq!(2, selected_candidates.len());
        assert_eq!(1, selected_candidates[&slot1].len());
        assert_eq!(1, selected_candidates[&slot2].len());

        assert!(selected_candidates[&slot1].contains(&store1.append_vec_id()));
        assert!(selected_candidates[&slot2].contains(&store2.append_vec_id()));
        assert_eq!(0, next_candidates.len());
    }

    const UPSERT_POPULATE_RECLAIMS: UpsertReclaim = UpsertReclaim::PopulateReclaims;

    // returns the rooted entries and the storage ref count
    fn roots_and_ref_count<T: IndexValue>(
        index: &AccountsIndex<T>,
        locked_account_entry: &ReadAccountMapEntry<T>,
        max_inclusive: Option<Slot>,
    ) -> (SlotList<T>, RefCount) {
        (
            index.get_rooted_entries(locked_account_entry.slot_list(), max_inclusive),
            locked_account_entry.ref_count(),
        )
    }

    #[test]
    fn test_delete_dependencies() {
        solana_logger::setup();
        let accounts_index = AccountsIndex::default_for_tests();
        let key0 = Pubkey::new_from_array([0u8; 32]);
        let key1 = Pubkey::new_from_array([1u8; 32]);
        let key2 = Pubkey::new_from_array([2u8; 32]);
        let info0 = AccountInfo::new(StorageLocation::AppendVec(0, 0), 0, 0);
        let info1 = AccountInfo::new(StorageLocation::AppendVec(1, 0), 0, 0);
        let info2 = AccountInfo::new(StorageLocation::AppendVec(2, 0), 0, 0);
        let info3 = AccountInfo::new(StorageLocation::AppendVec(3, 0), 0, 0);
        let mut reclaims = vec![];
        accounts_index.upsert(
            0,
            0,
            &key0,
            &AccountSharedData::default(),
            &AccountSecondaryIndexes::default(),
            info0,
            &mut reclaims,
            UPSERT_POPULATE_RECLAIMS,
        );
        accounts_index.upsert(
            1,
            1,
            &key0,
            &AccountSharedData::default(),
            &AccountSecondaryIndexes::default(),
            info1,
            &mut reclaims,
            UPSERT_POPULATE_RECLAIMS,
        );
        accounts_index.upsert(
            1,
            1,
            &key1,
            &AccountSharedData::default(),
            &AccountSecondaryIndexes::default(),
            info1,
            &mut reclaims,
            UPSERT_POPULATE_RECLAIMS,
        );
        accounts_index.upsert(
            2,
            2,
            &key1,
            &AccountSharedData::default(),
            &AccountSecondaryIndexes::default(),
            info2,
            &mut reclaims,
            UPSERT_POPULATE_RECLAIMS,
        );
        accounts_index.upsert(
            2,
            2,
            &key2,
            &AccountSharedData::default(),
            &AccountSecondaryIndexes::default(),
            info2,
            &mut reclaims,
            UPSERT_POPULATE_RECLAIMS,
        );
        accounts_index.upsert(
            3,
            3,
            &key2,
            &AccountSharedData::default(),
            &AccountSecondaryIndexes::default(),
            info3,
            &mut reclaims,
            UPSERT_POPULATE_RECLAIMS,
        );
        accounts_index.add_root(0);
        accounts_index.add_root(1);
        accounts_index.add_root(2);
        accounts_index.add_root(3);
        let mut purges = HashMap::new();
        let (key0_entry, _) = accounts_index.get_for_tests(&key0, None, None).unwrap();
        purges.insert(
            key0,
            roots_and_ref_count(&accounts_index, &key0_entry, None),
        );
        let (key1_entry, _) = accounts_index.get_for_tests(&key1, None, None).unwrap();
        purges.insert(
            key1,
            roots_and_ref_count(&accounts_index, &key1_entry, None),
        );
        let (key2_entry, _) = accounts_index.get_for_tests(&key2, None, None).unwrap();
        purges.insert(
            key2,
            roots_and_ref_count(&accounts_index, &key2_entry, None),
        );
        for (key, (list, ref_count)) in &purges {
            info!(" purge {} ref_count {} =>", key, ref_count);
            for x in list {
                info!("  {:?}", x);
            }
        }

        let mut store_counts = HashMap::new();
        store_counts.insert(0, (0, HashSet::from_iter(vec![key0])));
        store_counts.insert(1, (0, HashSet::from_iter(vec![key0, key1])));
        store_counts.insert(2, (0, HashSet::from_iter(vec![key1, key2])));
        store_counts.insert(3, (1, HashSet::from_iter(vec![key2])));
        AccountsDb::calc_delete_dependencies(&purges, &mut store_counts, None);
        let mut stores: Vec<_> = store_counts.keys().cloned().collect();
        stores.sort_unstable();
        for store in &stores {
            info!(
                "store: {:?} : {:?}",
                store,
                store_counts.get(store).unwrap()
            );
        }
        for x in 0..3 {
            // if the store count doesn't exist for this id, then it is implied to be > 0
            assert!(store_counts
                .get(&x)
                .map(|entry| entry.0 >= 1)
                .unwrap_or(true));
        }
    }

    #[test]
    fn test_account_balance_for_capitalization_sysvar() {
        let normal_sysvar = solana_sdk::account::create_account_for_test(
            &solana_sdk::slot_history::SlotHistory::default(),
        );
        assert_eq!(normal_sysvar.lamports(), 1);
    }

    #[test]
    fn test_account_balance_for_capitalization_native_program() {
        let normal_native_program =
            solana_sdk::native_loader::create_loadable_account_for_test("foo");
        assert_eq!(normal_native_program.lamports(), 1);
    }

    #[test]
    fn test_checked_sum_for_capitalization_normal() {
        assert_eq!(
            AccountsDb::checked_sum_for_capitalization(vec![1, 2].into_iter()),
            3
        );
    }

    #[test]
    #[should_panic(expected = "overflow is detected while summing capitalization")]
    fn test_checked_sum_for_capitalization_overflow() {
        assert_eq!(
            AccountsDb::checked_sum_for_capitalization(vec![1, u64::max_value()].into_iter()),
            3
        );
    }

    #[test]
    fn test_store_overhead() {
        solana_logger::setup();
        let accounts = AccountsDb::new_single_for_tests();
        let account = AccountSharedData::default();
        let pubkey = solana_sdk::pubkey::new_rand();
        accounts.store_for_tests(0, &[(&pubkey, &account)]);
        accounts.add_root_and_flush_write_cache(0);
        let slot_stores = accounts.storage.get_slot_stores(0).unwrap();
        let mut total_len = 0;
        for (_id, store) in slot_stores.read().unwrap().iter() {
            total_len += store.accounts.len();
        }
        info!("total: {}", total_len);
        assert_eq!(total_len, STORE_META_OVERHEAD);
    }

    #[test]
    fn test_store_clean_after_shrink() {
        solana_logger::setup();
        let accounts = AccountsDb::new_with_config_for_tests(
            vec![],
            &ClusterType::Development,
            AccountSecondaryIndexes::default(),
            AccountShrinkThreshold::default(),
        );

        let account = AccountSharedData::new(1, 16 * 4096, &Pubkey::default());
        let pubkey1 = solana_sdk::pubkey::new_rand();
        accounts.store_cached((0, &[(&pubkey1, &account)][..]), None);

        let pubkey2 = solana_sdk::pubkey::new_rand();
        accounts.store_cached((0, &[(&pubkey2, &account)][..]), None);

        let zero_account = AccountSharedData::new(0, 1, &Pubkey::default());
        accounts.store_cached((1, &[(&pubkey1, &zero_account)][..]), None);

        // Add root 0 and flush separately
        accounts.get_accounts_delta_hash(0);
        accounts.add_root(0);
        accounts.flush_accounts_cache(true, None);

        // clear out the dirty keys
        accounts.clean_accounts_for_tests();

        // flush 1
        accounts.get_accounts_delta_hash(1);
        accounts.add_root(1);
        accounts.flush_accounts_cache(true, None);

        accounts.print_accounts_stats("pre-clean");

        // clean to remove pubkey1 from 0,
        // shrink to shrink pubkey1 from 0
        // then another clean to remove pubkey1 from slot 1
        accounts.clean_accounts_for_tests();

        accounts.shrink_candidate_slots();

        accounts.clean_accounts_for_tests();

        accounts.print_accounts_stats("post-clean");
        assert_eq!(accounts.accounts_index.ref_count_from_storage(&pubkey1), 0);
    }

    #[test]
    fn test_store_reuse() {
        solana_logger::setup();
        let accounts = AccountsDb::new_sized_caching(vec![], 4096);

        let size = 100;
        let num_accounts: usize = 100;
        let mut keys = Vec::new();
        for i in 0..num_accounts {
            let account = AccountSharedData::new((i + 1) as u64, size, &Pubkey::default());
            let pubkey = solana_sdk::pubkey::new_rand();
            accounts.store_cached((0 as Slot, &[(&pubkey, &account)][..]), None);
            keys.push(pubkey);
        }
        // get delta hash to feed these accounts to clean
        accounts.get_accounts_delta_hash(0);
        accounts.add_root(0);
        // we have to flush just slot 0
        // if we slot 0 and 1 together, then they are cleaned and slot 0 doesn't contain the accounts
        // this test wants to clean and then allow us to shrink
        accounts.flush_accounts_cache(true, None);

        for (i, key) in keys[1..].iter().enumerate() {
            let account =
                AccountSharedData::new((1 + i + num_accounts) as u64, size, &Pubkey::default());
            accounts.store_cached((1 as Slot, &[(key, &account)][..]), None);
        }
        accounts.get_accounts_delta_hash(1);
        accounts.add_root(1);
        accounts.flush_accounts_cache(true, None);
        accounts.clean_accounts_for_tests();
        accounts.shrink_all_slots(false, None);

        // Clean again to flush the dirty stores
        // and allow them to be recycled in the next step
        accounts.clean_accounts_for_tests();
        accounts.print_accounts_stats("post-shrink");
        let num_stores = accounts.recycle_stores.read().unwrap().entry_count();
        assert!(num_stores > 0);

        let mut account_refs = Vec::new();
        let num_to_store = 20;
        for (i, key) in keys[..num_to_store].iter().enumerate() {
            let account = AccountSharedData::new(
                (1 + i + 2 * num_accounts) as u64,
                i + 20,
                &Pubkey::default(),
            );
            accounts.store_uncached(2, &[(key, &account)]);
            account_refs.push(account);
        }
        assert!(accounts.recycle_stores.read().unwrap().entry_count() < num_stores);

        accounts.print_accounts_stats("post-store");

        let mut ancestors = Ancestors::default();
        ancestors.insert(1, 0);
        ancestors.insert(2, 1);
        for (key, account_ref) in keys[..num_to_store].iter().zip(account_refs) {
            assert_eq!(
                accounts.load_without_fixed_root(&ancestors, key).unwrap().0,
                account_ref
            );
        }
    }

    #[test]
    #[should_panic(expected = "We've run out of storage ids!")]
    fn test_wrapping_append_vec_id() {
        let db = AccountsDb::new(Vec::new(), &ClusterType::Development);

        let zero_lamport_account =
            AccountSharedData::new(0, 0, AccountSharedData::default().owner());

        // set 'next' id to the max possible value
        db.next_id.store(AppendVecId::MAX, Ordering::Release);
        let slots = 3;
        let keys = (0..slots).map(|_| Pubkey::new_unique()).collect::<Vec<_>>();
        // write unique keys to successive slots
        keys.iter().enumerate().for_each(|(slot, key)| {
            let slot = slot as Slot;
            db.store_for_tests(slot, &[(key, &zero_lamport_account)]);
            db.get_accounts_delta_hash(slot);
            db.add_root_and_flush_write_cache(slot);
        });
        assert_eq!(slots - 1, db.next_id.load(Ordering::Acquire));
        let ancestors = Ancestors::default();
        keys.iter().for_each(|key| {
            assert!(db.load_without_fixed_root(&ancestors, key).is_some());
        });
    }

    #[test]
    #[should_panic(expected = "We've run out of storage ids!")]
    fn test_reuse_append_vec_id() {
        solana_logger::setup();
        let db = AccountsDb::new(Vec::new(), &ClusterType::Development);

        let zero_lamport_account =
            AccountSharedData::new(0, 0, AccountSharedData::default().owner());

        // set 'next' id to the max possible value
        db.next_id.store(AppendVecId::MAX, Ordering::Release);
        let slots = 3;
        let keys = (0..slots).map(|_| Pubkey::new_unique()).collect::<Vec<_>>();
        // write unique keys to successive slots
        keys.iter().enumerate().for_each(|(slot, key)| {
            let slot = slot as Slot;
            db.store_for_tests(slot, &[(key, &zero_lamport_account)]);
            db.get_accounts_delta_hash(slot);
            db.add_root_and_flush_write_cache(slot);
            // reset next_id to what it was previously to cause us to re-use the same id
            db.next_id.store(AppendVecId::MAX, Ordering::Release);
        });
        let ancestors = Ancestors::default();
        keys.iter().for_each(|key| {
            assert!(db.load_without_fixed_root(&ancestors, key).is_some());
        });
    }

    #[test]
    fn test_zero_lamport_new_root_not_cleaned() {
        let db = AccountsDb::new(Vec::new(), &ClusterType::Development);
        let account_key = Pubkey::new_unique();
        let zero_lamport_account =
            AccountSharedData::new(0, 0, AccountSharedData::default().owner());

        // Store zero lamport account into slots 0 and 1, root both slots
        db.store_for_tests(0, &[(&account_key, &zero_lamport_account)]);
        db.store_for_tests(1, &[(&account_key, &zero_lamport_account)]);
        db.get_accounts_delta_hash(0);
        db.add_root_and_flush_write_cache(0);
        db.get_accounts_delta_hash(1);
        db.add_root_and_flush_write_cache(1);

        // Only clean zero lamport accounts up to slot 0
        db.clean_accounts(Some(0), false, None);

        // Should still be able to find zero lamport account in slot 1
        assert_eq!(
            db.load_without_fixed_root(&Ancestors::default(), &account_key),
            Some((zero_lamport_account, 1))
        );
    }

    #[test]
    fn test_store_load_cached() {
        let db = AccountsDb::new(Vec::new(), &ClusterType::Development);
        let key = Pubkey::default();
        let account0 = AccountSharedData::new(1, 0, &key);
        let slot = 0;
        db.store_cached((slot, &[(&key, &account0)][..]), None);

        // Load with no ancestors and no root will return nothing
        assert!(db
            .load_without_fixed_root(&Ancestors::default(), &key)
            .is_none());

        // Load with ancestors not equal to `slot` will return nothing
        let ancestors = vec![(slot + 1, 1)].into_iter().collect();
        assert!(db.load_without_fixed_root(&ancestors, &key).is_none());

        // Load with ancestors equal to `slot` will return the account
        let ancestors = vec![(slot, 1)].into_iter().collect();
        assert_eq!(
            db.load_without_fixed_root(&ancestors, &key),
            Some((account0.clone(), slot))
        );

        // Adding root will return the account even without ancestors
        db.add_root(slot);
        assert_eq!(
            db.load_without_fixed_root(&Ancestors::default(), &key),
            Some((account0, slot))
        );
    }

    #[test]
    fn test_store_flush_load_cached() {
        let db = AccountsDb::new(Vec::new(), &ClusterType::Development);
        let key = Pubkey::default();
        let account0 = AccountSharedData::new(1, 0, &key);
        let slot = 0;
        db.store_cached((slot, &[(&key, &account0)][..]), None);
        db.mark_slot_frozen(slot);

        // No root was added yet, requires an ancestor to find
        // the account
        db.flush_accounts_cache(true, None);
        let ancestors = vec![(slot, 1)].into_iter().collect();
        assert_eq!(
            db.load_without_fixed_root(&ancestors, &key),
            Some((account0.clone(), slot))
        );

        // Add root then flush
        db.add_root(slot);
        db.flush_accounts_cache(true, None);
        assert_eq!(
            db.load_without_fixed_root(&Ancestors::default(), &key),
            Some((account0, slot))
        );
    }

    #[test]
    fn test_flush_accounts_cache() {
        let db = AccountsDb::new(Vec::new(), &ClusterType::Development);
        let account0 = AccountSharedData::new(1, 0, &Pubkey::default());

        let unrooted_slot = 4;
        let root5 = 5;
        let root6 = 6;
        let unrooted_key = solana_sdk::pubkey::new_rand();
        let key5 = solana_sdk::pubkey::new_rand();
        let key6 = solana_sdk::pubkey::new_rand();
        db.store_cached((unrooted_slot, &[(&unrooted_key, &account0)][..]), None);
        db.store_cached((root5, &[(&key5, &account0)][..]), None);
        db.store_cached((root6, &[(&key6, &account0)][..]), None);
        for slot in &[unrooted_slot, root5, root6] {
            db.mark_slot_frozen(*slot);
        }
        db.add_root(root5);
        db.add_root(root6);

        // Unrooted slot should be able to be fetched before the flush
        let ancestors = vec![(unrooted_slot, 1)].into_iter().collect();
        assert_eq!(
            db.load_without_fixed_root(&ancestors, &unrooted_key),
            Some((account0.clone(), unrooted_slot))
        );
        db.flush_accounts_cache(true, None);

        // After the flush, the unrooted slot is still in the cache
        assert!(db
            .load_without_fixed_root(&ancestors, &unrooted_key)
            .is_some());
        assert!(db
            .accounts_index
            .get_account_read_entry(&unrooted_key)
            .is_some());
        assert_eq!(db.accounts_cache.num_slots(), 1);
        assert!(db.accounts_cache.slot_cache(unrooted_slot).is_some());
        assert_eq!(
            db.load_without_fixed_root(&Ancestors::default(), &key5),
            Some((account0.clone(), root5))
        );
        assert_eq!(
            db.load_without_fixed_root(&Ancestors::default(), &key6),
            Some((account0, root6))
        );
    }

    fn max_cache_slots() -> usize {
        // this used to be the limiting factor - used here to facilitate tests.
        200
    }

    #[test]
    fn test_flush_accounts_cache_if_needed() {
        run_test_flush_accounts_cache_if_needed(0, 2 * max_cache_slots());
        run_test_flush_accounts_cache_if_needed(2 * max_cache_slots(), 0);
        run_test_flush_accounts_cache_if_needed(max_cache_slots() - 1, 0);
        run_test_flush_accounts_cache_if_needed(0, max_cache_slots() - 1);
        run_test_flush_accounts_cache_if_needed(max_cache_slots(), 0);
        run_test_flush_accounts_cache_if_needed(0, max_cache_slots());
        run_test_flush_accounts_cache_if_needed(2 * max_cache_slots(), 2 * max_cache_slots());
        run_test_flush_accounts_cache_if_needed(max_cache_slots() - 1, max_cache_slots() - 1);
        run_test_flush_accounts_cache_if_needed(max_cache_slots(), max_cache_slots());
    }

    fn run_test_flush_accounts_cache_if_needed(num_roots: usize, num_unrooted: usize) {
        let mut db = AccountsDb::new(Vec::new(), &ClusterType::Development);
        db.write_cache_limit_bytes = Some(max_cache_slots() as u64);
        let space = 1; // # data bytes per account. write cache counts data len
        let account0 = AccountSharedData::new(1, space, &Pubkey::default());
        let mut keys = vec![];
        let num_slots = 2 * max_cache_slots();
        for i in 0..num_roots + num_unrooted {
            let key = Pubkey::new_unique();
            db.store_cached((i as Slot, &[(&key, &account0)][..]), None);
            keys.push(key);
            db.mark_slot_frozen(i as Slot);
            if i < num_roots {
                db.add_root(i as Slot);
            }
        }

        db.flush_accounts_cache(false, None);

        let total_slots = num_roots + num_unrooted;
        // If there's <= the max size, then nothing will be flushed from the slot
        if total_slots <= max_cache_slots() {
            assert_eq!(db.accounts_cache.num_slots(), total_slots);
        } else {
            // Otherwise, all the roots are flushed, and only at most max_cache_slots()
            // of the unrooted slots are kept in the cache
            let expected_size = std::cmp::min(num_unrooted, max_cache_slots());
            if expected_size > 0 {
                // +1: slot is 1-based. slot 1 has 1 byte of data
                for unrooted_slot in (total_slots - expected_size + 1)..total_slots {
                    assert!(
                        db.accounts_cache
                            .slot_cache(unrooted_slot as Slot)
                            .is_some(),
                        "unrooted_slot: {unrooted_slot}, total_slots: {total_slots}, expected_size: {expected_size}"
                    );
                }
            }
        }

        // Should still be able to fetch all the accounts after flush
        for (slot, key) in (0..num_slots as Slot).zip(keys) {
            let ancestors = if slot < num_roots as Slot {
                Ancestors::default()
            } else {
                vec![(slot, 1)].into_iter().collect()
            };
            assert_eq!(
                db.load_without_fixed_root(&ancestors, &key),
                Some((account0.clone(), slot))
            );
        }
    }

    fn slot_stores(db: &AccountsDb, slot: Slot) -> SnapshotStorage {
        db.get_storages_for_slot(slot).unwrap_or_default()
    }

    #[test]
    fn test_read_only_accounts_cache() {
        let db = Arc::new(AccountsDb::new_with_config_for_tests(
            Vec::new(),
            &ClusterType::Development,
            AccountSecondaryIndexes::default(),
            AccountShrinkThreshold::default(),
        ));

        let account_key = Pubkey::new_unique();
        let zero_lamport_account =
            AccountSharedData::new(0, 0, AccountSharedData::default().owner());
        let slot1_account = AccountSharedData::new(1, 1, AccountSharedData::default().owner());
        db.store_cached((0, &[(&account_key, &zero_lamport_account)][..]), None);
        db.store_cached((1, &[(&account_key, &slot1_account)][..]), None);

        db.add_root(0);
        db.add_root(1);
        db.clean_accounts_for_tests();
        db.flush_accounts_cache(true, None);
        db.clean_accounts_for_tests();
        db.add_root(2);

        assert_eq!(db.read_only_accounts_cache.cache_len(), 0);
        let account = db
            .load_with_fixed_root(&Ancestors::default(), &account_key)
            .map(|(account, _)| account)
            .unwrap();
        assert_eq!(account.lamports(), 1);
        assert_eq!(db.read_only_accounts_cache.cache_len(), 1);
        let account = db
            .load_with_fixed_root(&Ancestors::default(), &account_key)
            .map(|(account, _)| account)
            .unwrap();
        assert_eq!(account.lamports(), 1);
        assert_eq!(db.read_only_accounts_cache.cache_len(), 1);
        db.store_cached((2, &[(&account_key, &zero_lamport_account)][..]), None);
        assert_eq!(db.read_only_accounts_cache.cache_len(), 1);
        let account = db
            .load_with_fixed_root(&Ancestors::default(), &account_key)
            .map(|(account, _)| account);
        assert!(account.is_none());
        assert_eq!(db.read_only_accounts_cache.cache_len(), 1);
    }

    /// a test that will accept either answer
    const LOAD_ZERO_LAMPORTS_ANY_TESTS: LoadZeroLamports = LoadZeroLamports::None;

    #[test]
    fn test_flush_cache_clean() {
        let db = Arc::new(AccountsDb::new_with_config_for_tests(
            Vec::new(),
            &ClusterType::Development,
            AccountSecondaryIndexes::default(),
            AccountShrinkThreshold::default(),
        ));

        let account_key = Pubkey::new_unique();
        let zero_lamport_account =
            AccountSharedData::new(0, 0, AccountSharedData::default().owner());
        let slot1_account = AccountSharedData::new(1, 1, AccountSharedData::default().owner());
        db.store_cached((0, &[(&account_key, &zero_lamport_account)][..]), None);
        db.store_cached((1, &[(&account_key, &slot1_account)][..]), None);

        db.add_root(0);
        db.add_root(1);

        // Clean should not remove anything yet as nothing has been flushed
        db.clean_accounts_for_tests();
        let account = db
            .do_load(
                &Ancestors::default(),
                &account_key,
                Some(0),
                LoadHint::Unspecified,
                LoadZeroLamports::SomeWithZeroLamportAccountForTests,
            )
            .unwrap();
        assert_eq!(account.0.lamports(), 0);
        // since this item is in the cache, it should not be in the read only cache
        assert_eq!(db.read_only_accounts_cache.cache_len(), 0);

        // Flush, then clean again. Should not need another root to initiate the cleaning
        // because `accounts_index.uncleaned_roots` should be correct
        db.flush_accounts_cache(true, None);
        db.clean_accounts_for_tests();
        assert!(db
            .do_load(
                &Ancestors::default(),
                &account_key,
                Some(0),
                LoadHint::Unspecified,
                LOAD_ZERO_LAMPORTS_ANY_TESTS
            )
            .is_none());
    }

    #[test]
    fn test_flush_cache_dont_clean_zero_lamport_account() {
        let db = Arc::new(AccountsDb::new_with_config_for_tests(
            Vec::new(),
            &ClusterType::Development,
            AccountSecondaryIndexes::default(),
            AccountShrinkThreshold::default(),
        ));

        let zero_lamport_account_key = Pubkey::new_unique();
        let other_account_key = Pubkey::new_unique();

        let original_lamports = 1;
        let slot0_account =
            AccountSharedData::new(original_lamports, 1, AccountSharedData::default().owner());
        let zero_lamport_account =
            AccountSharedData::new(0, 0, AccountSharedData::default().owner());

        // Store into slot 0, and then flush the slot to storage
        db.store_cached(
            (0, &[(&zero_lamport_account_key, &slot0_account)][..]),
            None,
        );
        // Second key keeps other lamport account entry for slot 0 alive,
        // preventing clean of the zero_lamport_account in slot 1.
        db.store_cached((0, &[(&other_account_key, &slot0_account)][..]), None);
        db.add_root(0);
        db.flush_accounts_cache(true, None);
        assert!(!db.storage.is_empty(0));

        // Store into slot 1, a dummy slot that will be dead and purged before flush
        db.store_cached(
            (1, &[(&zero_lamport_account_key, &zero_lamport_account)][..]),
            None,
        );

        // Store into slot 2, which makes all updates from slot 1 outdated.
        // This means slot 1 is a dead slot. Later, slot 1 will be cleaned/purged
        // before it even reaches storage, but this purge of slot 1should not affect
        // the refcount of `zero_lamport_account_key` because cached keys do not bump
        // the refcount in the index. This means clean should *not* remove
        // `zero_lamport_account_key` from slot 2
        db.store_cached(
            (2, &[(&zero_lamport_account_key, &zero_lamport_account)][..]),
            None,
        );
        db.add_root(1);
        db.add_root(2);

        // Flush, then clean. Should not need another root to initiate the cleaning
        // because `accounts_index.uncleaned_roots` should be correct
        db.flush_accounts_cache(true, None);
        db.clean_accounts_for_tests();

        // The `zero_lamport_account_key` is still alive in slot 1, so refcount for the
        // pubkey should be 2
        assert_eq!(
            db.accounts_index
                .ref_count_from_storage(&zero_lamport_account_key),
            2
        );
        assert_eq!(
            db.accounts_index.ref_count_from_storage(&other_account_key),
            1
        );

        // The zero-lamport account in slot 2 should not be purged yet, because the
        // entry in slot 1 is blocking cleanup of the zero-lamport account.
        let max_root = None;
        // Fine to simulate a transaction load since we are not doing any out of band
        // removals, only using clean_accounts
        let load_hint = LoadHint::FixedMaxRoot;
        assert_eq!(
            db.do_load(
                &Ancestors::default(),
                &zero_lamport_account_key,
                max_root,
                load_hint,
                LoadZeroLamports::SomeWithZeroLamportAccountForTests,
            )
            .unwrap()
            .0
            .lamports(),
            0
        );
    }

    struct ScanTracker {
        t_scan: JoinHandle<()>,
        exit: Arc<AtomicBool>,
    }

    impl ScanTracker {
        fn exit(self) -> thread::Result<()> {
            self.exit.store(true, Ordering::Relaxed);
            self.t_scan.join()
        }
    }

    fn setup_scan(
        db: Arc<AccountsDb>,
        scan_ancestors: Arc<Ancestors>,
        bank_id: BankId,
        stall_key: Pubkey,
    ) -> ScanTracker {
        let exit = Arc::new(AtomicBool::new(false));
        let exit_ = exit.clone();
        let ready = Arc::new(AtomicBool::new(false));
        let ready_ = ready.clone();

        let t_scan = Builder::new()
            .name("scan".to_string())
            .spawn(move || {
                db.scan_accounts(
                    &scan_ancestors,
                    bank_id,
                    |maybe_account| {
                        ready_.store(true, Ordering::Relaxed);
                        if let Some((pubkey, _, _)) = maybe_account {
                            if *pubkey == stall_key {
                                loop {
                                    if exit_.load(Ordering::Relaxed) {
                                        break;
                                    } else {
                                        sleep(Duration::from_millis(10));
                                    }
                                }
                            }
                        }
                    },
                    &ScanConfig::default(),
                )
                .unwrap();
            })
            .unwrap();

        // Wait for scan to start
        while !ready.load(Ordering::Relaxed) {
            sleep(Duration::from_millis(10));
        }

        ScanTracker { t_scan, exit }
    }

    #[test]
    fn test_scan_flush_accounts_cache_then_clean_drop() {
        let db = Arc::new(AccountsDb::new_with_config_for_tests(
            Vec::new(),
            &ClusterType::Development,
            AccountSecondaryIndexes::default(),
            AccountShrinkThreshold::default(),
        ));
        let account_key = Pubkey::new_unique();
        let account_key2 = Pubkey::new_unique();
        let zero_lamport_account =
            AccountSharedData::new(0, 0, AccountSharedData::default().owner());
        let slot1_account = AccountSharedData::new(1, 1, AccountSharedData::default().owner());
        let slot2_account = AccountSharedData::new(2, 1, AccountSharedData::default().owner());

        /*
            Store zero lamport account into slots 0, 1, 2 where
            root slots are 0, 2, and slot 1 is unrooted.
                                    0 (root)
                                /        \
                              1            2 (root)
        */
        db.store_cached((0, &[(&account_key, &zero_lamport_account)][..]), None);
        db.store_cached((1, &[(&account_key, &slot1_account)][..]), None);
        // Fodder for the scan so that the lock on `account_key` is not held
        db.store_cached((1, &[(&account_key2, &slot1_account)][..]), None);
        db.store_cached((2, &[(&account_key, &slot2_account)][..]), None);
        db.get_accounts_delta_hash(0);

        let max_scan_root = 0;
        db.add_root(max_scan_root);
        let scan_ancestors: Arc<Ancestors> = Arc::new(vec![(0, 1), (1, 1)].into_iter().collect());
        let bank_id = 0;
        let scan_tracker = setup_scan(db.clone(), scan_ancestors.clone(), bank_id, account_key2);

        // Add a new root 2
        let new_root = 2;
        db.get_accounts_delta_hash(new_root);
        db.add_root(new_root);

        // Check that the scan is properly set up
        assert_eq!(
            db.accounts_index.min_ongoing_scan_root().unwrap(),
            max_scan_root
        );

        // If we specify a requested_flush_root == 2, then `slot 2 <= max_flush_slot` will
        // be flushed even though `slot 2 > max_scan_root`. The unrooted slot 1 should
        // remain in the cache
        db.flush_accounts_cache(true, Some(new_root));
        assert_eq!(db.accounts_cache.num_slots(), 1);
        assert!(db.accounts_cache.slot_cache(1).is_some());

        // Intra cache cleaning should not clean the entry for `account_key` from slot 0,
        // even though it was updated in slot `2` because of the ongoing scan
        let account = db
            .do_load(
                &Ancestors::default(),
                &account_key,
                Some(0),
                LoadHint::Unspecified,
                LoadZeroLamports::SomeWithZeroLamportAccountForTests,
            )
            .unwrap();
        assert_eq!(account.0.lamports(), zero_lamport_account.lamports());

        // Run clean, unrooted slot 1 should not be purged, and still readable from the cache,
        // because we're still doing a scan on it.
        db.clean_accounts_for_tests();
        let account = db
            .do_load(
                &scan_ancestors,
                &account_key,
                Some(max_scan_root),
                LoadHint::Unspecified,
                LOAD_ZERO_LAMPORTS_ANY_TESTS,
            )
            .unwrap();
        assert_eq!(account.0.lamports(), slot1_account.lamports());

        // When the scan is over, clean should not panic and should not purge something
        // still in the cache.
        scan_tracker.exit().unwrap();
        db.clean_accounts_for_tests();
        let account = db
            .do_load(
                &scan_ancestors,
                &account_key,
                Some(max_scan_root),
                LoadHint::Unspecified,
                LOAD_ZERO_LAMPORTS_ANY_TESTS,
            )
            .unwrap();
        assert_eq!(account.0.lamports(), slot1_account.lamports());

        // Simulate dropping the bank, which finally removes the slot from the cache
        let bank_id = 1;
        db.purge_slot(1, bank_id, false);
        assert!(db
            .do_load(
                &scan_ancestors,
                &account_key,
                Some(max_scan_root),
                LoadHint::Unspecified,
                LOAD_ZERO_LAMPORTS_ANY_TESTS
            )
            .is_none());
    }

    impl AccountsDb {
        fn get_and_assert_single_storage(&self, slot: Slot) -> Arc<AccountStorageEntry> {
            let mut storage_maps: SnapshotStorage =
                self.get_storages_for_slot(slot).unwrap_or_default();

            assert_eq!(storage_maps.len(), 1);
            storage_maps.pop().unwrap()
        }
    }

    #[test]
    fn test_alive_bytes() {
        let accounts_db = AccountsDb::new_with_config_for_tests(
            Vec::new(),
            &ClusterType::Development,
            AccountSecondaryIndexes::default(),
            AccountShrinkThreshold::default(),
        );
        let slot: Slot = 0;
        let num_keys = 10;

        for data_size in 0..num_keys {
            let account = AccountSharedData::new(1, data_size, &Pubkey::default());
            accounts_db.store_cached((slot, &[(&Pubkey::new_unique(), &account)][..]), None);
        }

        accounts_db.add_root(slot);
        accounts_db.flush_accounts_cache(true, None);

        // Flushing cache should only create one storage entry
        let storage0 = accounts_db.get_and_assert_single_storage(slot);
        let accounts = storage0.all_accounts();

        for account in accounts {
            let before_size = storage0.alive_bytes.load(Ordering::Acquire);
            let account_info = accounts_db
                .accounts_index
                .get_account_read_entry(account.pubkey())
                .map(|locked_entry| {
                    // Should only be one entry per key, since every key was only stored to slot 0
                    locked_entry.slot_list()[0]
                })
                .unwrap();
            let removed_data_size = account_info.1.stored_size();
            // Fetching the account from storage should return the same
            // stored size as in the index.
            assert_eq!(removed_data_size, account.stored_size as StoredSize);
            assert_eq!(account_info.0, slot);
            let reclaims = vec![account_info];
            accounts_db.remove_dead_accounts(reclaims.iter(), None, None, true);
            let after_size = storage0.alive_bytes.load(Ordering::Acquire);
            assert_eq!(before_size, after_size + account.stored_size);
        }
    }

    fn setup_accounts_db_cache_clean(
        num_slots: usize,
        scan_slot: Option<Slot>,
        write_cache_limit_bytes: Option<u64>,
    ) -> (Arc<AccountsDb>, Vec<Pubkey>, Vec<Slot>, Option<ScanTracker>) {
        let mut accounts_db = AccountsDb::new_with_config_for_tests(
            Vec::new(),
            &ClusterType::Development,
            AccountSecondaryIndexes::default(),
            AccountShrinkThreshold::default(),
        );
        accounts_db.write_cache_limit_bytes = write_cache_limit_bytes;
        let accounts_db = Arc::new(accounts_db);

        let slots: Vec<_> = (0..num_slots as Slot).collect();
        let stall_slot = num_slots as Slot;
        let scan_stall_key = Pubkey::new_unique();
        let keys: Vec<Pubkey> = std::iter::repeat_with(Pubkey::new_unique)
            .take(num_slots)
            .collect();
        if scan_slot.is_some() {
            accounts_db.store_cached(
                // Store it in a slot that isn't returned in `slots`
                (
                    stall_slot,
                    &[(
                        &scan_stall_key,
                        &AccountSharedData::new(1, 0, &Pubkey::default()),
                    )][..],
                ),
                None,
            );
        }

        // Store some subset of the keys in slots 0..num_slots
        let mut scan_tracker = None;
        for slot in &slots {
            for key in &keys[*slot as usize..] {
                let space = 1; // 1 byte allows us to track by size
                accounts_db.store_cached(
                    (
                        *slot,
                        &[(key, &AccountSharedData::new(1, space, &Pubkey::default()))][..],
                    ),
                    None,
                );
            }
            accounts_db.add_root(*slot as Slot);
            if Some(*slot) == scan_slot {
                let ancestors = Arc::new(vec![(stall_slot, 1), (*slot, 1)].into_iter().collect());
                let bank_id = 0;
                scan_tracker = Some(setup_scan(
                    accounts_db.clone(),
                    ancestors,
                    bank_id,
                    scan_stall_key,
                ));
                assert_eq!(
                    accounts_db.accounts_index.min_ongoing_scan_root().unwrap(),
                    *slot
                );
            }
        }

        accounts_db.accounts_cache.remove_slot(stall_slot);

        // If there's <= max_cache_slots(), no slots should be flushed
        if accounts_db.accounts_cache.num_slots() <= max_cache_slots() {
            accounts_db.flush_accounts_cache(false, None);
            assert_eq!(accounts_db.accounts_cache.num_slots(), num_slots);
        }

        (accounts_db, keys, slots, scan_tracker)
    }

    #[test]
    fn test_accounts_db_cache_clean_dead_slots() {
        let num_slots = 10;
        let (accounts_db, keys, mut slots, _) =
            setup_accounts_db_cache_clean(num_slots, None, None);
        let last_dead_slot = (num_slots - 1) as Slot;
        assert_eq!(*slots.last().unwrap(), last_dead_slot);
        let alive_slot = last_dead_slot as Slot + 1;
        slots.push(alive_slot);
        for key in &keys {
            // Store a slot that overwrites all previous keys, rendering all previous keys dead
            accounts_db.store_cached(
                (
                    alive_slot,
                    &[(key, &AccountSharedData::new(1, 0, &Pubkey::default()))][..],
                ),
                None,
            );
            accounts_db.add_root(alive_slot);
        }

        // Before the flush, we can find entries in the database for slots < alive_slot if we specify
        // a smaller max root
        for key in &keys {
            assert!(accounts_db
                .do_load(
                    &Ancestors::default(),
                    key,
                    Some(last_dead_slot),
                    LoadHint::Unspecified,
                    LOAD_ZERO_LAMPORTS_ANY_TESTS
                )
                .is_some());
        }

        // If no `max_clean_root` is specified, cleaning should purge all flushed slots
        accounts_db.flush_accounts_cache(true, None);
        assert_eq!(accounts_db.accounts_cache.num_slots(), 0);
        let mut uncleaned_roots = accounts_db
            .accounts_index
            .clear_uncleaned_roots(None)
            .into_iter()
            .collect::<Vec<_>>();
        uncleaned_roots.sort_unstable();
        assert_eq!(uncleaned_roots, slots);
        assert_eq!(
            accounts_db.accounts_cache.fetch_max_flush_root(),
            alive_slot,
        );

        // Specifying a max_root < alive_slot, should not return any more entries,
        // as those have been purged from the accounts index for the dead slots.
        for key in &keys {
            assert!(accounts_db
                .do_load(
                    &Ancestors::default(),
                    key,
                    Some(last_dead_slot),
                    LoadHint::Unspecified,
                    LOAD_ZERO_LAMPORTS_ANY_TESTS
                )
                .is_none());
        }
        // Each slot should only have one entry in the storage, since all other accounts were
        // cleaned due to later updates
        for slot in &slots {
            if let ScanStorageResult::Stored(slot_accounts) = accounts_db.scan_account_storage(
                *slot as Slot,
                |_| Some(0),
                |slot_accounts: &DashSet<Pubkey>, loaded_account: LoadedAccount| {
                    slot_accounts.insert(*loaded_account.pubkey());
                },
            ) {
                if *slot == alive_slot {
                    assert_eq!(slot_accounts.len(), keys.len());
                } else {
                    assert!(slot_accounts.is_empty());
                }
            } else {
                panic!("Expected slot to be in storage, not cache");
            }
        }
    }

    #[test]
    fn test_accounts_db_cache_clean() {
        let (accounts_db, keys, slots, _) = setup_accounts_db_cache_clean(10, None, None);

        // If no `max_clean_root` is specified, cleaning should purge all flushed slots
        accounts_db.flush_accounts_cache(true, None);
        assert_eq!(accounts_db.accounts_cache.num_slots(), 0);
        let mut uncleaned_roots = accounts_db
            .accounts_index
            .clear_uncleaned_roots(None)
            .into_iter()
            .collect::<Vec<_>>();
        uncleaned_roots.sort_unstable();
        assert_eq!(uncleaned_roots, slots);
        assert_eq!(
            accounts_db.accounts_cache.fetch_max_flush_root(),
            *slots.last().unwrap()
        );

        // Each slot should only have one entry in the storage, since all other accounts were
        // cleaned due to later updates
        for slot in &slots {
            if let ScanStorageResult::Stored(slot_account) = accounts_db.scan_account_storage(
                *slot as Slot,
                |_| Some(0),
                |slot_account: &Arc<RwLock<Pubkey>>, loaded_account: LoadedAccount| {
                    *slot_account.write().unwrap() = *loaded_account.pubkey();
                },
            ) {
                assert_eq!(*slot_account.read().unwrap(), keys[*slot as usize]);
            } else {
                panic!("Everything should have been flushed")
            }
        }
    }

    fn run_test_accounts_db_cache_clean_max_root(
        num_slots: usize,
        requested_flush_root: Slot,
        scan_root: Option<Slot>,
    ) {
        assert!(requested_flush_root < (num_slots as Slot));
        let (accounts_db, keys, slots, scan_tracker) =
            setup_accounts_db_cache_clean(num_slots, scan_root, Some(max_cache_slots() as u64));
        let is_cache_at_limit = num_slots - requested_flush_root as usize - 1 > max_cache_slots();

        // If:
        // 1) `requested_flush_root` is specified,
        // 2) not at the cache limit, i.e. `is_cache_at_limit == false`, then
        // `flush_accounts_cache()` should clean and flush only slots <= requested_flush_root,
        accounts_db.flush_accounts_cache(true, Some(requested_flush_root));

        if !is_cache_at_limit {
            // Should flush all slots between 0..=requested_flush_root
            assert_eq!(
                accounts_db.accounts_cache.num_slots(),
                slots.len() - requested_flush_root as usize - 1
            );
        } else {
            // Otherwise, if we are at the cache limit, all roots will be flushed
            assert_eq!(accounts_db.accounts_cache.num_slots(), 0,);
        }

        let mut uncleaned_roots = accounts_db
            .accounts_index
            .clear_uncleaned_roots(None)
            .into_iter()
            .collect::<Vec<_>>();
        uncleaned_roots.sort_unstable();

        let expected_max_flushed_root = if !is_cache_at_limit {
            // Should flush all slots between 0..=requested_flush_root
            requested_flush_root
        } else {
            // Otherwise, if we are at the cache limit, all roots will be flushed
            num_slots as Slot - 1
        };

        assert_eq!(
            uncleaned_roots,
            slots[0..=expected_max_flushed_root as usize].to_vec()
        );
        assert_eq!(
            accounts_db.accounts_cache.fetch_max_flush_root(),
            expected_max_flushed_root,
        );

        for slot in &slots {
            let slot_accounts = accounts_db.scan_account_storage(
                *slot as Slot,
                |loaded_account: LoadedAccount| {
                    assert!(
                        !is_cache_at_limit,
                        "When cache is at limit, all roots should have been flushed to storage"
                    );
                    // All slots <= requested_flush_root should have been flushed, regardless
                    // of ongoing scans
                    assert!(*slot > requested_flush_root);
                    Some(*loaded_account.pubkey())
                },
                |slot_accounts: &DashSet<Pubkey>, loaded_account: LoadedAccount| {
                    slot_accounts.insert(*loaded_account.pubkey());
                    if !is_cache_at_limit {
                        // Only true when the limit hasn't been reached and there are still
                        // slots left in the cache
                        assert!(*slot <= requested_flush_root);
                    }
                },
            );

            let slot_accounts = match slot_accounts {
                ScanStorageResult::Cached(slot_accounts) => {
                    slot_accounts.into_iter().collect::<HashSet<Pubkey>>()
                }
                ScanStorageResult::Stored(slot_accounts) => {
                    slot_accounts.into_iter().collect::<HashSet<Pubkey>>()
                }
            };

            let expected_accounts =
                if *slot >= requested_flush_root || *slot >= scan_root.unwrap_or(Slot::MAX) {
                    // 1) If slot > `requested_flush_root`, then  either:
                    //   a) If `is_cache_at_limit == false`, still in the cache
                    //   b) if `is_cache_at_limit == true`, were not cleaned before being flushed to storage.
                    //
                    // In both cases all the *original* updates at index `slot` were uncleaned and thus
                    // should be discoverable by this scan.
                    //
                    // 2) If slot == `requested_flush_root`, the slot was not cleaned before being flushed to storage,
                    // so it also contains all the original updates.
                    //
                    // 3) If *slot >= scan_root, then we should not clean it either
                    keys[*slot as usize..]
                        .iter()
                        .cloned()
                        .collect::<HashSet<Pubkey>>()
                } else {
                    // Slots less than `requested_flush_root` and `scan_root` were cleaned in the cache before being flushed
                    // to storage, should only contain one account
                    std::iter::once(keys[*slot as usize]).collect::<HashSet<Pubkey>>()
                };

            assert_eq!(slot_accounts, expected_accounts);
        }

        if let Some(scan_tracker) = scan_tracker {
            scan_tracker.exit().unwrap();
        }
    }

    #[test]
    fn test_accounts_db_cache_clean_max_root() {
        let requested_flush_root = 5;
        run_test_accounts_db_cache_clean_max_root(10, requested_flush_root, None);
    }

    #[test]
    fn test_accounts_db_cache_clean_max_root_with_scan() {
        let requested_flush_root = 5;
        run_test_accounts_db_cache_clean_max_root(
            10,
            requested_flush_root,
            Some(requested_flush_root - 1),
        );
        run_test_accounts_db_cache_clean_max_root(
            10,
            requested_flush_root,
            Some(requested_flush_root + 1),
        );
    }

    #[test]
    fn test_accounts_db_cache_clean_max_root_with_cache_limit_hit() {
        let requested_flush_root = 5;
        // Test that if there are > max_cache_slots() in the cache after flush, then more roots
        // will be flushed
        run_test_accounts_db_cache_clean_max_root(
            max_cache_slots() + requested_flush_root as usize + 2,
            requested_flush_root,
            None,
        );
    }

    #[test]
    fn test_accounts_db_cache_clean_max_root_with_cache_limit_hit_and_scan() {
        let requested_flush_root = 5;
        // Test that if there are > max_cache_slots() in the cache after flush, then more roots
        // will be flushed
        run_test_accounts_db_cache_clean_max_root(
            max_cache_slots() + requested_flush_root as usize + 2,
            requested_flush_root,
            Some(requested_flush_root - 1),
        );
        run_test_accounts_db_cache_clean_max_root(
            max_cache_slots() + requested_flush_root as usize + 2,
            requested_flush_root,
            Some(requested_flush_root + 1),
        );
    }

    fn run_flush_rooted_accounts_cache(should_clean: bool) {
        let num_slots = 10;
        let (accounts_db, keys, slots, _) = setup_accounts_db_cache_clean(num_slots, None, None);
        let mut cleaned_bytes = 0;
        let mut cleaned_accounts = 0;
        let should_clean_tracker = if should_clean {
            Some((&mut cleaned_bytes, &mut cleaned_accounts))
        } else {
            None
        };

        // If no cleaning is specified, then flush everything
        accounts_db.flush_rooted_accounts_cache(None, should_clean_tracker);
        for slot in &slots {
            let slot_accounts = if let ScanStorageResult::Stored(slot_accounts) = accounts_db
                .scan_account_storage(
                    *slot as Slot,
                    |_| Some(0),
                    |slot_account: &DashSet<Pubkey>, loaded_account: LoadedAccount| {
                        slot_account.insert(*loaded_account.pubkey());
                    },
                ) {
                slot_accounts.into_iter().collect::<HashSet<Pubkey>>()
            } else {
                panic!("All roots should have been flushed to storage");
            };
            let expected_accounts = if !should_clean || slot == slots.last().unwrap() {
                // The slot was not cleaned before being flushed to storage,
                // so it also contains all the original updates.
                keys[*slot as usize..]
                    .iter()
                    .cloned()
                    .collect::<HashSet<Pubkey>>()
            } else {
                // If clean was specified, only the latest slot should have all the updates.
                // All these other slots have been cleaned before flush
                std::iter::once(keys[*slot as usize]).collect::<HashSet<Pubkey>>()
            };
            assert_eq!(slot_accounts, expected_accounts);
        }
    }

    #[test]
    fn test_flush_rooted_accounts_cache_with_clean() {
        run_flush_rooted_accounts_cache(true);
    }

    #[test]
    fn test_flush_rooted_accounts_cache_without_clean() {
        run_flush_rooted_accounts_cache(false);
    }

    fn run_test_shrink_unref(do_intra_cache_clean: bool) {
        let db = AccountsDb::new_with_config_for_tests(
            Vec::new(),
            &ClusterType::Development,
            AccountSecondaryIndexes::default(),
            AccountShrinkThreshold::default(),
        );
        let account_key1 = Pubkey::new_unique();
        let account_key2 = Pubkey::new_unique();
        let account1 = AccountSharedData::new(1, 0, AccountSharedData::default().owner());

        // Store into slot 0
        db.store_cached((0, &[(&account_key1, &account1)][..]), None);
        db.store_cached((0, &[(&account_key2, &account1)][..]), None);
        db.add_root(0);
        if !do_intra_cache_clean {
            // If we don't want the cache doing purges before flush,
            // then we cannot flush multiple roots at once, otherwise the later
            // roots will clean the earlier roots before they are stored.
            // Thus flush the roots individually
            db.flush_accounts_cache(true, None);

            // Add an additional ref within the same slot to pubkey 1
            db.store_uncached(0, &[(&account_key1, &account1)]);
        }

        // Make account_key1 in slot 0 outdated by updating in rooted slot 1
        db.store_cached((1, &[(&account_key1, &account1)][..]), None);
        db.add_root(1);
        // Flushes all roots
        db.flush_accounts_cache(true, None);
        db.get_accounts_delta_hash(0);
        db.get_accounts_delta_hash(1);

        // Clean to remove outdated entry from slot 0
        db.clean_accounts(Some(1), false, None);

        // Shrink Slot 0
        let slot0_store = db.get_and_assert_single_storage(0);
        {
            let mut shrink_candidate_slots = db.shrink_candidate_slots.lock().unwrap();
            shrink_candidate_slots
                .entry(0)
                .or_default()
                .insert(slot0_store.append_vec_id(), slot0_store);
        }
        db.shrink_candidate_slots();

        // Make slot 0 dead by updating the remaining key
        db.store_cached((2, &[(&account_key2, &account1)][..]), None);
        db.add_root(2);

        // Flushes all roots
        db.flush_accounts_cache(true, None);

        // Should be one store before clean for slot 0
        db.get_and_assert_single_storage(0);
        db.get_accounts_delta_hash(2);
        db.clean_accounts(Some(2), false, None);

        // No stores should exist for slot 0 after clean
        assert_no_storages_at_slot(&db, 0);

        // Ref count for `account_key1` (account removed earlier by shrink)
        // should be 1, since it was only stored in slot 0 and 1, and slot 0
        // is now dead
        assert_eq!(db.accounts_index.ref_count_from_storage(&account_key1), 1);
    }

    #[test]
    fn test_shrink_unref() {
        run_test_shrink_unref(false)
    }

    #[test]
    fn test_shrink_unref_with_intra_slot_cleaning() {
        run_test_shrink_unref(true)
    }

    #[test]
    fn test_partial_clean() {
        solana_logger::setup();
        let db = AccountsDb::new(Vec::new(), &ClusterType::Development);
        let account_key1 = Pubkey::new_unique();
        let account_key2 = Pubkey::new_unique();
        let account1 = AccountSharedData::new(1, 0, AccountSharedData::default().owner());
        let account2 = AccountSharedData::new(2, 0, AccountSharedData::default().owner());
        let account3 = AccountSharedData::new(3, 0, AccountSharedData::default().owner());
        let account4 = AccountSharedData::new(4, 0, AccountSharedData::default().owner());

        // Store accounts into slots 0 and 1
        db.store_uncached(0, &[(&account_key1, &account1)]);
        db.store_uncached(0, &[(&account_key2, &account1)]);
        db.store_uncached(1, &[(&account_key1, &account2)]);
        db.get_accounts_delta_hash(0);
        db.get_accounts_delta_hash(1);

        db.print_accounts_stats("pre-clean1");

        // clean accounts - no accounts should be cleaned, since no rooted slots
        //
        // Checking that the uncleaned_pubkeys are not pre-maturely removed
        // such that when the slots are rooted, and can actually be cleaned, then the
        // delta keys are still there.
        db.clean_accounts_for_tests();

        db.print_accounts_stats("post-clean1");
        // Check stores > 0
        assert!(!slot_stores(&db, 0).is_empty());
        assert!(!slot_stores(&db, 1).is_empty());

        // root slot 0
        db.add_root_and_flush_write_cache(0);

        // store into slot 2
        db.store_uncached(2, &[(&account_key2, &account3)]);
        db.store_uncached(2, &[(&account_key1, &account3)]);
        db.get_accounts_delta_hash(2);

        db.clean_accounts_for_tests();
        db.print_accounts_stats("post-clean2");

        // root slots 1
        db.add_root_and_flush_write_cache(1);
        db.clean_accounts_for_tests();

        db.print_accounts_stats("post-clean3");

        db.store_uncached(3, &[(&account_key2, &account4)]);
        db.get_accounts_delta_hash(3);
        db.add_root_and_flush_write_cache(3);

        // Check that we can clean where max_root=3 and slot=2 is not rooted
        db.clean_accounts_for_tests();

        assert!(db.uncleaned_pubkeys.is_empty());

        db.print_accounts_stats("post-clean4");

        assert!(slot_stores(&db, 0).is_empty());
        assert!(!slot_stores(&db, 1).is_empty());
    }

    #[test]
    fn test_recycle_stores_expiration() {
        solana_logger::setup();

        let common_store_path = Path::new("");
        let common_slot_id = 12;
        let store_file_size = 1000;

        let store1_id = 22;
        let entry1 = Arc::new(AccountStorageEntry::new(
            common_store_path,
            common_slot_id,
            store1_id,
            store_file_size,
        ));

        let store2_id = 44;
        let entry2 = Arc::new(AccountStorageEntry::new(
            common_store_path,
            common_slot_id,
            store2_id,
            store_file_size,
        ));

        let mut recycle_stores = RecycleStores::default();
        recycle_stores.add_entry(entry1);
        recycle_stores.add_entry(entry2);
        assert_eq!(recycle_stores.entry_count(), 2);

        // no expiration for newly added entries
        let expired = recycle_stores.expire_old_entries();
        assert_eq!(
            expired
                .iter()
                .map(|e| e.append_vec_id())
                .collect::<Vec<_>>(),
            Vec::<AppendVecId>::new()
        );
        assert_eq!(
            recycle_stores
                .iter()
                .map(|(_, e)| e.append_vec_id())
                .collect::<Vec<_>>(),
            vec![store1_id, store2_id]
        );
        assert_eq!(recycle_stores.entry_count(), 2);
        assert_eq!(recycle_stores.total_bytes(), store_file_size * 2);

        // expiration for only too old entries
        recycle_stores.entries[0].0 = Instant::now()
            .checked_sub(Duration::from_secs(EXPIRATION_TTL_SECONDS + 1))
            .unwrap();
        let expired = recycle_stores.expire_old_entries();
        assert_eq!(
            expired
                .iter()
                .map(|e| e.append_vec_id())
                .collect::<Vec<_>>(),
            vec![store1_id]
        );
        assert_eq!(
            recycle_stores
                .iter()
                .map(|(_, e)| e.append_vec_id())
                .collect::<Vec<_>>(),
            vec![store2_id]
        );
        assert_eq!(recycle_stores.entry_count(), 1);
        assert_eq!(recycle_stores.total_bytes(), store_file_size);
    }

    const RACY_SLEEP_MS: u64 = 10;
    const RACE_TIME: u64 = 5;

    fn start_load_thread(
        with_retry: bool,
        ancestors: Ancestors,
        db: Arc<AccountsDb>,
        exit: Arc<AtomicBool>,
        pubkey: Arc<Pubkey>,
        expected_lamports: impl Fn(&(AccountSharedData, Slot)) -> u64 + Send + 'static,
    ) -> JoinHandle<()> {
        let load_hint = if with_retry {
            LoadHint::FixedMaxRoot
        } else {
            LoadHint::Unspecified
        };

        std::thread::Builder::new()
            .name("account-do-load".to_string())
            .spawn(move || {
                loop {
                    if exit.load(Ordering::Relaxed) {
                        return;
                    }
                    // Meddle load_limit to cover all branches of implementation.
                    // There should absolutely no behaviorial difference; the load_limit triggered
                    // slow branch should only affect the performance.
                    // Ordering::Relaxed is ok because of no data dependencies; the modified field is
                    // completely free-standing cfg(test) control-flow knob.
                    db.load_limit
                        .store(thread_rng().gen_range(0, 10) as u64, Ordering::Relaxed);

                    // Load should never be unable to find this key
                    let loaded_account = db
                        .do_load(
                            &ancestors,
                            &pubkey,
                            None,
                            load_hint,
                            LOAD_ZERO_LAMPORTS_ANY_TESTS,
                        )
                        .unwrap();
                    // slot + 1 == account.lamports because of the account-cache-flush thread
                    assert_eq!(
                        loaded_account.0.lamports(),
                        expected_lamports(&loaded_account)
                    );
                }
            })
            .unwrap()
    }

    fn do_test_load_account_and_cache_flush_race(with_retry: bool) {
        solana_logger::setup();

        let mut db = AccountsDb::new_with_config_for_tests(
            Vec::new(),
            &ClusterType::Development,
            AccountSecondaryIndexes::default(),
            AccountShrinkThreshold::default(),
        );
        db.load_delay = RACY_SLEEP_MS;
        let db = Arc::new(db);
        let pubkey = Arc::new(Pubkey::new_unique());
        let exit = Arc::new(AtomicBool::new(false));
        db.store_cached(
            (
                0,
                &[(
                    pubkey.as_ref(),
                    &AccountSharedData::new(1, 0, AccountSharedData::default().owner()),
                )][..],
            ),
            None,
        );
        db.add_root(0);
        db.flush_accounts_cache(true, None);

        let t_flush_accounts_cache = {
            let db = db.clone();
            let exit = exit.clone();
            let pubkey = pubkey.clone();
            let mut account = AccountSharedData::new(1, 0, AccountSharedData::default().owner());
            std::thread::Builder::new()
                .name("account-cache-flush".to_string())
                .spawn(move || {
                    let mut slot: Slot = 1;
                    loop {
                        if exit.load(Ordering::Relaxed) {
                            return;
                        }
                        account.set_lamports(slot + 1);
                        db.store_cached((slot, &[(pubkey.as_ref(), &account)][..]), None);
                        db.add_root(slot);
                        sleep(Duration::from_millis(RACY_SLEEP_MS));
                        db.flush_accounts_cache(true, None);
                        slot += 1;
                    }
                })
                .unwrap()
        };

        let t_do_load = start_load_thread(
            with_retry,
            Ancestors::default(),
            db,
            exit.clone(),
            pubkey,
            |(_, slot)| slot + 1,
        );

        sleep(Duration::from_secs(RACE_TIME));
        exit.store(true, Ordering::Relaxed);
        t_flush_accounts_cache.join().unwrap();
        t_do_load.join().map_err(std::panic::resume_unwind).unwrap()
    }

    #[test]
    fn test_load_account_and_cache_flush_race_with_retry() {
        do_test_load_account_and_cache_flush_race(true);
    }

    #[test]
    fn test_load_account_and_cache_flush_race_without_retry() {
        do_test_load_account_and_cache_flush_race(false);
    }

    fn do_test_load_account_and_shrink_race(with_retry: bool) {
        let mut db = AccountsDb::new_with_config_for_tests(
            Vec::new(),
            &ClusterType::Development,
            AccountSecondaryIndexes::default(),
            AccountShrinkThreshold::default(),
        );
        db.load_delay = RACY_SLEEP_MS;
        let db = Arc::new(db);
        let pubkey = Arc::new(Pubkey::new_unique());
        let exit = Arc::new(AtomicBool::new(false));
        let slot = 1;

        // Store an account
        let lamports = 42;
        let mut account = AccountSharedData::new(1, 0, AccountSharedData::default().owner());
        account.set_lamports(lamports);
        db.store_uncached(slot, &[(&pubkey, &account)]);

        // Set the slot as a root so account loads will see the contents of this slot
        db.add_root(slot);

        let t_shrink_accounts = {
            let db = db.clone();
            let exit = exit.clone();

            std::thread::Builder::new()
                .name("account-shrink".to_string())
                .spawn(move || loop {
                    if exit.load(Ordering::Relaxed) {
                        return;
                    }
                    // Simulate adding shrink candidates from clean_accounts()
                    let store = db.get_and_assert_single_storage(slot);
                    let store_id = store.append_vec_id();
                    db.shrink_candidate_slots
                        .lock()
                        .unwrap()
                        .entry(slot)
                        .or_default()
                        .insert(store_id, store.clone());
                    db.shrink_candidate_slots();
                })
                .unwrap()
        };

        let t_do_load = start_load_thread(
            with_retry,
            Ancestors::default(),
            db,
            exit.clone(),
            pubkey,
            move |_| lamports,
        );

        sleep(Duration::from_secs(RACE_TIME));
        exit.store(true, Ordering::Relaxed);
        t_shrink_accounts.join().unwrap();
        t_do_load.join().map_err(std::panic::resume_unwind).unwrap()
    }

    #[test]
    fn test_load_account_and_shrink_race_with_retry() {
        do_test_load_account_and_shrink_race(true);
    }

    #[test]
    fn test_load_account_and_shrink_race_without_retry() {
        do_test_load_account_and_shrink_race(false);
    }

    #[test]
    fn test_cache_flush_delayed_remove_unrooted_race() {
        let mut db = AccountsDb::new_with_config_for_tests(
            Vec::new(),
            &ClusterType::Development,
            AccountSecondaryIndexes::default(),
            AccountShrinkThreshold::default(),
        );
        db.load_delay = RACY_SLEEP_MS;
        let db = Arc::new(db);
        let slot = 10;
        let bank_id = 10;

        let lamports = 42;
        let mut account = AccountSharedData::new(1, 0, AccountSharedData::default().owner());
        account.set_lamports(lamports);

        // Start up a thread to flush the accounts cache
        let (flush_trial_start_sender, flush_trial_start_receiver) = unbounded();
        let (flush_done_sender, flush_done_receiver) = unbounded();
        let t_flush_cache = {
            let db = db.clone();
            std::thread::Builder::new()
                .name("account-cache-flush".to_string())
                .spawn(move || loop {
                    // Wait for the signal to start a trial
                    if flush_trial_start_receiver.recv().is_err() {
                        return;
                    }
                    db.flush_slot_cache(10);
                    flush_done_sender.send(()).unwrap();
                })
                .unwrap()
        };

        // Start up a thread remove the slot
        let (remove_trial_start_sender, remove_trial_start_receiver) = unbounded();
        let (remove_done_sender, remove_done_receiver) = unbounded();
        let t_remove = {
            let db = db.clone();
            std::thread::Builder::new()
                .name("account-remove".to_string())
                .spawn(move || loop {
                    // Wait for the signal to start a trial
                    if remove_trial_start_receiver.recv().is_err() {
                        return;
                    }
                    db.remove_unrooted_slots(&[(slot, bank_id)]);
                    remove_done_sender.send(()).unwrap();
                })
                .unwrap()
        };

        let num_trials = 10;
        for _ in 0..num_trials {
            let pubkey = Pubkey::new_unique();
            db.store_cached((slot, &[(&pubkey, &account)][..]), None);
            // Wait for both threads to finish
            flush_trial_start_sender.send(()).unwrap();
            remove_trial_start_sender.send(()).unwrap();
            let _ = flush_done_receiver.recv();
            let _ = remove_done_receiver.recv();
        }

        drop(flush_trial_start_sender);
        drop(remove_trial_start_sender);
        t_flush_cache.join().unwrap();
        t_remove.join().unwrap();
    }

    #[test]
    fn test_cache_flush_remove_unrooted_race_multiple_slots() {
        let db = AccountsDb::new_with_config_for_tests(
            Vec::new(),
            &ClusterType::Development,
            AccountSecondaryIndexes::default(),
            AccountShrinkThreshold::default(),
        );
        let db = Arc::new(db);
        let num_cached_slots = 100;

        let num_trials = 100;
        let (new_trial_start_sender, new_trial_start_receiver) = unbounded();
        let (flush_done_sender, flush_done_receiver) = unbounded();
        // Start up a thread to flush the accounts cache
        let t_flush_cache = {
            let db = db.clone();

            std::thread::Builder::new()
                .name("account-cache-flush".to_string())
                .spawn(move || loop {
                    // Wait for the signal to start a trial
                    if new_trial_start_receiver.recv().is_err() {
                        return;
                    }
                    for slot in 0..num_cached_slots {
                        db.flush_slot_cache(slot);
                    }
                    flush_done_sender.send(()).unwrap();
                })
                .unwrap()
        };

        let exit = Arc::new(AtomicBool::new(false));

        let t_spurious_signal = {
            let db = db.clone();
            let exit = exit.clone();
            std::thread::Builder::new()
                .name("account-cache-flush".to_string())
                .spawn(move || loop {
                    if exit.load(Ordering::Relaxed) {
                        return;
                    }
                    // Simulate spurious wake-up that can happen, but is too rare to
                    // otherwise depend on in tests.
                    db.remove_unrooted_slots_synchronization.signal.notify_all();
                })
                .unwrap()
        };

        // Run multiple trials. Has the added benefit of rewriting the same slots after we've
        // dumped them in previous trials.
        for _ in 0..num_trials {
            // Store an account
            let lamports = 42;
            let mut account = AccountSharedData::new(1, 0, AccountSharedData::default().owner());
            account.set_lamports(lamports);

            // Pick random 50% of the slots to pass to `remove_unrooted_slots()`
            let mut all_slots: Vec<(Slot, BankId)> = (0..num_cached_slots)
                .map(|slot| {
                    let bank_id = slot + 1;
                    (slot, bank_id)
                })
                .collect();
            all_slots.shuffle(&mut rand::thread_rng());
            let slots_to_dump = &all_slots[0..num_cached_slots as usize / 2];
            let slots_to_keep = &all_slots[num_cached_slots as usize / 2..];

            // Set up a one account per slot across many different slots, track which
            // pubkey was stored in each slot.
            let slot_to_pubkey_map: HashMap<Slot, Pubkey> = (0..num_cached_slots)
                .map(|slot| {
                    let pubkey = Pubkey::new_unique();
                    db.store_cached((slot, &[(&pubkey, &account)][..]), None);
                    (slot, pubkey)
                })
                .collect();

            // Signal the flushing shred to start flushing
            new_trial_start_sender.send(()).unwrap();

            // Here we want to test both:
            // 1) Flush thread starts flushing a slot before we try dumping it.
            // 2) Flushing thread trying to flush while/after we're trying to dump the slot,
            // in which case flush should ignore/move past the slot to be dumped
            //
            // Hence, we split into chunks to get the dumping of each chunk to race with the
            // flushes. If we were to dump the entire chunk at once, then this reduces the possibility
            // of the flush occurring first since the dumping logic reserves all the slots it's about
            // to dump immediately.

            for chunks in slots_to_dump.chunks(slots_to_dump.len() / 2) {
                db.remove_unrooted_slots(chunks);
            }

            // Check that all the slots in `slots_to_dump` were completely removed from the
            // cache, storage, and index

            for (slot, _) in slots_to_dump {
                assert_no_storages_at_slot(&db, *slot);
                assert!(db.accounts_cache.slot_cache(*slot).is_none());
                let account_in_slot = slot_to_pubkey_map[slot];
                let item = db.accounts_index.get_account_read_entry(&account_in_slot);
                assert!(item.is_none(), "item: {item:?}");
            }

            // Wait for flush to finish before starting next trial

            flush_done_receiver.recv().unwrap();

            for (slot, bank_id) in slots_to_keep {
                let account_in_slot = slot_to_pubkey_map[slot];
                assert!(db
                    .load(
                        &Ancestors::from(vec![(*slot, 0)]),
                        &account_in_slot,
                        LoadHint::FixedMaxRoot
                    )
                    .is_some());
                // Clear for next iteration so that `assert!(self.storage.get_slot_stores(purged_slot).is_none());`
                // in `purge_slot_pubkeys()` doesn't trigger
                db.remove_unrooted_slots(&[(*slot, *bank_id)]);
            }
        }

        exit.store(true, Ordering::Relaxed);
        drop(new_trial_start_sender);
        t_flush_cache.join().unwrap();

        t_spurious_signal.join().unwrap();
    }

    #[test]
    fn test_collect_uncleaned_slots_up_to_slot() {
        solana_logger::setup();
        let db = AccountsDb::new(Vec::new(), &ClusterType::Development);

        let slot1 = 11;
        let slot2 = 222;
        let slot3 = 3333;

        let pubkey1 = Pubkey::new_unique();
        let pubkey2 = Pubkey::new_unique();
        let pubkey3 = Pubkey::new_unique();

        db.uncleaned_pubkeys.insert(slot1, vec![pubkey1]);
        db.uncleaned_pubkeys.insert(slot2, vec![pubkey2]);
        db.uncleaned_pubkeys.insert(slot3, vec![pubkey3]);

        let mut uncleaned_slots1 = db.collect_uncleaned_slots_up_to_slot(slot1);
        let mut uncleaned_slots2 = db.collect_uncleaned_slots_up_to_slot(slot2);
        let mut uncleaned_slots3 = db.collect_uncleaned_slots_up_to_slot(slot3);

        uncleaned_slots1.sort_unstable();
        uncleaned_slots2.sort_unstable();
        uncleaned_slots3.sort_unstable();

        assert_eq!(uncleaned_slots1, [slot1]);
        assert_eq!(uncleaned_slots2, [slot1, slot2]);
        assert_eq!(uncleaned_slots3, [slot1, slot2, slot3]);
    }

    #[test]
    fn test_remove_uncleaned_slots_and_collect_pubkeys() {
        solana_logger::setup();
        let db = AccountsDb::new(Vec::new(), &ClusterType::Development);

        let slot1 = 11;
        let slot2 = 222;
        let slot3 = 3333;

        let pubkey1 = Pubkey::new_unique();
        let pubkey2 = Pubkey::new_unique();
        let pubkey3 = Pubkey::new_unique();

        let account1 = AccountSharedData::new(0, 0, &pubkey1);
        let account2 = AccountSharedData::new(0, 0, &pubkey2);
        let account3 = AccountSharedData::new(0, 0, &pubkey3);

        db.store_for_tests(slot1, &[(&pubkey1, &account1)]);
        db.store_for_tests(slot2, &[(&pubkey2, &account2)]);
        db.store_for_tests(slot3, &[(&pubkey3, &account3)]);

        db.add_root(slot1);
        // slot 2 is _not_ a root on purpose
        db.add_root(slot3);

        db.uncleaned_pubkeys.insert(slot1, vec![pubkey1]);
        db.uncleaned_pubkeys.insert(slot2, vec![pubkey2]);
        db.uncleaned_pubkeys.insert(slot3, vec![pubkey3]);

        let uncleaned_pubkeys1 = db
            .remove_uncleaned_slots_and_collect_pubkeys(vec![slot1])
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let uncleaned_pubkeys2 = db
            .remove_uncleaned_slots_and_collect_pubkeys(vec![slot2])
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let uncleaned_pubkeys3 = db
            .remove_uncleaned_slots_and_collect_pubkeys(vec![slot3])
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();

        assert!(uncleaned_pubkeys1.contains(&pubkey1));
        assert!(!uncleaned_pubkeys1.contains(&pubkey2));
        assert!(!uncleaned_pubkeys1.contains(&pubkey3));

        assert!(!uncleaned_pubkeys2.contains(&pubkey1));
        assert!(uncleaned_pubkeys2.contains(&pubkey2));
        assert!(!uncleaned_pubkeys2.contains(&pubkey3));

        assert!(!uncleaned_pubkeys3.contains(&pubkey1));
        assert!(!uncleaned_pubkeys3.contains(&pubkey2));
        assert!(uncleaned_pubkeys3.contains(&pubkey3));
    }

    #[test]
    fn test_remove_uncleaned_slots_and_collect_pubkeys_up_to_slot() {
        solana_logger::setup();
        let db = AccountsDb::new(Vec::new(), &ClusterType::Development);

        let slot1 = 11;
        let slot2 = 222;
        let slot3 = 3333;

        let pubkey1 = Pubkey::new_unique();
        let pubkey2 = Pubkey::new_unique();
        let pubkey3 = Pubkey::new_unique();

        let account1 = AccountSharedData::new(0, 0, &pubkey1);
        let account2 = AccountSharedData::new(0, 0, &pubkey2);
        let account3 = AccountSharedData::new(0, 0, &pubkey3);

        db.store_for_tests(slot1, &[(&pubkey1, &account1)]);
        db.store_for_tests(slot2, &[(&pubkey2, &account2)]);
        db.store_for_tests(slot3, &[(&pubkey3, &account3)]);

        // slot 1 is _not_ a root on purpose
        db.add_root(slot2);
        db.add_root(slot3);

        db.uncleaned_pubkeys.insert(slot1, vec![pubkey1]);
        db.uncleaned_pubkeys.insert(slot2, vec![pubkey2]);
        db.uncleaned_pubkeys.insert(slot3, vec![pubkey3]);

        let uncleaned_pubkeys = db
            .remove_uncleaned_slots_and_collect_pubkeys_up_to_slot(slot3)
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();

        assert!(uncleaned_pubkeys.contains(&pubkey1));
        assert!(uncleaned_pubkeys.contains(&pubkey2));
        assert!(uncleaned_pubkeys.contains(&pubkey3));
    }

    #[test]
    fn test_shrink_productive() {
        solana_logger::setup();
        let s1 = AccountStorageEntry::new(Path::new("."), 0, 0, 1024);
        let store = Arc::new(s1);
        assert!(!AccountsDb::is_shrinking_productive(0, &store));

        let s1 = AccountStorageEntry::new(Path::new("."), 0, 0, PAGE_SIZE * 4);
        let store = Arc::new(s1);
        store.add_account((3 * PAGE_SIZE as usize) - 1);
        store.add_account(10);
        store.remove_account(10, false);
        assert!(AccountsDb::is_shrinking_productive(0, &store));

        store.add_account(PAGE_SIZE as usize);
        assert!(!AccountsDb::is_shrinking_productive(0, &store));
    }

    #[test]
    fn test_is_candidate_for_shrink() {
        solana_logger::setup();

        let mut accounts = AccountsDb::new_single_for_tests();
        let common_store_path = Path::new("");
        let store_file_size = 2 * PAGE_SIZE;
        let entry = Arc::new(AccountStorageEntry::new(
            common_store_path,
            0,
            1,
            store_file_size,
        ));
        match accounts.shrink_ratio {
            AccountShrinkThreshold::TotalSpace { shrink_ratio } => {
                assert_eq!(
                    (DEFAULT_ACCOUNTS_SHRINK_RATIO * 100.) as u64,
                    (shrink_ratio * 100.) as u64
                )
            }
            AccountShrinkThreshold::IndividualStore { shrink_ratio: _ } => {
                panic!("Expect the default to be TotalSpace")
            }
        }
        entry.alive_bytes.store(3000, Ordering::Release);
        assert!(accounts.is_candidate_for_shrink(&entry, false));
        entry.alive_bytes.store(5000, Ordering::Release);
        assert!(!accounts.is_candidate_for_shrink(&entry, false));
        accounts.shrink_ratio = AccountShrinkThreshold::TotalSpace { shrink_ratio: 0.3 };
        entry.alive_bytes.store(3000, Ordering::Release);
        assert!(accounts.is_candidate_for_shrink(&entry, false));
        accounts.shrink_ratio = AccountShrinkThreshold::IndividualStore { shrink_ratio: 0.3 };
        assert!(!accounts.is_candidate_for_shrink(&entry, false));
    }

    #[test]
    fn test_calculate_storage_count_and_alive_bytes() {
        let accounts = AccountsDb::new_single_for_tests();
        let shared_key = solana_sdk::pubkey::new_rand();
        let account = AccountSharedData::new(1, 1, AccountSharedData::default().owner());
        let slot0 = 0;
        accounts.store_for_tests(slot0, &[(&shared_key, &account)]);
        accounts.add_root_and_flush_write_cache(slot0);

        let storage = accounts.storage.get_slot_storage_entry(slot0).unwrap();
        let storage_info = StorageSizeAndCountMap::default();
        let accounts_map = accounts.process_storage_slot(&storage);
        AccountsDb::update_storage_info(&storage_info, &accounts_map, &Mutex::default());
        assert_eq!(storage_info.len(), 1);
        for entry in storage_info.iter() {
            assert_eq!(
                (entry.key(), entry.value().count, entry.value().stored_size),
                (&0, 1, 144)
            );
        }
    }

    #[test]
    fn test_calculate_storage_count_and_alive_bytes_0_accounts() {
        let accounts = AccountsDb::new_single_for_tests();
        // empty store
        let storage = accounts.create_and_insert_store(0, 1, "test");
        let storage_info = StorageSizeAndCountMap::default();
        let accounts_map = accounts.process_storage_slot(&storage);
        AccountsDb::update_storage_info(&storage_info, &accounts_map, &Mutex::default());
        assert!(storage_info.is_empty());
    }

    #[test]
    fn test_calculate_storage_count_and_alive_bytes_2_accounts() {
        let accounts = AccountsDb::new_single_for_tests();
        let keys = [
            solana_sdk::pubkey::Pubkey::new(&[0; 32]),
            solana_sdk::pubkey::Pubkey::new(&[255; 32]),
        ];
        // make sure accounts are in 2 different bins
        assert!(
            (accounts.accounts_index.bins() == 1)
                ^ (accounts
                    .accounts_index
                    .bin_calculator
                    .bin_from_pubkey(&keys[0])
                    != accounts
                        .accounts_index
                        .bin_calculator
                        .bin_from_pubkey(&keys[1]))
        );
        let account = AccountSharedData::new(1, 1, AccountSharedData::default().owner());
        let account_big = AccountSharedData::new(1, 1000, AccountSharedData::default().owner());
        let slot0 = 0;
        accounts.store_for_tests(slot0, &[(&keys[0], &account)]);
        accounts.store_for_tests(slot0, &[(&keys[1], &account_big)]);
        accounts.add_root_and_flush_write_cache(slot0);

        let storage = accounts.storage.get_slot_storage_entry(slot0).unwrap();
        let storage_info = StorageSizeAndCountMap::default();
        let accounts_map = accounts.process_storage_slot(&storage);
        AccountsDb::update_storage_info(&storage_info, &accounts_map, &Mutex::default());
        assert_eq!(storage_info.len(), 1);
        for entry in storage_info.iter() {
            assert_eq!(
                (entry.key(), entry.value().count, entry.value().stored_size),
                (&0, 2, 1280)
            );
        }
    }

    #[test]
    fn test_set_storage_count_and_alive_bytes() {
        let accounts = AccountsDb::new_single_for_tests();

        // make sure we have storage 0
        let shared_key = solana_sdk::pubkey::new_rand();
        let account = AccountSharedData::new(1, 1, AccountSharedData::default().owner());
        let slot0 = 0;
        accounts.store_for_tests(slot0, &[(&shared_key, &account)]);
        accounts.add_root_and_flush_write_cache(slot0);

        // fake out the store count to avoid the assert
        for slot_stores in accounts.storage.iter() {
            for (_id, store) in slot_stores.value().read().unwrap().iter() {
                store.alive_bytes.store(0, Ordering::Release);
            }
        }

        // populate based on made up hash data
        let dashmap = DashMap::default();
        dashmap.insert(
            0,
            StorageSizeAndCount {
                stored_size: 2,
                count: 3,
            },
        );
        accounts.set_storage_count_and_alive_bytes(dashmap, &mut GenerateIndexTimings::default());
        assert_eq!(accounts.storage.len(), 1);
        for slot_stores in accounts.storage.iter() {
            for (id, store) in slot_stores.value().read().unwrap().iter() {
                assert_eq!(id, &0);
                assert_eq!(store.count_and_status.read().unwrap().0, 3);
                assert_eq!(store.alive_bytes.load(Ordering::Acquire), 2);
            }
        }
    }

    #[test]
    fn test_purge_alive_unrooted_slots_after_clean() {
        let accounts = AccountsDb::new_single_for_tests();

        // Key shared between rooted and nonrooted slot
        let shared_key = solana_sdk::pubkey::new_rand();
        // Key to keep the storage entry for the unrooted slot alive
        let unrooted_key = solana_sdk::pubkey::new_rand();
        let slot0 = 0;
        let slot1 = 1;

        // Store accounts with greater than 0 lamports
        let account = AccountSharedData::new(1, 1, AccountSharedData::default().owner());
        accounts.store_for_tests(slot0, &[(&shared_key, &account)]);
        accounts.store_for_tests(slot0, &[(&unrooted_key, &account)]);

        // Simulate adding dirty pubkeys on bank freeze. Note this is
        // not a rooted slot
        accounts.get_accounts_delta_hash(slot0);

        // On the next *rooted* slot, update the `shared_key` account to zero lamports
        let zero_lamport_account =
            AccountSharedData::new(0, 0, AccountSharedData::default().owner());
        accounts.store_for_tests(slot1, &[(&shared_key, &zero_lamport_account)]);

        // Simulate adding dirty pubkeys on bank freeze, set root
        accounts.get_accounts_delta_hash(slot1);
        accounts.add_root_and_flush_write_cache(slot1);

        // The later rooted zero-lamport update to `shared_key` cannot be cleaned
        // because it is kept alive by the unrooted slot.
        accounts.clean_accounts_for_tests();
        assert!(accounts
            .accounts_index
            .get_account_read_entry(&shared_key)
            .is_some());

        // Simulate purge_slot() all from AccountsBackgroundService
        accounts.purge_slot(slot0, 0, true);

        // Now clean should clean up the remaining key
        accounts.clean_accounts_for_tests();
        assert!(accounts
            .accounts_index
            .get_account_read_entry(&shared_key)
            .is_none());
        assert_no_storages_at_slot(&accounts, slot0);
    }

    /// asserts that not only are there 0 append vecs, but there is not even an entry in the storage map for 'slot'
    fn assert_no_storages_at_slot(db: &AccountsDb, slot: Slot) {
        assert!(db.get_storages_for_slot(slot).is_none());
    }

    /// Test to make sure `clean_accounts()` works properly with the `last_full_snapshot_slot`
    /// parameter.  Basically:
    ///
    /// - slot 1: set Account1's balance to non-zero
    /// - slot 2: set Account1's balance to a different non-zero amount
    /// - slot 3: set Account1's balance to zero
    /// - call `clean_accounts()` with `max_clean_root` set to 2
    ///     - ensure Account1 has *not* been purged
    ///     - ensure the store from slot 1 is cleaned up
    /// - call `clean_accounts()` with `last_full_snapshot_slot` set to 2
    ///     - ensure Account1 has *not* been purged
    /// - call `clean_accounts()` with `last_full_snapshot_slot` set to 3
    ///     - ensure Account1 *has* been purged
    #[test]
    fn test_clean_accounts_with_last_full_snapshot_slot() {
        solana_logger::setup();
        let accounts_db = AccountsDb::new_single_for_tests();
        let pubkey = solana_sdk::pubkey::new_rand();
        let owner = solana_sdk::pubkey::new_rand();
        let space = 0;

        let slot1: Slot = 1;
        let account = AccountSharedData::new(111, space, &owner);
        accounts_db.store_cached((slot1, &[(&pubkey, &account)][..]), None);
        accounts_db.get_accounts_delta_hash(slot1);
        accounts_db.add_root_and_flush_write_cache(slot1);

        let slot2: Slot = 2;
        let account = AccountSharedData::new(222, space, &owner);
        accounts_db.store_cached((slot2, &[(&pubkey, &account)][..]), None);
        accounts_db.get_accounts_delta_hash(slot2);
        accounts_db.add_root_and_flush_write_cache(slot2);

        let slot3: Slot = 3;
        let account = AccountSharedData::new(0, space, &owner);
        accounts_db.store_cached((slot3, &[(&pubkey, &account)][..]), None);
        accounts_db.get_accounts_delta_hash(slot3);
        accounts_db.add_root_and_flush_write_cache(slot3);

        assert_eq!(accounts_db.ref_count_for_pubkey(&pubkey), 3);

        accounts_db.clean_accounts(Some(slot2), false, Some(slot2));
        assert_eq!(accounts_db.ref_count_for_pubkey(&pubkey), 2);

        accounts_db.clean_accounts(None, false, Some(slot2));
        assert_eq!(accounts_db.ref_count_for_pubkey(&pubkey), 1);

        accounts_db.clean_accounts(None, false, Some(slot3));
        assert_eq!(accounts_db.ref_count_for_pubkey(&pubkey), 0);
    }

    #[test]
    fn test_filter_zero_lamport_clean_for_incremental_snapshots() {
        solana_logger::setup();
        let slot = 10;

        struct TestParameters {
            last_full_snapshot_slot: Option<Slot>,
            max_clean_root: Option<Slot>,
            should_contain: bool,
        }

        let do_test = |test_params: TestParameters| {
            let account_info = AccountInfo::new(StorageLocation::AppendVec(42, 128), 234, 0);
            let pubkey = solana_sdk::pubkey::new_rand();
            let mut key_set = HashSet::default();
            key_set.insert(pubkey);
            let store_count = 0;
            let mut store_counts = HashMap::default();
            store_counts.insert(account_info.store_id(), (store_count, key_set));
            let mut purges_zero_lamports = HashMap::default();
            purges_zero_lamports.insert(pubkey, (vec![(slot, account_info)], 1));

            let accounts_db = AccountsDb::new_single_for_tests();
            accounts_db.filter_zero_lamport_clean_for_incremental_snapshots(
                test_params.max_clean_root,
                test_params.last_full_snapshot_slot,
                &store_counts,
                &mut purges_zero_lamports,
            );

            assert_eq!(
                purges_zero_lamports.contains_key(&pubkey),
                test_params.should_contain
            );
        };

        // Scenario 1: last full snapshot is NONE
        // In this scenario incremental snapshots are OFF, so always purge
        {
            let last_full_snapshot_slot = None;

            do_test(TestParameters {
                last_full_snapshot_slot,
                max_clean_root: Some(slot),
                should_contain: true,
            });

            do_test(TestParameters {
                last_full_snapshot_slot,
                max_clean_root: None,
                should_contain: true,
            });
        }

        // Scenario 2: last full snapshot is GREATER THAN zero lamport account slot
        // In this scenario always purge, and just test the various permutations of
        // `should_filter_for_incremental_snapshots` based on `max_clean_root`.
        {
            let last_full_snapshot_slot = Some(slot + 1);

            do_test(TestParameters {
                last_full_snapshot_slot,
                max_clean_root: last_full_snapshot_slot,
                should_contain: true,
            });

            do_test(TestParameters {
                last_full_snapshot_slot,
                max_clean_root: last_full_snapshot_slot.map(|s| s + 1),
                should_contain: true,
            });

            do_test(TestParameters {
                last_full_snapshot_slot,
                max_clean_root: None,
                should_contain: true,
            });
        }

        // Scenario 3: last full snapshot is EQUAL TO zero lamport account slot
        // In this scenario always purge, as it's the same as Scenario 2.
        {
            let last_full_snapshot_slot = Some(slot);

            do_test(TestParameters {
                last_full_snapshot_slot,
                max_clean_root: last_full_snapshot_slot,
                should_contain: true,
            });

            do_test(TestParameters {
                last_full_snapshot_slot,
                max_clean_root: last_full_snapshot_slot.map(|s| s + 1),
                should_contain: true,
            });

            do_test(TestParameters {
                last_full_snapshot_slot,
                max_clean_root: None,
                should_contain: true,
            });
        }

        // Scenario 4: last full snapshot is LESS THAN zero lamport account slot
        // In this scenario do *not* purge, except when `should_filter_for_incremental_snapshots`
        // is false
        {
            let last_full_snapshot_slot = Some(slot - 1);

            do_test(TestParameters {
                last_full_snapshot_slot,
                max_clean_root: last_full_snapshot_slot,
                should_contain: true,
            });

            do_test(TestParameters {
                last_full_snapshot_slot,
                max_clean_root: last_full_snapshot_slot.map(|s| s + 1),
                should_contain: false,
            });

            do_test(TestParameters {
                last_full_snapshot_slot,
                max_clean_root: None,
                should_contain: false,
            });
        }
    }

    #[test]
    fn test_calc_alive_ancient_historical_roots() {
        let db = AccountsDb::new_single_for_tests();
        let min_root = 0;
        let result = db.calc_alive_ancient_historical_roots(min_root);
        assert!(result.is_empty());
        for extra in 1..3 {
            let result = db.calc_alive_ancient_historical_roots(extra);
            assert_eq!(result, HashSet::default(), "extra: {extra}");
        }

        let extra = 3;
        let active_root = 2;
        db.accounts_index.add_root(active_root);
        let result = db.calc_alive_ancient_historical_roots(extra);
        let expected_alive_roots = [active_root].into_iter().collect();
        assert_eq!(result, expected_alive_roots, "extra: {extra}");
    }

    impl AccountsDb {
        /// useful to adapt tests written prior to introduction of the write cache
        /// to use the write cache
        pub fn add_root_and_flush_write_cache(&self, slot: Slot) {
            self.add_root(slot);
            self.flush_root_write_cache(slot);
        }

        /// useful to adapt tests written prior to introduction of the write cache
        /// to use the write cache
        fn flush_root_write_cache(&self, root: Slot) {
            assert!(
                self.accounts_index
                    .roots_tracker
                    .read()
                    .unwrap()
                    .alive_roots
                    .contains(&root),
                "slot: {root}"
            );
            self.flush_accounts_cache(true, Some(root));
        }

        /// callers use to call store_uncached. But, this is not allowed anymore.
        pub fn store_for_tests(&self, slot: Slot, accounts: &[(&Pubkey, &AccountSharedData)]) {
            self.store(
                (slot, accounts, INCLUDE_SLOT_IN_HASH_TESTS),
                true,
                None,
                StoreReclaims::Default,
            );
        }

        /// helper function to test unref_accounts or clean_dead_slots_from_accounts_index
        fn test_unref(
            &self,
            call_unref: bool,
            purged_slot_pubkeys: HashSet<(Slot, Pubkey)>,
            purged_stored_account_slots: &mut AccountSlots,
            pubkeys_removed_from_accounts_index: &PubkeysRemovedFromAccountsIndex,
        ) {
            if call_unref {
                self.unref_accounts(
                    purged_slot_pubkeys,
                    purged_stored_account_slots,
                    pubkeys_removed_from_accounts_index,
                );
            } else {
                let empty_vec = Vec::default();
                self.clean_dead_slots_from_accounts_index(
                    empty_vec.iter(),
                    purged_slot_pubkeys,
                    Some(purged_stored_account_slots),
                    pubkeys_removed_from_accounts_index,
                );
            }
        }
    }

    #[test]
    /// test 'unref' parameter 'pubkeys_removed_from_accounts_index'
    fn test_unref_pubkeys_removed_from_accounts_index() {
        let slot1 = 1;
        let pk1 = Pubkey::new(&[1; 32]);
        for already_removed in [false, true] {
            let mut pubkeys_removed_from_accounts_index =
                PubkeysRemovedFromAccountsIndex::default();
            if already_removed {
                pubkeys_removed_from_accounts_index.insert(pk1);
            }
            // pk1 in slot1, purge it
            let db = AccountsDb::new_single_for_tests();
            let mut purged_slot_pubkeys = HashSet::default();
            purged_slot_pubkeys.insert((slot1, pk1));
            let mut reclaims = SlotList::default();
            db.accounts_index.upsert(
                slot1,
                slot1,
                &pk1,
                &AccountSharedData::default(),
                &AccountSecondaryIndexes::default(),
                AccountInfo::default(),
                &mut reclaims,
                UpsertReclaim::IgnoreReclaims,
            );

            let mut purged_stored_account_slots = AccountSlots::default();
            db.test_unref(
                true,
                purged_slot_pubkeys,
                &mut purged_stored_account_slots,
                &pubkeys_removed_from_accounts_index,
            );
            assert_eq!(
                vec![(pk1, vec![slot1].into_iter().collect::<HashSet<_>>())],
                purged_stored_account_slots.into_iter().collect::<Vec<_>>()
            );
            let expected = u64::from(already_removed);
            assert_eq!(db.accounts_index.ref_count_from_storage(&pk1), expected);
        }
    }

    #[test]
    fn test_unref_accounts() {
        let pubkeys_removed_from_accounts_index = PubkeysRemovedFromAccountsIndex::default();
        for call_unref in [false, true] {
            {
                let db = AccountsDb::new_single_for_tests();
                let mut purged_stored_account_slots = AccountSlots::default();

                db.test_unref(
                    call_unref,
                    HashSet::default(),
                    &mut purged_stored_account_slots,
                    &pubkeys_removed_from_accounts_index,
                );
                assert!(purged_stored_account_slots.is_empty());
            }

            let slot1 = 1;
            let slot2 = 2;
            let pk1 = Pubkey::new(&[1; 32]);
            let pk2 = Pubkey::new(&[2; 32]);
            {
                // pk1 in slot1, purge it
                let db = AccountsDb::new_single_for_tests();
                let mut purged_slot_pubkeys = HashSet::default();
                purged_slot_pubkeys.insert((slot1, pk1));
                let mut reclaims = SlotList::default();
                db.accounts_index.upsert(
                    slot1,
                    slot1,
                    &pk1,
                    &AccountSharedData::default(),
                    &AccountSecondaryIndexes::default(),
                    AccountInfo::default(),
                    &mut reclaims,
                    UpsertReclaim::IgnoreReclaims,
                );

                let mut purged_stored_account_slots = AccountSlots::default();
                db.test_unref(
                    call_unref,
                    purged_slot_pubkeys,
                    &mut purged_stored_account_slots,
                    &pubkeys_removed_from_accounts_index,
                );
                assert_eq!(
                    vec![(pk1, vec![slot1].into_iter().collect::<HashSet<_>>())],
                    purged_stored_account_slots.into_iter().collect::<Vec<_>>()
                );
                assert_eq!(db.accounts_index.ref_count_from_storage(&pk1), 0);
            }
            {
                let db = AccountsDb::new_single_for_tests();
                let mut purged_stored_account_slots = AccountSlots::default();
                let mut purged_slot_pubkeys = HashSet::default();
                let mut reclaims = SlotList::default();
                // pk1 and pk2 both in slot1 and slot2, so each has refcount of 2
                for slot in [slot1, slot2] {
                    for pk in [pk1, pk2] {
                        db.accounts_index.upsert(
                            slot,
                            slot,
                            &pk,
                            &AccountSharedData::default(),
                            &AccountSecondaryIndexes::default(),
                            AccountInfo::default(),
                            &mut reclaims,
                            UpsertReclaim::IgnoreReclaims,
                        );
                    }
                }
                // purge pk1 from both 1 and 2 and pk2 from slot 1
                let purges = vec![(slot1, pk1), (slot1, pk2), (slot2, pk1)];
                purges.into_iter().for_each(|(slot, pk)| {
                    purged_slot_pubkeys.insert((slot, pk));
                });
                db.test_unref(
                    call_unref,
                    purged_slot_pubkeys,
                    &mut purged_stored_account_slots,
                    &pubkeys_removed_from_accounts_index,
                );
                for (pk, slots) in vec![(pk1, vec![slot1, slot2]), (pk2, vec![slot1])] {
                    let result = purged_stored_account_slots.remove(&pk).unwrap();
                    assert_eq!(result, slots.into_iter().collect::<HashSet<_>>());
                }
                assert!(purged_stored_account_slots.is_empty());
                assert_eq!(db.accounts_index.ref_count_from_storage(&pk1), 0);
                assert_eq!(db.accounts_index.ref_count_from_storage(&pk2), 1);
            }
        }
    }

    #[test]
    fn test_many_unrefs() {
        let db = AccountsDb::new_single_for_tests();
        let mut purged_stored_account_slots = AccountSlots::default();
        let mut reclaims = SlotList::default();
        let pk1 = Pubkey::new(&[1; 32]);
        // make sure we have > 1 batch. Bigger numbers cost more in test time here.
        let n = (UNREF_ACCOUNTS_BATCH_SIZE + 1) as Slot;
        // put the pubkey into the acct idx in 'n' slots
        let purged_slot_pubkeys = (0..n)
            .map(|slot| {
                db.accounts_index.upsert(
                    slot,
                    slot,
                    &pk1,
                    &AccountSharedData::default(),
                    &AccountSecondaryIndexes::default(),
                    AccountInfo::default(),
                    &mut reclaims,
                    UpsertReclaim::IgnoreReclaims,
                );
                (slot, pk1)
            })
            .collect::<HashSet<_>>();

        assert_eq!(db.accounts_index.ref_count_from_storage(&pk1), n);
        // unref all 'n' slots
        db.unref_accounts(
            purged_slot_pubkeys,
            &mut purged_stored_account_slots,
            &HashSet::default(),
        );
        assert_eq!(db.accounts_index.ref_count_from_storage(&pk1), 0);
    }

    #[test]
    fn test_get_one_epoch_old_slot_for_hash_calc_scan() {
        let mut db = AccountsDb::new_single_for_tests();
        let config = CalcAccountsHashConfig::default();
        let slot = config.epoch_schedule.slots_per_epoch;
        assert_ne!(slot, 0);
        let offset = 10;
        assert_eq!(
            db.get_one_epoch_old_slot_for_hash_calc_scan(slot + offset, &config),
            0
        );
        db.ancient_append_vec_offset = Some(0);
        assert_eq!(
            db.get_one_epoch_old_slot_for_hash_calc_scan(slot, &config),
            0
        );
        assert_eq!(
            db.get_one_epoch_old_slot_for_hash_calc_scan(slot + offset, &config),
            offset
        );
    }

    #[test]
    fn test_mark_dirty_dead_stores_empty() {
        let db = AccountsDb::new_single_for_tests();
        let slot = 0;
        for add_dirty_stores in [false, true] {
            let (remaining_stores, dead_storages) =
                db.mark_dirty_dead_stores(slot, add_dirty_stores, None);
            assert_eq!(remaining_stores, 0);
            assert!(dead_storages.is_empty());
            assert!(db.dirty_stores.is_empty());
        }
    }

    #[test]
    fn test_mark_dirty_dead_stores_no_shrink_in_progress() {
        // None for shrink_in_progress, 1 existing store at the slot
        // There should be no more append vecs at that slot after the call to mark_dirty_dead_stores.
        // This tests the case where this slot was combined into an ancient append vec from an older slot and
        // there is no longer an append vec at this slot.
        for add_dirty_stores in [false, true] {
            let slot = 0;
            let db = AccountsDb::new_single_for_tests();
            let size = 1;
            let existing_store = db.create_and_insert_store(slot, size, "test");
            let old_id = existing_store.append_vec_id();
            let (remaining_stores, dead_storages) =
                db.mark_dirty_dead_stores(slot, add_dirty_stores, None);
            assert_eq!(0, remaining_stores);
            assert_eq!(dead_storages.len(), 1);
            assert_eq!(dead_storages.first().unwrap().append_vec_id(), old_id);
            if add_dirty_stores {
                assert_eq!(1, db.dirty_stores.len());
                let dirty_store = db.dirty_stores.get(&(slot, old_id)).unwrap();
                assert_eq!(dirty_store.append_vec_id(), old_id);
            } else {
                assert!(db.dirty_stores.is_empty());
            }
            assert!(db.storage.is_empty_entry(slot));
        }
    }

    #[test]
    fn test_mark_dirty_dead_stores() {
        let slot = 0;

        // use shrink_in_progress to cause us to drop the initial store
        for add_dirty_stores in [false, true] {
            let db = AccountsDb::new_single_for_tests();
            let size = 1;
            let old_store = db.create_and_insert_store(slot, size, "test");
            let old_id = old_store.append_vec_id();
            let shrink_in_progress = db.get_store_for_shrink(slot, 100);
            let (remaining_stores, dead_storages) =
                db.mark_dirty_dead_stores(slot, add_dirty_stores, Some(shrink_in_progress));
            assert_eq!(1, remaining_stores);
            assert_eq!(dead_storages.len(), 1);
            assert_eq!(dead_storages.first().unwrap().append_vec_id(), old_id);
            if add_dirty_stores {
                assert_eq!(1, db.dirty_stores.len());
                let dirty_store = db.dirty_stores.get(&(slot, old_id)).unwrap();
                assert_eq!(dirty_store.append_vec_id(), old_id);
            } else {
                assert!(db.dirty_stores.is_empty());
            }
            assert_eq!(
                1,
                db.get_storages_for_slot(slot)
                    .map(|storages| storages.len())
                    .unwrap_or_default()
            );
        }
    }

    #[test]
    fn test_split_storages_ancient_chunks() {
        let storages = SortedStorages::empty();
        assert_eq!(storages.max_slot_inclusive(), 0);
        let result = SplitAncientStorages::new(0, &storages);
        assert_eq!(result, SplitAncientStorages::default());
    }

    /// get all the ranges the splitter produces
    fn get_all_slot_ranges(splitter: &SplitAncientStorages) -> Vec<Option<Range<Slot>>> {
        (0..splitter.chunk_count)
            .map(|chunk| {
                assert_eq!(
                    splitter.get_starting_slot_from_normal_chunk(chunk),
                    if chunk == 0 {
                        splitter.normal_slot_range.start
                    } else {
                        (splitter.first_chunk_start + ((chunk as Slot) - 1) * MAX_ITEMS_PER_CHUNK)
                            .max(splitter.normal_slot_range.start)
                    },
                    "chunk: {chunk}, num_chunks: {}, splitter: {:?}",
                    splitter.chunk_count,
                    splitter,
                );
                splitter.get_slot_range(chunk)
            })
            .collect::<Vec<_>>()
    }

    /// test function to make sure the split range covers exactly every slot in the original range
    fn verify_all_slots_covered_exactly_once(
        splitter: &SplitAncientStorages,
        overall_range: &Range<Slot>,
    ) {
        // verify all slots covered exactly once
        let result = get_all_slot_ranges(splitter);
        let mut expected = overall_range.start;
        result.iter().for_each(|range| {
            if let Some(range) = range {
                assert!(
                    overall_range.start == range.start || range.start % MAX_ITEMS_PER_CHUNK == 0
                );
                for slot in range.clone() {
                    assert_eq!(slot, expected);
                    expected += 1;
                }
            }
        });
        assert_eq!(expected, overall_range.end);
    }

    /// new splitter for test
    /// without any ancient append vecs
    fn new_splitter(range: &Range<Slot>) -> SplitAncientStorages {
        let splitter =
            SplitAncientStorages::new_with_ancient_info(range, Vec::default(), range.start);

        verify_all_slots_covered_exactly_once(&splitter, range);

        splitter
    }

    /// new splitter for test
    /// without any ancient append vecs
    fn new_splitter2(start: Slot, count: Slot) -> SplitAncientStorages {
        new_splitter(&Range {
            start,
            end: start + count,
        })
    }

    #[test]
    fn test_split_storages_splitter_simple() {
        let plus_1 = MAX_ITEMS_PER_CHUNK + 1;
        let plus_2 = plus_1 + 1;

        // starting at 0 is aligned with beginning, so 1st chunk is unnecessary since beginning slot starts at boundary
        // second chunk is the final chunk, which is not full (does not have 2500 entries)
        let splitter = new_splitter2(0, 1);
        let result = get_all_slot_ranges(&splitter);
        assert_eq!(result, [Some(0..1), None]);

        // starting at 1 is not aligned with beginning, but since we don't have enough for a full chunk, it gets returned in the last chunk
        let splitter = new_splitter2(1, 1);
        let result = get_all_slot_ranges(&splitter);
        assert_eq!(result, [Some(1..2), None]);

        // 1 full chunk, aligned
        let splitter = new_splitter2(0, MAX_ITEMS_PER_CHUNK);
        let result = get_all_slot_ranges(&splitter);
        assert_eq!(result, [Some(0..MAX_ITEMS_PER_CHUNK), None, None]);

        // 1 full chunk + 1, aligned
        let splitter = new_splitter2(0, plus_1);
        let result = get_all_slot_ranges(&splitter);
        assert_eq!(
            result,
            [
                Some(0..MAX_ITEMS_PER_CHUNK),
                Some(MAX_ITEMS_PER_CHUNK..plus_1),
                None
            ]
        );

        // 1 full chunk + 2, aligned
        let splitter = new_splitter2(0, plus_2);
        let result = get_all_slot_ranges(&splitter);
        assert_eq!(
            result,
            [
                Some(0..MAX_ITEMS_PER_CHUNK),
                Some(MAX_ITEMS_PER_CHUNK..plus_2),
                None
            ]
        );

        // 1 full chunk, mis-aligned by 1
        let offset = 1;
        let splitter = new_splitter2(offset, MAX_ITEMS_PER_CHUNK);
        let result = get_all_slot_ranges(&splitter);
        assert_eq!(
            result,
            [
                Some(offset..MAX_ITEMS_PER_CHUNK),
                Some(MAX_ITEMS_PER_CHUNK..MAX_ITEMS_PER_CHUNK + offset),
                None
            ]
        );

        // starting at 1 is not aligned with beginning
        let offset = 1;
        let splitter = new_splitter2(offset, plus_1);
        let result = get_all_slot_ranges(&splitter);
        assert_eq!(
            result,
            [
                Some(offset..MAX_ITEMS_PER_CHUNK),
                Some(MAX_ITEMS_PER_CHUNK..plus_1 + offset),
                None
            ],
            "{splitter:?}"
        );

        // 2 full chunks, aligned
        let offset = 0;
        let splitter = new_splitter2(offset, MAX_ITEMS_PER_CHUNK * 2);
        let result = get_all_slot_ranges(&splitter);
        assert_eq!(
            result,
            [
                Some(offset..MAX_ITEMS_PER_CHUNK),
                Some(MAX_ITEMS_PER_CHUNK..MAX_ITEMS_PER_CHUNK * 2),
                None,
                None
            ],
            "{splitter:?}"
        );

        // 2 full chunks + 1, mis-aligned
        let offset = 1;
        let splitter = new_splitter2(offset, MAX_ITEMS_PER_CHUNK * 2);
        let result = get_all_slot_ranges(&splitter);
        assert_eq!(
            result,
            [
                Some(offset..MAX_ITEMS_PER_CHUNK),
                Some(MAX_ITEMS_PER_CHUNK..MAX_ITEMS_PER_CHUNK * 2),
                Some(MAX_ITEMS_PER_CHUNK * 2..MAX_ITEMS_PER_CHUNK * 2 + offset),
                None,
            ],
            "{splitter:?}"
        );

        // 3 full chunks - 1, mis-aligned by 2
        // we need ALL the chunks here
        let offset = 2;
        let splitter = new_splitter2(offset, MAX_ITEMS_PER_CHUNK * 3 - 1);
        let result = get_all_slot_ranges(&splitter);
        assert_eq!(
            result,
            [
                Some(offset..MAX_ITEMS_PER_CHUNK),
                Some(MAX_ITEMS_PER_CHUNK..MAX_ITEMS_PER_CHUNK * 2),
                Some(MAX_ITEMS_PER_CHUNK * 2..MAX_ITEMS_PER_CHUNK * 3),
                Some(MAX_ITEMS_PER_CHUNK * 3..MAX_ITEMS_PER_CHUNK * 3 + 1),
            ],
            "{splitter:?}"
        );

        // 1 full chunk - 1, mis-aligned by 2
        // we need ALL the chunks here
        let offset = 2;
        let splitter = new_splitter2(offset, MAX_ITEMS_PER_CHUNK - 1);
        let result = get_all_slot_ranges(&splitter);
        assert_eq!(
            result,
            [
                Some(offset..MAX_ITEMS_PER_CHUNK),
                Some(MAX_ITEMS_PER_CHUNK..MAX_ITEMS_PER_CHUNK + 1),
            ],
            "{splitter:?}"
        );

        // 1 full chunk - 1, aligned at big offset
        // huge offset
        // we need ALL the chunks here
        let offset = MAX_ITEMS_PER_CHUNK * 100;
        let splitter = new_splitter2(offset, MAX_ITEMS_PER_CHUNK - 1);
        let result = get_all_slot_ranges(&splitter);
        assert_eq!(
            result,
            [Some(offset..MAX_ITEMS_PER_CHUNK * 101 - 1), None,],
            "{splitter:?}"
        );

        // 1 full chunk - 1, mis-aligned by 2 at big offset
        // huge offset
        // we need ALL the chunks here
        let offset = MAX_ITEMS_PER_CHUNK * 100 + 2;
        let splitter = new_splitter2(offset, MAX_ITEMS_PER_CHUNK - 1);
        let result = get_all_slot_ranges(&splitter);
        assert_eq!(
            result,
            [
                Some(offset..MAX_ITEMS_PER_CHUNK * 101),
                Some(MAX_ITEMS_PER_CHUNK * 101..MAX_ITEMS_PER_CHUNK * 101 + 1),
            ],
            "{splitter:?}"
        );
    }

    #[test]
    fn test_split_storages_splitter_large_offset() {
        solana_logger::setup();
        // 1 full chunk - 1, mis-aligned by 2 at big offset
        // huge offset
        // we need ALL the chunks here
        let offset = MAX_ITEMS_PER_CHUNK * 100 + 2;
        let splitter = new_splitter2(offset, MAX_ITEMS_PER_CHUNK - 1);
        let result = get_all_slot_ranges(&splitter);
        assert_eq!(
            result,
            [
                Some(offset..MAX_ITEMS_PER_CHUNK * 101),
                Some(MAX_ITEMS_PER_CHUNK * 101..MAX_ITEMS_PER_CHUNK * 101 + 1),
            ],
            "{splitter:?}"
        );
    }

    #[test]
    fn test_split_storages_parametric_splitter() {
        for offset_multiplier in [1, 1000] {
            for offset in [
                0,
                1,
                2,
                MAX_ITEMS_PER_CHUNK - 2,
                MAX_ITEMS_PER_CHUNK - 1,
                MAX_ITEMS_PER_CHUNK,
                MAX_ITEMS_PER_CHUNK + 1,
            ] {
                for full_chunks in [0, 1, 2, 3] {
                    for reduced_items in [0, 1, 2] {
                        for added_items in [0, 1, 2] {
                            // this will verify the entire range correctly
                            _ = new_splitter2(
                                offset * offset_multiplier,
                                (full_chunks * MAX_ITEMS_PER_CHUNK + added_items)
                                    .saturating_sub(reduced_items),
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn test_add_uncleaned_pubkeys_after_shrink() {
        let db = AccountsDb::new_single_for_tests();
        let slot = 0;
        let pubkey = Pubkey::new(&[1; 32]);
        db.add_uncleaned_pubkeys_after_shrink(slot, vec![pubkey].into_iter());
        assert_eq!(&*db.uncleaned_pubkeys.get(&slot).unwrap(), &vec![pubkey]);
    }

    #[test]
    fn test_get_ancient_slots() {
        // test permutations of ancient, non-ancient, ancient with sparse slot #s and not
        for sparse in [false, true] {
            let (slot1_ancient, slot2, slot3_ancient, slot1_plus_ancient) = if sparse {
                (1, 10, 20, 5)
            } else {
                // we only test with 2 ancient append vecs when sparse
                (1, 2, 3, 4 /* irrelevant */)
            };

            let db = AccountsDb::new_single_for_tests();
            // there has to be an existing append vec at this slot for a new current ancient at the slot to make sense
            let _existing_append_vec = db.create_and_insert_store(slot1_ancient, 1000, "test");

            let ancient = db
                .create_ancient_append_vec(slot1_ancient)
                .new_storage()
                .clone();
            let _existing_append_vec = db.create_and_insert_store(slot1_plus_ancient, 1000, "test");
            let ancient_1_plus = db
                .create_ancient_append_vec(slot1_plus_ancient)
                .new_storage()
                .clone();
            let _existing_append_vec = db.create_and_insert_store(slot3_ancient, 1000, "test");
            let ancient3 = db.create_ancient_append_vec(slot3_ancient);
            let temp_dir = TempDir::new().unwrap();
            let path = temp_dir.path();
            let id = 1;
            let size = 1;
            let non_ancient_storage = Arc::new(AccountStorageEntry::new(path, slot2, id, size));
            let raw_storages = vec![vec![non_ancient_storage.clone()]];
            let snapshot_storages = SortedStorages::new(&raw_storages);
            // test without an ancient append vec
            let one_epoch_old_slot = 0;
            let ancient_slots =
                SplitAncientStorages::get_ancient_slots(one_epoch_old_slot, &snapshot_storages);
            assert_eq!(Vec::<Slot>::default(), ancient_slots);
            let one_epoch_old_slot = 3;
            let ancient_slots =
                SplitAncientStorages::get_ancient_slots(one_epoch_old_slot, &snapshot_storages);
            assert_eq!(Vec::<Slot>::default(), ancient_slots);

            // now test with an ancient append vec
            let raw_storages = vec![vec![ancient.clone()]];
            let snapshot_storages = SortedStorages::new(&raw_storages);
            let one_epoch_old_slot = 0;
            let ancient_slots =
                SplitAncientStorages::get_ancient_slots(one_epoch_old_slot, &snapshot_storages);
            assert_eq!(Vec::<Slot>::default(), ancient_slots);
            let one_epoch_old_slot = slot2 + 1;
            let ancient_slots =
                SplitAncientStorages::get_ancient_slots(one_epoch_old_slot, &snapshot_storages);
            assert_eq!(vec![slot1_ancient], ancient_slots);

            // now test with an ancient append vec and then a non-ancient append vec
            let raw_storages = vec![vec![ancient.clone()], vec![non_ancient_storage.clone()]];
            let snapshot_storages = SortedStorages::new(&raw_storages);
            let one_epoch_old_slot = 0;
            let ancient_slots =
                SplitAncientStorages::get_ancient_slots(one_epoch_old_slot, &snapshot_storages);
            assert_eq!(Vec::<Slot>::default(), ancient_slots);
            let one_epoch_old_slot = slot2 + 1;
            let ancient_slots =
                SplitAncientStorages::get_ancient_slots(one_epoch_old_slot, &snapshot_storages);
            assert_eq!(vec![slot1_ancient], ancient_slots);

            // ancient, non-ancient, ancient
            let raw_storages = vec![
                vec![ancient.clone()],
                vec![non_ancient_storage.clone()],
                vec![ancient3.new_storage().clone()],
            ];
            let snapshot_storages = SortedStorages::new(&raw_storages);
            let one_epoch_old_slot = 0;
            let ancient_slots =
                SplitAncientStorages::get_ancient_slots(one_epoch_old_slot, &snapshot_storages);
            assert_eq!(Vec::<Slot>::default(), ancient_slots);
            let one_epoch_old_slot = slot3_ancient + 1;
            let ancient_slots =
                SplitAncientStorages::get_ancient_slots(one_epoch_old_slot, &snapshot_storages);
            assert_eq!(vec![slot1_ancient], ancient_slots);

            if sparse {
                // ancient, ancient, non-ancient, ancient
                let raw_storages = vec![
                    vec![Arc::clone(&ancient)],
                    vec![Arc::clone(&ancient_1_plus)],
                    vec![non_ancient_storage],
                    vec![Arc::clone(ancient3.new_storage())],
                ];
                let snapshot_storages = SortedStorages::new(&raw_storages[..]);
                let one_epoch_old_slot = 0;
                let ancient_slots =
                    SplitAncientStorages::get_ancient_slots(one_epoch_old_slot, &snapshot_storages);
                assert_eq!(Vec::<Slot>::default(), ancient_slots);
                let one_epoch_old_slot = slot3_ancient + 1;
                let ancient_slots =
                    SplitAncientStorages::get_ancient_slots(one_epoch_old_slot, &snapshot_storages);
                assert_eq!(vec![slot1_ancient, slot1_plus_ancient], ancient_slots);
            }
        }
    }

    #[test]
    fn test_hash_storage_info() {
        {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            let storages = None;
            let slot = 1;
            let load = AccountsDb::hash_storage_info(&mut hasher, storages, slot);
            let hash = hasher.finish();
            assert_eq!(15130871412783076140, hash);
            assert!(load);
        }
        {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            let slot: Slot = 0;
            let tf = crate::append_vec::test_utils::get_append_vec_path(
                "test_accountsdb_scan_account_storage_no_bank",
            );
            let write_version1 = 0;
            let pubkey1 = solana_sdk::pubkey::new_rand();
            let storages = sample_storage_with_entries(&tf, write_version1, slot, &pubkey1);

            let load = AccountsDb::hash_storage_info(&mut hasher, Some(&storages[0]), slot);
            let hash = hasher.finish();
            // can't assert hash here - it is a function of mod date
            assert!(load);
            let slot = 2; // changed this
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            let load = AccountsDb::hash_storage_info(&mut hasher, Some(&storages[0]), slot);
            let hash2 = hasher.finish();
            assert_ne!(hash, hash2); // slot changed, these should be different
                                     // can't assert hash here - it is a function of mod date
            assert!(load);
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            append_sample_data_to_storage(
                &storages,
                &solana_sdk::pubkey::new_rand(),
                write_version1,
            );
            let load = AccountsDb::hash_storage_info(&mut hasher, Some(&storages[0]), slot);
            let hash3 = hasher.finish();
            assert_ne!(hash2, hash3); // moddate and written size changed
                                      // can't assert hash here - it is a function of mod date
            assert!(load);
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            let load = AccountsDb::hash_storage_info(&mut hasher, Some(&storages[0]), slot);
            let hash4 = hasher.finish();
            assert_eq!(hash4, hash3); // same
                                      // can't assert hash here - it is a function of mod date
            assert!(load);
        }
        {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            let slot: Slot = 0;
            let slot1 = 1;
            let tf = crate::append_vec::test_utils::get_append_vec_path(
                "test_accountsdb_scan_account_storage_no_bank",
            );
            let write_version1 = 0;
            let pubkey1 = solana_sdk::pubkey::new_rand();
            let mut storages = sample_storage_with_entries(&tf, write_version1, slot, &pubkey1);
            let mut storages2 = sample_storage_with_entries(&tf, write_version1, slot1, &pubkey1);
            storages[0].push(storages2[0].remove(0));

            let load = AccountsDb::hash_storage_info(&mut hasher, Some(&storages[0]), slot);
            let _ = hasher.finish();
            // cannot load because we have 2 storages
            assert!(!load);
        }
    }

    #[test]
    fn test_get_accounts_hash_complete_one_epoch_old() {
        let db = AccountsDb::new_single_for_tests();
        assert_eq!(db.get_accounts_hash_complete_one_epoch_old(), 0);
        let epoch_schedule = EpochSchedule::default();
        let completed_slot = epoch_schedule.slots_per_epoch;
        db.notify_accounts_hash_calculated_complete(completed_slot, &epoch_schedule);
        assert_eq!(db.get_accounts_hash_complete_one_epoch_old(), 0);
        let offset = 1;
        let completed_slot = completed_slot + offset;
        db.notify_accounts_hash_calculated_complete(completed_slot, &epoch_schedule);
        let earliest = AccountsDb::get_slot_one_epoch_prior(completed_slot, &epoch_schedule);
        assert_eq!(db.get_accounts_hash_complete_one_epoch_old(), earliest);
        let offset = 5;
        let completed_slot = completed_slot + offset;
        db.notify_accounts_hash_calculated_complete(completed_slot, &epoch_schedule);
        let earliest = AccountsDb::get_slot_one_epoch_prior(completed_slot, &epoch_schedule);
        assert_eq!(db.get_accounts_hash_complete_one_epoch_old(), earliest);
    }

    #[test]
    #[should_panic(expected = "called `Option::unwrap()` on a `None` value")]
    fn test_current_ancient_slot_assert() {
        let current_ancient = CurrentAncientAppendVec::default();
        _ = current_ancient.slot();
    }

    #[test]
    #[should_panic(expected = "called `Option::unwrap()` on a `None` value")]
    fn test_current_ancient_append_vec_assert() {
        let current_ancient = CurrentAncientAppendVec::default();
        _ = current_ancient.append_vec();
    }

    #[test]
    fn test_current_ancient_simple() {
        let slot = 1;
        let slot2 = 2;
        let slot3 = 3;
        {
            // new
            let db = AccountsDb::new_single_for_tests();
            let size = 1000;
            let append_vec = db.create_and_insert_store(slot, size, "test");
            let mut current_ancient = CurrentAncientAppendVec::new(slot, append_vec.clone());
            assert_eq!(current_ancient.slot(), slot);
            assert_eq!(current_ancient.append_vec_id(), append_vec.append_vec_id());
            assert_eq!(
                current_ancient.append_vec().append_vec_id(),
                append_vec.append_vec_id()
            );

            let _shrink_in_progress = current_ancient.create_if_necessary(slot2, &db);
            assert_eq!(current_ancient.slot(), slot);
            assert_eq!(current_ancient.append_vec_id(), append_vec.append_vec_id());
        }

        {
            // create_if_necessary
            let db = AccountsDb::new_single_for_tests();
            // there has to be an existing append vec at this slot for a new current ancient at the slot to make sense
            let _existing_append_vec = db.create_and_insert_store(slot2, 1000, "test");

            let mut current_ancient = CurrentAncientAppendVec::default();
            let mut _shrink_in_progress = current_ancient.create_if_necessary(slot2, &db);
            let id = current_ancient.append_vec_id();
            assert_eq!(current_ancient.slot(), slot2);
            assert!(is_ancient(&current_ancient.append_vec().accounts));
            let slot3 = 3;
            // should do nothing
            let _shrink_in_progress = current_ancient.create_if_necessary(slot3, &db);
            assert_eq!(current_ancient.slot(), slot2);
            assert_eq!(current_ancient.append_vec_id(), id);
            assert!(is_ancient(&current_ancient.append_vec().accounts));
        }

        {
            // create_ancient_append_vec
            let db = AccountsDb::new_single_for_tests();
            let mut current_ancient = CurrentAncientAppendVec::default();
            // there has to be an existing append vec at this slot for a new current ancient at the slot to make sense
            let _existing_append_vec = db.create_and_insert_store(slot2, 1000, "test");

            {
                let _shrink_in_progress = current_ancient.create_ancient_append_vec(slot2, &db);
            }
            let id = current_ancient.append_vec_id();
            assert_eq!(current_ancient.slot(), slot2);
            assert!(is_ancient(&current_ancient.append_vec().accounts));

            // there has to be an existing append vec at this slot for a new current ancient at the slot to make sense
            let _existing_append_vec = db.create_and_insert_store(slot3, 1000, "test");

            let mut _shrink_in_progress = current_ancient.create_ancient_append_vec(slot3, &db);
            assert_eq!(current_ancient.slot(), slot3);
            assert!(is_ancient(&current_ancient.append_vec().accounts));
            assert_ne!(current_ancient.append_vec_id(), id);
        }
    }

    #[test]
    fn test_get_sorted_potential_ancient_slots() {
        let db = AccountsDb::new_single_for_tests();
        assert!(db.get_sorted_potential_ancient_slots().is_empty());
        let root0 = 0;
        db.add_root(root0);
        let root1 = 1;
        let root2 = 2;
        db.add_root(root1);
        assert!(db.get_sorted_potential_ancient_slots().is_empty());
        let epoch_schedule = EpochSchedule::default();
        let completed_slot = epoch_schedule.slots_per_epoch;
        db.notify_accounts_hash_calculated_complete(completed_slot, &epoch_schedule);
        // get_sorted_potential_ancient_slots uses 'less than' as opposed to 'less or equal'
        // so, we need to get more than an epoch away to get the first valid root
        assert!(db.get_sorted_potential_ancient_slots().is_empty());
        let completed_slot = epoch_schedule.slots_per_epoch + root1;
        db.notify_accounts_hash_calculated_complete(completed_slot, &epoch_schedule);
        assert_eq!(db.get_sorted_potential_ancient_slots(), vec![root0]);
        let completed_slot = epoch_schedule.slots_per_epoch + root2;
        db.notify_accounts_hash_calculated_complete(completed_slot, &epoch_schedule);
        assert_eq!(db.get_sorted_potential_ancient_slots(), vec![root0, root1]);
        db.accounts_index
            .roots_tracker
            .write()
            .unwrap()
            .alive_roots
            .remove(&root0);
        assert_eq!(db.get_sorted_potential_ancient_slots(), vec![root1]);
    }

    #[test]
    fn test_shrink_collect_simple() {
        solana_logger::setup();
        let account_counts = [
            1,
            SHRINK_COLLECT_CHUNK_SIZE,
            SHRINK_COLLECT_CHUNK_SIZE + 1,
            SHRINK_COLLECT_CHUNK_SIZE * 2,
        ];
        // 2 = append_opposite_alive_account + append_opposite_zero_lamport_account
        let max_appended_accounts = 2;
        let max_num_accounts = *account_counts.iter().max().unwrap();
        let pubkeys = (0..(max_num_accounts + max_appended_accounts))
            .map(|_| solana_sdk::pubkey::new_rand())
            .collect::<Vec<_>>();
        // write accounts, maybe remove from index
        // check shrink_collect results
        for lamports in [0, 1] {
            for space in [0, 8] {
                if lamports == 0 && space != 0 {
                    // illegal - zero lamport accounts are written with 0 space
                    continue;
                }
                for alive in [false, true] {
                    for append_opposite_alive_account in [false, true] {
                        for append_opposite_zero_lamport_account in [true, false] {
                            for mut account_count in account_counts {
                                let mut normal_account_count = account_count;
                                let mut pubkey_opposite_zero_lamports = None;
                                if append_opposite_zero_lamport_account {
                                    pubkey_opposite_zero_lamports = Some(&pubkeys[account_count]);
                                    normal_account_count += 1;
                                    account_count += 1;
                                }
                                let mut pubkey_opposite_alive = None;
                                if append_opposite_alive_account {
                                    // this needs to happen AFTER append_opposite_zero_lamport_account
                                    pubkey_opposite_alive = Some(&pubkeys[account_count]);
                                    account_count += 1;
                                }
                                debug!("space: {space}, lamports: {lamports}, alive: {alive}, account_count: {account_count}, append_opposite_alive_account: {append_opposite_alive_account}, append_opposite_zero_lamport_account: {append_opposite_zero_lamport_account}, normal_account_count: {normal_account_count}");
                                let db = AccountsDb::new_single_for_tests();
                                let slot5 = 5;
                                let mut account = AccountSharedData::new(
                                    lamports,
                                    space,
                                    AccountSharedData::default().owner(),
                                );
                                let mut to_purge = Vec::default();
                                for pubkey in pubkeys.iter().take(account_count) {
                                    // store in append vec and index
                                    let old_lamports = account.lamports();
                                    if Some(pubkey) == pubkey_opposite_zero_lamports {
                                        account.set_lamports(u64::from(old_lamports == 0));
                                    }

                                    db.store_for_tests(slot5, &[(pubkey, &account)]);
                                    account.set_lamports(old_lamports);
                                    let mut alive = alive;
                                    if append_opposite_alive_account
                                        && Some(pubkey) == pubkey_opposite_alive
                                    {
                                        // invert this for one special pubkey
                                        alive = !alive;
                                    }
                                    if !alive {
                                        // remove from index so pubkey is 'dead'
                                        to_purge.push(*pubkey);
                                    }
                                }
                                db.add_root_and_flush_write_cache(slot5);
                                to_purge.iter().for_each(|pubkey| {
                                    db.accounts_index.purge_exact(
                                        pubkey,
                                        &([slot5].into_iter().collect::<HashSet<_>>()),
                                        &mut Vec::default(),
                                    );
                                });

                                let storage = db.get_storage_for_slot(slot5).unwrap();
                                let mut stored_accounts = Vec::default();
                                let shrink_collect = db.shrink_collect(
                                    std::iter::once(&storage),
                                    &mut stored_accounts,
                                    &ShrinkStats::default(),
                                );
                                let expect_single_opposite_alive_account =
                                    if append_opposite_alive_account {
                                        vec![*pubkey_opposite_alive.unwrap()]
                                    } else {
                                        vec![]
                                    };

                                let expected_alive_accounts = if alive {
                                    pubkeys[..normal_account_count]
                                        .iter()
                                        .filter(|p| Some(p) != pubkey_opposite_alive.as_ref())
                                        .sorted()
                                        .cloned()
                                        .collect::<Vec<_>>()
                                } else {
                                    expect_single_opposite_alive_account.clone()
                                };

                                let expected_unrefed = if alive {
                                    expect_single_opposite_alive_account.clone()
                                } else {
                                    pubkeys[..normal_account_count]
                                        .iter()
                                        .sorted()
                                        .cloned()
                                        .collect::<Vec<_>>()
                                };

                                assert_eq!(
                                    shrink_collect
                                        .alive_accounts
                                        .iter()
                                        .map(|account| *account.pubkey())
                                        .sorted()
                                        .collect::<Vec<_>>(),
                                    expected_alive_accounts
                                );
                                assert_eq!(
                                    shrink_collect
                                        .unrefed_pubkeys
                                        .iter()
                                        .sorted()
                                        .cloned()
                                        .cloned()
                                        .collect::<Vec<_>>(),
                                    expected_unrefed
                                );

                                let alive_total_one_account = 136 + space;
                                if alive {
                                    assert_eq!(
                                        shrink_collect.aligned_total_bytes,
                                        PAGE_SIZE
                                            * if account_count >= 100 {
                                                4
                                            } else if account_count >= 50 {
                                                2
                                            } else {
                                                1
                                            }
                                    );
                                    let mut expected_alive_total_bytes =
                                        alive_total_one_account * normal_account_count;
                                    if append_opposite_zero_lamport_account {
                                        // zero lamport accounts store size=0 data
                                        expected_alive_total_bytes -= space;
                                    }
                                    assert_eq!(
                                        shrink_collect.alive_total_bytes,
                                        expected_alive_total_bytes
                                    );
                                } else if append_opposite_alive_account {
                                    assert_eq!(shrink_collect.aligned_total_bytes, 4096);
                                    assert_eq!(
                                        shrink_collect.alive_total_bytes,
                                        alive_total_one_account
                                    );
                                } else {
                                    assert_eq!(shrink_collect.aligned_total_bytes, 0);
                                    assert_eq!(shrink_collect.alive_total_bytes, 0);
                                }
                                // these constants are multiples of page size (4096).
                                // They are determined by what size append vec gets created when the write cache is flushed to an append vec.
                                // Thus, they are dependent on the # of accounts that are written. They were identified by hitting the asserts and noting the value
                                // for shrink_collect.original_bytes at each account_count and then encoding it here.
                                let expected_original_bytes = if account_count >= 100 {
                                    16384
                                } else if account_count >= 50 {
                                    8192
                                } else {
                                    4096
                                };
                                assert_eq!(shrink_collect.original_bytes, expected_original_bytes);
                                assert_eq!(shrink_collect.total_starting_accounts, account_count);
                                let mut expected_all_are_zero_lamports = lamports == 0;
                                if !append_opposite_alive_account {
                                    expected_all_are_zero_lamports |= !alive;
                                }
                                if append_opposite_zero_lamport_account && lamports == 0 && alive {
                                    expected_all_are_zero_lamports =
                                        !expected_all_are_zero_lamports;
                                }
                                assert_eq!(
                                    shrink_collect.all_are_zero_lamports,
                                    expected_all_are_zero_lamports
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    const CAN_RANDOMLY_SHRINK_FALSE: bool = false;

    #[test]
    fn test_combine_ancient_slots_empty() {
        solana_logger::setup();
        let db = AccountsDb::new_single_for_tests();
        // empty slots
        db.combine_ancient_slots(Vec::default(), CAN_RANDOMLY_SHRINK_FALSE);
    }

    #[test]
    fn test_combine_ancient_slots_simple() {
        for alive in [false, true] {
            _ = get_one_ancient_append_vec_and_others(alive, 0);
        }
    }

    fn get_all_accounts(db: &AccountsDb, slots: Range<Slot>) -> Vec<(Pubkey, AccountSharedData)> {
        slots
            .clone()
            .filter_map(|slot| {
                let storages = db.get_storages_for_slot(slot);
                storages.map(|storages| {
                    assert_eq!(storages.len(), 1, "slot: {slot}, slots: {slots:?}");
                    let storage = storages.first().unwrap();
                    storage
                        .accounts
                        .account_iter()
                        .map(|account| (*account.pubkey(), account.to_account_shared_data()))
                        .collect::<Vec<_>>()
                })
            })
            .flatten()
            .collect::<Vec<_>>()
    }

    fn compare_all_accounts(
        one: &[(Pubkey, AccountSharedData)],
        two: &[(Pubkey, AccountSharedData)],
    ) {
        let mut two_indexes = (0..two.len()).collect::<Vec<_>>();
        one.iter().for_each(|(pubkey, account)| {
            for i in 0..two_indexes.len() {
                let pubkey2 = two[two_indexes[i]].0;
                if pubkey2 == *pubkey {
                    assert!(accounts_equal(account, &two[i].1));
                    two_indexes.remove(i);
                    break;
                }
            }
        });
        assert!(
            two_indexes.is_empty(),
            "one: {one:?}, two: {two:?}, two_indexes: {two_indexes:?}"
        );
    }

    #[test]
    fn test_shrink_ancient_overflow() {
        solana_logger::setup();

        let num_normal_slots = 2;
        // build an ancient append vec at slot 'ancient_slot'
        let (db, ancient_slot) = get_one_ancient_append_vec_and_others(true, num_normal_slots);

        let max_slot_inclusive = ancient_slot + (num_normal_slots as Slot);
        let initial_accounts = get_all_accounts(&db, ancient_slot..(max_slot_inclusive + 1));

        let ancient = db.storage.get_slot_storage_entry(ancient_slot).unwrap();
        let initial_len = ancient.alive_bytes();
        // set size of ancient to be 'full'
        adjust_append_vec_len_for_tests(&ancient, ancient.accounts.capacity() as usize);

        // combine 1 normal append vec into existing ancient append vec
        // this will overflow the original ancient append vec because of the marking full above
        db.combine_ancient_slots(
            (ancient_slot..max_slot_inclusive).collect(),
            CAN_RANDOMLY_SHRINK_FALSE,
        );

        // Restore size of ancient so we don't read garbage accounts when comparing. Now that we have created a second ancient append vec,
        // This first one is happy to be quite empty.
        adjust_append_vec_len_for_tests(&ancient, initial_len);

        compare_all_accounts(
            &initial_accounts,
            &get_all_accounts(&db, ancient_slot..max_slot_inclusive),
        );

        // the append vec at max_slot_inclusive-1 should NOT have been removed since we created an ancient append vec there
        assert!(is_ancient(
            &db.storage
                .get_slot_storage_entry(max_slot_inclusive - 1)
                .unwrap()
                .accounts
        ));

        // combine normal append vec(s) into existing ancient append vec
        // this will overflow the original ancient append vec because of the marking full above
        db.combine_ancient_slots(
            (ancient_slot..=max_slot_inclusive).collect(),
            CAN_RANDOMLY_SHRINK_FALSE,
        );

        // now, combine the next slot into the one that was just overflow
        compare_all_accounts(
            &initial_accounts,
            &get_all_accounts(&db, ancient_slot..(max_slot_inclusive + 1)),
        );

        // 2 ancients and then missing (because combined into 2nd ancient)
        assert!(is_ancient(
            &db.storage
                .get_slot_storage_entry(ancient_slot)
                .unwrap()
                .accounts
        ));
        assert!(is_ancient(
            &db.storage
                .get_slot_storage_entry(max_slot_inclusive - 1)
                .unwrap()
                .accounts
        ));
        assert!(db.get_storages_for_slot(max_slot_inclusive).is_none());
    }

    #[test]
    fn test_shrink_ancient() {
        solana_logger::setup();

        let num_normal_slots = 1;
        // build an ancient append vec at slot 'ancient_slot'
        let (db, ancient_slot) = get_one_ancient_append_vec_and_others(true, num_normal_slots);

        let max_slot_inclusive = ancient_slot + (num_normal_slots as Slot);
        let initial_accounts = get_all_accounts(&db, ancient_slot..(max_slot_inclusive + 1));
        compare_all_accounts(
            &initial_accounts,
            &get_all_accounts(&db, ancient_slot..(max_slot_inclusive + 1)),
        );

        // combine normal append vec(s) into existing ancient append vec
        db.combine_ancient_slots(
            (ancient_slot..=max_slot_inclusive).collect(),
            CAN_RANDOMLY_SHRINK_FALSE,
        );

        compare_all_accounts(
            &initial_accounts,
            &get_all_accounts(&db, ancient_slot..(max_slot_inclusive + 1)),
        );

        // create a 2nd ancient append vec at 'next_slot'
        let tf = crate::append_vec::test_utils::get_append_vec_path("test_shrink_ancient");
        let next_slot = max_slot_inclusive + 1;
        create_storages_and_update_index(&db, &tf, next_slot, num_normal_slots, true);
        let max_slot_inclusive = next_slot + (num_normal_slots as Slot);

        let initial_accounts = get_all_accounts(&db, ancient_slot..(max_slot_inclusive + 1));
        compare_all_accounts(
            &initial_accounts,
            &get_all_accounts(&db, ancient_slot..(max_slot_inclusive + 1)),
        );

        db.combine_ancient_slots(
            (next_slot..=max_slot_inclusive).collect(),
            CAN_RANDOMLY_SHRINK_FALSE,
        );

        compare_all_accounts(
            &initial_accounts,
            &get_all_accounts(&db, ancient_slot..(max_slot_inclusive + 1)),
        );

        // now, shrink the second ancient append vec into the first one
        let mut current_ancient = CurrentAncientAppendVec::new(
            ancient_slot,
            db.get_storage_for_slot(ancient_slot).unwrap(),
        );
        let mut dropped_roots = Vec::default();
        db.combine_one_store_into_ancient(
            next_slot,
            &[db.get_storage_for_slot(next_slot).unwrap()],
            &mut current_ancient,
            &mut AncientSlotPubkeys::default(),
            &mut dropped_roots,
        );
        assert!(db.storage.is_empty_entry(next_slot));
        // this removes the storages entry completely from the hashmap for 'next_slot'.
        // Otherwise, we have a zero length vec in that hashmap
        db.handle_dropped_roots_for_ancient(dropped_roots);
        assert!(db.get_storages_for_slot(next_slot).is_none());

        // include all the slots we put into the ancient append vec - they should contain nothing
        compare_all_accounts(
            &initial_accounts,
            &get_all_accounts(&db, ancient_slot..(max_slot_inclusive + 1)),
        );
        // look at just the ancient append vec
        compare_all_accounts(
            &initial_accounts,
            &get_all_accounts(&db, ancient_slot..(ancient_slot + 1)),
        );
        // make sure there is only 1 ancient append vec at the ancient slot
        assert_eq!(db.get_storages_for_slot(ancient_slot).unwrap().len(), 1);
        assert!(is_ancient(
            &db.storage
                .get_slot_storage_entry(ancient_slot)
                .unwrap()
                .accounts
        ));
        ((ancient_slot + 1)..=max_slot_inclusive)
            .for_each(|slot| assert!(db.get_storages_for_slot(slot).is_none()));
    }

    #[test]
    fn test_combine_ancient_slots_append() {
        solana_logger::setup();
        // combine 2-4 slots into a single ancient append vec
        for num_normal_slots in 1..3 {
            // but some slots contain only dead accounts
            for dead_accounts in 0..=num_normal_slots {
                let mut originals = Vec::default();
                // ancient_slot: contains ancient append vec
                // ancient_slot + 1: contains normal append vec with 1 alive account
                let (db, ancient_slot) =
                    get_one_ancient_append_vec_and_others(true, num_normal_slots);

                let max_slot_inclusive = ancient_slot + (num_normal_slots as Slot);

                for slot in ancient_slot..=max_slot_inclusive {
                    originals.push(db.get_storage_for_slot(slot).unwrap());
                }

                {
                    // remove the intended dead slots from the index so they look dead
                    for (count_marked_dead, original) in originals.iter().skip(1).enumerate() {
                        // skip the ancient one
                        if count_marked_dead >= dead_accounts {
                            break;
                        }
                        let original = original.accounts.account_iter().next().unwrap();
                        let slot = ancient_slot + 1 + (count_marked_dead as Slot);
                        _ = db.purge_keys_exact(
                            [(
                                *original.pubkey(),
                                vec![slot].into_iter().collect::<HashSet<_>>(),
                            )]
                            .iter(),
                        );
                    }
                    // the entries from these original append vecs should not expect to be in the final ancient append vec
                    for _ in 0..dead_accounts {
                        originals.remove(1); // remove the first non-ancient original entry each time
                    }
                }

                // combine normal append vec(s) into existing ancient append vec
                db.combine_ancient_slots(
                    (ancient_slot..=max_slot_inclusive).collect(),
                    CAN_RANDOMLY_SHRINK_FALSE,
                );

                // normal slots should have been appended to the ancient append vec in the first slot
                assert_eq!(1, db.get_storages_for_slot(ancient_slot).unwrap().len());
                let ancient = db.get_storage_for_slot(ancient_slot).unwrap();
                assert!(is_ancient(&ancient.accounts));
                for slot in (ancient_slot + 1)..=max_slot_inclusive {
                    assert!(db.get_storages_for_slot(slot).is_none());
                }

                let GetUniqueAccountsResult {
                    stored_accounts: mut after_stored_accounts,
                    ..
                } = db.get_unique_accounts_from_storages(std::iter::once(&ancient));
                assert_eq!(
                    after_stored_accounts.len(),
                    num_normal_slots + 1 - dead_accounts,
                    "normal_slots: {num_normal_slots}, dead_accounts: {dead_accounts}"
                );
                for original in &originals {
                    let original = original.accounts.account_iter().next().unwrap();

                    let i = after_stored_accounts
                        .iter()
                        .enumerate()
                        .find_map(|(i, stored_ancient)| {
                            (stored_ancient.pubkey() == original.pubkey()).then_some({
                                assert!(accounts_equal(&stored_ancient.account, &original));
                                i
                            })
                        })
                        .expect("did not find account");
                    after_stored_accounts.remove(i);
                }
                assert!(
                    after_stored_accounts.is_empty(),
                    "originals: {}, num_normal_slots: {}",
                    originals.len(),
                    num_normal_slots
                );
            }
        }
    }

    fn populate_index(db: &AccountsDb, slots: Range<Slot>) {
        slots.into_iter().for_each(|slot| {
            if let Some(storage) = db.get_storage_for_slot(slot) {
                storage.accounts.account_iter().for_each(|account| {
                    let info = AccountInfo::new(
                        StorageLocation::AppendVec(storage.append_vec_id(), account.offset),
                        account.stored_size as u32,
                        account.lamports(),
                    );
                    db.accounts_index.upsert(
                        slot,
                        slot,
                        account.pubkey(),
                        &account,
                        &AccountSecondaryIndexes::default(),
                        info,
                        &mut Vec::default(),
                        UpsertReclaim::IgnoreReclaims,
                    );
                })
            }
        })
    }

    fn create_storages_and_update_index(
        db: &AccountsDb,
        tf: &TempFile,
        starting_slot: Slot,
        num_slots: usize,
        alive: bool,
    ) {
        let write_version1 = 0;
        let starting_id = 999;
        for i in 0..num_slots {
            let id = starting_id + (i as AppendVecId);
            let pubkey1 = solana_sdk::pubkey::new_rand();
            let storages = sample_storage_with_entries_id(
                tf,
                write_version1,
                starting_slot + (i as Slot),
                &pubkey1,
                id,
            )
            .pop()
            .unwrap();
            insert_store(db, Arc::clone(&storages[0]));
        }

        let storage = db.get_storage_for_slot(starting_slot).unwrap();
        let created_accounts = db.get_unique_accounts_from_storages(std::iter::once(&storage));
        assert_eq!(created_accounts.stored_accounts.len(), 1);

        if alive {
            populate_index(db, starting_slot..(starting_slot + (num_slots as Slot) + 1));
        }
    }

    fn create_db_with_storages_and_index(alive: bool, num_slots: usize) -> (AccountsDb, Slot) {
        solana_logger::setup();

        let db = AccountsDb::new_single_for_tests();

        // create a single append vec with a single account in a slot
        // add the pubkey to index if alive
        // call combine_ancient_slots with the slot
        // verify we create an ancient appendvec that has alive accounts and does not have dead accounts

        let slot1 = 1;
        let tf = crate::append_vec::test_utils::get_append_vec_path(
            "get_one_ancient_append_vec_and_others",
        );
        create_storages_and_update_index(&db, &tf, slot1, num_slots, alive);

        let slot1 = slot1 as Slot;
        (db, slot1)
    }

    fn get_one_ancient_append_vec_and_others(
        alive: bool,
        num_normal_slots: usize,
    ) -> (AccountsDb, Slot) {
        let (db, slot1) = create_db_with_storages_and_index(alive, num_normal_slots + 1);
        let storage = db.get_storage_for_slot(slot1).unwrap();
        let created_accounts = db.get_unique_accounts_from_storages(std::iter::once(&storage));

        db.combine_ancient_slots(vec![slot1], CAN_RANDOMLY_SHRINK_FALSE);
        assert_eq!(1, db.get_storages_for_slot(slot1).unwrap().len());
        let ancient = db.get_storage_for_slot(slot1).unwrap();
        assert!(is_ancient(&ancient.accounts));
        let after_store = db.get_storage_for_slot(slot1).unwrap();
        let GetUniqueAccountsResult {
            stored_accounts: after_stored_accounts,
            original_bytes: after_original_bytes,
        } = db.get_unique_accounts_from_storages(std::iter::once(&after_store));
        assert_ne!(created_accounts.original_bytes, after_original_bytes);
        assert_eq!(created_accounts.stored_accounts.len(), 1);
        assert_eq!(after_stored_accounts.len(), usize::from(alive));
        (db, slot1)
    }

    #[test]
    fn test_handle_dropped_roots_for_ancient() {
        solana_logger::setup();
        let db = AccountsDb::new_single_for_tests();
        db.handle_dropped_roots_for_ancient(Vec::default());
        let slot0 = 0;
        let dropped_roots = vec![slot0];
        db.bank_hashes
            .write()
            .unwrap()
            .insert(slot0, BankHashInfo::default());
        db.storage.insert_empty_at_slot(slot0);
        assert!(!db.bank_hashes.read().unwrap().is_empty());
        db.accounts_index.add_root(slot0);
        db.accounts_index.add_uncleaned_roots([slot0].into_iter());
        assert!(db.accounts_index.is_uncleaned_root(slot0));
        assert!(db.accounts_index.is_alive_root(slot0));
        db.handle_dropped_roots_for_ancient(dropped_roots);
        assert!(db.bank_hashes.read().unwrap().is_empty());
        assert!(!db.accounts_index.is_uncleaned_root(slot0));
        assert!(!db.accounts_index.is_alive_root(slot0));
    }

    fn insert_store(db: &AccountsDb, append_vec: Arc<AccountStorageEntry>) {
        db.storage.insert(append_vec.slot(), append_vec);
    }

    #[test]
    #[should_panic(
        expected = "assertion failed: self.storage.remove(slot).unwrap().1.read().unwrap().is_empty()"
    )]
    fn test_handle_dropped_roots_for_ancient_assert() {
        solana_logger::setup();
        let common_store_path = Path::new("");
        let store_file_size = 2 * PAGE_SIZE;
        let entry = Arc::new(AccountStorageEntry::new(
            common_store_path,
            0,
            1,
            store_file_size,
        ));
        let db = AccountsDb::new_single_for_tests();
        let slot0 = 0;
        let dropped_roots = vec![slot0];
        db.bank_hashes
            .write()
            .unwrap()
            .insert(slot0, BankHashInfo::default());
        insert_store(&db, entry);
        db.handle_dropped_roots_for_ancient(dropped_roots);
    }

    #[test]
    fn test_should_move_to_ancient_append_vec() {
        solana_logger::setup();
        let db = AccountsDb::new_single_for_tests();
        let slot5 = 5;
        let tf = crate::append_vec::test_utils::get_append_vec_path(
            "test_should_move_to_ancient_append_vec",
        );
        let write_version1 = 0;
        let pubkey1 = solana_sdk::pubkey::new_rand();
        let storages = sample_storage_with_entries(&tf, write_version1, slot5, &pubkey1)
            .pop()
            .unwrap();
        let mut current_ancient = CurrentAncientAppendVec::default();

        let should_move = db.should_move_to_ancient_append_vec(
            &storages[0],
            &mut current_ancient,
            slot5,
            CAN_RANDOMLY_SHRINK_FALSE,
        );
        assert!(current_ancient.slot_and_append_vec.is_none());
        // slot is not ancient, so it is good to move
        assert!(should_move);

        current_ancient = CurrentAncientAppendVec::new(slot5, Arc::clone(&storages[0])); // just 'some', contents don't matter
        let should_move = db.should_move_to_ancient_append_vec(
            &storages[0],
            &mut current_ancient,
            slot5,
            CAN_RANDOMLY_SHRINK_FALSE,
        );
        // should have kept the same 'current_ancient'
        assert_eq!(current_ancient.slot(), slot5);
        assert_eq!(current_ancient.append_vec().slot(), slot5);
        assert_eq!(current_ancient.append_vec_id(), storages[0].append_vec_id());

        // slot is not ancient, so it is good to move
        assert!(should_move);

        // now, create an ancient slot and make sure that it does NOT think it needs to be moved and that it becomes the ancient append vec to use
        let mut current_ancient = CurrentAncientAppendVec::default();
        let slot1_ancient = 1;
        // there has to be an existing append vec at this slot for a new current ancient at the slot to make sense
        let _existing_append_vec = db.create_and_insert_store(slot1_ancient, 1000, "test");
        let ancient1 = db
            .create_ancient_append_vec(slot1_ancient)
            .new_storage()
            .clone();
        let should_move = db.should_move_to_ancient_append_vec(
            &ancient1,
            &mut current_ancient,
            slot1_ancient,
            CAN_RANDOMLY_SHRINK_FALSE,
        );
        assert!(!should_move);
        assert_eq!(current_ancient.append_vec_id(), ancient1.append_vec_id());
        assert_eq!(current_ancient.slot(), slot1_ancient);

        // current is ancient1
        // try to move ancient2
        // current should become ancient2
        let slot2_ancient = 2;
        let mut current_ancient = CurrentAncientAppendVec::new(slot1_ancient, ancient1.clone());
        // there has to be an existing append vec at this slot for a new current ancient at the slot to make sense
        let _existing_append_vec = db.create_and_insert_store(slot2_ancient, 1000, "test");
        let ancient2 = db
            .create_ancient_append_vec(slot2_ancient)
            .new_storage()
            .clone();
        let should_move = db.should_move_to_ancient_append_vec(
            &ancient2,
            &mut current_ancient,
            slot2_ancient,
            CAN_RANDOMLY_SHRINK_FALSE,
        );
        assert!(!should_move);
        assert_eq!(current_ancient.append_vec_id(), ancient2.append_vec_id());
        assert_eq!(current_ancient.slot(), slot2_ancient);

        // now try a full ancient append vec
        // current is None
        let slot3_full_ancient = 3;
        let mut current_ancient = CurrentAncientAppendVec::default();
        // there has to be an existing append vec at this slot for a new current ancient at the slot to make sense
        let _existing_append_vec = db.create_and_insert_store(slot3_full_ancient, 1000, "test");
        let full_ancient_3 = make_full_ancient_append_vec(&db, slot3_full_ancient);
        let should_move = db.should_move_to_ancient_append_vec(
            &full_ancient_3.new_storage().clone(),
            &mut current_ancient,
            slot3_full_ancient,
            CAN_RANDOMLY_SHRINK_FALSE,
        );
        assert!(!should_move);
        assert_eq!(
            current_ancient.append_vec_id(),
            full_ancient_3.new_storage().append_vec_id()
        );
        assert_eq!(current_ancient.slot(), slot3_full_ancient);

        // now set current_ancient to something
        let mut current_ancient = CurrentAncientAppendVec::new(slot1_ancient, ancient1.clone());
        let should_move = db.should_move_to_ancient_append_vec(
            &full_ancient_3.new_storage().clone(),
            &mut current_ancient,
            slot3_full_ancient,
            CAN_RANDOMLY_SHRINK_FALSE,
        );
        assert!(!should_move);
        assert_eq!(
            current_ancient.append_vec_id(),
            full_ancient_3.new_storage().append_vec_id()
        );
        assert_eq!(current_ancient.slot(), slot3_full_ancient);

        // now mark the full ancient as candidate for shrink
        adjust_alive_bytes(full_ancient_3.new_storage(), 0);

        // should shrink here, returning none for current
        let mut current_ancient = CurrentAncientAppendVec::default();
        let should_move = db.should_move_to_ancient_append_vec(
            &full_ancient_3.new_storage().clone(),
            &mut current_ancient,
            slot3_full_ancient,
            CAN_RANDOMLY_SHRINK_FALSE,
        );
        assert!(should_move);
        assert!(current_ancient.slot_and_append_vec.is_none());

        // should return true here, returning current from prior
        // now set current_ancient to something and see if it still goes to None
        let mut current_ancient = CurrentAncientAppendVec::new(slot1_ancient, ancient1.clone());
        let should_move = db.should_move_to_ancient_append_vec(
            &Arc::clone(full_ancient_3.new_storage()),
            &mut current_ancient,
            slot3_full_ancient,
            CAN_RANDOMLY_SHRINK_FALSE,
        );
        assert!(should_move);
        assert_eq!(current_ancient.append_vec_id(), ancient1.append_vec_id());
        assert_eq!(current_ancient.slot(), slot1_ancient);
    }

    fn adjust_alive_bytes(storage: &Arc<AccountStorageEntry>, alive_bytes: usize) {
        storage.alive_bytes.store(alive_bytes, Ordering::Release);
    }

    /// cause 'ancient' to appear to contain 'len' bytes
    fn adjust_append_vec_len_for_tests(ancient: &Arc<AccountStorageEntry>, len: usize) {
        assert!(is_ancient(&ancient.accounts));
        ancient.accounts.set_current_len_for_tests(len);
        adjust_alive_bytes(ancient, len);
    }

    fn make_ancient_append_vec_full(ancient: &Arc<AccountStorageEntry>) {
        let vecs = vec![vec![ancient.clone()]];
        for _ in 0..100 {
            append_sample_data_to_storage(&vecs, &Pubkey::default(), 0);
        }
        // since we're not adding to the index, this is how we specify that all these accounts are alive
        adjust_alive_bytes(ancient, ancient.total_bytes() as usize);
    }

    fn make_full_ancient_append_vec(db: &AccountsDb, slot: Slot) -> ShrinkInProgress<'_> {
        let full = db.create_ancient_append_vec(slot);
        make_ancient_append_vec_full(full.new_storage());
        full
    }
}

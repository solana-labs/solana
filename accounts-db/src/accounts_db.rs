//! Persistent accounts are stored at this path location:
//!  `<path>/<pid>/data/`
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
//! [`AppendVec`]'s only store accounts for single slots.  To bootstrap the
//! index from a persistent store of [`AppendVec`]'s, the entries include
//! a "write_version".  A single global atomic `AccountsDb::write_version`
//! tracks the number of commits to the entire data store. So the latest
//! commit for each slot entry would be indexed.

mod geyser_plugin_utils;
mod scan_account_storage;

#[cfg(feature = "dev-context-only-utils")]
use qualifier_attr::qualifiers;
use {
    crate::{
        account_info::{AccountInfo, StorageLocation},
        account_storage::{
            meta::StoredAccountMeta, AccountStorage, AccountStorageStatus, ShrinkInProgress,
        },
        accounts_cache::{AccountsCache, CachedAccount, SlotCache},
        accounts_file::{
            AccountsFile, AccountsFileError, AccountsFileProvider, MatchAccountOwnerError,
            StorageAccess, ALIGN_BOUNDARY_OFFSET,
        },
        accounts_hash::{
            AccountHash, AccountLtHash, AccountsDeltaHash, AccountsHash, AccountsHashKind,
            AccountsHasher, CalcAccountsHashConfig, CalculateHashIntermediate, HashStats,
            IncrementalAccountsHash, SerdeAccountsDeltaHash, SerdeAccountsHash,
            SerdeIncrementalAccountsHash, ZeroLamportAccounts, ZERO_LAMPORT_ACCOUNT_HASH,
            ZERO_LAMPORT_ACCOUNT_LT_HASH,
        },
        accounts_index::{
            in_mem_accounts_index::StartupStats, AccountMapEntry, AccountSecondaryIndexes,
            AccountsIndex, AccountsIndexConfig, AccountsIndexRootsStats, AccountsIndexScanResult,
            DiskIndexValue, IndexKey, IndexValue, IsCached, RefCount, ScanConfig, ScanResult,
            SlotList, UpsertReclaim, ZeroLamport, ACCOUNTS_INDEX_CONFIG_FOR_BENCHMARKS,
            ACCOUNTS_INDEX_CONFIG_FOR_TESTING,
        },
        accounts_index_storage::Startup,
        accounts_partition::RentPayingAccountsByPartition,
        accounts_update_notifier_interface::AccountsUpdateNotifier,
        active_stats::{ActiveStatItem, ActiveStats},
        ancestors::Ancestors,
        ancient_append_vecs::{
            get_ancient_append_vec_capacity, is_ancient, AccountsToStore, StorageSelector,
        },
        append_vec::{
            aligned_stored_size, APPEND_VEC_MMAPPED_FILES_DIRTY, APPEND_VEC_MMAPPED_FILES_OPEN,
            APPEND_VEC_OPEN_AS_FILE_IO, STORE_META_OVERHEAD,
        },
        cache_hash_data::{CacheHashData, DeletionPolicy as CacheHashDeletionPolicy},
        contains::Contains,
        epoch_accounts_hash::EpochAccountsHashManager,
        partitioned_rewards::{PartitionedEpochRewardsConfig, TestPartitionedEpochRewards},
        read_only_accounts_cache::ReadOnlyAccountsCache,
        sorted_storages::SortedStorages,
        storable_accounts::{StorableAccounts, StorableAccountsBySlot},
        u64_align, utils,
        verify_accounts_hash_in_background::VerifyAccountsHashInBackground,
    },
    crossbeam_channel::{unbounded, Receiver, Sender},
    dashmap::{DashMap, DashSet},
    log::*,
    rand::{thread_rng, Rng},
    rayon::{prelude::*, ThreadPool},
    seqlock::SeqLock,
    serde::{Deserialize, Serialize},
    smallvec::SmallVec,
    solana_lattice_hash::lt_hash::LtHash,
    solana_measure::{measure::Measure, measure_us},
    solana_nohash_hasher::{IntMap, IntSet},
    solana_rayon_threadlimit::get_thread_count,
    solana_sdk::{
        account::{Account, AccountSharedData, ReadableAccount},
        clock::{BankId, Epoch, Slot},
        epoch_schedule::EpochSchedule,
        genesis_config::{ClusterType, GenesisConfig},
        hash::Hash,
        pubkey::Pubkey,
        rent_collector::RentCollector,
        timing::AtomicInterval,
        transaction::SanitizedTransaction,
    },
    std::{
        borrow::Cow,
        boxed::Box,
        collections::{BTreeSet, HashMap, HashSet},
        fs,
        hash::{Hash as StdHash, Hasher as StdHasher},
        io::Result as IoResult,
        num::Saturating,
        ops::{Range, RangeBounds},
        path::{Path, PathBuf},
        sync::{
            atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering},
            Arc, Condvar, Mutex,
        },
        thread::{sleep, Builder},
        time::{Duration, Instant},
    },
    tempfile::TempDir,
};

// when the accounts write cache exceeds this many bytes, we will flush it
// this can be specified on the command line, too (--accounts-db-cache-limit-mb)
const WRITE_CACHE_LIMIT_BYTES_DEFAULT: u64 = 15_000_000_000;
const SCAN_SLOT_PAR_ITER_THRESHOLD: usize = 4000;

const UNREF_ACCOUNTS_BATCH_SIZE: usize = 10_000;

pub const DEFAULT_FILE_SIZE: u64 = 4 * 1024 * 1024;
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

// When getting accounts for shrinking from the index, this is the # of accounts to lookup per thread.
// This allows us to split up accounts index accesses across multiple threads.
const SHRINK_COLLECT_CHUNK_SIZE: usize = 50;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum CreateAncientStorage {
    /// ancient storages are created by appending
    Append,
    /// ancient storages are created by 1-shot write to pack multiple accounts together more efficiently with new formats
    #[default]
    Pack,
}

#[derive(Debug)]
enum StoreTo<'a> {
    /// write to cache
    Cache,
    /// write to storage
    Storage(&'a Arc<AccountStorageEntry>),
}

impl<'a> StoreTo<'a> {
    fn is_cached(&self) -> bool {
        matches!(self, StoreTo::Cache)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScanAccountStorageData {
    /// callback for accounts in storage will not include `data`
    NoData,
    /// return data (&[u8]) for each account.
    /// This can be expensive to get and is not necessary for many scan operations.
    DataRefForStorage,
}

#[derive(Default, Debug)]
/// hold alive accounts
/// alive means in the accounts index
pub(crate) struct AliveAccounts<'a> {
    /// slot the accounts are currently stored in
    pub(crate) slot: Slot,
    pub(crate) accounts: Vec<&'a AccountFromStorage>,
    pub(crate) bytes: usize,
}

/// separate pubkeys into those with a single refcount and those with > 1 refcount
#[derive(Debug)]
pub(crate) struct ShrinkCollectAliveSeparatedByRefs<'a> {
    /// accounts where ref_count = 1
    pub(crate) one_ref: AliveAccounts<'a>,
    /// account where ref_count > 1, but this slot contains the alive entry with the highest slot
    pub(crate) many_refs_this_is_newest_alive: AliveAccounts<'a>,
    /// account where ref_count > 1, and this slot is NOT the highest alive entry in the index for the pubkey
    pub(crate) many_refs_old_alive: AliveAccounts<'a>,
}

/// Configuration Parameters for running accounts hash and total lamports verification
#[derive(Debug, Clone)]
pub struct VerifyAccountsHashAndLamportsConfig<'a> {
    /// bank ancestors
    pub ancestors: &'a Ancestors,
    /// true to verify hash calculation
    pub test_hash_calculation: bool,
    /// epoch_schedule
    pub epoch_schedule: &'a EpochSchedule,
    /// rent_collector
    pub rent_collector: &'a RentCollector,
    /// true to ignore mismatches
    pub ignore_mismatch: bool,
    /// true to dump debug log if mismatch happens
    pub store_detailed_debug_info: bool,
    /// true to use dedicated background thread pool for verification
    pub use_bg_thread_pool: bool,
}

pub(crate) trait ShrinkCollectRefs<'a>: Sync + Send {
    fn with_capacity(capacity: usize, slot: Slot) -> Self;
    fn collect(&mut self, other: Self);
    fn add(
        &mut self,
        ref_count: u64,
        account: &'a AccountFromStorage,
        slot_list: &[(Slot, AccountInfo)],
    );
    fn len(&self) -> usize;
    fn alive_bytes(&self) -> usize;
    fn alive_accounts(&self) -> &Vec<&'a AccountFromStorage>;
}

impl<'a> ShrinkCollectRefs<'a> for AliveAccounts<'a> {
    fn collect(&mut self, mut other: Self) {
        self.bytes = self.bytes.saturating_add(other.bytes);
        self.accounts.append(&mut other.accounts);
    }
    fn with_capacity(capacity: usize, slot: Slot) -> Self {
        Self {
            accounts: Vec::with_capacity(capacity),
            bytes: 0,
            slot,
        }
    }
    fn add(
        &mut self,
        _ref_count: u64,
        account: &'a AccountFromStorage,
        _slot_list: &[(Slot, AccountInfo)],
    ) {
        self.accounts.push(account);
        self.bytes = self.bytes.saturating_add(account.stored_size());
    }
    fn len(&self) -> usize {
        self.accounts.len()
    }
    fn alive_bytes(&self) -> usize {
        self.bytes
    }
    fn alive_accounts(&self) -> &Vec<&'a AccountFromStorage> {
        &self.accounts
    }
}

impl<'a> ShrinkCollectRefs<'a> for ShrinkCollectAliveSeparatedByRefs<'a> {
    fn collect(&mut self, other: Self) {
        self.one_ref.collect(other.one_ref);
        self.many_refs_this_is_newest_alive
            .collect(other.many_refs_this_is_newest_alive);
        self.many_refs_old_alive.collect(other.many_refs_old_alive);
    }
    fn with_capacity(capacity: usize, slot: Slot) -> Self {
        Self {
            one_ref: AliveAccounts::with_capacity(capacity, slot),
            many_refs_this_is_newest_alive: AliveAccounts::with_capacity(0, slot),
            many_refs_old_alive: AliveAccounts::with_capacity(0, slot),
        }
    }
    fn add(
        &mut self,
        ref_count: u64,
        account: &'a AccountFromStorage,
        slot_list: &[(Slot, AccountInfo)],
    ) {
        let other = if ref_count == 1 {
            &mut self.one_ref
        } else if slot_list.len() == 1
            || !slot_list
                .iter()
                .any(|(slot_list_slot, _info)| slot_list_slot > &self.many_refs_old_alive.slot)
        {
            // this entry is alive but is newer than any other slot in the index
            &mut self.many_refs_this_is_newest_alive
        } else {
            // This entry is alive but is older than at least one other slot in the index.
            // We would expect clean to get rid of the entry for THIS slot at some point, but clean hasn't done that yet.
            &mut self.many_refs_old_alive
        };
        other.add(ref_count, account, slot_list);
    }
    fn len(&self) -> usize {
        self.one_ref
            .len()
            .saturating_add(self.many_refs_old_alive.len())
            .saturating_add(self.many_refs_this_is_newest_alive.len())
    }
    fn alive_bytes(&self) -> usize {
        self.one_ref
            .alive_bytes()
            .saturating_add(self.many_refs_old_alive.alive_bytes())
            .saturating_add(self.many_refs_this_is_newest_alive.alive_bytes())
    }
    fn alive_accounts(&self) -> &Vec<&'a AccountFromStorage> {
        unimplemented!("illegal use");
    }
}

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
struct CurrentAncientAccountsFile {
    slot_and_accounts_file: Option<(Slot, Arc<AccountStorageEntry>)>,
}

impl CurrentAncientAccountsFile {
    fn new(slot: Slot, append_vec: Arc<AccountStorageEntry>) -> CurrentAncientAccountsFile {
        Self {
            slot_and_accounts_file: Some((slot, append_vec)),
        }
    }

    /// Create ancient accounts file for a slot
    ///     min_bytes: the new accounts file needs to have at least this capacity
    #[must_use]
    fn create_ancient_accounts_file<'a>(
        &mut self,
        slot: Slot,
        db: &'a AccountsDb,
        min_bytes: usize,
    ) -> ShrinkInProgress<'a> {
        let size = get_ancient_append_vec_capacity().max(min_bytes as u64);
        let shrink_in_progress = db.get_store_for_shrink(slot, size);
        *self = Self::new(slot, Arc::clone(shrink_in_progress.new_storage()));
        shrink_in_progress
    }
    #[must_use]
    fn create_if_necessary<'a>(
        &mut self,
        slot: Slot,
        db: &'a AccountsDb,
        min_bytes: usize,
    ) -> Option<ShrinkInProgress<'a>> {
        if self.slot_and_accounts_file.is_none() {
            Some(self.create_ancient_accounts_file(slot, db, min_bytes))
        } else {
            None
        }
    }

    /// note this requires that 'slot_and_accounts_file' is Some
    fn slot(&self) -> Slot {
        self.slot_and_accounts_file.as_ref().unwrap().0
    }

    /// note this requires that 'slot_and_accounts_file' is Some
    fn accounts_file(&self) -> &Arc<AccountStorageEntry> {
        &self.slot_and_accounts_file.as_ref().unwrap().1
    }

    /// helper function to cleanup call to 'store_accounts_frozen'
    /// return timing and bytes written
    fn store_ancient_accounts(
        &self,
        db: &AccountsDb,
        accounts_to_store: &AccountsToStore,
        storage_selector: StorageSelector,
    ) -> (StoreAccountsTiming, u64) {
        let accounts = accounts_to_store.get(storage_selector);

        let previous_available = self.accounts_file().accounts.remaining_bytes();

        let accounts = [(accounts_to_store.slot(), accounts)];
        let storable_accounts = StorableAccountsBySlot::new(self.slot(), &accounts, db);
        let timing = db.store_accounts_frozen(storable_accounts, self.accounts_file());
        let bytes_written =
            previous_available.saturating_sub(self.accounts_file().accounts.remaining_bytes());
        assert_eq!(
            bytes_written,
            u64_align!(accounts_to_store.get_bytes(storage_selector)) as u64
        );

        (timing, bytes_written)
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
    #[cfg(feature = "dev-context-only-utils")]
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
        current_ancient: &CurrentAncientAccountsFile,
        to_store: &AccountsToStore,
    ) {
        if slot != current_ancient.slot() {
            // we are taking accounts from 'slot' and putting them into 'current_ancient.slot()'
            // StorageSelector::Primary here because only the accounts that are moving from 'slot' to 'current_ancient.slot()'
            // Any overflow accounts will get written into a new append vec AT 'slot', so they don't need to be unrefed
            let accounts = to_store.get(StorageSelector::Primary);
            if Some(current_ancient.slot()) != self.inner.as_ref().map(|ap| ap.slot) {
                let mut pubkeys = HashSet::new();
                current_ancient
                    .accounts_file()
                    .accounts
                    .scan_pubkeys(|pubkey| {
                        pubkeys.insert(*pubkey);
                    });
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

#[derive(Debug)]
pub(crate) struct ShrinkCollect<'a, T: ShrinkCollectRefs<'a>> {
    pub(crate) slot: Slot,
    pub(crate) capacity: u64,
    pub(crate) unrefed_pubkeys: Vec<&'a Pubkey>,
    pub(crate) alive_accounts: T,
    /// total size in storage of all alive accounts
    pub(crate) alive_total_bytes: usize,
    pub(crate) total_starting_accounts: usize,
    /// true if all alive accounts are zero lamports
    pub(crate) all_are_zero_lamports: bool,
    /// index entries that need to be held in memory while shrink is in progress
    /// These aren't read - they are just held so that entries cannot be flushed.
    pub(crate) _index_entries_being_shrunk: Vec<AccountMapEntry<AccountInfo>>,
}

pub const ACCOUNTS_DB_CONFIG_FOR_TESTING: AccountsDbConfig = AccountsDbConfig {
    index: Some(ACCOUNTS_INDEX_CONFIG_FOR_TESTING),
    base_working_path: None,
    accounts_hash_cache_path: None,
    shrink_paths: None,
    read_cache_limit_bytes: None,
    write_cache_limit_bytes: None,
    ancient_append_vec_offset: None,
    skip_initial_hash_calc: false,
    exhaustively_verify_refcounts: false,
    create_ancient_storage: CreateAncientStorage::Pack,
    test_partitioned_epoch_rewards: TestPartitionedEpochRewards::CompareResults,
    test_skip_rewrites_but_include_in_bank_hash: false,
    storage_access: StorageAccess::Mmap,
};
pub const ACCOUNTS_DB_CONFIG_FOR_BENCHMARKS: AccountsDbConfig = AccountsDbConfig {
    index: Some(ACCOUNTS_INDEX_CONFIG_FOR_BENCHMARKS),
    base_working_path: None,
    accounts_hash_cache_path: None,
    shrink_paths: None,
    read_cache_limit_bytes: None,
    write_cache_limit_bytes: None,
    ancient_append_vec_offset: None,
    skip_initial_hash_calc: false,
    exhaustively_verify_refcounts: false,
    create_ancient_storage: CreateAncientStorage::Pack,
    test_partitioned_epoch_rewards: TestPartitionedEpochRewards::None,
    test_skip_rewrites_but_include_in_bank_hash: false,
    storage_access: StorageAccess::Mmap,
};

pub type BinnedHashData = Vec<Vec<CalculateHashIntermediate>>;

struct LoadAccountsIndexForShrink<'a, T: ShrinkCollectRefs<'a>> {
    /// all alive accounts
    alive_accounts: T,
    /// pubkeys that were unref'd in the accounts index because they were dead
    unrefed_pubkeys: Vec<&'a Pubkey>,
    /// true if all alive accounts are zero lamport accounts
    all_are_zero_lamports: bool,
    /// index entries we need to hold onto to keep them from getting flushed
    index_entries_being_shrunk: Vec<AccountMapEntry<AccountInfo>>,
}

/// reference an account found during scanning a storage. This is a byval struct to replace
/// `StoredAccountMeta`
#[derive(Debug, PartialEq, Copy, Clone)]
pub struct AccountFromStorage {
    pub index_info: AccountInfo,
    pub data_len: u64,
    pub pubkey: Pubkey,
}

impl ZeroLamport for AccountFromStorage {
    fn is_zero_lamport(&self) -> bool {
        self.index_info.is_zero_lamport()
    }
}

impl AccountFromStorage {
    pub fn pubkey(&self) -> &Pubkey {
        &self.pubkey
    }
    pub fn stored_size(&self) -> usize {
        aligned_stored_size(self.data_len as usize)
    }
    pub fn data_len(&self) -> usize {
        self.data_len as usize
    }
    pub fn new(account: &StoredAccountMeta) -> Self {
        // the id is irrelevant in this account info. This structure is only used DURING shrink operations.
        // In those cases, there is only 1 append vec id per slot when we read the accounts.
        // Any value of storage id in account info works fine when we want the 'normal' storage.
        let storage_id = 0;
        AccountFromStorage {
            index_info: AccountInfo::new(
                StorageLocation::AppendVec(storage_id, account.offset()),
                account.lamports(),
            ),
            pubkey: *account.pubkey(),
            data_len: account.data_len() as u64,
        }
    }
}

pub struct GetUniqueAccountsResult {
    pub stored_accounts: Vec<AccountFromStorage>,
    pub capacity: u64,
    pub num_duplicated_accounts: usize,
}

pub struct AccountsAddRootTiming {
    pub index_us: u64,
    pub cache_us: u64,
    pub store_us: u64,
}

const ANCIENT_APPEND_VEC_DEFAULT_OFFSET: Option<i64> = Some(-10_000);

#[derive(Debug, Default, Clone)]
pub struct AccountsDbConfig {
    pub index: Option<AccountsIndexConfig>,
    /// Base directory for various necessary files
    pub base_working_path: Option<PathBuf>,
    pub accounts_hash_cache_path: Option<PathBuf>,
    pub shrink_paths: Option<Vec<PathBuf>>,
    /// The low and high watermark sizes for the read cache, in bytes.
    /// If None, defaults will be used.
    pub read_cache_limit_bytes: Option<(usize, usize)>,
    pub write_cache_limit_bytes: Option<u64>,
    /// if None, ancient append vecs are set to ANCIENT_APPEND_VEC_DEFAULT_OFFSET
    /// Some(offset) means include slots up to (max_slot - (slots_per_epoch - 'offset'))
    pub ancient_append_vec_offset: Option<i64>,
    pub test_skip_rewrites_but_include_in_bank_hash: bool,
    pub skip_initial_hash_calc: bool,
    pub exhaustively_verify_refcounts: bool,
    /// how to create ancient storages
    pub create_ancient_storage: CreateAncientStorage,
    pub test_partitioned_epoch_rewards: TestPartitionedEpochRewards,
    pub storage_access: StorageAccess,
}

#[cfg(not(test))]
const ABSURD_CONSECUTIVE_FAILED_ITERATIONS: usize = 100;

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
    pub total_time_us: u64,
    pub index_time: u64,
    pub scan_time: u64,
    pub insertion_time_us: u64,
    pub min_bin_size: usize,
    pub max_bin_size: usize,
    pub total_items: usize,
    pub storage_size_storages_us: u64,
    pub index_flush_us: u64,
    pub rent_paying: AtomicUsize,
    pub amount_to_top_off_rent: AtomicU64,
    pub total_including_duplicates: u64,
    pub accounts_data_len_dedup_time_us: u64,
    pub total_duplicate_slot_keys: u64,
    pub populate_duplicate_keys_us: u64,
    pub total_slots: u64,
    pub slots_to_clean: u64,
}

#[derive(Default, Debug, PartialEq, Eq)]
struct StorageSizeAndCount {
    /// total size stored, including both alive and dead bytes
    pub stored_size: usize,
    /// number of accounts in the storage including both alive and dead accounts
    pub count: usize,
}
type StorageSizeAndCountMap = DashMap<AccountsFileId, StorageSizeAndCount>;

impl GenerateIndexTimings {
    pub fn report(&self, startup_stats: &StartupStats) {
        datapoint_info!(
            "generate_index",
            ("overall_us", self.total_time_us, i64),
            // we cannot accurately measure index insertion time because of many threads and lock contention
            ("total_us", self.index_time, i64),
            ("scan_stores_us", self.scan_time, i64),
            ("insertion_time_us", self.insertion_time_us, i64),
            ("min_bin_size", self.min_bin_size as i64, i64),
            ("max_bin_size", self.max_bin_size as i64, i64),
            (
                "storage_size_storages_us",
                self.storage_size_storages_us as i64,
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
                "total_items_including_duplicates",
                self.total_including_duplicates as i64,
                i64
            ),
            ("total_items", self.total_items as i64, i64),
            (
                "accounts_data_len_dedup_time_us",
                self.accounts_data_len_dedup_time_us as i64,
                i64
            ),
            (
                "total_duplicate_slot_keys",
                self.total_duplicate_slot_keys as i64,
                i64
            ),
            (
                "populate_duplicate_keys_us",
                self.populate_duplicate_keys_us as i64,
                i64
            ),
            ("total_slots", self.total_slots, i64),
            ("slots_to_clean", self.slots_to_clean, i64),
            (
                "copy_data_us",
                startup_stats.copy_data_us.swap(0, Ordering::Relaxed),
                i64
            ),
        );
    }
}

impl IndexValue for AccountInfo {}
impl DiskIndexValue for AccountInfo {}

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
    start_time: Instant,
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
            start_time: Instant::now(),
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
                let total = previous_total_processed_slots_across_all_threads
                    + my_total_newly_processed_slots_since_last_report;
                info!(
                    "generating index: {}/{} slots... ({}/s)",
                    total,
                    self.ultimate_count,
                    total / self.start_time.elapsed().as_secs().max(1),
                );
            }
            self.last_update = now;
        }
    }
}

/// An offset into the AccountsDb::storage vector
pub type AtomicAccountsFileId = AtomicU32;
pub type AccountsFileId = u32;

type AccountSlots = HashMap<Pubkey, HashSet<Slot>>;
type SlotOffsets = HashMap<Slot, IntSet<usize>>;
type ReclaimResult = (AccountSlots, SlotOffsets);
type PubkeysRemovedFromAccountsIndex = HashSet<Pubkey>;
type ShrinkCandidates = IntSet<Slot>;

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
    /// same as `FixedMaxRoot`, except do not populate the read cache on load
    FixedMaxRootDoNotPopulateReadCache,
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

impl<'a> LoadedAccountAccessor<'a> {
    fn check_and_get_loaded_account_shared_data(&mut self) -> AccountSharedData {
        // all of these following .expect() and .unwrap() are like serious logic errors,
        // ideal for representing this as rust type system....

        match self {
            LoadedAccountAccessor::Stored(Some((maybe_storage_entry, offset))) => {
                // If we do find the storage entry, we can guarantee that the storage entry is
                // safe to read from because we grabbed a reference to the storage entry while it
                // was still in the storage map. This means even if the storage entry is removed
                // from the storage map after we grabbed the storage entry, the recycler should not
                // reset the storage entry until we drop the reference to the storage entry.
                maybe_storage_entry
                            .get_account_shared_data(*offset)
                    .expect("If a storage entry was found in the storage map, it must not have been reset yet")
            }
            _ => self.check_and_get_loaded_account(|loaded_account| loaded_account.take_account()),
        }
    }

    fn check_and_get_loaded_account<T>(
        &mut self,
        callback: impl for<'local> FnMut(LoadedAccount<'local>) -> T,
    ) -> T {
        // all of these following .expect() and .unwrap() are like serious logic errors,
        // ideal for representing this as rust type system....

        match self {
            LoadedAccountAccessor::Cached(None) | LoadedAccountAccessor::Stored(None) => {
                panic!("Should have already been taken care of when creating this LoadedAccountAccessor");
            }
            LoadedAccountAccessor::Cached(Some(_cached_account)) => {
                // Cached(Some(x)) variant always produces `Some` for get_loaded_account() since
                // it just returns the inner `x` without additional fetches
                self.get_loaded_account(callback).unwrap()
            }
            LoadedAccountAccessor::Stored(Some(_maybe_storage_entry)) => {
                // If we do find the storage entry, we can guarantee that the storage entry is
                // safe to read from because we grabbed a reference to the storage entry while it
                // was still in the storage map. This means even if the storage entry is removed
                // from the storage map after we grabbed the storage entry, the recycler should not
                // reset the storage entry until we drop the reference to the storage entry.
                self.get_loaded_account(callback)
                    .expect("If a storage entry was found in the storage map, it must not have been reset yet")
            }
        }
    }

    fn get_loaded_account<T>(
        &mut self,
        mut callback: impl for<'local> FnMut(LoadedAccount<'local>) -> T,
    ) -> Option<T> {
        match self {
            LoadedAccountAccessor::Cached(cached_account) => {
                let cached_account: Cow<'a, CachedAccount> = cached_account.take().expect(
                    "Cache flushed/purged should be handled before trying to fetch account",
                );
                Some(callback(LoadedAccount::Cached(cached_account)))
            }
            LoadedAccountAccessor::Stored(maybe_storage_entry) => {
                // storage entry may not be present if slot was cleaned up in
                // between reading the accounts index and calling this function to
                // get account meta from the storage entry here
                maybe_storage_entry
                    .as_ref()
                    .and_then(|(storage_entry, offset)| {
                        storage_entry
                            .accounts
                            .get_stored_account_meta_callback(*offset, |account| {
                                callback(LoadedAccount::Stored(account))
                            })
                    })
            }
        }
    }

    fn account_matches_owners(&self, owners: &[Pubkey]) -> Result<usize, MatchAccountOwnerError> {
        match self {
            LoadedAccountAccessor::Cached(cached_account) => cached_account
                .as_ref()
                .and_then(|cached_account| {
                    if cached_account.account.is_zero_lamport() {
                        None
                    } else {
                        owners
                            .iter()
                            .position(|entry| cached_account.account.owner() == entry)
                    }
                })
                .ok_or(MatchAccountOwnerError::NoMatch),
            LoadedAccountAccessor::Stored(maybe_storage_entry) => {
                // storage entry may not be present if slot was cleaned up in
                // between reading the accounts index and calling this function to
                // get account meta from the storage entry here
                maybe_storage_entry
                    .as_ref()
                    .map(|(storage_entry, offset)| {
                        storage_entry
                            .accounts
                            .account_matches_owners(*offset, owners)
                    })
                    .unwrap_or(Err(MatchAccountOwnerError::UnableToLoad))
            }
        }
    }
}

pub enum LoadedAccount<'a> {
    Stored(StoredAccountMeta<'a>),
    Cached(Cow<'a, CachedAccount>),
}

impl<'a> LoadedAccount<'a> {
    pub fn loaded_hash(&self) -> AccountHash {
        match self {
            LoadedAccount::Stored(stored_account_meta) => *stored_account_meta.hash(),
            LoadedAccount::Cached(cached_account) => cached_account.hash(),
        }
    }

    pub fn pubkey(&self) -> &Pubkey {
        match self {
            LoadedAccount::Stored(stored_account_meta) => stored_account_meta.pubkey(),
            LoadedAccount::Cached(cached_account) => cached_account.pubkey(),
        }
    }

    pub fn take_account(&self) -> AccountSharedData {
        match self {
            LoadedAccount::Stored(stored_account_meta) => {
                stored_account_meta.to_account_shared_data()
            }
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

    /// data_len can be calculated without having access to `&data` in future implementations
    pub fn data_len(&self) -> usize {
        self.data().len()
    }
}

impl<'a> ReadableAccount for LoadedAccount<'a> {
    fn lamports(&self) -> u64 {
        match self {
            LoadedAccount::Stored(stored_account_meta) => stored_account_meta.lamports(),
            LoadedAccount::Cached(cached_account) => cached_account.account.lamports(),
        }
    }
    fn data(&self) -> &[u8] {
        match self {
            LoadedAccount::Stored(stored_account_meta) => stored_account_meta.data(),
            LoadedAccount::Cached(cached_account) => cached_account.account.data(),
        }
    }
    fn owner(&self) -> &Pubkey {
        match self {
            LoadedAccount::Stored(stored_account_meta) => stored_account_meta.owner(),
            LoadedAccount::Cached(cached_account) => cached_account.account.owner(),
        }
    }
    fn executable(&self) -> bool {
        match self {
            LoadedAccount::Stored(stored_account_meta) => stored_account_meta.executable(),
            LoadedAccount::Cached(cached_account) => cached_account.account.executable(),
        }
    }
    fn rent_epoch(&self) -> Epoch {
        match self {
            LoadedAccount::Stored(stored_account_meta) => stored_account_meta.rent_epoch(),
            LoadedAccount::Cached(cached_account) => cached_account.account.rent_epoch(),
        }
    }
    fn to_account_shared_data(&self) -> AccountSharedData {
        self.take_account()
    }
}

#[derive(Debug)]
pub enum AccountsHashVerificationError {
    MissingAccountsHash,
    MismatchedAccountsHash,
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
    pub(crate) id: AccountsFileId,

    pub(crate) slot: Slot,

    /// storage holding the accounts
    pub accounts: AccountsFile,

    /// Keeps track of the number of accounts stored in a specific AppendVec.
    ///  This is periodically checked to reuse the stores that do not have
    ///  any accounts in it
    /// status corresponding to the storage, lets us know that
    ///  the append_vec, once maxed out, then emptied, can be reclaimed
    count_and_status: SeqLock<(usize, AccountStorageStatus)>,

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
    pub fn new(
        path: &Path,
        slot: Slot,
        id: AccountsFileId,
        file_size: u64,
        provider: AccountsFileProvider,
    ) -> Self {
        let tail = AccountsFile::file_name(slot, id);
        let path = Path::new(path).join(tail);
        let accounts = provider.new_writable(path, file_size);

        Self {
            id,
            slot,
            accounts,
            count_and_status: SeqLock::new((0, AccountStorageStatus::Available)),
            approx_store_count: AtomicUsize::new(0),
            alive_bytes: AtomicUsize::new(0),
        }
    }

    /// open a new instance of the storage that is readonly
    fn reopen_as_readonly(&self, storage_access: StorageAccess) -> Option<Self> {
        if storage_access != StorageAccess::File {
            // if we are only using mmap, then no reason to re-open
            return None;
        }

        let count_and_status = self.count_and_status.lock_write();
        self.accounts.reopen_as_readonly().map(|accounts| Self {
            id: self.id,
            slot: self.slot,
            count_and_status: SeqLock::new(*count_and_status),
            approx_store_count: AtomicUsize::new(self.approx_stored_count()),
            alive_bytes: AtomicUsize::new(self.alive_bytes()),
            accounts,
        })
    }

    pub fn new_existing(
        slot: Slot,
        id: AccountsFileId,
        accounts: AccountsFile,
        num_accounts: usize,
    ) -> Self {
        Self {
            id,
            slot,
            accounts,
            count_and_status: SeqLock::new((0, AccountStorageStatus::Available)),
            approx_store_count: AtomicUsize::new(num_accounts),
            alive_bytes: AtomicUsize::new(0),
        }
    }

    pub fn set_status(&self, mut status: AccountStorageStatus) {
        let mut count_and_status = self.count_and_status.lock_write();

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

    pub fn status(&self) -> AccountStorageStatus {
        self.count_and_status.read().1
    }

    pub fn count(&self) -> usize {
        self.count_and_status.read().0
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

    pub fn capacity(&self) -> u64 {
        self.accounts.capacity()
    }

    pub fn has_accounts(&self) -> bool {
        self.count() > 0
    }

    pub fn slot(&self) -> Slot {
        self.slot
    }

    pub fn id(&self) -> AccountsFileId {
        self.id
    }

    pub fn flush(&self) -> Result<(), AccountsFileError> {
        self.accounts.flush()
    }

    fn get_account_shared_data(&self, offset: usize) -> Option<AccountSharedData> {
        self.accounts.get_account_shared_data(offset)
    }

    fn add_accounts(&self, num_accounts: usize, num_bytes: usize) {
        let mut count_and_status = self.count_and_status.lock_write();
        *count_and_status = (count_and_status.0 + num_accounts, count_and_status.1);
        self.approx_store_count
            .fetch_add(num_accounts, Ordering::Relaxed);
        self.alive_bytes.fetch_add(num_bytes, Ordering::SeqCst);
    }

    fn try_available(&self) -> bool {
        let mut count_and_status = self.count_and_status.lock_write();
        let (count, status) = *count_and_status;

        if status == AccountStorageStatus::Available {
            *count_and_status = (count, AccountStorageStatus::Candidate);
            true
        } else {
            false
        }
    }

    fn remove_accounts(
        &self,
        num_bytes: usize,
        reset_accounts: bool,
        num_accounts: usize,
    ) -> usize {
        let mut count_and_status = self.count_and_status.lock_write();
        let (mut count, mut status) = *count_and_status;

        if count == num_accounts && status == AccountStorageStatus::Full && reset_accounts {
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
            count >= num_accounts,
            "double remove of account in slot: {}/store: {}!!",
            self.slot(),
            self.id(),
        );

        self.alive_bytes.fetch_sub(num_bytes, Ordering::SeqCst);
        count = count.saturating_sub(num_accounts);
        *count_and_status = (count, status);
        count
    }

    /// Returns the path to the underlying accounts storage file
    pub fn path(&self) -> &Path {
        self.accounts.path()
    }
}

pub fn get_temp_accounts_paths(count: u32) -> IoResult<(Vec<TempDir>, Vec<PathBuf>)> {
    let temp_dirs: IoResult<Vec<TempDir>> = (0..count).map(|_| TempDir::new()).collect();
    let temp_dirs = temp_dirs?;

    let paths: IoResult<Vec<_>> = temp_dirs
        .iter()
        .map(|temp_dir| {
            utils::create_accounts_run_and_snapshot_dirs(temp_dir)
                .map(|(run_dir, _snapshot_dir)| run_dir)
        })
        .collect();
    let paths = paths?;
    Ok((temp_dirs, paths))
}

#[cfg_attr(feature = "frozen-abi", derive(AbiExample))]
#[derive(Clone, Default, Debug, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(Default, Debug)]
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

/// Removing unrooted slots in Accounts Background Service needs to be synchronized with flushing
/// slots from the Accounts Cache.  This keeps track of those slots and the Mutex + Condvar for
/// synchronization.
#[derive(Debug, Default)]
struct RemoveUnrootedSlotsSynchronization {
    // slots being flushed from the cache or being purged
    slots_under_contention: Mutex<IntSet<Slot>>,
    signal: Condvar,
}

type AccountInfoAccountsIndex = AccountsIndex<AccountInfo, AccountInfo>;

// This structure handles the load/store of the accounts
#[derive(Debug)]
pub struct AccountsDb {
    /// Keeps tracks of index into AppendVec on a per slot basis
    pub accounts_index: AccountInfoAccountsIndex,

    /// Some(offset) iff we want to squash old append vecs together into 'ancient append vecs'
    /// Some(offset) means for slots up to (max_slot - (slots_per_epoch - 'offset')), put them in ancient append vecs
    pub ancient_append_vec_offset: Option<i64>,

    /// true iff we want to skip the initial hash calculation on startup
    pub skip_initial_hash_calc: bool,

    pub storage: AccountStorage,

    /// from AccountsDbConfig
    create_ancient_storage: CreateAncientStorage,

    /// true if this client should skip rewrites but still include those rewrites in the bank hash as if rewrites had occurred.
    pub test_skip_rewrites_but_include_in_bank_hash: bool,

    pub accounts_cache: AccountsCache,

    write_cache_limit_bytes: Option<u64>,

    sender_bg_hasher: Option<Sender<Vec<CachedAccount>>>,
    read_only_accounts_cache: ReadOnlyAccountsCache,

    /// distribute the accounts across storage lists
    pub next_id: AtomicAccountsFileId,

    /// Set of shrinkable stores organized by map of slot to storage id
    pub shrink_candidate_slots: Mutex<ShrinkCandidates>,

    pub write_version: AtomicU64,

    /// Set of storage paths to pick from
    pub paths: Vec<PathBuf>,

    /// Base directory for various necessary files
    base_working_path: PathBuf,
    // used by tests - held until we are dropped
    #[allow(dead_code)]
    base_working_temp_dir: Option<TempDir>,

    accounts_hash_cache_path: PathBuf,

    shrink_paths: Vec<PathBuf>,

    /// Directory of paths this accounts_db needs to hold/remove
    #[allow(dead_code)]
    pub temp_paths: Option<Vec<TempDir>>,

    /// Starting file size of appendvecs
    file_size: u64,

    /// Thread pool used for par_iter
    pub thread_pool: ThreadPool,

    pub thread_pool_clean: ThreadPool,

    bank_hash_stats: Mutex<HashMap<Slot, BankHashStats>>,
    accounts_delta_hashes: Mutex<HashMap<Slot, AccountsDeltaHash>>,
    accounts_hashes: Mutex<HashMap<Slot, (AccountsHash, /*capitalization*/ u64)>>,
    incremental_accounts_hashes:
        Mutex<HashMap<Slot, (IncrementalAccountsHash, /*capitalization*/ u64)>>,

    pub stats: AccountsStats,

    clean_accounts_stats: CleanAccountsStats,

    // Stats for purges called outside of clean_accounts()
    external_purge_slots_stats: PurgeStats,

    pub shrink_stats: ShrinkStats,

    pub(crate) shrink_ancient_stats: ShrinkAncientStats,

    pub cluster_type: Option<ClusterType>,

    pub account_indexes: AccountSecondaryIndexes,

    /// Set of unique keys per slot which is used
    /// to drive clean_accounts
    /// Generated by calculate_accounts_delta_hash
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
    dirty_stores: DashMap<Slot, Arc<AccountStorageEntry>>,

    /// Zero-lamport accounts that are *not* purged during clean because they need to stay alive
    /// for incremental snapshot support.
    zero_lamport_accounts_to_purge_after_full_snapshot: DashSet<(Slot, Pubkey)>,

    /// GeyserPlugin accounts update notifier
    accounts_update_notifier: Option<AccountsUpdateNotifier>,

    pub(crate) active_stats: ActiveStats,

    pub verify_accounts_hash_in_bg: VerifyAccountsHashInBackground,

    /// Used to disable logging dead slots during removal.
    /// allow disabling noisy log
    pub log_dead_slots: AtomicBool,

    /// debug feature to scan every append vec and verify refcounts are equal
    exhaustively_verify_refcounts: bool,

    /// storage format to use for new storages
    accounts_file_provider: AccountsFileProvider,

    /// method to use for accessing storages
    storage_access: StorageAccess,

    /// this will live here until the feature for partitioned epoch rewards is activated.
    /// At that point, this and other code can be deleted.
    pub partitioned_epoch_rewards_config: PartitionedEpochRewardsConfig,

    /// the full accounts hash calculation as of a predetermined block height 'N'
    /// to be included in the bank hash at a predetermined block height 'M'
    /// The cadence is once per epoch, all nodes calculate a full accounts hash as of a known slot calculated using 'N'
    /// Some time later (to allow for slow calculation time), the bank hash at a slot calculated using 'M' includes the full accounts hash.
    /// Thus, the state of all accounts on a validator is known to be correct at least once per epoch.
    pub epoch_accounts_hash_manager: EpochAccountsHashManager,

    /// The latest full snapshot slot dictates how to handle zero lamport accounts
    latest_full_snapshot_slot: SeqLock<Option<Slot>>,
}

#[derive(Debug, Default)]
pub struct AccountsStats {
    delta_hash_scan_time_total_us: AtomicU64,
    delta_hash_accumulate_time_total_us: AtomicU64,
    delta_hash_num: AtomicU64,
    skipped_rewrites_num: AtomicUsize,

    last_store_report: AtomicInterval,
    store_hash_accounts: AtomicU64,
    calc_stored_meta: AtomicU64,
    store_accounts: AtomicU64,
    store_update_index: AtomicU64,
    store_handle_reclaims: AtomicU64,
    store_append_accounts: AtomicU64,
    pub stakes_cache_check_and_store_us: AtomicU64,
    store_num_accounts: AtomicU64,
    store_total_data: AtomicU64,
    create_store_count: AtomicU64,
    store_get_slot_store: AtomicU64,
    store_find_existing: AtomicU64,
    dropped_stores: AtomicU64,
    store_uncleaned_update: AtomicU64,
    handle_dead_keys_us: AtomicU64,
    purge_exact_us: AtomicU64,
    purge_exact_count: AtomicU64,
}

#[derive(Debug, Default)]
pub struct PurgeStats {
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
    fn new(oldest_non_ancient_slot: Option<Slot>, snapshot_storages: &SortedStorages) -> Self {
        let range = snapshot_storages.range();

        let (ancient_slots, first_non_ancient_slot) = if let Some(oldest_non_ancient_slot) =
            oldest_non_ancient_slot
        {
            // any ancient append vecs should definitely be cached
            // We need to break the ranges into:
            // 1. individual ancient append vecs (may be empty)
            // 2. first unevenly divided chunk starting at 1 epoch old slot (may be empty)
            // 3. evenly divided full chunks in the middle
            // 4. unevenly divided chunk of most recent slots (may be empty)
            let ancient_slots =
                Self::get_ancient_slots(oldest_non_ancient_slot, snapshot_storages, |storage| {
                    storage.capacity() > get_ancient_append_vec_capacity() * 50 / 100
                });

            let first_non_ancient_slot = ancient_slots
                .last()
                .map(|last_ancient_slot| last_ancient_slot.saturating_add(1))
                .unwrap_or(range.start);

            (ancient_slots, first_non_ancient_slot)
        } else {
            (vec![], range.start)
        };

        Self::new_with_ancient_info(range, ancient_slots, first_non_ancient_slot)
    }

    /// return all ancient append vec slots from the early slots referenced by 'snapshot_storages'
    /// `treat_as_ancient` returns true if the storage at this slot is large and should be treated individually by accounts hash calculation.
    /// `treat_as_ancient` is a fn so that we can test this well. Otherwise, we have to generate large append vecs to pass the intended checks.
    fn get_ancient_slots(
        oldest_non_ancient_slot: Slot,
        snapshot_storages: &SortedStorages,
        treat_as_ancient: impl Fn(&AccountStorageEntry) -> bool,
    ) -> Vec<Slot> {
        let range = snapshot_storages.range();
        let mut i = 0;
        let mut len_truncate = 0;
        let mut possible_ancient_slots = snapshot_storages
            .iter_range(&(range.start..oldest_non_ancient_slot))
            .filter_map(|(slot, storage)| {
                storage.map(|storage| {
                    i += 1;
                    if treat_as_ancient(storage) {
                        // even though the slot is in range of being an ancient append vec, if it isn't actually a large append vec,
                        // then we are better off treating all these slots as normally cacheable to reduce work in dedup.
                        // Since this one is large, for the moment, this one becomes the highest slot where we want to individually cache files.
                        len_truncate = i;
                    }
                    slot
                })
            })
            .collect::<Vec<_>>();
        possible_ancient_slots.truncate(len_truncate);
        possible_ancient_slots
    }

    /// create once ancient slots have been identified
    /// This is easier to test, removing SortedStorages as a type to deal with here.
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
    num_flushed: Saturating<usize>,
    num_purged: Saturating<usize>,
    total_size: Saturating<u64>,
    store_accounts_timing: StoreAccountsTiming,
    store_accounts_total_us: Saturating<u64>,
}

impl FlushStats {
    fn accumulate(&mut self, other: &Self) {
        self.num_flushed += other.num_flushed;
        self.num_purged += other.num_purged;
        self.total_size += other.total_size;
        self.store_accounts_timing
            .accumulate(&other.store_accounts_timing);
        self.store_accounts_total_us += other.store_accounts_total_us;
    }
}

#[derive(Debug, Default)]
struct LatestAccountsIndexRootsStats {
    roots_len: AtomicUsize,
    uncleaned_roots_len: AtomicUsize,
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
                "uncleaned_roots_len",
                self.uncleaned_roots_len.load(Ordering::Relaxed) as i64,
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
            ),
            (
                "append_vecs_dirty",
                APPEND_VEC_MMAPPED_FILES_DIRTY.load(Ordering::Relaxed),
                i64
            ),
            (
                "append_vecs_open_as_file_io",
                APPEND_VEC_OPEN_AS_FILE_IO.load(Ordering::Relaxed),
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
    uncleaned_roots_slot_list_1: AtomicU64,
}

impl CleanAccountsStats {
    fn report(&self) {
        self.purge_stats.report("clean_purge_slots_stats", None);
        self.latest_accounts_index_roots_stats.report();
    }
}

#[derive(Debug, Default)]
pub(crate) struct ShrinkAncientStats {
    pub(crate) shrink_stats: ShrinkStats,
    pub(crate) ancient_append_vecs_shrunk: AtomicU64,
    pub(crate) total_us: AtomicU64,
    pub(crate) random_shrink: AtomicU64,
    pub(crate) slots_considered: AtomicU64,
    pub(crate) ancient_scanned: AtomicU64,
    pub(crate) bytes_ancient_created: AtomicU64,
    pub(crate) bytes_from_must_shrink: AtomicU64,
    pub(crate) bytes_from_smallest_storages: AtomicU64,
    pub(crate) bytes_from_newest_storages: AtomicU64,
    pub(crate) many_ref_slots_skipped: AtomicU64,
    pub(crate) slots_cannot_move_count: AtomicU64,
    pub(crate) many_refs_old_alive: AtomicU64,
    pub(crate) slots_eligible_to_shrink: AtomicU64,
    pub(crate) total_dead_bytes: AtomicU64,
}

#[derive(Debug, Default)]
pub(crate) struct ShrinkStatsSub {
    pub(crate) store_accounts_timing: StoreAccountsTiming,
    pub(crate) rewrite_elapsed_us: Saturating<u64>,
    pub(crate) create_and_insert_store_elapsed_us: Saturating<u64>,
    pub(crate) unpackable_slots_count: Saturating<usize>,
    pub(crate) newest_alive_packed_count: Saturating<usize>,
}

impl ShrinkStatsSub {
    pub(crate) fn accumulate(&mut self, other: &Self) {
        self.store_accounts_timing
            .accumulate(&other.store_accounts_timing);
        self.rewrite_elapsed_us += other.rewrite_elapsed_us;
        self.create_and_insert_store_elapsed_us += other.create_and_insert_store_elapsed_us;
        self.unpackable_slots_count += other.unpackable_slots_count;
        self.newest_alive_packed_count += other.newest_alive_packed_count;
    }
}
#[derive(Debug, Default)]
pub struct ShrinkStats {
    last_report: AtomicInterval,
    pub(crate) num_slots_shrunk: AtomicUsize,
    storage_read_elapsed: AtomicU64,
    num_duplicated_accounts: AtomicU64,
    index_read_elapsed: AtomicU64,
    create_and_insert_store_elapsed: AtomicU64,
    store_accounts_elapsed: AtomicU64,
    update_index_elapsed: AtomicU64,
    handle_reclaims_elapsed: AtomicU64,
    remove_old_stores_shrink_us: AtomicU64,
    rewrite_elapsed: AtomicU64,
    unpackable_slots_count: AtomicU64,
    newest_alive_packed_count: AtomicU64,
    drop_storage_entries_elapsed: AtomicU64,
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
                    "num_duplicated_accounts",
                    self.num_duplicated_accounts.swap(0, Ordering::Relaxed),
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
    pub(crate) fn report(&self) {
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
                "num_duplicated_accounts",
                self.shrink_stats
                    .num_duplicated_accounts
                    .swap(0, Ordering::Relaxed),
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
                "unpackable_slots_count",
                self.shrink_stats
                    .unpackable_slots_count
                    .swap(0, Ordering::Relaxed) as i64,
                i64
            ),
            (
                "newest_alive_packed_count",
                self.shrink_stats
                    .newest_alive_packed_count
                    .swap(0, Ordering::Relaxed) as i64,
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
                "slots_eligible_to_shrink",
                self.slots_eligible_to_shrink.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "total_dead_bytes",
                self.total_dead_bytes.swap(0, Ordering::Relaxed),
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
            (
                "bytes_ancient_created",
                self.bytes_ancient_created.swap(0, Ordering::Relaxed) as i64,
                i64
            ),
            (
                "bytes_from_must_shrink",
                self.bytes_from_must_shrink.swap(0, Ordering::Relaxed) as i64,
                i64
            ),
            (
                "bytes_from_smallest_storages",
                self.bytes_from_smallest_storages.swap(0, Ordering::Relaxed) as i64,
                i64
            ),
            (
                "bytes_from_newest_storages",
                self.bytes_from_newest_storages.swap(0, Ordering::Relaxed) as i64,
                i64
            ),
            (
                "many_ref_slots_skipped",
                self.many_ref_slots_skipped.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "slots_cannot_move_count",
                self.slots_cannot_move_count.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "many_refs_old_alive",
                self.many_refs_old_alive.swap(0, Ordering::Relaxed),
                i64
            ),
        );
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

#[cfg(all(RUSTC_WITH_SPECIALIZATION, feature = "frozen-abi"))]
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PubkeyHashAccount {
    pub pubkey: Pubkey,
    pub hash: AccountHash,
    pub account: AccountSharedData,
}

impl AccountsDb {
    pub const DEFAULT_ACCOUNTS_HASH_CACHE_DIR: &'static str = "accounts_hash_cache";

    // read only cache does not update lru on read of an entry unless it has been at least this many ms since the last lru update
    const READ_ONLY_CACHE_MS_TO_SKIP_LRU_UPDATE: u32 = 100;

    // The default high and low watermark sizes for the accounts read cache.
    // If the cache size exceeds MAX_SIZE_HI, it'll evict entries until the size is <= MAX_SIZE_LO.
    const DEFAULT_MAX_READ_ONLY_CACHE_DATA_SIZE_LO: usize = 400 * 1024 * 1024;
    const DEFAULT_MAX_READ_ONLY_CACHE_DATA_SIZE_HI: usize = 410 * 1024 * 1024;

    pub fn default_for_tests() -> Self {
        Self::default_with_accounts_index(AccountInfoAccountsIndex::default_for_tests(), None, None)
    }

    fn default_with_accounts_index(
        accounts_index: AccountInfoAccountsIndex,
        base_working_path: Option<PathBuf>,
        accounts_hash_cache_path: Option<PathBuf>,
    ) -> Self {
        let num_threads = get_thread_count();

        let (base_working_path, base_working_temp_dir) =
            if let Some(base_working_path) = base_working_path {
                (base_working_path, None)
            } else {
                let base_working_temp_dir = TempDir::new().unwrap();
                let base_working_path = base_working_temp_dir.path().to_path_buf();
                (base_working_path, Some(base_working_temp_dir))
            };

        let accounts_hash_cache_path = accounts_hash_cache_path.unwrap_or_else(|| {
            let accounts_hash_cache_path =
                base_working_path.join(Self::DEFAULT_ACCOUNTS_HASH_CACHE_DIR);
            if !accounts_hash_cache_path.exists() {
                fs::create_dir(&accounts_hash_cache_path).expect("create accounts hash cache dir");
            }
            accounts_hash_cache_path
        });

        let mut bank_hash_stats = HashMap::new();
        bank_hash_stats.insert(0, BankHashStats::default());

        // Increase the stack for accounts threads
        // rayon needs a lot of stack
        const ACCOUNTS_STACK_SIZE: usize = 8 * 1024 * 1024;

        AccountsDb {
            create_ancient_storage: CreateAncientStorage::default(),
            verify_accounts_hash_in_bg: VerifyAccountsHashInBackground::default(),
            active_stats: ActiveStats::default(),
            skip_initial_hash_calc: false,
            ancient_append_vec_offset: None,
            accounts_index,
            storage: AccountStorage::default(),
            accounts_cache: AccountsCache::default(),
            sender_bg_hasher: None,
            read_only_accounts_cache: ReadOnlyAccountsCache::new(
                Self::DEFAULT_MAX_READ_ONLY_CACHE_DATA_SIZE_LO,
                Self::DEFAULT_MAX_READ_ONLY_CACHE_DATA_SIZE_HI,
                Self::READ_ONLY_CACHE_MS_TO_SKIP_LRU_UPDATE,
            ),
            uncleaned_pubkeys: DashMap::new(),
            next_id: AtomicAccountsFileId::new(0),
            shrink_candidate_slots: Mutex::new(ShrinkCandidates::default()),
            write_cache_limit_bytes: None,
            write_version: AtomicU64::new(0),
            paths: vec![],
            base_working_path,
            base_working_temp_dir,
            accounts_hash_cache_path,
            shrink_paths: Vec::default(),
            temp_paths: None,
            file_size: DEFAULT_FILE_SIZE,
            thread_pool: rayon::ThreadPoolBuilder::new()
                .num_threads(num_threads)
                .thread_name(|i| format!("solAccounts{i:02}"))
                .stack_size(ACCOUNTS_STACK_SIZE)
                .build()
                .unwrap(),
            thread_pool_clean: make_min_priority_thread_pool(),
            bank_hash_stats: Mutex::new(bank_hash_stats),
            accounts_delta_hashes: Mutex::new(HashMap::new()),
            accounts_hashes: Mutex::new(HashMap::new()),
            incremental_accounts_hashes: Mutex::new(HashMap::new()),
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
            log_dead_slots: AtomicBool::new(true),
            exhaustively_verify_refcounts: false,
            accounts_file_provider: AccountsFileProvider::default(),
            storage_access: StorageAccess::default(),
            partitioned_epoch_rewards_config: PartitionedEpochRewardsConfig::default(),
            epoch_accounts_hash_manager: EpochAccountsHashManager::new_invalid(),
            test_skip_rewrites_but_include_in_bank_hash: false,
            latest_full_snapshot_slot: SeqLock::new(None),
        }
    }

    pub fn new_single_for_tests() -> Self {
        AccountsDb::new_for_tests(Vec::new(), &ClusterType::Development)
    }

    pub fn new_single_for_tests_with_provider(file_provider: AccountsFileProvider) -> Self {
        AccountsDb::new_for_tests_with_provider(
            Vec::new(),
            &ClusterType::Development,
            file_provider,
        )
    }

    pub fn new_for_tests(paths: Vec<PathBuf>, cluster_type: &ClusterType) -> Self {
        Self::new_for_tests_with_provider(paths, cluster_type, AccountsFileProvider::default())
    }

    fn new_for_tests_with_provider(
        paths: Vec<PathBuf>,
        cluster_type: &ClusterType,
        accounts_file_provider: AccountsFileProvider,
    ) -> Self {
        let mut db = AccountsDb::new_with_config(
            paths,
            cluster_type,
            AccountSecondaryIndexes::default(),
            AccountShrinkThreshold::default(),
            Some(ACCOUNTS_DB_CONFIG_FOR_TESTING),
            None,
            Arc::default(),
        );
        db.accounts_file_provider = accounts_file_provider;
        db
    }

    pub fn new_with_config(
        paths: Vec<PathBuf>,
        cluster_type: &ClusterType,
        account_indexes: AccountSecondaryIndexes,
        shrink_ratio: AccountShrinkThreshold,
        mut accounts_db_config: Option<AccountsDbConfig>,
        accounts_update_notifier: Option<AccountsUpdateNotifier>,
        exit: Arc<AtomicBool>,
    ) -> Self {
        let accounts_index = AccountsIndex::new(
            accounts_db_config.as_mut().and_then(|x| x.index.take()),
            exit,
        );
        let base_working_path = accounts_db_config
            .as_ref()
            .and_then(|x| x.base_working_path.clone());
        let accounts_hash_cache_path = accounts_db_config
            .as_ref()
            .and_then(|config| config.accounts_hash_cache_path.clone());
        let skip_initial_hash_calc = accounts_db_config
            .as_ref()
            .map(|config| config.skip_initial_hash_calc)
            .unwrap_or_default();

        let ancient_append_vec_offset = accounts_db_config
            .as_ref()
            .and_then(|config| config.ancient_append_vec_offset)
            .or(ANCIENT_APPEND_VEC_DEFAULT_OFFSET);

        let exhaustively_verify_refcounts = accounts_db_config
            .as_ref()
            .map(|config| config.exhaustively_verify_refcounts)
            .unwrap_or_default();

        let create_ancient_storage = accounts_db_config
            .as_ref()
            .map(|config| config.create_ancient_storage)
            .unwrap_or_default();

        let test_partitioned_epoch_rewards = accounts_db_config
            .as_ref()
            .map(|config| config.test_partitioned_epoch_rewards)
            .unwrap_or_default();

        let test_skip_rewrites_but_include_in_bank_hash = accounts_db_config
            .as_ref()
            .map(|config| config.test_skip_rewrites_but_include_in_bank_hash)
            .unwrap_or_default();

        let partitioned_epoch_rewards_config: PartitionedEpochRewardsConfig =
            PartitionedEpochRewardsConfig::new(test_partitioned_epoch_rewards);

        let read_cache_size = accounts_db_config
            .as_ref()
            .and_then(|config| config.read_cache_limit_bytes)
            .unwrap_or((
                Self::DEFAULT_MAX_READ_ONLY_CACHE_DATA_SIZE_LO,
                Self::DEFAULT_MAX_READ_ONLY_CACHE_DATA_SIZE_HI,
            ));

        let storage_access = accounts_db_config
            .as_ref()
            .map(|config| config.storage_access)
            .unwrap_or_default();

        let paths_is_empty = paths.is_empty();
        let mut new = Self {
            paths,
            skip_initial_hash_calc,
            ancient_append_vec_offset,
            cluster_type: Some(*cluster_type),
            account_indexes,
            shrink_ratio,
            accounts_update_notifier,
            create_ancient_storage,
            read_only_accounts_cache: ReadOnlyAccountsCache::new(
                read_cache_size.0,
                read_cache_size.1,
                Self::READ_ONLY_CACHE_MS_TO_SKIP_LRU_UPDATE,
            ),
            write_cache_limit_bytes: accounts_db_config
                .as_ref()
                .and_then(|x| x.write_cache_limit_bytes),
            partitioned_epoch_rewards_config,
            exhaustively_verify_refcounts,
            test_skip_rewrites_but_include_in_bank_hash,
            storage_access,
            ..Self::default_with_accounts_index(
                accounts_index,
                base_working_path,
                accounts_hash_cache_path,
            )
        };
        if paths_is_empty {
            // Create a temporary set of accounts directories, used primarily
            // for testing
            let (temp_dirs, paths) = get_temp_accounts_paths(DEFAULT_NUM_DIRS).unwrap();
            new.accounts_update_notifier = None;
            new.paths = paths;
            new.temp_paths = Some(temp_dirs);
        };
        new.shrink_paths = accounts_db_config
            .as_ref()
            .and_then(|config| config.shrink_paths.clone())
            .unwrap_or_else(|| new.paths.clone());

        new.start_background_hasher();
        {
            for path in new.paths.iter() {
                std::fs::create_dir_all(path).expect("Create directory failed.");
            }
        }
        new
    }

    pub fn file_size(&self) -> u64 {
        self.file_size
    }

    /// Get the base working directory
    pub fn get_base_working_path(&self) -> PathBuf {
        self.base_working_path.clone()
    }

    fn next_id(&self) -> AccountsFileId {
        let next_id = self.next_id.fetch_add(1, Ordering::AcqRel);
        assert!(
            next_id != AccountsFileId::MAX,
            "We've run out of storage ids!"
        );
        next_id
    }

    fn new_storage_entry(&self, slot: Slot, path: &Path, size: u64) -> AccountStorageEntry {
        AccountStorageEntry::new(
            path,
            slot,
            self.next_id(),
            size,
            self.accounts_file_provider,
        )
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
        epoch_schedule: &EpochSchedule,
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

        let one_epoch_old = self.get_oldest_non_ancient_slot(epoch_schedule);
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

        let reclaim_result = self.handle_reclaims(
            (!reclaim_vecs.is_empty()).then(|| reclaim_vecs.iter().flatten()),
            None,
            reset_accounts,
            &pubkeys_removed_from_accounts_index,
            HandleReclaims::ProcessDeadSlots(&self.clean_accounts_stats.purge_stats),
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
        store_counts: &mut HashMap<Slot, (usize, HashSet<Pubkey>)>,
        min_slot: Option<Slot>,
    ) {
        // Another pass to check if there are some filtered accounts which
        // do not match the criteria of deleting all appendvecs which contain them
        // then increment their storage count.
        let mut already_counted = IntSet::default();
        for (pubkey, (slot_list, ref_count)) in purges.iter() {
            let mut failed_slot = None;
            let all_stores_being_deleted = slot_list.len() as RefCount == *ref_count;
            if all_stores_being_deleted {
                let mut delete = true;
                for (slot, _account_info) in slot_list {
                    if let Some(count) = store_counts.get(slot).map(|s| s.0) {
                        debug!(
                            "calc_delete_dependencies()
                            slot: {slot},
                            count len: {count}"
                        );
                        if count == 0 {
                            // this store CAN be removed
                            continue;
                        }
                    }
                    // One of the pubkeys in the store has account info to a store whose store count is not going to zero.
                    // If the store cannot be found, that also means store isn't being deleted.
                    failed_slot = Some(*slot);
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
                    slot_list: {:?},
                    slot_list_len: {},
                    ref_count: {}",
                    pubkey,
                    slot_list,
                    slot_list.len(),
                    ref_count,
                );
            }

            // increment store_counts to non-zero for all stores that can not be deleted.
            let mut pending_stores = IntSet::default();
            for (slot, _account_info) in slot_list {
                if !already_counted.contains(slot) {
                    pending_stores.insert(*slot);
                }
            }
            while !pending_stores.is_empty() {
                let slot = pending_stores.iter().next().cloned().unwrap();
                if Some(slot) == min_slot {
                    if let Some(failed_slot) = failed_slot.take() {
                        info!("calc_delete_dependencies, oldest slot is not able to be deleted because of {pubkey} in slot {failed_slot}");
                    } else {
                        info!("calc_delete_dependencies, oldest slot is not able to be deleted because of {pubkey}, slot list len: {}, ref count: {ref_count}", slot_list.len());
                    }
                }

                pending_stores.remove(&slot);
                if !already_counted.insert(slot) {
                    continue;
                }
                // the point of all this code: remove the store count for all stores we cannot remove
                if let Some(store_count) = store_counts.remove(&slot) {
                    // all pubkeys in this store also cannot be removed from all stores they are in
                    let affected_pubkeys = &store_count.1;
                    for key in affected_pubkeys {
                        for (slot, _account_info) in &purges.get(key).unwrap().0 {
                            if !already_counted.contains(slot) {
                                pending_stores.insert(*slot);
                            }
                        }
                    }
                }
            }
        }
    }

    fn background_hasher(receiver: Receiver<Vec<CachedAccount>>) {
        info!("Background account hasher has started");
        loop {
            let result = receiver.recv();
            match result {
                Ok(accounts) => {
                    for account in accounts {
                        // if we hold the only ref, then this account doesn't need to be hashed, we ignore this account and it will disappear
                        if Arc::strong_count(&account) > 1 {
                            // this will cause the hash to be calculated and store inside account if it needs to be calculated
                            let _ = (*account).hash();
                        };
                    }
                }
                Err(err) => {
                    info!("Background account hasher is stopping because: {err}");
                    break;
                }
            }
        }
        info!("Background account hasher has stopped");
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
    pub fn purge_keys_exact<'a, C>(
        &'a self,
        pubkey_to_slot_set: impl Iterator<Item = &'a (Pubkey, C)>,
    ) -> (Vec<(Slot, AccountInfo)>, PubkeysRemovedFromAccountsIndex)
    where
        C: Contains<'a, Slot> + 'a,
    {
        let mut reclaims = Vec::new();
        let mut dead_keys = Vec::new();

        let mut purge_exact_count = 0;
        let (_, purge_exact_us) = measure_us!(for (pubkey, slots_set) in pubkey_to_slot_set {
            purge_exact_count += 1;
            let is_empty = self
                .accounts_index
                .purge_exact(pubkey, slots_set, &mut reclaims);
            if is_empty {
                dead_keys.push(pubkey);
            }
        });

        let (pubkeys_removed_from_accounts_index, handle_dead_keys_us) = measure_us!(self
            .accounts_index
            .handle_dead_keys(&dead_keys, &self.account_indexes));

        self.stats
            .purge_exact_count
            .fetch_add(purge_exact_count, Ordering::Relaxed);
        self.stats
            .handle_dead_keys_us
            .fetch_add(handle_dead_keys_us, Ordering::Relaxed);
        self.stats
            .purge_exact_us
            .fetch_add(purge_exact_us, Ordering::Relaxed);
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

    /// get the oldest slot that is within one epoch of the highest known root.
    /// The slot will have been offset by `self.ancient_append_vec_offset`
    fn get_oldest_non_ancient_slot(&self, epoch_schedule: &EpochSchedule) -> Slot {
        self.get_oldest_non_ancient_slot_from_slot(
            epoch_schedule,
            self.accounts_index.max_root_inclusive(),
        )
    }

    /// get the oldest slot that is within one epoch of `max_root_inclusive`.
    /// The slot will have been offset by `self.ancient_append_vec_offset`
    fn get_oldest_non_ancient_slot_from_slot(
        &self,
        epoch_schedule: &EpochSchedule,
        max_root_inclusive: Slot,
    ) -> Slot {
        let mut result = max_root_inclusive;
        if let Some(offset) = self.ancient_append_vec_offset {
            result = Self::apply_offset_to_slot(result, offset);
        }
        result = Self::apply_offset_to_slot(
            result,
            -((epoch_schedule.slots_per_epoch as i64).saturating_sub(1)),
        );
        result.min(max_root_inclusive)
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

    /// Construct a vec of pubkeys for cleaning from:
    ///   uncleaned_pubkeys - the delta set of updated pubkeys in rooted slots from the last clean
    ///   dirty_stores - set of stores which had accounts removed or recently rooted
    /// returns the minimum slot we encountered
    fn construct_candidate_clean_keys(
        &self,
        max_clean_root_inclusive: Option<Slot>,
        is_startup: bool,
        timings: &mut CleanKeyTimings,
        epoch_schedule: &EpochSchedule,
    ) -> (Vec<Pubkey>, Option<Slot>) {
        let oldest_non_ancient_slot = self.get_oldest_non_ancient_slot(epoch_schedule);
        let mut dirty_store_processing_time = Measure::start("dirty_store_processing");
        let max_slot_inclusive =
            max_clean_root_inclusive.unwrap_or_else(|| self.accounts_index.max_root_inclusive());
        let mut dirty_stores = Vec::with_capacity(self.dirty_stores.len());
        // find the oldest dirty slot
        // we'll add logging if that append vec cannot be marked dead
        let mut min_dirty_slot = None::<u64>;
        self.dirty_stores.retain(|slot, store| {
            if *slot > max_slot_inclusive {
                true
            } else {
                min_dirty_slot = min_dirty_slot.map(|min| min.min(*slot)).or(Some(*slot));
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
                        if slot < &oldest_non_ancient_slot {
                            dirty_ancient_stores.fetch_add(1, Ordering::Relaxed);
                        }
                        oldest_dirty_slot = oldest_dirty_slot.min(*slot);
                        store.accounts.scan_pubkeys(|k| {
                            pubkeys.insert(*k);
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
        // latest_full_snapshot_slot.
        let latest_full_snapshot_slot = self.latest_full_snapshot_slot();
        assert!(
            latest_full_snapshot_slot.is_some() || self.zero_lamport_accounts_to_purge_after_full_snapshot.is_empty(),
            "if snapshots are disabled, then zero_lamport_accounts_to_purge_later should always be empty"
        );
        if let Some(latest_full_snapshot_slot) = latest_full_snapshot_slot {
            self.zero_lamport_accounts_to_purge_after_full_snapshot
                .retain(|(slot, pubkey)| {
                    let is_candidate_for_clean =
                        max_slot_inclusive >= *slot && latest_full_snapshot_slot >= *slot;
                    if is_candidate_for_clean {
                        pubkeys.push(*pubkey);
                    }
                    !is_candidate_for_clean
                });
        }

        (pubkeys, min_dirty_slot)
    }

    /// Call clean_accounts() with the common parameters that tests/benches use.
    pub fn clean_accounts_for_tests(&self) {
        self.clean_accounts(None, false, &EpochSchedule::default())
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
                storage.accounts.scan_accounts(|account| {
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

                    self.accounts_index.get_and_then(entry.key(), |index_entry| {
                        if let Some(index_entry) = index_entry {
                            match (index_entry.ref_count() as usize).cmp(&entry.value().len()) {
                                std::cmp::Ordering::Equal => {
                                    // ref counts match, nothing to do here
                                }
                                std::cmp::Ordering::Greater => {
                                    let slot_list = index_entry.slot_list.read().unwrap();
                                    let num_too_new = slot_list
                                        .iter()
                                        .filter(|(slot, _)| slot > &max_slot_inclusive)
                                        .count();

                                    if ((index_entry.ref_count() as usize) - num_too_new) > entry.value().len() {
                                        failed.store(true, Ordering::Relaxed);
                                        error!("exhaustively_verify_refcounts: {} refcount too large: {}, should be: {}, {:?}, {:?}, too_new: {num_too_new}", entry.key(), index_entry.ref_count(), entry.value().len(), *entry.value(), slot_list);
                                    }
                                }
                                std::cmp::Ordering::Less => {
                                    error!("exhaustively_verify_refcounts: {} refcount too small: {}, should be: {}, {:?}, {:?}", entry.key(), index_entry.ref_count(), entry.value().len(), *entry.value(), index_entry.slot_list.read().unwrap());
                                }
                            }
                        };
                        (false, ())
                    });
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
        epoch_schedule: &EpochSchedule,
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
        let (mut candidates, min_dirty_slot) = self.construct_candidate_clean_keys(
            max_clean_root_inclusive,
            is_startup,
            &mut key_timings,
            epoch_schedule,
        );

        let mut sort = Measure::start("sort");
        if is_startup {
            candidates.par_sort_unstable();
        } else {
            self.thread_pool_clean
                .install(|| candidates.par_sort_unstable());
        }
        sort.stop();

        let num_candidates = candidates.len();
        let mut accounts_scan = Measure::start("accounts_scan");
        let uncleaned_roots = self.accounts_index.clone_uncleaned_roots();
        let found_not_zero_accum = AtomicU64::new(0);
        let not_found_on_fork_accum = AtomicU64::new(0);
        let missing_accum = AtomicU64::new(0);
        let useful_accum = AtomicU64::new(0);

        // parallel scan the index.
        let (mut purges_zero_lamports, purges_old_accounts) = {
            let do_clean_scan = || {
                candidates
                    .par_chunks(4096)
                    .map(|candidates: &[Pubkey]| {
                        let mut purges_zero_lamports = HashMap::new();
                        let mut purges_old_accounts = Vec::new();
                        let mut found_not_zero = 0;
                        let mut not_found_on_fork = 0;
                        let mut missing = 0;
                        let mut useful = 0;
                        self.accounts_index.scan(
                            candidates.iter(),
                            |candidate, slot_list_and_ref_count, _entry| {
                                let mut useless = true;
                                if let Some((slot_list, ref_count)) = slot_list_and_ref_count {
                                    // find the highest rooted slot in the slot list
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
                                                // the latest one is zero lamports. we may be able to purge it.
                                                // so, add to purges_zero_lamports
                                                purges_zero_lamports.insert(
                                                    *candidate,
                                                    (
                                                        // add all the rooted entries that contain this pubkey. we know the highest rooted entry is zero lamports
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
                                                if slot_list.len() > 1 {
                                                    // no need to purge old accounts if there is only 1 slot in the slot list
                                                    purges_old_accounts.push(*candidate);
                                                    useless = false;
                                                } else {
                                                    self.clean_accounts_stats
                                                        .uncleaned_roots_slot_list_1
                                                        .fetch_add(1, Ordering::Relaxed);
                                                }
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
                                            purges_old_accounts.push(*candidate);
                                        }
                                    }
                                } else {
                                    missing += 1;
                                }
                                if !useless {
                                    useful += 1;
                                }
                                AccountsIndexScanResult::OnlyKeepInMemoryIfDirty
                            },
                            None,
                            false,
                        );
                        found_not_zero_accum.fetch_add(found_not_zero, Ordering::Relaxed);
                        not_found_on_fork_accum.fetch_add(not_found_on_fork, Ordering::Relaxed);
                        missing_accum.fetch_add(missing, Ordering::Relaxed);
                        useful_accum.fetch_add(useful, Ordering::Relaxed);
                        (purges_zero_lamports, purges_old_accounts)
                    })
                    .reduce(
                        || (HashMap::new(), Vec::new()),
                        |mut a, b| {
                            // Collapse down the hashmaps/vecs into one.
                            a.0.extend(b.0);
                            a.1.extend(b.1);
                            a
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
                epoch_schedule,
            );

        self.do_reset_uncleaned_roots(max_clean_root_inclusive);
        clean_old_rooted.stop();

        let mut store_counts_time = Measure::start("store_counts");

        // Calculate store counts as if everything was purged
        // Then purge if we can
        let mut store_counts: HashMap<Slot, (usize, HashSet<Pubkey>)> = HashMap::new();
        for (pubkey, (slot_list, ref_count)) in purges_zero_lamports.iter_mut() {
            if purged_account_slots.contains_key(pubkey) {
                *ref_count = self.accounts_index.ref_count_from_storage(pubkey);
            }
            slot_list.retain(|(slot, account_info)| {
                let was_slot_purged = purged_account_slots
                    .get(pubkey)
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
                    .get(slot)
                    .map(|store_removed| store_removed.contains(&account_info.offset()))
                    .unwrap_or(false);
                if was_reclaimed {
                    return false;
                }
                if let Some(store_count) = store_counts.get_mut(slot) {
                    store_count.0 -= 1;
                    store_count.1.insert(*pubkey);
                } else {
                    let mut key_set = HashSet::new();
                    key_set.insert(*pubkey);
                    assert!(
                        !account_info.is_cached(),
                        "The Accounts Cache must be flushed first for this account info. pubkey: {}, slot: {}",
                        *pubkey,
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
                    store_counts.insert(*slot, (count, key_set));
                }
                true
            });
        }
        store_counts_time.stop();

        let mut calc_deps_time = Measure::start("calc_deps");
        Self::calc_delete_dependencies(&purges_zero_lamports, &mut store_counts, min_dirty_slot);
        calc_deps_time.stop();

        let mut purge_filter = Measure::start("purge_filter");
        self.filter_zero_lamport_clean_for_incremental_snapshots(
            max_clean_root_inclusive,
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
        pubkeys_removed_from_accounts_index.extend(pubkeys_removed_from_accounts_index2);

        // Don't reset from clean, since the pubkeys in those stores may need to be unref'ed
        // and those stores may be used for background hashing.
        let reset_accounts = false;
        self.handle_reclaims(
            (!reclaims.is_empty()).then(|| reclaims.iter()),
            None,
            reset_accounts,
            &pubkeys_removed_from_accounts_index,
            HandleReclaims::ProcessDeadSlots(&self.clean_accounts_stats.purge_stats),
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
            ("total_keys_count", num_candidates, i64),
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
                "uncleaned_roots_slot_list_1",
                self.clean_accounts_stats
                    .uncleaned_roots_slot_list_1
                    .swap(0, Ordering::Relaxed),
                i64
            ),
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
                self.accounts_index.roots_added.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "purge_older_root_entries_one_slot_list",
                self.accounts_index
                    .purge_older_root_entries_one_slot_list
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "roots_removed",
                self.accounts_index.roots_removed.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "active_scans",
                self.accounts_index.active_scans.load(Ordering::Relaxed),
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
    /// * `reset_accounts` - Reset the append_vec store when the store is dead (count==0)
    ///    From the clean and shrink paths it should be false since there may be an in-progress
    ///    hash operation and the stores may hold accounts that need to be unref'ed.
    /// * `pubkeys_removed_from_accounts_index` - These keys have already been removed from the accounts index
    ///    and should not be unref'd. If they exist in the accounts index, they are NEW.
    /// * `handle_reclaims`. `purge_stats` are stats used to track performance of purging dead slots if
    ///    value is `ProcessDeadSlots`.
    ///    Otherwise, there can be no dead slots
    ///    that happen as a result of this call, and the function will check that no slots are
    ///    cleaned up/removed via `process_dead_slots`. For instance, on store, no slots should
    ///    be cleaned up, but during the background clean accounts purges accounts from old rooted
    ///    slots, so outdated slots may be removed.
    fn handle_reclaims<'a, I>(
        &'a self,
        reclaims: Option<I>,
        expected_single_dead_slot: Option<Slot>,
        reset_accounts: bool,
        pubkeys_removed_from_accounts_index: &PubkeysRemovedFromAccountsIndex,
        handle_reclaims: HandleReclaims<'a>,
    ) -> ReclaimResult
    where
        I: Iterator<Item = &'a (Slot, AccountInfo)>,
    {
        let mut reclaim_result = ReclaimResult::default();
        if let Some(reclaims) = reclaims {
            let (dead_slots, reclaimed_offsets) =
                self.remove_dead_accounts(reclaims, expected_single_dead_slot, reset_accounts);
            reclaim_result.1 = reclaimed_offsets;

            if let HandleReclaims::ProcessDeadSlots(purge_stats) = handle_reclaims {
                if let Some(expected_single_dead_slot) = expected_single_dead_slot {
                    assert!(dead_slots.len() <= 1);
                    if dead_slots.len() == 1 {
                        assert!(dead_slots.contains(&expected_single_dead_slot));
                    }
                }

                self.process_dead_slots(
                    &dead_slots,
                    Some(&mut reclaim_result.0),
                    purge_stats,
                    pubkeys_removed_from_accounts_index,
                );
            } else {
                assert!(dead_slots.is_empty());
            }
        }
        reclaim_result
    }

    /// During clean, some zero-lamport accounts that are marked for purge should *not* actually
    /// get purged.  Filter out those accounts here by removing them from 'purges_zero_lamports'
    ///
    /// When using incremental snapshots, do not purge zero-lamport accounts if the slot is higher
    /// than the latest full snapshot slot.  This is to protect against the following scenario:
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
    /// This filtering step can be skipped if there is no `latest_full_snapshot_slot`, or if the
    /// `max_clean_root_inclusive` is less-than-or-equal-to the `latest_full_snapshot_slot`.
    fn filter_zero_lamport_clean_for_incremental_snapshots(
        &self,
        max_clean_root_inclusive: Option<Slot>,
        store_counts: &HashMap<Slot, (usize, HashSet<Pubkey>)>,
        purges_zero_lamports: &mut HashMap<Pubkey, (SlotList<AccountInfo>, RefCount)>,
    ) {
        let latest_full_snapshot_slot = self.latest_full_snapshot_slot();
        let should_filter_for_incremental_snapshots = max_clean_root_inclusive.unwrap_or(Slot::MAX)
            > latest_full_snapshot_slot.unwrap_or(Slot::MAX);
        assert!(
            latest_full_snapshot_slot.is_some() || !should_filter_for_incremental_snapshots,
            "if filtering for incremental snapshots, then snapshots should be enabled",
        );

        purges_zero_lamports.retain(|pubkey, (slot_account_infos, _ref_count)| {
            // Only keep purges_zero_lamports where the entire history of the account in the root set
            // can be purged. All AppendVecs for those updates are dead.
            for (slot, _account_info) in slot_account_infos.iter() {
                if let Some(store_count) = store_counts.get(slot) {
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
                let cannot_purge = *slot > latest_full_snapshot_slot.unwrap();
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
        dead_slots: &IntSet<Slot>,
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
    fn load_accounts_index_for_shrink<'a, T: ShrinkCollectRefs<'a>>(
        &self,
        accounts: &'a [AccountFromStorage],
        stats: &ShrinkStats,
        slot_to_shrink: Slot,
    ) -> LoadAccountsIndexForShrink<'a, T> {
        let count = accounts.len();
        let mut alive_accounts = T::with_capacity(count, slot_to_shrink);
        let mut unrefed_pubkeys = Vec::with_capacity(count);

        let mut alive = 0;
        let mut dead = 0;
        let mut index = 0;
        let mut all_are_zero_lamports = true;
        let mut index_entries_being_shrunk = Vec::with_capacity(accounts.len());
        self.accounts_index.scan(
            accounts.iter().map(|account| account.pubkey()),
            |pubkey, slots_refs, entry| {
                let mut result = AccountsIndexScanResult::OnlyKeepInMemoryIfDirty;
                if let Some((slot_list, ref_count)) = slots_refs {
                    let stored_account = &accounts[index];
                    let is_alive = slot_list.iter().any(|(slot, _acct_info)| {
                        // if the accounts index contains an entry at this slot, then the append vec we're asking about contains this item and thus, it is alive at this slot
                        *slot == slot_to_shrink
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
                        // Hold onto the index entry arc so that it cannot be flushed.
                        // Since we are shrinking these entries, we need to disambiguate storage ids during this period and those only exist in the in-memory accounts index.
                        index_entries_being_shrunk.push(Arc::clone(entry.unwrap()));
                        all_are_zero_lamports &= stored_account.is_zero_lamport();
                        alive_accounts.add(ref_count, stored_account, slot_list);
                        alive += 1;
                    }
                }
                index += 1;
                result
            },
            None,
            true,
        );
        assert_eq!(index, std::cmp::min(accounts.len(), count));
        stats.alive_accounts.fetch_add(alive, Ordering::Relaxed);
        stats.dead_accounts.fetch_add(dead, Ordering::Relaxed);

        LoadAccountsIndexForShrink {
            alive_accounts,
            unrefed_pubkeys,
            all_are_zero_lamports,
            index_entries_being_shrunk,
        }
    }

    /// get all accounts in all the storages passed in
    /// for duplicate pubkeys, the account with the highest write_value is returned
    pub fn get_unique_accounts_from_storage(
        &self,
        store: &Arc<AccountStorageEntry>,
    ) -> GetUniqueAccountsResult {
        let capacity = store.capacity();
        let mut stored_accounts = Vec::with_capacity(store.count());
        store.accounts.scan_index(|info| {
            // file_id is unused and can be anything. We will always be loading whatever storage is in the slot.
            let file_id = 0;
            stored_accounts.push(AccountFromStorage {
                index_info: AccountInfo::new(
                    StorageLocation::AppendVec(file_id, info.index_info.offset),
                    info.index_info.lamports,
                ),
                pubkey: info.index_info.pubkey,
                data_len: info.index_info.data_len,
            });
        });

        // sort by pubkey to keep account index lookups close
        let num_duplicated_accounts = Self::sort_and_remove_dups(&mut stored_accounts);

        GetUniqueAccountsResult {
            stored_accounts,
            capacity,
            num_duplicated_accounts,
        }
    }

    /// Sort `accounts` by pubkey and removes all but the *last* of consecutive
    /// accounts in the vector with the same pubkey.
    ///
    /// Return the number of duplicated elements in the vector.
    #[cfg_attr(feature = "dev-context-only-utils", qualifiers(pub))]
    fn sort_and_remove_dups(accounts: &mut Vec<AccountFromStorage>) -> usize {
        // stable sort because we want the most recent only
        accounts.sort_by(|a, b| a.pubkey().cmp(b.pubkey()));
        let len0 = accounts.len();
        if accounts.len() > 1 {
            let mut last = 0;
            let mut curr = 1;

            while curr < accounts.len() {
                if accounts[curr].pubkey() == accounts[last].pubkey() {
                    accounts[last] = accounts[curr];
                } else {
                    last += 1;
                    accounts[last] = accounts[curr];
                }
                curr += 1;
            }
            accounts.truncate(last + 1);
        }
        len0 - accounts.len()
    }

    pub(crate) fn get_unique_accounts_from_storage_for_shrink(
        &self,
        store: &Arc<AccountStorageEntry>,
        stats: &ShrinkStats,
    ) -> GetUniqueAccountsResult {
        let (result, storage_read_elapsed_us) =
            measure_us!(self.get_unique_accounts_from_storage(store));
        stats
            .storage_read_elapsed
            .fetch_add(storage_read_elapsed_us, Ordering::Relaxed);
        stats
            .num_duplicated_accounts
            .fetch_add(result.num_duplicated_accounts as u64, Ordering::Relaxed);
        result
    }

    /// shared code for shrinking normal slots and combining into ancient append vecs
    /// note 'unique_accounts' is passed by ref so we can return references to data within it, avoiding self-references
    pub(crate) fn shrink_collect<'a: 'b, 'b, T: ShrinkCollectRefs<'b>>(
        &self,
        store: &'a Arc<AccountStorageEntry>,
        unique_accounts: &'b GetUniqueAccountsResult,
        stats: &ShrinkStats,
    ) -> ShrinkCollect<'b, T> {
        let slot = store.slot();

        let GetUniqueAccountsResult {
            stored_accounts,
            capacity,
            num_duplicated_accounts,
        } = unique_accounts;

        let mut index_read_elapsed = Measure::start("index_read_elapsed");

        let len = stored_accounts.len();
        let alive_accounts_collect = Mutex::new(T::with_capacity(len, slot));
        let unrefed_pubkeys_collect = Mutex::new(Vec::with_capacity(len));
        stats
            .accounts_loaded
            .fetch_add(len as u64, Ordering::Relaxed);
        stats
            .num_duplicated_accounts
            .fetch_add(*num_duplicated_accounts as u64, Ordering::Relaxed);
        let all_are_zero_lamports_collect = Mutex::new(true);
        let index_entries_being_shrunk_outer = Mutex::new(Vec::default());
        self.thread_pool_clean.install(|| {
            stored_accounts
                .par_chunks(SHRINK_COLLECT_CHUNK_SIZE)
                .for_each(|stored_accounts| {
                    let LoadAccountsIndexForShrink {
                        alive_accounts,
                        mut unrefed_pubkeys,
                        all_are_zero_lamports,
                        mut index_entries_being_shrunk,
                    } = self.load_accounts_index_for_shrink(stored_accounts, stats, slot);

                    // collect
                    alive_accounts_collect
                        .lock()
                        .unwrap()
                        .collect(alive_accounts);
                    unrefed_pubkeys_collect
                        .lock()
                        .unwrap()
                        .append(&mut unrefed_pubkeys);
                    index_entries_being_shrunk_outer
                        .lock()
                        .unwrap()
                        .append(&mut index_entries_being_shrunk);
                    if !all_are_zero_lamports {
                        *all_are_zero_lamports_collect.lock().unwrap() = false;
                    }
                });
        });

        let alive_accounts = alive_accounts_collect.into_inner().unwrap();
        let unrefed_pubkeys = unrefed_pubkeys_collect.into_inner().unwrap();

        index_read_elapsed.stop();
        stats
            .index_read_elapsed
            .fetch_add(index_read_elapsed.as_us(), Ordering::Relaxed);

        let alive_total_bytes = alive_accounts.alive_bytes();

        stats
            .accounts_removed
            .fetch_add(len - alive_accounts.len(), Ordering::Relaxed);
        stats.bytes_removed.fetch_add(
            capacity.saturating_sub(alive_total_bytes as u64),
            Ordering::Relaxed,
        );
        stats
            .bytes_written
            .fetch_add(alive_total_bytes as u64, Ordering::Relaxed);

        ShrinkCollect {
            slot,
            capacity: *capacity,
            unrefed_pubkeys,
            alive_accounts,
            alive_total_bytes,
            total_starting_accounts: len,
            all_are_zero_lamports: all_are_zero_lamports_collect.into_inner().unwrap(),
            _index_entries_being_shrunk: index_entries_being_shrunk_outer.into_inner().unwrap(),
        }
    }

    /// common code from shrink and combine_ancient_slots
    /// get rid of all original store_ids in the slot
    pub(crate) fn remove_old_stores_shrink<'a, T: ShrinkCollectRefs<'a>>(
        &self,
        shrink_collect: &ShrinkCollect<'a, T>,
        stats: &ShrinkStats,
        shrink_in_progress: Option<ShrinkInProgress>,
        shrink_can_be_active: bool,
    ) {
        let mut time = Measure::start("remove_old_stores_shrink");
        // Purge old, overwritten storage entries
        let dead_storages = self.mark_dirty_dead_stores(
            shrink_collect.slot,
            // If all accounts are zero lamports, then we want to mark the entire OLD append vec as dirty.
            // otherwise, we'll call 'add_uncleaned_pubkeys_after_shrink' just on the unref'd keys below.
            shrink_collect.all_are_zero_lamports,
            shrink_in_progress,
            shrink_can_be_active,
        );
        let dead_storages_len = dead_storages.len();

        if !shrink_collect.all_are_zero_lamports {
            self.add_uncleaned_pubkeys_after_shrink(
                shrink_collect.slot,
                shrink_collect.unrefed_pubkeys.iter().cloned().cloned(),
            );
        }

        let (_, drop_storage_entries_elapsed) = measure_us!(drop(dead_storages));
        time.stop();

        self.stats
            .dropped_stores
            .fetch_add(dead_storages_len as u64, Ordering::Relaxed);
        stats
            .drop_storage_entries_elapsed
            .fetch_add(drop_storage_entries_elapsed, Ordering::Relaxed);
        stats
            .remove_old_stores_shrink_us
            .fetch_add(time.as_us(), Ordering::Relaxed);
    }

    fn do_shrink_slot_store(&self, slot: Slot, store: &Arc<AccountStorageEntry>) {
        if self.accounts_cache.contains(slot) {
            // It is not correct to shrink a slot while it is in the write cache until flush is complete and the slot is removed from the write cache.
            // There can exist a window after a slot is made a root and before the write cache flushing for that slot begins and then completes.
            // There can also exist a window after a slot is being flushed from the write cache until the index is updated and the slot is removed from the write cache.
            // During the second window, once an append vec has been created for the slot, it could be possible to try to shrink that slot.
            // Shrink no-ops before this function if there is no store for the slot (notice this function requires 'store' to be passed).
            // So, if we enter this function but the slot is still in the write cache, reasonable behavior is to skip shrinking this slot.
            // Flush will ONLY write alive accounts to the append vec, which is what shrink does anyway.
            // Flush then adds the slot to 'uncleaned_roots', which causes clean to take a look at the slot.
            // Clean causes us to mark accounts as dead, which causes shrink to later take a look at the slot.
            // This could be an assert, but it could lead to intermittency in tests.
            // It is 'correct' to ignore calls to shrink when a slot is still in the write cache.
            return;
        }
        let unique_accounts =
            self.get_unique_accounts_from_storage_for_shrink(store, &self.shrink_stats);
        debug!("do_shrink_slot_store: slot: {}", slot);
        let shrink_collect =
            self.shrink_collect::<AliveAccounts<'_>>(store, &unique_accounts, &self.shrink_stats);

        // This shouldn't happen if alive_bytes/approx_stored_count are accurate.
        // However, it is possible that the remaining alive bytes could be 0. In that case, the whole slot should be marked dead by clean.
        if Self::should_not_shrink(
            shrink_collect.alive_total_bytes as u64,
            shrink_collect.capacity,
        ) || shrink_collect.alive_total_bytes == 0
        {
            if shrink_collect.alive_total_bytes == 0 {
                // clean needs to take care of this dead slot
                self.accounts_index.add_uncleaned_roots([slot]);
            }
            info!(
                "Unexpected shrink for slot {} alive {} capacity {}, \
                likely caused by a bug for calculating alive bytes.",
                slot, shrink_collect.alive_total_bytes, shrink_collect.capacity
            );

            self.shrink_stats
                .skipped_shrink
                .fetch_add(1, Ordering::Relaxed);

            self.accounts_index.scan(
                shrink_collect.unrefed_pubkeys.into_iter(),
                |pubkey, _slot_refs, entry| {
                    // pubkeys in `unrefed_pubkeys` were unref'd in `shrink_collect` above under the assumption that we would shrink everything.
                    // Since shrink is not occurring, we need to addref the pubkeys to get the system back to the prior state since the account still exists at this slot.
                    if let Some(entry) = entry {
                        entry.addref();
                    } else {
                        // We also expect that the accounts index must contain an
                        // entry for `pubkey`. Log a warning for now. In future,
                        // we will panic when this happens.
                        warn!("pubkey {pubkey} in slot {slot} was NOT found in accounts index during shrink");
                        datapoint_warn!(
                            "accounts_db-shink_pubkey_missing_from_index",
                            ("store_slot", slot, i64),
                            ("pubkey", pubkey.to_string(), String),
                        )
                    }
                    AccountsIndexScanResult::OnlyKeepInMemoryIfDirty
                },
                None,
                true,
            );
            return;
        }

        let total_accounts_after_shrink = shrink_collect.alive_accounts.len();
        debug!(
            "shrinking: slot: {}, accounts: ({} => {}) bytes: {} original: {}",
            slot,
            shrink_collect.total_starting_accounts,
            total_accounts_after_shrink,
            shrink_collect.alive_total_bytes,
            shrink_collect.capacity,
        );

        let mut stats_sub = ShrinkStatsSub::default();
        let mut rewrite_elapsed = Measure::start("rewrite_elapsed");
        let (shrink_in_progress, time_us) =
            measure_us!(self.get_store_for_shrink(slot, shrink_collect.alive_total_bytes as u64));
        stats_sub.create_and_insert_store_elapsed_us = Saturating(time_us);

        // here, we're writing back alive_accounts. That should be an atomic operation
        // without use of rather wide locks in this whole function, because we're
        // mutating rooted slots; There should be no writers to them.
        let accounts = [(slot, &shrink_collect.alive_accounts.alive_accounts()[..])];
        let storable_accounts = StorableAccountsBySlot::new(slot, &accounts, self);
        stats_sub.store_accounts_timing =
            self.store_accounts_frozen(storable_accounts, shrink_in_progress.new_storage());

        rewrite_elapsed.stop();
        stats_sub.rewrite_elapsed_us = Saturating(rewrite_elapsed.as_us());

        // `store_accounts_frozen()` above may have purged accounts from some
        // other storage entries (the ones that were just overwritten by this
        // new storage entry). This means some of those stores might have caused
        // this slot to be read to `self.shrink_candidate_slots`, so delete
        // those here
        self.shrink_candidate_slots.lock().unwrap().remove(&slot);

        self.remove_old_stores_shrink(
            &shrink_collect,
            &self.shrink_stats,
            Some(shrink_in_progress),
            false,
        );

        self.reopen_storage_as_readonly_shrinking_in_progress_ok(slot);

        Self::update_shrink_stats(&self.shrink_stats, stats_sub, true);
        self.shrink_stats.report();
    }

    pub(crate) fn update_shrink_stats(
        shrink_stats: &ShrinkStats,
        stats_sub: ShrinkStatsSub,
        increment_count: bool,
    ) {
        if increment_count {
            shrink_stats
                .num_slots_shrunk
                .fetch_add(1, Ordering::Relaxed);
        }
        shrink_stats.create_and_insert_store_elapsed.fetch_add(
            stats_sub.create_and_insert_store_elapsed_us.0,
            Ordering::Relaxed,
        );
        shrink_stats.store_accounts_elapsed.fetch_add(
            stats_sub.store_accounts_timing.store_accounts_elapsed,
            Ordering::Relaxed,
        );
        shrink_stats.update_index_elapsed.fetch_add(
            stats_sub.store_accounts_timing.update_index_elapsed,
            Ordering::Relaxed,
        );
        shrink_stats.handle_reclaims_elapsed.fetch_add(
            stats_sub.store_accounts_timing.handle_reclaims_elapsed,
            Ordering::Relaxed,
        );
        shrink_stats
            .rewrite_elapsed
            .fetch_add(stats_sub.rewrite_elapsed_us.0, Ordering::Relaxed);
        shrink_stats
            .unpackable_slots_count
            .fetch_add(stats_sub.unpackable_slots_count.0 as u64, Ordering::Relaxed);
        shrink_stats.newest_alive_packed_count.fetch_add(
            stats_sub.newest_alive_packed_count.0 as u64,
            Ordering::Relaxed,
        );
    }

    /// get stores for 'slot'
    /// Drop 'shrink_in_progress', which will cause the old store to be removed from the storage map.
    /// For 'shrink_in_progress'.'old_storage' which is not retained, insert in 'dead_storages' and optionally 'dirty_stores'
    /// This is the end of the life cycle of `shrink_in_progress`.
    pub fn mark_dirty_dead_stores(
        &self,
        slot: Slot,
        add_dirty_stores: bool,
        shrink_in_progress: Option<ShrinkInProgress>,
        shrink_can_be_active: bool,
    ) -> Vec<Arc<AccountStorageEntry>> {
        let mut dead_storages = Vec::default();

        let mut not_retaining_store = |store: &Arc<AccountStorageEntry>| {
            if add_dirty_stores {
                self.dirty_stores.insert(slot, store.clone());
            }
            dead_storages.push(store.clone());
        };

        if let Some(shrink_in_progress) = shrink_in_progress {
            // shrink is in progress, so 1 new append vec to keep, 1 old one to throw away
            not_retaining_store(shrink_in_progress.old_storage());
            // dropping 'shrink_in_progress' removes the old append vec that was being shrunk from db's storage
        } else if let Some(store) = self.storage.remove(&slot, shrink_can_be_active) {
            // no shrink in progress, so all append vecs in this slot are dead
            not_retaining_store(&store);
        }

        dead_storages
    }

    /// we are done writing to the storage at `slot`. It can be re-opened as read-only if that would help
    /// system performance.
    pub(crate) fn reopen_storage_as_readonly_shrinking_in_progress_ok(&self, slot: Slot) {
        if let Some(storage) = self
            .storage
            .get_slot_storage_entry_shrinking_in_progress_ok(slot)
        {
            if let Some(new_storage) = storage.reopen_as_readonly(self.storage_access) {
                // consider here the race condition of tx processing having looked up something in the index,
                // which could return (slot, append vec id). We want the lookup for the storage to get a storage
                // that works whether the lookup occurs before or after the replace call here.
                // So, the two storages have to be exactly equivalent wrt offsets, counts, len, id, etc.
                assert_eq!(storage.id(), new_storage.id());
                assert_eq!(storage.accounts.len(), new_storage.accounts.len());
                self.storage
                    .replace_storage_with_equivalent(slot, Arc::new(new_storage));
            }
        }
    }

    /// return a store that can contain 'size' bytes
    pub fn get_store_for_shrink(&self, slot: Slot, size: u64) -> ShrinkInProgress<'_> {
        let shrunken_store = self.create_store(slot, size, "shrink", self.shrink_paths.as_slice());
        self.storage.shrinking_in_progress(slot, shrunken_store)
    }

    // Reads all accounts in given slot's AppendVecs and filter only to alive,
    // then create a minimum AppendVec filled with the alive.
    fn shrink_slot_forced(&self, slot: Slot) {
        debug!("shrink_slot_forced: slot: {}", slot);

        if let Some(store) = self
            .storage
            .get_slot_storage_entry_shrinking_in_progress_ok(slot)
        {
            if !Self::is_shrinking_productive(slot, &store) {
                return;
            }
            self.do_shrink_slot_store(slot, &store)
        }
    }

    fn all_slots_in_storage(&self) -> Vec<Slot> {
        self.storage.all_slots()
    }

    /// Given the input `ShrinkCandidates`, this function sorts the stores by their alive ratio
    /// in increasing order with the most sparse entries in the front. It will then simulate the
    /// shrinking by working on the most sparse entries first and if the overall alive ratio is
    /// achieved, it will stop and return:
    /// first tuple element: the filtered-down candidates and
    /// second duple element: the candidates which
    /// are skipped in this round and might be eligible for the future shrink.
    fn select_candidates_by_total_usage(
        &self,
        shrink_slots: &ShrinkCandidates,
        shrink_ratio: f64,
        oldest_non_ancient_slot: Option<Slot>,
    ) -> (IntMap<Slot, Arc<AccountStorageEntry>>, ShrinkCandidates) {
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
        for slot in shrink_slots {
            if oldest_non_ancient_slot
                .map(|oldest_non_ancient_slot| slot < &oldest_non_ancient_slot)
                .unwrap_or_default()
            {
                // this slot will be 'shrunk' by ancient code
                continue;
            }
            let Some(store) = self.storage.get_slot_storage_entry(*slot) else {
                continue;
            };
            candidates_count += 1;
            let alive_bytes = store.alive_bytes();
            total_alive_bytes += alive_bytes as u64;
            total_bytes += store.capacity();
            let alive_ratio = alive_bytes as f64 / store.capacity() as f64;
            store_usage.push(StoreUsageInfo {
                slot: *slot,
                alive_ratio,
                store: store.clone(),
            });
            total_candidate_stores += 1;
        }
        store_usage.sort_by(|a, b| {
            a.alive_ratio
                .partial_cmp(&b.alive_ratio)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Working from the beginning of store_usage which are the most sparse and see when we can stop
        // shrinking while still achieving the overall goals.
        let mut shrink_slots = IntMap::default();
        let mut shrink_slots_next_batch = ShrinkCandidates::default();
        for usage in &store_usage {
            let store = &usage.store;
            let alive_ratio = (total_alive_bytes as f64) / (total_bytes as f64);
            debug!("alive_ratio: {:?} store_id: {:?}, store_ratio: {:?} requirement: {:?}, total_bytes: {:?} total_alive_bytes: {:?}",
                alive_ratio, usage.store.id(), usage.alive_ratio, shrink_ratio, total_bytes, total_alive_bytes);
            if alive_ratio > shrink_ratio {
                // we have reached our goal, stop
                debug!(
                    "Shrinking goal can be achieved at slot {:?}, total_alive_bytes: {:?} \
                    total_bytes: {:?}, alive_ratio: {:}, shrink_ratio: {:?}",
                    usage.slot, total_alive_bytes, total_bytes, alive_ratio, shrink_ratio
                );
                if usage.alive_ratio < shrink_ratio {
                    shrink_slots_next_batch.insert(usage.slot);
                } else {
                    break;
                }
            } else {
                let current_store_size = store.capacity();
                let after_shrink_size = store.alive_bytes() as u64;
                let bytes_saved = current_store_size.saturating_sub(after_shrink_size);
                total_bytes -= bytes_saved;
                shrink_slots.insert(usage.slot, Arc::clone(store));
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

    /// return all slots that are more than one epoch old and thus could already be an ancient append vec
    /// or which could need to be combined into a new or existing ancient append vec
    /// offset is used to combine newer slots than we normally would. This is designed to be used for testing.
    fn get_sorted_potential_ancient_slots(&self, oldest_non_ancient_slot: Slot) -> Vec<Slot> {
        let mut ancient_slots = self.get_roots_less_than(oldest_non_ancient_slot);
        ancient_slots.sort_unstable();
        ancient_slots
    }

    /// get a sorted list of slots older than an epoch
    /// squash those slots into ancient append vecs
    pub fn shrink_ancient_slots(&self, epoch_schedule: &EpochSchedule) {
        if self.ancient_append_vec_offset.is_none() {
            return;
        }

        let oldest_non_ancient_slot = self.get_oldest_non_ancient_slot(epoch_schedule);
        let can_randomly_shrink = true;
        let sorted_slots = self.get_sorted_potential_ancient_slots(oldest_non_ancient_slot);
        if self.create_ancient_storage == CreateAncientStorage::Append {
            self.combine_ancient_slots(sorted_slots, can_randomly_shrink);
        } else {
            self.combine_ancient_slots_packed(sorted_slots, can_randomly_shrink);
        }
    }

    /// 'accounts' that exist in the current slot we are combining into a different ancient slot
    /// 'existing_ancient_pubkeys': pubkeys that exist currently in the ancient append vec slot
    /// returns the pubkeys that are in 'accounts' that are already in 'existing_ancient_pubkeys'
    /// Also updated 'existing_ancient_pubkeys' to include all pubkeys in 'accounts' since they will soon be written into the ancient slot.
    fn get_keys_to_unref_ancient<'a>(
        accounts: &'a [&AccountFromStorage],
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
        accounts: &[&AccountFromStorage],
        existing_ancient_pubkeys: &mut HashSet<Pubkey>,
    ) {
        let unref = Self::get_keys_to_unref_ancient(accounts, existing_ancient_pubkeys);

        self.unref_pubkeys(
            unref.iter().cloned(),
            unref.len(),
            &PubkeysRemovedFromAccountsIndex::default(),
        );
    }

    /// get the storage from 'slot' to squash
    /// or None if this slot should be skipped
    /// side effect could be updating 'current_ancient'
    fn get_storage_to_move_to_ancient_accounts_file(
        &self,
        slot: Slot,
        current_ancient: &mut CurrentAncientAccountsFile,
        can_randomly_shrink: bool,
    ) -> Option<Arc<AccountStorageEntry>> {
        self.storage
            .get_slot_storage_entry(slot)
            .and_then(|storage| {
                self.should_move_to_ancient_accounts_file(
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
    fn should_move_to_ancient_accounts_file(
        &self,
        storage: &Arc<AccountStorageEntry>,
        current_ancient: &mut CurrentAncientAccountsFile,
        slot: Slot,
        can_randomly_shrink: bool,
    ) -> bool {
        let accounts = &storage.accounts;

        self.shrink_ancient_stats
            .slots_considered
            .fetch_add(1, Ordering::Relaxed);

        // if an append vec is at least 80% of the ideal capacity of an ancient append vec, that's close enough.
        // If we packed, then we end up allocating exact size ancient append vecs. Those will likely never be exactly the ideal ancient capacity.
        if accounts.capacity() * 100 / get_ancient_append_vec_capacity() > 80 {
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
            if is_candidate || (can_randomly_shrink && thread_rng().gen_range(0..10000) == 0) {
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
            if storage.accounts.can_append() {
                // this slot is ancient and can become the 'current' ancient for other slots to be squashed into
                *current_ancient = CurrentAncientAccountsFile::new(slot, Arc::clone(storage));
            } else {
                *current_ancient = CurrentAncientAccountsFile::default();
            }
            return false; // we're done with this slot - this slot IS the ancient append vec
        }

        // otherwise, yes, squash this slot into the current ancient append vec or create one at this slot
        true
    }

    /// Combine all account data from storages in 'sorted_slots' into ancient append vecs.
    /// This keeps us from accumulating append vecs for each slot older than an epoch.
    fn combine_ancient_slots(&self, sorted_slots: Vec<Slot>, can_randomly_shrink: bool) {
        if sorted_slots.is_empty() {
            return;
        }

        let mut total = Measure::start("combine_ancient_slots");
        let mut guard = None;

        // the ancient append vec currently being written to
        let mut current_ancient = CurrentAncientAccountsFile::default();
        let mut dropped_roots = vec![];

        // we have to keep track of what pubkeys exist in the current ancient append vec so we can unref correctly
        let mut ancient_slot_pubkeys = AncientSlotPubkeys::default();

        let len = sorted_slots.len();
        for slot in sorted_slots {
            let Some(old_storage) = self.get_storage_to_move_to_ancient_accounts_file(
                slot,
                &mut current_ancient,
                can_randomly_shrink,
            ) else {
                // nothing to squash for this slot
                continue;
            };

            if guard.is_none() {
                // we are now doing interesting work in squashing ancient
                guard = Some(self.active_stats.activate(ActiveStatItem::SquashAncient));
                info!(
                    "ancient_append_vec: combine_ancient_slots first slot: {}, num_roots: {}",
                    slot, len
                );
            }

            self.combine_one_store_into_ancient(
                slot,
                &old_storage,
                &mut current_ancient,
                &mut ancient_slot_pubkeys,
                &mut dropped_roots,
            );
        }

        self.handle_dropped_roots_for_ancient(dropped_roots.into_iter());

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

    /// put entire alive contents of 'old_storage' into the current ancient append vec or a newly created ancient append vec
    fn combine_one_store_into_ancient(
        &self,
        slot: Slot,
        old_storage: &Arc<AccountStorageEntry>,
        current_ancient: &mut CurrentAncientAccountsFile,
        ancient_slot_pubkeys: &mut AncientSlotPubkeys,
        dropped_roots: &mut Vec<Slot>,
    ) {
        let unique_accounts = self.get_unique_accounts_from_storage_for_shrink(
            old_storage,
            &self.shrink_ancient_stats.shrink_stats,
        );
        let shrink_collect = self.shrink_collect::<AliveAccounts<'_>>(
            old_storage,
            &unique_accounts,
            &self.shrink_ancient_stats.shrink_stats,
        );

        // could follow what shrink does more closely
        if shrink_collect.total_starting_accounts == 0 || shrink_collect.alive_total_bytes == 0 {
            return; // skipping slot with no useful accounts to write
        }

        let mut stats_sub = ShrinkStatsSub::default();
        let mut bytes_remaining_to_write = shrink_collect.alive_total_bytes;
        let (mut shrink_in_progress, create_and_insert_store_elapsed_us) = measure_us!(
            current_ancient.create_if_necessary(slot, self, shrink_collect.alive_total_bytes)
        );
        stats_sub.create_and_insert_store_elapsed_us =
            Saturating(create_and_insert_store_elapsed_us);
        let available_bytes = current_ancient.accounts_file().accounts.remaining_bytes();
        // split accounts in 'slot' into:
        // 'Primary', which can fit in 'current_ancient'
        // 'Overflow', which will have to go into a new ancient append vec at 'slot'
        let to_store = AccountsToStore::new(
            available_bytes,
            shrink_collect.alive_accounts.alive_accounts(),
            shrink_collect.alive_total_bytes,
            slot,
        );

        ancient_slot_pubkeys.maybe_unref_accounts_already_in_ancient(
            slot,
            self,
            current_ancient,
            &to_store,
        );

        let mut rewrite_elapsed = Measure::start("rewrite_elapsed");
        // write what we can to the current ancient storage
        let (store_accounts_timing, bytes_written) =
            current_ancient.store_ancient_accounts(self, &to_store, StorageSelector::Primary);
        stats_sub.store_accounts_timing = store_accounts_timing;
        bytes_remaining_to_write = bytes_remaining_to_write.saturating_sub(bytes_written as usize);

        // handle accounts from 'slot' which did not fit into the current ancient append vec
        if to_store.has_overflow() {
            // We need a new ancient append vec at this slot.
            // Assert: it cannot be the case that we already had an ancient append vec at this slot and
            // yet that ancient append vec does not have room for the accounts stored at this slot currently
            assert_ne!(slot, current_ancient.slot());

            // we filled one up
            self.reopen_storage_as_readonly_shrinking_in_progress_ok(current_ancient.slot());

            // Now we create an ancient append vec at `slot` to store the overflows.
            let (shrink_in_progress_overflow, time_us) = measure_us!(current_ancient
                .create_ancient_accounts_file(
                    slot,
                    self,
                    to_store.get_bytes(StorageSelector::Overflow)
                ));
            stats_sub.create_and_insert_store_elapsed_us += time_us;
            // We cannot possibly be shrinking the original slot that created an ancient append vec
            // AND not have enough room in the ancient append vec at that slot
            // to hold all the contents of that slot.
            // We need this new 'shrink_in_progress' to be used in 'remove_old_stores_shrink' below.
            // All non-overflow accounts were put in a prior slot's ancient append vec. All overflow accounts
            // are essentially being shrunk into a new ancient append vec in 'slot'.
            assert!(shrink_in_progress.is_none());
            shrink_in_progress = Some(shrink_in_progress_overflow);

            // write the overflow accounts to the next ancient storage
            let (store_accounts_timing, bytes_written) =
                current_ancient.store_ancient_accounts(self, &to_store, StorageSelector::Overflow);
            bytes_remaining_to_write =
                bytes_remaining_to_write.saturating_sub(bytes_written as usize);

            stats_sub
                .store_accounts_timing
                .accumulate(&store_accounts_timing);
        }
        assert_eq!(bytes_remaining_to_write, 0);
        rewrite_elapsed.stop();
        stats_sub.rewrite_elapsed_us = Saturating(rewrite_elapsed.as_us());

        if slot != current_ancient.slot() {
            // all append vecs in this slot have been combined into an ancient append vec
            dropped_roots.push(slot);
        }

        self.remove_old_stores_shrink(
            &shrink_collect,
            &self.shrink_ancient_stats.shrink_stats,
            shrink_in_progress,
            false,
        );

        // we should not try to shrink any of the stores from this slot anymore. All shrinking for this slot is now handled by ancient append vec code.
        self.shrink_candidate_slots.lock().unwrap().remove(&slot);

        Self::update_shrink_stats(&self.shrink_ancient_stats.shrink_stats, stats_sub, true);
    }

    /// each slot in 'dropped_roots' has been combined into an ancient append vec.
    /// We are done with the slot now forever.
    pub(crate) fn handle_dropped_roots_for_ancient(
        &self,
        dropped_roots: impl Iterator<Item = Slot>,
    ) {
        let mut accounts_delta_hashes = self.accounts_delta_hashes.lock().unwrap();
        let mut bank_hash_stats = self.bank_hash_stats.lock().unwrap();

        dropped_roots.for_each(|slot| {
            self.accounts_index.clean_dead_slot(slot);
            accounts_delta_hashes.remove(&slot);
            bank_hash_stats.remove(&slot);
            // the storage has been removed from this slot and recycled or dropped
            assert!(self.storage.remove(&slot, false).is_none());
            debug_assert!(
                !self
                    .accounts_index
                    .roots_tracker
                    .read()
                    .unwrap()
                    .alive_roots
                    .contains(&slot),
                "slot: {slot}"
            );
        });
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

        let mut uncleaned_pubkeys = self.uncleaned_pubkeys.entry(slot).or_default();
        uncleaned_pubkeys.extend(pubkeys);
    }

    pub fn shrink_candidate_slots(&self, epoch_schedule: &EpochSchedule) -> usize {
        let oldest_non_ancient_slot = self.get_oldest_non_ancient_slot(epoch_schedule);

        let shrink_candidates_slots =
            std::mem::take(&mut *self.shrink_candidate_slots.lock().unwrap());

        let (shrink_slots, shrink_slots_next_batch) = {
            if let AccountShrinkThreshold::TotalSpace { shrink_ratio } = self.shrink_ratio {
                let (shrink_slots, shrink_slots_next_batch) = self
                    .select_candidates_by_total_usage(
                        &shrink_candidates_slots,
                        shrink_ratio,
                        self.ancient_append_vec_offset
                            .map(|_| oldest_non_ancient_slot),
                    );
                (shrink_slots, Some(shrink_slots_next_batch))
            } else {
                (
                    // lookup storage for each slot
                    shrink_candidates_slots
                        .into_iter()
                        .filter_map(|slot| {
                            self.storage
                                .get_slot_storage_entry(slot)
                                .map(|storage| (slot, storage))
                        })
                        .collect(),
                    None,
                )
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

        let _guard = (!shrink_slots.is_empty())
            .then_some(|| self.active_stats.activate(ActiveStatItem::Shrink));

        let mut measure_shrink_all_candidates = Measure::start("shrink_all_candidate_slots-ms");
        let num_candidates = shrink_slots.len();
        let shrink_candidates_count = shrink_slots.len();
        self.thread_pool_clean.install(|| {
            shrink_slots
                .into_par_iter()
                .for_each(|(slot, slot_shrink_candidate)| {
                    let mut measure = Measure::start("shrink_candidate_slots-ms");
                    self.do_shrink_slot_store(slot, &slot_shrink_candidate);
                    measure.stop();
                    inc_new_counter_info!("shrink_candidate_slots-ms", measure.as_ms() as usize);
                });
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
            pended_counts += shrink_slots_next_batch.len();
            for slot in shrink_slots_next_batch {
                shrink_slots.insert(slot);
            }
        }
        inc_new_counter_info!("shrink_pended_stores-count", pended_counts);

        num_candidates
    }

    /// This is only called at startup from bank when we are being extra careful such as when we downloaded a snapshot.
    /// Also called from tests.
    /// `newest_slot_skip_shrink_inclusive` is used to avoid shrinking the slot we are loading a snapshot from. If we shrink that slot, we affect
    /// the bank hash calculation verification at startup.
    pub fn shrink_all_slots(
        &self,
        is_startup: bool,
        epoch_schedule: &EpochSchedule,
        newest_slot_skip_shrink_inclusive: Option<Slot>,
    ) {
        let _guard = self.active_stats.activate(ActiveStatItem::Shrink);
        const DIRTY_STORES_CLEANING_THRESHOLD: usize = 10_000;
        const OUTER_CHUNK_SIZE: usize = 2000;
        let mut slots = self.all_slots_in_storage();
        if let Some(newest_slot_skip_shrink_inclusive) = newest_slot_skip_shrink_inclusive {
            // at startup, we cannot shrink the slot that we're about to replay and recalculate bank hash for.
            // That storage's contents are used to verify the bank hash (and accounts delta hash) of the startup slot.
            slots.retain(|slot| slot < &newest_slot_skip_shrink_inclusive);
        }

        // if we are restoring from incremental + full snapshot, then we cannot clean past latest_full_snapshot_slot.
        // If we were to clean past that, then we could mark accounts prior to latest_full_snapshot_slot as dead.
        // If we mark accounts prior to latest_full_snapshot_slot as dead, then we could shrink those accounts away.
        // If we shrink accounts away, then when we run the full hash of all accounts calculation up to latest_full_snapshot_slot,
        // then we will get the wrong answer, because some accounts may be GONE from the slot range up to latest_full_snapshot_slot.
        // So, we can only clean UP TO and including latest_full_snapshot_slot.
        // As long as we don't mark anything as dead at slots > latest_full_snapshot_slot, then shrink will have nothing to do for
        // slots > latest_full_snapshot_slot.
        let maybe_clean = || {
            if self.dirty_stores.len() > DIRTY_STORES_CLEANING_THRESHOLD {
                let latest_full_snapshot_slot = self.latest_full_snapshot_slot();
                self.clean_accounts(latest_full_snapshot_slot, is_startup, epoch_schedule);
            }
        };

        if is_startup {
            let threads = num_cpus::get();
            let inner_chunk_size = std::cmp::max(OUTER_CHUNK_SIZE / threads, 1);
            slots.chunks(OUTER_CHUNK_SIZE).for_each(|chunk| {
                chunk.par_chunks(inner_chunk_size).for_each(|slots| {
                    for slot in slots {
                        self.shrink_slot_forced(*slot);
                    }
                });
                maybe_clean();
            });
        } else {
            for slot in slots {
                self.shrink_slot_forced(slot);
                maybe_clean();
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
                    .get_loaded_account(|loaded_account| {
                        (pubkey, loaded_account.take_account(), slot)
                    });
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
                self.get_account_accessor(slot, pubkey, &account_info.storage_location())
                    .get_loaded_account(|loaded_account| {
                        scan_func(pubkey, loaded_account, slot);
                    });
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
                    .get_loaded_account(|loaded_account| {
                        (pubkey, loaded_account.take_account(), slot)
                    })
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
                    .get_loaded_account(|loaded_account| {
                        (pubkey, loaded_account.take_account(), slot)
                    });
                scan_func(account_slot)
            },
            config,
        )?;
        let used_index = true;
        Ok(used_index)
    }

    /// Scan a specific slot through all the account storage
    pub(crate) fn scan_account_storage<R, B>(
        &self,
        slot: Slot,
        cache_map_func: impl Fn(&LoadedAccount) -> Option<R> + Sync,
        storage_scan_func: impl Fn(&B, &LoadedAccount, Option<&[u8]>) + Sync,
        scan_account_storage_data: ScanAccountStorageData,
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
                            cache_map_func(&LoadedAccount::Cached(Cow::Borrowed(
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
                            cache_map_func(&LoadedAccount::Cached(Cow::Borrowed(
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
            // Regarding `shrinking_in_progress_ok`:
            // This fn could be running in the foreground, so shrinking could be running in the background, independently.
            // Even if shrinking is running, there will be 0-1 active storages to scan here at any point.
            // When a concurrent shrink completes, the active storage at this slot will
            // be replaced with an equivalent storage with only alive accounts in it.
            // A shrink on this slot could have completed anytime before the call here, a shrink could currently be in progress,
            // or the shrink could complete immediately or anytime after this call. This has always been true.
            // So, whether we get a never-shrunk, an about-to-be shrunk, or a will-be-shrunk-in-future storage here to scan,
            // all are correct and possible in a normally running system.
            if let Some(storage) = self
                .storage
                .get_slot_storage_entry_shrinking_in_progress_ok(slot)
            {
                storage.accounts.scan_accounts(|account| {
                    let loaded_account = LoadedAccount::Stored(account);
                    let data = (scan_account_storage_data
                        == ScanAccountStorageData::DataRefForStorage)
                        .then_some(loaded_account.data());
                    storage_scan_func(&retval, &loaded_account, data)
                });
            }

            ScanStorageResult::Stored(retval)
        }
    }

    /// Insert a default bank hash stats for `slot`
    ///
    /// This fn is called when creating a new bank from parent.
    pub fn insert_default_bank_hash_stats(&self, slot: Slot, parent_slot: Slot) {
        let mut bank_hash_stats = self.bank_hash_stats.lock().unwrap();
        if bank_hash_stats.get(&slot).is_some() {
            error!("set_hash: already exists; multiple forks with shared slot {slot} as child (parent: {parent_slot})!?");
            return;
        }
        bank_hash_stats.insert(slot, BankHashStats::default());
    }

    pub fn load(
        &self,
        ancestors: &Ancestors,
        pubkey: &Pubkey,
        load_hint: LoadHint,
    ) -> Option<(AccountSharedData, Slot)> {
        self.do_load(ancestors, pubkey, None, load_hint, LoadZeroLamports::None)
    }

    /// Return Ok(index_of_matching_owner) if the account owner at `offset` is one of the pubkeys in `owners`.
    /// Return Err(MatchAccountOwnerError::NoMatch) if the account has 0 lamports or the owner is not one of
    /// the pubkeys in `owners`.
    /// Return Err(MatchAccountOwnerError::UnableToLoad) if the account could not be accessed.
    pub fn account_matches_owners(
        &self,
        ancestors: &Ancestors,
        account: &Pubkey,
        owners: &[Pubkey],
    ) -> Result<usize, MatchAccountOwnerError> {
        let (slot, storage_location, _maybe_account_accesor) = self
            .read_index_for_accessor_or_load_slow(ancestors, account, None, false)
            .ok_or(MatchAccountOwnerError::UnableToLoad)?;

        if !storage_location.is_cached() {
            let result = self.read_only_accounts_cache.load(*account, slot);
            if let Some(account) = result {
                return if account.is_zero_lamport() {
                    Err(MatchAccountOwnerError::NoMatch)
                } else {
                    owners
                        .iter()
                        .position(|entry| account.owner() == entry)
                        .ok_or(MatchAccountOwnerError::NoMatch)
                };
            }
        }

        let (account_accessor, _slot) = self
            .retry_to_get_account_accessor(
                slot,
                storage_location,
                ancestors,
                account,
                None,
                LoadHint::Unspecified,
            )
            .ok_or(MatchAccountOwnerError::UnableToLoad)?;
        account_accessor.account_matches_owners(owners)
    }

    /// load the account with `pubkey` into the read only accounts cache.
    /// The goal is to make subsequent loads (which caller expects to occur) to find the account quickly.
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
        self.accounts_index.get_with_and_then(
            pubkey,
            Some(ancestors),
            max_root,
            true,
            |(slot, account_info)| {
                let storage_location = account_info.storage_location();
                let account_accessor = clone_in_lock
                    .then(|| self.get_account_accessor(slot, pubkey, &storage_location));
                (slot, storage_location, account_accessor)
            },
        )
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
        // S1 do_shrink_slot_store()            | N/A
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
        // S4 do_shrink_slot_store()/           | map of stores (removes old entry)
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
                        LoadHint::FixedMaxRootDoNotPopulateReadCache | LoadHint::FixedMaxRoot => {
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
                        LoadHint::FixedMaxRootDoNotPopulateReadCache | LoadHint::FixedMaxRoot => {
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
                    self.accounts_index.get_cloned(pubkey)
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

    /// Load account with `pubkey` and maybe put into read cache.
    ///
    /// If the account is not already cached, invoke `should_put_in_read_cache_fn`.
    /// The caller can inspect the account and indicate if it should be put into the read cache or not.
    ///
    /// Return the account and the slot when the account was last stored.
    /// Return None for ZeroLamport accounts.
    pub fn load_account_with(
        &self,
        ancestors: &Ancestors,
        pubkey: &Pubkey,
        should_put_in_read_cache_fn: impl Fn(&AccountSharedData) -> bool,
    ) -> Option<(AccountSharedData, Slot)> {
        let (slot, storage_location, _maybe_account_accesor) =
            self.read_index_for_accessor_or_load_slow(ancestors, pubkey, None, false)?;
        // Notice the subtle `?` at previous line, we bail out pretty early if missing.

        let in_write_cache = storage_location.is_cached();
        if !in_write_cache {
            let result = self.read_only_accounts_cache.load(*pubkey, slot);
            if let Some(account) = result {
                if account.is_zero_lamport() {
                    return None;
                }
                return Some((account, slot));
            }
        }

        let (mut account_accessor, slot) = self.retry_to_get_account_accessor(
            slot,
            storage_location,
            ancestors,
            pubkey,
            None,
            LoadHint::Unspecified,
        )?;

        // note that the account being in the cache could be different now than it was previously
        // since the cache could be flushed in between the 2 calls.
        let in_write_cache = matches!(account_accessor, LoadedAccountAccessor::Cached(_));
        let account = account_accessor.check_and_get_loaded_account_shared_data();
        if account.is_zero_lamport() {
            return None;
        }

        if !in_write_cache && should_put_in_read_cache_fn(&account) {
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
        // note that the account being in the cache could be different now than it was previously
        // since the cache could be flushed in between the 2 calls.
        let in_write_cache = matches!(account_accessor, LoadedAccountAccessor::Cached(_));
        let account = account_accessor.check_and_get_loaded_account_shared_data();
        if matches!(load_zero_lamports, LoadZeroLamports::None) && account.is_zero_lamport() {
            return None;
        }

        if !in_write_cache && load_hint != LoadHint::FixedMaxRootDoNotPopulateReadCache {
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
    ) -> Option<AccountHash> {
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
        account_accessor
            .check_and_get_loaded_account(|loaded_account| Some(loaded_account.loaded_hash()))
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

    fn find_storage_candidate(&self, slot: Slot) -> Arc<AccountStorageEntry> {
        let mut get_slot_stores = Measure::start("get_slot_stores");
        let store = self.storage.get_slot_storage_entry(slot);
        get_slot_stores.stop();
        self.stats
            .store_get_slot_store
            .fetch_add(get_slot_stores.as_us(), Ordering::Relaxed);
        let mut find_existing = Measure::start("find_existing");
        if let Some(store) = store {
            if store.try_available() {
                let ret = store.clone();
                drop(store);
                find_existing.stop();
                self.stats
                    .store_find_existing
                    .fetch_add(find_existing.as_us(), Ordering::Relaxed);
                return ret;
            }
        }
        find_existing.stop();
        self.stats
            .store_find_existing
            .fetch_add(find_existing.as_us(), Ordering::Relaxed);

        let store = self.create_store(slot, self.file_size, "store", &self.paths);

        // try_available is like taking a lock on the store,
        // preventing other threads from using it.
        // It must succeed here and happen before insert,
        // otherwise another thread could also grab it from the index.
        assert!(store.try_available());
        self.insert_store(slot, store.clone());
        store
    }

    fn has_space_available(&self, slot: Slot, size: u64) -> bool {
        let store = self.storage.get_slot_storage_entry(slot).unwrap();
        if store.status() == AccountStorageStatus::Available
            && store.accounts.remaining_bytes() >= size
        {
            return true;
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
        let path_index = thread_rng().gen_range(0..paths.len());
        let store = Arc::new(self.new_storage_entry(slot, Path::new(&paths[path_index]), size));

        debug!(
            "creating store: {} slot: {} len: {} size: {} from: {} path: {}",
            store.id(),
            slot,
            store.accounts.len(),
            store.accounts.capacity(),
            from,
            store.accounts.path().display(),
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

    pub fn enable_bank_drop_callback(&self) {
        self.is_bank_drop_callback_enabled
            .store(true, Ordering::Release);
    }

    /// This should only be called after the `Bank::drop()` runs in bank.rs, See BANK_DROP_SAFETY
    /// comment below for more explanation.
    ///   * `is_serialized_with_abs` - indicates whether this call runs sequentially with all other
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

    /// Purges every slot in `removed_slots` from both the cache and storage. This includes
    /// entries in the accounts index, cache entries, and any backing storage entries.
    pub fn purge_slots_from_cache_and_store<'a>(
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
                // Nobody else should have removed the slot cache entry yet
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

        let mut total_removed_stored_bytes = 0;
        let mut all_removed_slot_storages = vec![];

        let mut remove_storage_entries_elapsed = Measure::start("remove_storage_entries_elapsed");
        for remove_slot in removed_slots {
            // Remove the storage entries and collect some metrics
            if let Some(store) = self.storage.remove(remove_slot, false) {
                total_removed_stored_bytes += store.accounts.capacity();
                all_removed_slot_storages.push(store);
            }
        }
        remove_storage_entries_elapsed.stop();
        let num_stored_slots_removed = all_removed_slot_storages.len();

        // Backing mmaps for removed storages entries explicitly dropped here outside
        // of any locks
        let mut drop_storage_entries_elapsed = Measure::start("drop_storage_entries_elapsed");
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
            .fetch_add(num_stored_slots_removed, Ordering::Relaxed);
        purge_stats
            .total_removed_stored_bytes
            .fetch_add(total_removed_stored_bytes, Ordering::Relaxed);
        self.stats
            .dropped_stores
            .fetch_add(num_stored_slots_removed as u64, Ordering::Relaxed);
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
        assert!(self
            .storage
            .get_slot_storage_entry_shrinking_in_progress_ok(purged_slot)
            .is_none());
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
        let mut scan_storages_elapsed = Measure::start("scan_storages_elapsed");
        let mut stored_keys = HashSet::new();
        if let Some(storage) = self
            .storage
            .get_slot_storage_entry_shrinking_in_progress_ok(remove_slot)
        {
            storage.accounts.scan_pubkeys(|pk| {
                stored_keys.insert((*pk, remove_slot));
            });
        }
        scan_storages_elapsed.stop();
        purge_stats
            .scan_storages_elapsed
            .fetch_add(scan_storages_elapsed.as_us(), Ordering::Relaxed);

        let mut purge_accounts_index_elapsed = Measure::start("purge_accounts_index_elapsed");
        // Purge this slot from the accounts index
        let (reclaims, pubkeys_removed_from_accounts_index) =
            self.purge_keys_exact(stored_keys.iter());
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
            false,
            &pubkeys_removed_from_accounts_index,
            HandleReclaims::ProcessDeadSlots(purge_stats),
        );
        handle_reclaims_elapsed.stop();
        purge_stats
            .handle_reclaims_elapsed
            .fetch_add(handle_reclaims_elapsed.as_us(), Ordering::Relaxed);
        // After handling the reclaimed entries, this slot's
        // storage entries should be purged from self.storage
        assert!(
            self.storage.get_slot_storage_entry(remove_slot).is_none(),
            "slot {remove_slot} is not none"
        );
    }

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
        remove_unrooted_purge_stats.report("remove_unrooted_slots_purge_slots_stats", None);

        let mut currently_contended_slots = slots_under_contention.lock().unwrap();
        for (remove_slot, _) in remove_slots {
            assert!(currently_contended_slots.remove(remove_slot));
        }
    }

    /// Calculates the `AccountLtHash` of `account`
    pub fn lt_hash_account(account: &impl ReadableAccount, pubkey: &Pubkey) -> AccountLtHash {
        if account.lamports() == 0 {
            return ZERO_LAMPORT_ACCOUNT_LT_HASH;
        }

        let hasher = Self::hash_account_helper(account, pubkey);
        let lt_hash = LtHash::with(&hasher);
        AccountLtHash(lt_hash)
    }

    /// Calculates the `AccountHash` of `account`
    pub fn hash_account<T: ReadableAccount>(account: &T, pubkey: &Pubkey) -> AccountHash {
        if account.lamports() == 0 {
            return ZERO_LAMPORT_ACCOUNT_HASH;
        }

        let hasher = Self::hash_account_helper(account, pubkey);
        let hash = Hash::new_from_array(hasher.finalize().into());
        AccountHash(hash)
    }

    /// Hashes `account` and returns the underlying Hasher
    fn hash_account_helper(account: &impl ReadableAccount, pubkey: &Pubkey) -> blake3::Hasher {
        let mut hasher = blake3::Hasher::new();

        // allocate a buffer on the stack that's big enough
        // to hold a token account or a stake account
        const META_SIZE: usize = 8 /* lamports */ + 8 /* rent_epoch */ + 1 /* executable */ + 32 /* owner */ + 32 /* pubkey */;
        const DATA_SIZE: usize = 200; // stake accounts are 200 B and token accounts are 165-182ish B
        const BUFFER_SIZE: usize = META_SIZE + DATA_SIZE;
        let mut buffer = SmallVec::<[u8; BUFFER_SIZE]>::new();

        // collect lamports, rent_epoch into buffer to hash
        buffer.extend_from_slice(&account.lamports().to_le_bytes());
        buffer.extend_from_slice(&account.rent_epoch().to_le_bytes());

        let data = account.data();
        if data.len() > DATA_SIZE {
            // For larger accounts whose data can't fit into the buffer, update the hash now.
            hasher.update(&buffer);
            buffer.clear();

            // hash account's data
            hasher.update(data);
        } else {
            // For small accounts whose data can fit into the buffer, append it to the buffer.
            buffer.extend_from_slice(data);
        }

        // collect exec_flag, owner, pubkey into buffer to hash
        buffer.push(account.executable().into());
        buffer.extend_from_slice(account.owner().as_ref());
        buffer.extend_from_slice(pubkey.as_ref());
        hasher.update(&buffer);

        hasher
    }

    fn write_accounts_to_storage<'a>(
        &self,
        slot: Slot,
        storage: &AccountStorageEntry,
        accounts_and_meta_to_store: &impl StorableAccounts<'a>,
    ) -> Vec<AccountInfo> {
        let mut infos: Vec<AccountInfo> = Vec::with_capacity(accounts_and_meta_to_store.len());
        let mut total_append_accounts_us = 0;
        while infos.len() < accounts_and_meta_to_store.len() {
            let mut append_accounts = Measure::start("append_accounts");
            let stored_accounts_info = storage
                .accounts
                .append_accounts(accounts_and_meta_to_store, infos.len());
            append_accounts.stop();
            total_append_accounts_us += append_accounts.as_us();
            let Some(stored_accounts_info) = stored_accounts_info else {
                storage.set_status(AccountStorageStatus::Full);

                // See if an account overflows the append vecs in the slot.
                accounts_and_meta_to_store.account_default_if_zero_lamport(
                    infos.len(),
                    |account| {
                        let data_len = account.data().len();
                        let data_len = (data_len + STORE_META_OVERHEAD) as u64;
                        if !self.has_space_available(slot, data_len) {
                            info!(
                                "write_accounts_to_storage, no space: {}, {}, {}, {}, {}",
                                storage.accounts.capacity(),
                                storage.accounts.remaining_bytes(),
                                data_len,
                                infos.len(),
                                accounts_and_meta_to_store.len()
                            );
                            let special_store_size = std::cmp::max(data_len * 2, self.file_size);
                            self.create_and_insert_store(slot, special_store_size, "large create");
                        }
                    },
                );
                continue;
            };

            let store_id = storage.id();
            for (i, offset) in stored_accounts_info.offsets.iter().enumerate() {
                infos.push(AccountInfo::new(
                    StorageLocation::AppendVec(store_id, *offset),
                    accounts_and_meta_to_store
                        .account_default_if_zero_lamport(i, |account| account.lamports()),
                ));
            }
            storage.add_accounts(
                stored_accounts_info.offsets.len(),
                stored_accounts_info.size,
            );

            // restore the state to available
            storage.set_status(AccountStorageStatus::Available);
        }

        self.stats
            .store_append_accounts
            .fetch_add(total_append_accounts_us, Ordering::Relaxed);

        infos
    }

    pub fn mark_slot_frozen(&self, slot: Slot) {
        if let Some(slot_cache) = self.accounts_cache.slot_cache(slot) {
            slot_cache.mark_slot_frozen();
            slot_cache.report_slot_store_metrics();
        }
        self.accounts_cache.report_size();
    }

    // These functions/fields are only usable from a dev context (i.e. tests and benches)
    #[cfg(feature = "dev-context-only-utils")]
    pub fn flush_accounts_cache_slot_for_tests(&self, slot: Slot) {
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
        let (total_new_cleaned_roots, num_cleaned_roots_flushed, mut flush_stats) = self
            .flush_rooted_accounts_cache(
                requested_flush_root,
                Some((&mut account_bytes_saved, &mut num_accounts_saved)),
            );
        flush_roots_elapsed.stop();

        // Note we don't purge unrooted slots here because there may be ongoing scans/references
        // for those slot, let the Bank::drop() implementation do cleanup instead on dead
        // banks

        // If 'should_aggressively_flush_cache', then flush the excess ones to storage
        let (total_new_excess_roots, num_excess_roots_flushed, flush_stats_aggressively) =
            if self.should_aggressively_flush_cache() {
                // Start by flushing the roots
                //
                // Cannot do any cleaning on roots past `requested_flush_root` because future
                // snapshots may need updates from those later slots, hence we pass `None`
                // for `should_clean`.
                self.flush_rooted_accounts_cache(None, None)
            } else {
                (0, 0, FlushStats::default())
            };
        flush_stats.accumulate(&flush_stats_aggressively);

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
                            flush_stats.accumulate(&stats);
                        }
                    }
                } else {
                    unflushable_unrooted_slot_count += 1;
                }
            });
            datapoint_info!(
                "accounts_db-flush_accounts_cache_aggressively",
                ("num_flushed", flush_stats.num_flushed.0, i64),
                ("num_purged", flush_stats.num_purged.0, i64),
                ("total_flush_size", flush_stats.total_size.0, i64),
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
            (
                "store_accounts_total_us",
                flush_stats.store_accounts_total_us.0,
                i64
            ),
            (
                "update_index_us",
                flush_stats.store_accounts_timing.update_index_elapsed,
                i64
            ),
            (
                "store_accounts_elapsed_us",
                flush_stats.store_accounts_timing.store_accounts_elapsed,
                i64
            ),
            (
                "handle_reclaims_elapsed_us",
                flush_stats.store_accounts_timing.handle_reclaims_elapsed,
                i64
            ),
        );
    }

    fn flush_rooted_accounts_cache(
        &self,
        requested_flush_root: Option<Slot>,
        should_clean: Option<(&mut usize, &mut usize)>,
    ) -> (usize, usize, FlushStats) {
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
        let mut flush_stats = FlushStats::default();
        for &root in cached_roots.iter().rev() {
            if let Some(stats) =
                self.flush_slot_cache_with_clean(root, should_flush_f.as_mut(), max_clean_root)
            {
                num_roots_flushed += 1;
                flush_stats.accumulate(&stats);
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
        (num_new_roots, num_roots_flushed, flush_stats)
    }

    fn do_flush_slot_cache(
        &self,
        slot: Slot,
        slot_cache: &SlotCache,
        mut should_flush_f: Option<&mut impl FnMut(&Pubkey, &AccountSharedData) -> bool>,
        max_clean_root: Option<Slot>,
    ) -> FlushStats {
        let mut flush_stats = FlushStats::default();
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

        let accounts: Vec<(&Pubkey, &AccountSharedData)> = iter_items
            .iter()
            .filter_map(|iter_item| {
                let key = iter_item.key();
                let account = &iter_item.value().account;
                let should_flush = should_flush_f
                    .as_mut()
                    .map(|should_flush_f| should_flush_f(key, account))
                    .unwrap_or(true);
                if should_flush {
                    flush_stats.total_size += aligned_stored_size(account.data().len()) as u64;
                    flush_stats.num_flushed += 1;
                    Some((key, account))
                } else {
                    // If we don't flush, we have to remove the entry from the
                    // index, since it's equivalent to purging
                    purged_slot_pubkeys.insert((slot, *key));
                    pubkey_to_slot_set.push((*key, slot));
                    flush_stats.num_purged += 1;
                    None
                }
            })
            .collect();

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
            // This ensures that all updates are written to an AppendVec, before any
            // updates to the index happen, so anybody that sees a real entry in the index,
            // will be able to find the account in storage
            let flushed_store =
                self.create_and_insert_store(slot, flush_stats.total_size.0, "flush_slot_cache");
            let (store_accounts_timing_inner, store_accounts_total_inner_us) =
                measure_us!(self.store_accounts_frozen((slot, &accounts[..]), &flushed_store,));
            flush_stats.store_accounts_timing = store_accounts_timing_inner;
            flush_stats.store_accounts_total_us = Saturating(store_accounts_total_inner_us);

            // If the above sizing function is correct, just one AppendVec is enough to hold
            // all the data for the slot
            assert!(self.storage.get_slot_storage_entry(slot).is_some());
            self.reopen_storage_as_readonly_shrinking_in_progress_ok(slot);
        }

        // Remove this slot from the cache, which will to AccountsDb's new readers should look like an
        // atomic switch from the cache to storage.
        // There is some racy condition for existing readers who just has read exactly while
        // flushing. That case is handled by retry_to_get_account_accessor()
        assert!(self.accounts_cache.remove_slot(slot).is_some());

        flush_stats
    }

    /// flush all accounts in this slot
    fn flush_slot_cache(&self, slot: Slot) -> Option<FlushStats> {
        self.flush_slot_cache_with_clean(slot, None::<&mut fn(&_, &_) -> bool>, None)
    }

    /// `should_flush_f` is an optional closure that determines whether a given
    /// account should be flushed. Passing `None` will by default flush all
    /// accounts
    fn flush_slot_cache_with_clean(
        &self,
        slot: Slot,
        should_flush_f: Option<&mut impl FnMut(&Pubkey, &AccountSharedData) -> bool>,
        max_clean_root: Option<Slot>,
    ) -> Option<FlushStats> {
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
            assert!(self
                .remove_unrooted_slots_synchronization
                .slots_under_contention
                .lock()
                .unwrap()
                .remove(&slot));

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

    fn write_accounts_to_cache<'a, 'b>(
        &self,
        slot: Slot,
        accounts_and_meta_to_store: &impl StorableAccounts<'b>,
        txn_iter: Box<dyn std::iter::Iterator<Item = &Option<&SanitizedTransaction>> + 'a>,
    ) -> Vec<AccountInfo> {
        let mut write_version_producer: Box<dyn Iterator<Item = u64>> =
            if self.accounts_update_notifier.is_some() {
                let mut current_version = self
                    .write_version
                    .fetch_add(accounts_and_meta_to_store.len() as u64, Ordering::AcqRel);
                Box::new(std::iter::from_fn(move || {
                    let ret = current_version;
                    current_version += 1;
                    Some(ret)
                }))
            } else {
                Box::new(std::iter::empty())
            };

        let (account_infos, cached_accounts) = txn_iter
            .enumerate()
            .map(|(i, txn)| {
                let mut account_info = AccountInfo::default();
                accounts_and_meta_to_store.account_default_if_zero_lamport(i, |account| {
                    let account_shared_data = account.to_account_shared_data();
                    let pubkey = account.pubkey();
                    account_info = AccountInfo::new(StorageLocation::Cached, account.lamports());

                    self.notify_account_at_accounts_update(
                        slot,
                        &account_shared_data,
                        txn,
                        pubkey,
                        &mut write_version_producer,
                    );

                    let cached_account =
                        self.accounts_cache.store(slot, pubkey, account_shared_data);
                    (account_info, cached_account)
                })
            })
            .unzip();

        // hash this accounts in bg
        match &self.sender_bg_hasher {
            Some(ref sender) => {
                let _ = sender.send(cached_accounts);
            }
            None => (),
        };

        account_infos
    }

    fn store_accounts_to<'a: 'c, 'b, 'c>(
        &self,
        accounts: &'c impl StorableAccounts<'b>,
        store_to: &StoreTo,
        transactions: Option<&[Option<&'a SanitizedTransaction>]>,
    ) -> Vec<AccountInfo> {
        let mut calc_stored_meta_time = Measure::start("calc_stored_meta");
        let slot = accounts.target_slot();
        if self
            .read_only_accounts_cache
            .can_slot_be_in_cache(accounts.target_slot())
        {
            (0..accounts.len()).for_each(|index| {
                accounts.account(index, |account| {
                    // based on the patterns of how a validator writes accounts, it is almost always the case that there is no read only cache entry
                    // for this pubkey and slot. So, we can give that hint to the `remove` for performance.
                    self.read_only_accounts_cache
                        .remove_assume_not_present(*account.pubkey());
                })
            });
        }
        calc_stored_meta_time.stop();
        self.stats
            .calc_stored_meta
            .fetch_add(calc_stored_meta_time.as_us(), Ordering::Relaxed);

        match store_to {
            StoreTo::Cache => {
                let txn_iter: Box<dyn std::iter::Iterator<Item = &Option<&SanitizedTransaction>>> =
                    match transactions {
                        Some(transactions) => {
                            assert_eq!(transactions.len(), accounts.len());
                            Box::new(transactions.iter())
                        }
                        None => Box::new(std::iter::repeat(&None).take(accounts.len())),
                    };

                self.write_accounts_to_cache(slot, accounts, txn_iter)
            }
            StoreTo::Storage(storage) => self.write_accounts_to_storage(slot, storage, accounts),
        }
    }

    fn report_store_stats(&self) {
        let mut total_count = 0;
        let mut newest_slot = 0;
        let mut oldest_slot = u64::MAX;
        let mut total_bytes = 0;
        let mut total_alive_bytes = 0;
        for (slot, store) in self.storage.iter() {
            total_count += 1;
            newest_slot = std::cmp::max(newest_slot, slot);

            oldest_slot = std::cmp::min(oldest_slot, slot);

            total_alive_bytes += store.alive_bytes();
            total_bytes += store.capacity();
        }
        info!(
            "total_stores: {total_count}, newest_slot: {newest_slot}, oldest_slot: {oldest_slot}"
        );

        let total_alive_ratio = if total_bytes > 0 {
            total_alive_bytes as f64 / total_bytes as f64
        } else {
            0.
        };

        datapoint_info!(
            "accounts_db-stores",
            ("total_count", total_count, i64),
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
            (
                "skipped_rewrites_num",
                self.stats.skipped_rewrites_num.swap(0, Ordering::Relaxed),
                i64
            ),
        );
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
    ) -> (AccountsHash, u64) {
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

        // Pick a chunk size big enough to allow us to produce output vectors that are smaller than the overall size.
        // We'll also accumulate the lamports within each chunk and fewer chunks results in less contention to accumulate the sum.
        let chunks = crate::accounts_hash::MERKLE_FANOUT.pow(4);
        let total_lamports = Mutex::<u64>::new(0);

        let get_account_hashes = || {
            keys.par_chunks(chunks)
                .map(|pubkeys| {
                    let mut sum = 0u128;
                    let account_hashes: Vec<Hash> = pubkeys
                        .iter()
                        .filter_map(|pubkey| {
                            let index_entry = self.accounts_index.get_cloned(pubkey)?;
                            self.accounts_index
                                .get_account_info_with_and_then(
                                    &index_entry,
                                    config.ancestors,
                                    Some(max_slot),
                                    |(slot, account_info)| {
                                        if account_info.is_zero_lamport() {
                                            return None;
                                        }
                                        self.get_account_accessor(
                                            slot,
                                            pubkey,
                                            &account_info.storage_location(),
                                        )
                                        .get_loaded_account(|loaded_account| {
                                            let mut loaded_hash = loaded_account.loaded_hash();
                                            let balance = loaded_account.lamports();
                                            let hash_is_missing =
                                                loaded_hash == AccountHash(Hash::default());
                                            if hash_is_missing {
                                                let computed_hash = Self::hash_account(
                                                    &loaded_account,
                                                    loaded_account.pubkey(),
                                                );
                                                loaded_hash = computed_hash;
                                            }
                                            sum += balance as u128;
                                            loaded_hash.0
                                        })
                                    },
                                )
                                .flatten()
                        })
                        .collect();
                    let mut total = total_lamports.lock().unwrap();
                    *total = AccountsHasher::checked_cast_for_capitalization(*total as u128 + sum);
                    account_hashes
                })
                .collect()
        };

        let mut scan = Measure::start("scan");
        let account_hashes: Vec<Vec<Hash>> = self.thread_pool_clean.install(get_account_hashes);
        scan.stop();

        let total_lamports = *total_lamports.lock().unwrap();

        let mut hash_time = Measure::start("hash");
        let (accumulated_hash, hash_total) = AccountsHasher::calculate_hash(account_hashes);
        hash_time.stop();

        datapoint_info!(
            "calculate_accounts_hash_from_index",
            ("accounts_scan", scan.as_us(), i64),
            ("hash", hash_time.as_us(), i64),
            ("hash_total", hash_total, i64),
            ("collect", collect.as_us(), i64),
        );

        let accounts_hash = AccountsHash(accumulated_hash);
        (accounts_hash, total_lamports)
    }

    /// This is only valid to call from tests.
    /// run the accounts hash calculation and store the results
    pub fn update_accounts_hash_for_tests(
        &self,
        slot: Slot,
        ancestors: &Ancestors,
        debug_verify: bool,
        is_startup: bool,
    ) -> (AccountsHash, u64) {
        self.update_accounts_hash_with_verify_from(
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

    fn update_old_slot_stats(&self, stats: &HashStats, storage: Option<&Arc<AccountStorageEntry>>) {
        if let Some(storage) = storage {
            stats.roots_older_than_epoch.fetch_add(1, Ordering::Relaxed);
            let num_accounts = storage.count();
            let sizes = storage.capacity();
            stats
                .append_vec_sizes_older_than_epoch
                .fetch_add(sizes as usize, Ordering::Relaxed);
            stats
                .accounts_in_roots_older_than_epoch
                .fetch_add(num_accounts, Ordering::Relaxed);
        }
    }

    /// return slot + offset, where offset can be +/-
    fn apply_offset_to_slot(slot: Slot, offset: i64) -> Slot {
        if offset > 0 {
            slot.saturating_add(offset as u64)
        } else {
            slot.saturating_sub(offset.unsigned_abs())
        }
    }

    /// `oldest_non_ancient_slot` is only applicable when `Append` is used for ancient append vec packing.
    /// If `Pack` is used for ancient append vec packing, return None.
    /// Otherwise, return a slot 'max_slot_inclusive' - (slots_per_epoch - `self.ancient_append_vec_offset`)
    /// If ancient append vecs are not enabled, return 0.
    fn get_oldest_non_ancient_slot_for_hash_calc_scan(
        &self,
        max_slot_inclusive: Slot,
        config: &CalcAccountsHashConfig<'_>,
    ) -> Option<Slot> {
        if self.create_ancient_storage == CreateAncientStorage::Pack {
            // oldest_non_ancient_slot is only applicable when ancient storages are created with `Append`. When ancient storages are created with `Pack`, ancient storages
            // can be created in between non-ancient storages. Return None, because oldest_non_ancient_slot is not applicable here.
            None
        } else if self.ancient_append_vec_offset.is_some() {
            // For performance, this is required when ancient appendvecs are enabled
            Some(
                self.get_oldest_non_ancient_slot_from_slot(
                    config.epoch_schedule,
                    max_slot_inclusive,
                ),
            )
        } else {
            // This causes the entire range to be chunked together, treating older append vecs just like new ones.
            // This performs well if there are many old append vecs that haven't been cleaned yet.
            // 0 will have the effect of causing ALL older append vecs to be chunked together, just like every other append vec.
            Some(0)
        }
    }

    /// hash info about 'storage' into 'hasher'
    /// return true iff storage is valid for loading from cache
    fn hash_storage_info(
        hasher: &mut impl StdHasher,
        storage: &AccountStorageEntry,
        slot: Slot,
    ) -> bool {
        // hash info about this storage
        storage.written_bytes().hash(hasher);
        slot.hash(hasher);
        let storage_file = storage.accounts.path();
        storage_file.hash(hasher);
        let Ok(metadata) = std::fs::metadata(storage_file) else {
            return false;
        };
        let Ok(amod) = metadata.modified() else {
            return false;
        };
        let amod = amod
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        amod.hash(hasher);

        // if we made it here, we have hashed info and we should try to load from the cache
        true
    }

    /// storages are sorted by slot and have range info.
    /// add all stores older than slots_per_epoch to dirty_stores so clean visits these slots
    fn mark_old_slots_as_dirty(
        &self,
        storages: &SortedStorages,
        slots_per_epoch: Slot,
        stats: &mut crate::accounts_hash::HashStats,
    ) {
        // Nothing to do if ancient append vecs are enabled.
        // Ancient slots will be visited by the ancient append vec code and dealt with correctly.
        // we expect these ancient append vecs to be old and keeping accounts
        // We can expect the normal processes will keep them cleaned.
        // If we included them here then ALL accounts in ALL ancient append vecs will be visited by clean each time.
        if self.ancient_append_vec_offset.is_some() {
            return;
        }

        let mut mark_time = Measure::start("mark_time");
        let mut num_dirty_slots: usize = 0;
        let max = storages.max_slot_inclusive();
        let acceptable_straggler_slot_count = 100; // do nothing special for these old stores which will likely get cleaned up shortly
        let sub = slots_per_epoch + acceptable_straggler_slot_count;
        let in_epoch_range_start = max.saturating_sub(sub);
        for (slot, storage) in storages.iter_range(&(..in_epoch_range_start)) {
            if let Some(storage) = storage {
                self.dirty_stores.insert(slot, storage.clone());
                num_dirty_slots += 1;
            }
        }
        mark_time.stop();
        stats.mark_time_us = mark_time.as_us();
        stats.num_dirty_slots = num_dirty_slots;
    }

    pub fn calculate_accounts_hash_from(
        &self,
        data_source: CalcAccountsHashDataSource,
        slot: Slot,
        config: &CalcAccountsHashConfig<'_>,
    ) -> (AccountsHash, u64) {
        match data_source {
            CalcAccountsHashDataSource::Storages => {
                if self.accounts_cache.contains_any_slots(slot) {
                    // this indicates a race condition
                    inc_new_counter_info!("accounts_hash_items_in_write_cache", 1);
                }

                let mut collect_time = Measure::start("collect");
                let (combined_maps, slots) = self.get_snapshot_storages(..=slot);
                collect_time.stop();

                let mut sort_time = Measure::start("sort_storages");
                let min_root = self.accounts_index.min_alive_root();
                let storages = SortedStorages::new_with_slots(
                    combined_maps.iter().zip(slots),
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

                self.calculate_accounts_hash(config, &storages, timings)
            }
            CalcAccountsHashDataSource::IndexForTests => {
                self.calculate_accounts_hash_from_index(slot, config)
            }
        }
    }

    fn calculate_accounts_hash_with_verify_from(
        &self,
        data_source: CalcAccountsHashDataSource,
        debug_verify: bool,
        slot: Slot,
        config: CalcAccountsHashConfig<'_>,
        expected_capitalization: Option<u64>,
    ) -> (AccountsHash, u64) {
        let (accounts_hash, total_lamports) =
            self.calculate_accounts_hash_from(data_source, slot, &config);
        if debug_verify {
            // calculate the other way (store or non-store) and verify results match.
            let data_source_other = match data_source {
                CalcAccountsHashDataSource::IndexForTests => CalcAccountsHashDataSource::Storages,
                CalcAccountsHashDataSource::Storages => CalcAccountsHashDataSource::IndexForTests,
            };
            let (accounts_hash_other, total_lamports_other) =
                self.calculate_accounts_hash_from(data_source_other, slot, &config);

            let success = accounts_hash == accounts_hash_other
                && total_lamports == total_lamports_other
                && total_lamports == expected_capitalization.unwrap_or(total_lamports);
            assert!(success, "calculate_accounts_hash_with_verify mismatch. hashes: {}, {}; lamports: {}, {}; expected lamports: {:?}, data source: {:?}, slot: {}", accounts_hash.0, accounts_hash_other.0, total_lamports, total_lamports_other, expected_capitalization, data_source, slot);
        }
        (accounts_hash, total_lamports)
    }

    /// run the accounts hash calculation and store the results
    #[allow(clippy::too_many_arguments)]
    pub fn update_accounts_hash_with_verify_from(
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
        let (accounts_hash, total_lamports) = self.calculate_accounts_hash_with_verify_from(
            data_source,
            debug_verify,
            slot,
            CalcAccountsHashConfig {
                use_bg_thread_pool: !is_startup,
                ancestors: Some(ancestors),
                epoch_schedule,
                rent_collector,
                store_detailed_debug_info_on_failure: false,
            },
            expected_capitalization,
        );
        self.set_accounts_hash(slot, (accounts_hash, total_lamports));
        (accounts_hash, total_lamports)
    }

    /// Calculate the full accounts hash for `storages` and save the results at `slot`
    pub fn update_accounts_hash(
        &self,
        config: &CalcAccountsHashConfig<'_>,
        storages: &SortedStorages<'_>,
        slot: Slot,
        stats: HashStats,
    ) -> (AccountsHash, /*capitalization*/ u64) {
        let accounts_hash = self.calculate_accounts_hash(config, storages, stats);
        let old_accounts_hash = self.set_accounts_hash(slot, accounts_hash);
        if let Some(old_accounts_hash) = old_accounts_hash {
            warn!("Accounts hash was already set for slot {slot}! old: {old_accounts_hash:?}, new: {accounts_hash:?}");
        }
        accounts_hash
    }

    /// Calculate the incremental accounts hash for `storages` and save the results at `slot`
    pub fn update_incremental_accounts_hash(
        &self,
        config: &CalcAccountsHashConfig<'_>,
        storages: &SortedStorages<'_>,
        slot: Slot,
        stats: HashStats,
    ) -> (IncrementalAccountsHash, /*capitalization*/ u64) {
        let incremental_accounts_hash =
            self.calculate_incremental_accounts_hash(config, storages, stats);
        let old_incremental_accounts_hash =
            self.set_incremental_accounts_hash(slot, incremental_accounts_hash);
        if let Some(old_incremental_accounts_hash) = old_incremental_accounts_hash {
            warn!("Incremental accounts hash was already set for slot {slot}! old: {old_incremental_accounts_hash:?}, new: {incremental_accounts_hash:?}");
        }
        incremental_accounts_hash
    }

    /// Set the accounts hash for `slot`
    ///
    /// returns the previous accounts hash for `slot`
    #[cfg_attr(feature = "dev-context-only-utils", qualifiers(pub))]
    fn set_accounts_hash(
        &self,
        slot: Slot,
        accounts_hash: (AccountsHash, /*capitalization*/ u64),
    ) -> Option<(AccountsHash, /*capitalization*/ u64)> {
        self.accounts_hashes
            .lock()
            .unwrap()
            .insert(slot, accounts_hash)
    }

    /// After deserializing a snapshot, set the accounts hash for the new AccountsDb
    pub fn set_accounts_hash_from_snapshot(
        &mut self,
        slot: Slot,
        accounts_hash: SerdeAccountsHash,
        capitalization: u64,
    ) -> Option<(AccountsHash, /*capitalization*/ u64)> {
        self.set_accounts_hash(slot, (accounts_hash.into(), capitalization))
    }

    /// Get the accounts hash for `slot`
    pub fn get_accounts_hash(&self, slot: Slot) -> Option<(AccountsHash, /*capitalization*/ u64)> {
        self.accounts_hashes.lock().unwrap().get(&slot).cloned()
    }

    /// Get all accounts hashes
    pub fn get_accounts_hashes(&self) -> HashMap<Slot, (AccountsHash, /*capitalization*/ u64)> {
        self.accounts_hashes.lock().unwrap().clone()
    }

    /// Set the incremental accounts hash for `slot`
    ///
    /// returns the previous incremental accounts hash for `slot`
    pub fn set_incremental_accounts_hash(
        &self,
        slot: Slot,
        incremental_accounts_hash: (IncrementalAccountsHash, /*capitalization*/ u64),
    ) -> Option<(IncrementalAccountsHash, /*capitalization*/ u64)> {
        self.incremental_accounts_hashes
            .lock()
            .unwrap()
            .insert(slot, incremental_accounts_hash)
    }

    /// After deserializing a snapshot, set the incremental accounts hash for the new AccountsDb
    pub fn set_incremental_accounts_hash_from_snapshot(
        &mut self,
        slot: Slot,
        incremental_accounts_hash: SerdeIncrementalAccountsHash,
        capitalization: u64,
    ) -> Option<(IncrementalAccountsHash, /*capitalization*/ u64)> {
        self.set_incremental_accounts_hash(slot, (incremental_accounts_hash.into(), capitalization))
    }

    /// Get the incremental accounts hash for `slot`
    pub fn get_incremental_accounts_hash(
        &self,
        slot: Slot,
    ) -> Option<(IncrementalAccountsHash, /*capitalization*/ u64)> {
        self.incremental_accounts_hashes
            .lock()
            .unwrap()
            .get(&slot)
            .cloned()
    }

    /// Get all incremental accounts hashes
    pub fn get_incremental_accounts_hashes(
        &self,
    ) -> HashMap<Slot, (IncrementalAccountsHash, /*capitalization*/ u64)> {
        self.incremental_accounts_hashes.lock().unwrap().clone()
    }

    /// Purge accounts hashes that are older than `latest_full_snapshot_slot`
    ///
    /// Should only be called by AccountsHashVerifier, since it consumes the accounts hashes and
    /// knows which ones are still needed.
    pub fn purge_old_accounts_hashes(&self, latest_full_snapshot_slot: Slot) {
        self.accounts_hashes
            .lock()
            .unwrap()
            .retain(|&slot, _| slot >= latest_full_snapshot_slot);
        self.incremental_accounts_hashes
            .lock()
            .unwrap()
            .retain(|&slot, _| slot >= latest_full_snapshot_slot);
    }

    fn sort_slot_storage_scan(accum: &mut BinnedHashData) -> u64 {
        let (_, sort_time) = measure_us!(accum.iter_mut().for_each(|items| {
            // sort_by vs unstable because slot and write_version are already in order
            items.sort_by(AccountsHasher::compare_two_hash_entries);
        }));
        sort_time
    }

    /// normal code path returns the common cache path
    /// when called after a failure has been detected, redirect the cache storage to a separate folder for debugging later
    fn get_cache_hash_data(
        accounts_hash_cache_path: PathBuf,
        config: &CalcAccountsHashConfig<'_>,
        kind: CalcAccountsHashKind,
        slot: Slot,
        storages_start_slot: Slot,
    ) -> CacheHashData {
        let accounts_hash_cache_path = if !config.store_detailed_debug_info_on_failure {
            accounts_hash_cache_path
        } else {
            // this path executes when we are failing with a hash mismatch
            let failed_dir = accounts_hash_cache_path
                .join("failed_calculate_accounts_hash_cache")
                .join(slot.to_string());
            _ = std::fs::remove_dir_all(&failed_dir);
            failed_dir
        };
        let deletion_policy = match kind {
            CalcAccountsHashKind::Full => CacheHashDeletionPolicy::AllUnused,
            CalcAccountsHashKind::Incremental => {
                CacheHashDeletionPolicy::UnusedAtLeast(storages_start_slot)
            }
        };
        CacheHashData::new(accounts_hash_cache_path, deletion_policy)
    }

    /// Calculate the full accounts hash
    ///
    /// This is intended to be used by startup verification, and also AccountsHashVerifier.
    /// Uses account storage files as the data source for the calculation.
    pub fn calculate_accounts_hash(
        &self,
        config: &CalcAccountsHashConfig<'_>,
        storages: &SortedStorages<'_>,
        stats: HashStats,
    ) -> (AccountsHash, u64) {
        let (accounts_hash, capitalization) = self.calculate_accounts_hash_from_storages(
            config,
            storages,
            stats,
            CalcAccountsHashKind::Full,
        );
        let AccountsHashKind::Full(accounts_hash) = accounts_hash else {
            panic!("calculate_accounts_hash_from_storages must return a FullAccountsHash");
        };
        (accounts_hash, capitalization)
    }

    /// Calculate the incremental accounts hash
    ///
    /// This calculation is intended to be used by incremental snapshots, and thus differs from a
    /// "full" accounts hash in a few ways:
    /// - Zero-lamport accounts are *included* in the hash because zero-lamport accounts are also
    ///   included in the incremental snapshot.  This ensures reconstructing the AccountsDb is
    ///   still correct when using this incremental accounts hash.
    /// - `storages` must be the same as the ones going into the incremental snapshot.
    pub fn calculate_incremental_accounts_hash(
        &self,
        config: &CalcAccountsHashConfig<'_>,
        storages: &SortedStorages<'_>,
        stats: HashStats,
    ) -> (IncrementalAccountsHash, /* capitalization */ u64) {
        let (accounts_hash, capitalization) = self.calculate_accounts_hash_from_storages(
            config,
            storages,
            stats,
            CalcAccountsHashKind::Incremental,
        );
        let AccountsHashKind::Incremental(incremental_accounts_hash) = accounts_hash else {
            panic!("calculate_incremental_accounts_hash must return an IncrementalAccountsHash");
        };
        (incremental_accounts_hash, capitalization)
    }

    /// The shared code for calculating accounts hash from storages.
    /// Used for both full accounts hash and incremental accounts hash calculation.
    fn calculate_accounts_hash_from_storages(
        &self,
        config: &CalcAccountsHashConfig<'_>,
        storages: &SortedStorages<'_>,
        mut stats: HashStats,
        kind: CalcAccountsHashKind,
    ) -> (AccountsHashKind, u64) {
        let total_time = Measure::start("");
        let _guard = self.active_stats.activate(ActiveStatItem::Hash);
        let storages_start_slot = storages.range().start;
        stats.oldest_root = storages_start_slot;

        self.mark_old_slots_as_dirty(storages, config.epoch_schedule.slots_per_epoch, &mut stats);

        let slot = storages.max_slot_inclusive();
        let use_bg_thread_pool = config.use_bg_thread_pool;
        let accounts_hash_cache_path = self.accounts_hash_cache_path.clone();
        let transient_accounts_hash_cache_dir = TempDir::new_in(&accounts_hash_cache_path)
            .expect("create transient accounts hash cache dir");
        let transient_accounts_hash_cache_path =
            transient_accounts_hash_cache_dir.path().to_path_buf();
        let scan_and_hash = || {
            let (cache_hash_data, cache_hash_data_us) = measure_us!(Self::get_cache_hash_data(
                accounts_hash_cache_path,
                config,
                kind,
                slot,
                storages_start_slot,
            ));
            stats.cache_hash_data_us += cache_hash_data_us;

            let bounds = Range {
                start: 0,
                end: PUBKEY_BINS_FOR_CALCULATING_HASHES,
            };

            let accounts_hasher = AccountsHasher {
                zero_lamport_accounts: kind.zero_lamport_accounts(),
                dir_for_temp_cache_files: transient_accounts_hash_cache_path,
                active_stats: &self.active_stats,
            };

            // get raw data by scanning
            let cache_hash_data_file_references = self.scan_snapshot_stores_with_cache(
                &cache_hash_data,
                storages,
                &mut stats,
                PUBKEY_BINS_FOR_CALCULATING_HASHES,
                &bounds,
                config,
            );

            let cache_hash_data_files = cache_hash_data_file_references
                .iter()
                .map(|d| d.map())
                .collect::<Vec<_>>();

            if let Some(err) = cache_hash_data_files
                .iter()
                .filter_map(|r| r.as_ref().err())
                .next()
            {
                panic!("failed generating accounts hash files: {:?}", err);
            }

            // convert mmapped cache files into slices of data
            let cache_hash_intermediates = cache_hash_data_files
                .iter()
                .map(|d| d.as_ref().unwrap().get_cache_hash_data())
                .collect::<Vec<_>>();

            // turn raw data into merkle tree hashes and sum of lamports
            let (accounts_hash, capitalization) =
                accounts_hasher.rest_of_hash_calculation(&cache_hash_intermediates, &mut stats);
            let accounts_hash = match kind {
                CalcAccountsHashKind::Full => AccountsHashKind::Full(AccountsHash(accounts_hash)),
                CalcAccountsHashKind::Incremental => {
                    AccountsHashKind::Incremental(IncrementalAccountsHash(accounts_hash))
                }
            };
            info!("calculate_accounts_hash_from_storages: slot: {slot}, {accounts_hash:?}, capitalization: {capitalization}");
            (accounts_hash, capitalization)
        };

        let result = if use_bg_thread_pool {
            self.thread_pool_clean.install(scan_and_hash)
        } else {
            scan_and_hash()
        };
        stats.total_us = total_time.end_as_us();
        stats.log();
        result
    }

    /// Verify accounts hash at startup (or tests)
    ///
    /// Calculate accounts hash(es) and compare them to the values set at startup.
    /// If `base` is `None`, only calculates the full accounts hash for `[0, slot]`.
    /// If `base` is `Some`, calculate the full accounts hash for `[0, base slot]`
    /// and then calculate the incremental accounts hash for `(base slot, slot]`.
    pub fn verify_accounts_hash_and_lamports(
        &self,
        snapshot_storages_and_slots: (&[Arc<AccountStorageEntry>], &[Slot]),
        slot: Slot,
        total_lamports: u64,
        base: Option<(Slot, /*capitalization*/ u64)>,
        config: VerifyAccountsHashAndLamportsConfig,
    ) -> Result<(), AccountsHashVerificationError> {
        let calc_config = CalcAccountsHashConfig {
            use_bg_thread_pool: config.use_bg_thread_pool,
            ancestors: Some(config.ancestors),
            epoch_schedule: config.epoch_schedule,
            rent_collector: config.rent_collector,
            store_detailed_debug_info_on_failure: config.store_detailed_debug_info,
        };
        let hash_mismatch_is_error = !config.ignore_mismatch;

        if let Some((base_slot, base_capitalization)) = base {
            self.verify_accounts_hash_and_lamports(
                snapshot_storages_and_slots,
                base_slot,
                base_capitalization,
                None,
                config,
            )?;

            let storages_and_slots = snapshot_storages_and_slots
                .0
                .iter()
                .zip(snapshot_storages_and_slots.1.iter())
                .filter(|storage_and_slot| *storage_and_slot.1 > base_slot)
                .map(|(storage, slot)| (storage, *slot));
            let sorted_storages = SortedStorages::new_with_slots(storages_and_slots, None, None);
            let calculated_incremental_accounts_hash = self.calculate_incremental_accounts_hash(
                &calc_config,
                &sorted_storages,
                HashStats::default(),
            );
            let found_incremental_accounts_hash = self
                .get_incremental_accounts_hash(slot)
                .ok_or(AccountsHashVerificationError::MissingAccountsHash)?;
            if calculated_incremental_accounts_hash != found_incremental_accounts_hash {
                warn!(
                    "mismatched incremental accounts hash for slot {slot}: \
                    {calculated_incremental_accounts_hash:?} (calculated) != {found_incremental_accounts_hash:?} (expected)"
                );
                if hash_mismatch_is_error {
                    return Err(AccountsHashVerificationError::MismatchedAccountsHash);
                }
            }
        } else {
            let storages_and_slots = snapshot_storages_and_slots
                .0
                .iter()
                .zip(snapshot_storages_and_slots.1.iter())
                .filter(|storage_and_slot| *storage_and_slot.1 <= slot)
                .map(|(storage, slot)| (storage, *slot));
            let sorted_storages = SortedStorages::new_with_slots(storages_and_slots, None, None);
            let (calculated_accounts_hash, calculated_lamports) =
                self.calculate_accounts_hash(&calc_config, &sorted_storages, HashStats::default());
            if calculated_lamports != total_lamports {
                warn!(
                    "Mismatched total lamports: {} calculated: {}",
                    total_lamports, calculated_lamports
                );
                return Err(AccountsHashVerificationError::MismatchedTotalLamports(
                    calculated_lamports,
                    total_lamports,
                ));
            }
            let (found_accounts_hash, _) = self
                .get_accounts_hash(slot)
                .ok_or(AccountsHashVerificationError::MissingAccountsHash)?;
            if calculated_accounts_hash != found_accounts_hash {
                warn!(
                    "Mismatched accounts hash for slot {slot}: \
                    {calculated_accounts_hash:?} (calculated) != {found_accounts_hash:?} (expected)"
                );
                if hash_mismatch_is_error {
                    return Err(AccountsHashVerificationError::MismatchedAccountsHash);
                }
            }
        }

        Ok(())
    }

    /// helper to return
    /// 1. pubkey, hash pairs for the slot
    /// 2. us spent scanning
    /// 3. Measure started when we began accumulating
    pub fn get_pubkey_hash_for_slot(
        &self,
        slot: Slot,
    ) -> (Vec<(Pubkey, AccountHash)>, u64, Measure) {
        let mut scan = Measure::start("scan");
        let scan_result: ScanStorageResult<(Pubkey, AccountHash), DashMap<Pubkey, AccountHash>> =
            self.scan_account_storage(
                slot,
                |loaded_account: &LoadedAccount| {
                    // Cache only has one version per key, don't need to worry about versioning
                    Some((*loaded_account.pubkey(), loaded_account.loaded_hash()))
                },
                |accum: &DashMap<Pubkey, AccountHash>, loaded_account: &LoadedAccount, _data| {
                    let mut loaded_hash = loaded_account.loaded_hash();
                    if loaded_hash == AccountHash(Hash::default()) {
                        loaded_hash = Self::hash_account(loaded_account, loaded_account.pubkey())
                    }
                    accum.insert(*loaded_account.pubkey(), loaded_hash);
                },
                ScanAccountStorageData::NoData,
            );
        scan.stop();

        let accumulate = Measure::start("accumulate");
        let hashes: Vec<_> = match scan_result {
            ScanStorageResult::Cached(cached_result) => cached_result,
            ScanStorageResult::Stored(stored_result) => stored_result.into_iter().collect(),
        };

        (hashes, scan.as_us(), accumulate)
    }

    /// Return all of the accounts for a given slot
    pub fn get_pubkey_hash_account_for_slot(&self, slot: Slot) -> Vec<PubkeyHashAccount> {
        type ScanResult =
            ScanStorageResult<PubkeyHashAccount, DashMap<Pubkey, (AccountHash, AccountSharedData)>>;
        let scan_result: ScanResult = self.scan_account_storage(
            slot,
            |loaded_account: &LoadedAccount| {
                // Cache only has one version per key, don't need to worry about versioning
                Some(PubkeyHashAccount {
                    pubkey: *loaded_account.pubkey(),
                    hash: loaded_account.loaded_hash(),
                    account: loaded_account.take_account(),
                })
            },
            |accum: &DashMap<Pubkey, (AccountHash, AccountSharedData)>,
             loaded_account: &LoadedAccount,
             _data| {
                // Storage may have duplicates so only keep the latest version for each key
                let mut loaded_hash = loaded_account.loaded_hash();
                let key = *loaded_account.pubkey();
                let account = loaded_account.take_account();
                if loaded_hash == AccountHash(Hash::default()) {
                    loaded_hash = Self::hash_account(&account, &key)
                }
                accum.insert(key, (loaded_hash, account));
            },
            ScanAccountStorageData::NoData,
        );

        match scan_result {
            ScanStorageResult::Cached(cached_result) => cached_result,
            ScanStorageResult::Stored(stored_result) => stored_result
                .into_iter()
                .map(|(pubkey, (hash, account))| PubkeyHashAccount {
                    pubkey,
                    hash,
                    account,
                })
                .collect(),
        }
    }

    /// Wrapper function to calculate accounts delta hash for `slot` (only used for testing and benchmarking.)
    ///
    /// As part of calculating the accounts delta hash, get a list of accounts modified this slot
    /// (aka dirty pubkeys) and add them to `self.uncleaned_pubkeys` for future cleaning.
    #[cfg(feature = "dev-context-only-utils")]
    pub fn calculate_accounts_delta_hash(&self, slot: Slot) -> AccountsDeltaHash {
        self.calculate_accounts_delta_hash_internal(slot, None, HashMap::default())
    }

    /// Calculate accounts delta hash for `slot`
    ///
    /// As part of calculating the accounts delta hash, get a list of accounts modified this slot
    /// (aka dirty pubkeys) and add them to `self.uncleaned_pubkeys` for future cleaning.
    pub fn calculate_accounts_delta_hash_internal(
        &self,
        slot: Slot,
        ignore: Option<Pubkey>,
        mut skipped_rewrites: HashMap<Pubkey, AccountHash>,
    ) -> AccountsDeltaHash {
        let (mut hashes, scan_us, mut accumulate) = self.get_pubkey_hash_for_slot(slot);
        let dirty_keys = hashes.iter().map(|(pubkey, _hash)| *pubkey).collect();

        hashes.iter().for_each(|(k, _h)| {
            skipped_rewrites.remove(k);
        });

        let num_skipped_rewrites = skipped_rewrites.len();
        hashes.extend(skipped_rewrites);

        info!("skipped rewrite hashes {} {}", slot, num_skipped_rewrites);

        if let Some(ignore) = ignore {
            hashes.retain(|k| k.0 != ignore);
        }

        let accounts_delta_hash =
            AccountsDeltaHash(AccountsHasher::accumulate_account_hashes(hashes));
        accumulate.stop();
        let mut uncleaned_time = Measure::start("uncleaned_index");
        self.uncleaned_pubkeys.insert(slot, dirty_keys);
        uncleaned_time.stop();

        self.set_accounts_delta_hash(slot, accounts_delta_hash);

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
        self.stats
            .skipped_rewrites_num
            .fetch_add(num_skipped_rewrites, Ordering::Relaxed);

        accounts_delta_hash
    }

    /// Set the accounts delta hash for `slot` in the `accounts_delta_hashes` map
    ///
    /// returns the previous accounts delta hash for `slot`
    #[cfg_attr(feature = "dev-context-only-utils", qualifiers(pub))]
    fn set_accounts_delta_hash(
        &self,
        slot: Slot,
        accounts_delta_hash: AccountsDeltaHash,
    ) -> Option<AccountsDeltaHash> {
        self.accounts_delta_hashes
            .lock()
            .unwrap()
            .insert(slot, accounts_delta_hash)
    }

    /// After deserializing a snapshot, set the accounts delta hash for the new AccountsDb
    pub fn set_accounts_delta_hash_from_snapshot(
        &mut self,
        slot: Slot,
        accounts_delta_hash: SerdeAccountsDeltaHash,
    ) -> Option<AccountsDeltaHash> {
        self.set_accounts_delta_hash(slot, accounts_delta_hash.into())
    }

    /// Get the accounts delta hash for `slot` in the `accounts_delta_hashes` map
    pub fn get_accounts_delta_hash(&self, slot: Slot) -> Option<AccountsDeltaHash> {
        self.accounts_delta_hashes
            .lock()
            .unwrap()
            .get(&slot)
            .cloned()
    }

    /// When reconstructing AccountsDb from a snapshot, insert the `bank_hash_stats` into the
    /// internal bank hash stats map.
    ///
    /// This fn is only called when loading from a snapshot, which means AccountsDb is new and its
    /// bank hash stats map is unpopulated.  Except for slot 0.
    ///
    /// Slot 0 is a special case.  When a new AccountsDb is created--like when loading from a
    /// snapshot--the bank hash stats map is populated with a default entry at slot 0.  Remove the
    /// default entry at slot 0, and then insert the new value at `slot`.
    pub fn update_bank_hash_stats_from_snapshot(
        &mut self,
        slot: Slot,
        stats: BankHashStats,
    ) -> Option<BankHashStats> {
        let mut bank_hash_stats = self.bank_hash_stats.lock().unwrap();
        bank_hash_stats.remove(&0);
        bank_hash_stats.insert(slot, stats)
    }

    /// Get the bank hash stats for `slot` in the `bank_hash_stats` map
    pub fn get_bank_hash_stats(&self, slot: Slot) -> Option<BankHashStats> {
        self.bank_hash_stats.lock().unwrap().get(&slot).cloned()
    }

    fn update_index<'a>(
        &self,
        infos: Vec<AccountInfo>,
        accounts: &impl StorableAccounts<'a>,
        reclaim: UpsertReclaim,
        update_index_thread_selection: UpdateIndexThreadSelection,
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
                accounts.account(i, |account| {
                    let old_slot = accounts.slot(i);
                    self.accounts_index.upsert(
                        target_slot,
                        old_slot,
                        account.pubkey(),
                        &account,
                        &self.account_indexes,
                        info,
                        &mut reclaims,
                        reclaim,
                    );
                });
            });
            reclaims
        };
        if matches!(
            update_index_thread_selection,
            UpdateIndexThreadSelection::PoolWithThreshold,
        ) && len > threshold
        {
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

    fn should_not_shrink(alive_bytes: u64, total_bytes: u64) -> bool {
        alive_bytes >= total_bytes
    }

    fn is_shrinking_productive(slot: Slot, store: &AccountStorageEntry) -> bool {
        let alive_count = store.count();
        let stored_count = store.approx_stored_count();
        let alive_bytes = store.alive_bytes() as u64;
        let total_bytes = store.capacity();

        if Self::should_not_shrink(alive_bytes, total_bytes) {
            trace!(
                "shrink_slot_forced ({}): not able to shrink at all: alive/stored: {}/{} ({}b / {}b) save: {}",
                slot,
                alive_count,
                stored_count,
                alive_bytes,
                total_bytes,
                total_bytes.saturating_sub(alive_bytes),
            );
            return false;
        }

        true
    }

    fn is_candidate_for_shrink(&self, store: &AccountStorageEntry) -> bool {
        // appended ancient append vecs should not be shrunk by the normal shrink codepath.
        // It is not possible to identify ancient append vecs when we pack, so no check for ancient when we are not appending.
        let total_bytes = if self.create_ancient_storage == CreateAncientStorage::Append
            && is_ancient(&store.accounts)
            && store.accounts.can_append()
        {
            store.written_bytes()
        } else {
            store.capacity()
        };
        match self.shrink_ratio {
            AccountShrinkThreshold::TotalSpace { shrink_ratio: _ } => {
                (store.alive_bytes() as u64) < total_bytes
            }
            AccountShrinkThreshold::IndividualStore { shrink_ratio } => {
                (store.alive_bytes() as f64 / total_bytes as f64) < shrink_ratio
            }
        }
    }

    /// returns (dead slots, reclaimed_offsets)
    fn remove_dead_accounts<'a, I>(
        &'a self,
        reclaims: I,
        expected_slot: Option<Slot>,
        reset_accounts: bool,
    ) -> (IntSet<Slot>, SlotOffsets)
    where
        I: Iterator<Item = &'a (Slot, AccountInfo)>,
    {
        let mut reclaimed_offsets = SlotOffsets::default();

        assert!(self.storage.no_shrink_in_progress());

        let mut dead_slots = IntSet::default();
        let mut new_shrink_candidates = ShrinkCandidates::default();
        let mut measure = Measure::start("remove");
        for (slot, account_info) in reclaims {
            // No cached accounts should make it here
            assert!(!account_info.is_cached());
            reclaimed_offsets
                .entry(*slot)
                .or_default()
                .insert(account_info.offset());
        }
        if let Some(expected_slot) = expected_slot {
            assert_eq!(reclaimed_offsets.len(), 1);
            assert!(reclaimed_offsets.contains_key(&expected_slot));
        }

        reclaimed_offsets.iter().for_each(|(slot, offsets)| {
            if let Some(store) = self
                .storage
                .get_slot_storage_entry(*slot)
            {
                assert_eq!(
                    *slot, store.slot(),
                    "AccountsDB::accounts_index corrupted. Storage pointed to: {}, expected: {}, should only point to one slot",
                    store.slot(), *slot
                );
                if offsets.len() == store.count() {
                    store.remove_accounts(store.alive_bytes(), reset_accounts, offsets.len());
                    self.dirty_stores.insert(*slot, store.clone());
                    dead_slots.insert(*slot);
                }
                else {
                    let mut offsets = offsets.iter().cloned().collect::<Vec<_>>();
                    // sort so offsets are in order. This improves efficiency of loading the accounts.
                    offsets.sort_unstable();
                    let dead_bytes = store.accounts.get_account_sizes(&offsets).iter().sum();
                    store.remove_accounts(dead_bytes, reset_accounts, offsets.len());
                    if Self::is_shrinking_productive(*slot, &store)
                        && self.is_candidate_for_shrink(&store)
                    {
                        // Checking that this single storage entry is ready for shrinking,
                        // should be a sufficient indication that the slot is ready to be shrunk
                        // because slots should only have one storage entry, namely the one that was
                        // created by `flush_slot_cache()`.
                        new_shrink_candidates.insert(*slot);
                    }
                }
            }
        });
        measure.stop();
        self.clean_accounts_stats
            .remove_dead_accounts_remove_us
            .fetch_add(measure.as_us(), Ordering::Relaxed);

        let mut measure = Measure::start("shrink");
        let mut shrink_candidate_slots = self.shrink_candidate_slots.lock().unwrap();
        for slot in new_shrink_candidates {
            shrink_candidate_slots.insert(slot);
        }
        drop(shrink_candidate_slots);
        measure.stop();
        self.clean_accounts_stats
            .remove_dead_accounts_shrink_us
            .fetch_add(measure.as_us(), Ordering::Relaxed);

        dead_slots.retain(|slot| {
            if let Some(slot_store) = self.storage.get_slot_storage_entry(*slot) {
                if slot_store.count() != 0 {
                    return false;
                }
            }
            true
        });

        (dead_slots, reclaimed_offsets)
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

        let mut accounts_delta_hashes = self.accounts_delta_hashes.lock().unwrap();
        let mut bank_hash_stats = self.bank_hash_stats.lock().unwrap();
        for slot in dead_slots_iter {
            accounts_delta_hashes.remove(slot);
            bank_hash_stats.remove(slot);
        }
        drop(accounts_delta_hashes);
        drop(bank_hash_stats);

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
                    |_pubkey, _slots_refs, _entry| {
                        /* unused */
                        AccountsIndexScanResult::Unref
                    },
                    Some(AccountsIndexScanResult::Unref),
                    false,
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
                if self.accounts_index.clean_dead_slot(*slot) {
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
        self.accounts_index
            .update_roots_stats(&mut accounts_index_root_stats);
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
        dead_slots: &IntSet<Slot>,
        purged_account_slots: Option<&mut AccountSlots>,
        pubkeys_removed_from_accounts_index: &PubkeysRemovedFromAccountsIndex,
    ) {
        let mut measure = Measure::start("clean_stored_dead_slots-ms");
        let mut stores = vec![];
        // get all stores in a vec so we can iterate in parallel
        for slot in dead_slots.iter() {
            if let Some(slot_storage) = self.storage.get_slot_storage_entry(*slot) {
                stores.push(slot_storage);
            }
        }
        // get all pubkeys in all dead slots
        let purged_slot_pubkeys: HashSet<(Slot, Pubkey)> = {
            self.thread_pool_clean.install(|| {
                stores
                    .into_par_iter()
                    .map(|store| {
                        let slot = store.slot();
                        let mut pubkeys = Vec::with_capacity(store.count());
                        store.accounts.scan_pubkeys(|pubkey| {
                            pubkeys.push((slot, *pubkey));
                        });
                        pubkeys
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

    pub fn store_cached<'a>(
        &self,
        accounts: impl StorableAccounts<'a>,
        transactions: Option<&'a [Option<&'a SanitizedTransaction>]>,
    ) {
        self.store(
            accounts,
            &StoreTo::Cache,
            transactions,
            StoreReclaims::Default,
            UpdateIndexThreadSelection::PoolWithThreshold,
        );
    }

    pub(crate) fn store_cached_inline_update_index<'a>(
        &self,
        accounts: impl StorableAccounts<'a>,
        transactions: Option<&'a [Option<&'a SanitizedTransaction>]>,
    ) {
        self.store(
            accounts,
            &StoreTo::Cache,
            transactions,
            StoreReclaims::Default,
            UpdateIndexThreadSelection::Inline,
        );
    }

    /// Store the account update.
    /// only called by tests
    pub fn store_uncached(&self, slot: Slot, accounts: &[(&Pubkey, &AccountSharedData)]) {
        let storage = self.find_storage_candidate(slot);
        self.store(
            (slot, accounts),
            &StoreTo::Storage(&storage),
            None,
            StoreReclaims::Default,
            UpdateIndexThreadSelection::PoolWithThreshold,
        );
    }

    fn store<'a>(
        &self,
        accounts: impl StorableAccounts<'a>,
        store_to: &StoreTo,
        transactions: Option<&'a [Option<&'a SanitizedTransaction>]>,
        reclaim: StoreReclaims,
        update_index_thread_selection: UpdateIndexThreadSelection,
    ) {
        // If all transactions in a batch are errored,
        // it's possible to get a store with no accounts.
        if accounts.is_empty() {
            return;
        }

        let mut stats = BankHashStats::default();
        let mut total_data = 0;
        (0..accounts.len()).for_each(|index| {
            accounts.account(index, |account| {
                total_data += account.data().len();
                stats.update(&account);
            })
        });

        self.stats
            .store_total_data
            .fetch_add(total_data as u64, Ordering::Relaxed);

        {
            // we need to drop the bank_hash_stats lock to prevent deadlocks
            self.bank_hash_stats
                .lock()
                .unwrap()
                .entry(accounts.target_slot())
                .or_default()
                .accumulate(&stats);
        }

        // we use default hashes for now since the same account may be stored to the cache multiple times
        self.store_accounts_unfrozen(
            accounts,
            store_to,
            transactions,
            reclaim,
            update_index_thread_selection,
        );
        self.report_store_timings();
    }

    fn report_store_timings(&self) {
        if self.stats.last_store_report.should_update(1000) {
            let read_cache_stats = self.read_only_accounts_cache.get_and_reset_stats();
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
                ("read_only_accounts_cache_hits", read_cache_stats.hits, i64),
                (
                    "read_only_accounts_cache_misses",
                    read_cache_stats.misses,
                    i64
                ),
                (
                    "read_only_accounts_cache_evicts",
                    read_cache_stats.evicts,
                    i64
                ),
                (
                    "read_only_accounts_cache_load_us",
                    read_cache_stats.load_us,
                    i64
                ),
                (
                    "read_only_accounts_cache_store_us",
                    read_cache_stats.store_us,
                    i64
                ),
                (
                    "read_only_accounts_cache_evict_us",
                    read_cache_stats.evict_us,
                    i64
                ),
                (
                    "read_only_accounts_cache_evictor_wakeup_count_all",
                    read_cache_stats.evictor_wakeup_count_all,
                    i64
                ),
                (
                    "read_only_accounts_cache_evictor_wakeup_count_productive",
                    read_cache_stats.evictor_wakeup_count_productive,
                    i64
                ),
                (
                    "calc_stored_meta_us",
                    self.stats.calc_stored_meta.swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "handle_dead_keys_us",
                    self.stats.handle_dead_keys_us.swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "purge_exact_us",
                    self.stats.purge_exact_us.swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "purge_exact_count",
                    self.stats.purge_exact_count.swap(0, Ordering::Relaxed),
                    i64
                ),
            );

            datapoint_info!(
                "accounts_db_store_timings2",
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

    fn store_accounts_unfrozen<'a>(
        &self,
        accounts: impl StorableAccounts<'a>,
        store_to: &StoreTo,
        transactions: Option<&'a [Option<&'a SanitizedTransaction>]>,
        reclaim: StoreReclaims,
        update_index_thread_selection: UpdateIndexThreadSelection,
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
            store_to,
            reset_accounts,
            transactions,
            reclaim,
            update_index_thread_selection,
        );
    }

    pub fn store_accounts_frozen<'a>(
        &self,
        accounts: impl StorableAccounts<'a>,
        storage: &Arc<AccountStorageEntry>,
    ) -> StoreAccountsTiming {
        // stores on a frozen slot should not reset
        // the append vec so that hashing could happen on the store
        // and accounts in the append_vec can be unrefed correctly
        let reset_accounts = false;
        self.store_accounts_custom(
            accounts,
            &StoreTo::Storage(storage),
            reset_accounts,
            None,
            StoreReclaims::Ignore,
            UpdateIndexThreadSelection::PoolWithThreshold,
        )
    }

    fn store_accounts_custom<'a>(
        &self,
        accounts: impl StorableAccounts<'a>,
        store_to: &StoreTo,
        reset_accounts: bool,
        transactions: Option<&[Option<&SanitizedTransaction>]>,
        reclaim: StoreReclaims,
        update_index_thread_selection: UpdateIndexThreadSelection,
    ) -> StoreAccountsTiming {
        self.stats
            .store_num_accounts
            .fetch_add(accounts.len() as u64, Ordering::Relaxed);
        let mut store_accounts_time = Measure::start("store_accounts");
        let infos = self.store_accounts_to(&accounts, store_to, transactions);
        store_accounts_time.stop();
        self.stats
            .store_accounts
            .fetch_add(store_accounts_time.as_us(), Ordering::Relaxed);
        let mut update_index_time = Measure::start("update_index");

        let reclaim = if matches!(reclaim, StoreReclaims::Ignore) {
            UpsertReclaim::IgnoreReclaims
        } else if store_to.is_cached() {
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
        let mut reclaims =
            self.update_index(infos, &accounts, reclaim, update_index_thread_selection);

        // For each updated account, `reclaims` should only have at most one
        // item (if the account was previously updated in this slot).
        // filter out the cached reclaims as those don't actually map
        // to anything that needs to be cleaned in the backing storage
        // entries
        reclaims.retain(|(_, r)| !r.is_cached());

        if store_to.is_cached() {
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
        let mut handle_reclaims_elapsed = 0;
        if reclaim == UpsertReclaim::PopulateReclaims {
            let mut handle_reclaims_time = Measure::start("handle_reclaims");
            self.handle_reclaims(
                (!reclaims.is_empty()).then(|| reclaims.iter()),
                expected_single_dead_slot,
                reset_accounts,
                &HashSet::default(),
                // this callsite does NOT process dead slots
                HandleReclaims::DoNotProcessDeadSlots,
            );
            handle_reclaims_time.stop();
            handle_reclaims_elapsed = handle_reclaims_time.as_us();
            self.stats
                .store_handle_reclaims
                .fetch_add(handle_reclaims_elapsed, Ordering::Relaxed);
        } else {
            assert!(reclaims.is_empty());
        }

        StoreAccountsTiming {
            store_accounts_elapsed: store_accounts_time.as_us(),
            update_index_elapsed: update_index_time.as_us(),
            handle_reclaims_elapsed,
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
        // We would not expect this slot to be shrinking right now, but other slots may be.
        // But, even if it was, we would just mark a store id as dirty unnecessarily and that is ok.
        // So, allow shrinking to be in progress.
        if let Some(store) = self
            .storage
            .get_slot_storage_entry_shrinking_in_progress_ok(slot)
        {
            self.dirty_stores.insert(slot, store);
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
    ) -> (Vec<Arc<AccountStorageEntry>>, Vec<Slot>) {
        let mut m = Measure::start("get slots");
        let mut slots_and_storages = self
            .storage
            .iter()
            .filter_map(|(slot, store)| {
                requested_slots
                    .contains(&slot)
                    .then_some((slot, Some(store)))
            })
            .collect::<Vec<_>>();
        m.stop();
        let mut m2 = Measure::start("filter");
        let chunk_size = 5_000;
        let (result, slots): (Vec<_>, Vec<_>) = self.thread_pool_clean.install(|| {
            slots_and_storages
                .par_chunks_mut(chunk_size)
                .map(|slots_and_storages| {
                    slots_and_storages
                        .iter_mut()
                        .filter(|(slot, _)| self.accounts_index.is_alive_root(*slot))
                        .filter_map(|(slot, store)| {
                            let store = std::mem::take(store).unwrap();
                            store.has_accounts().then_some((store, *slot))
                        })
                        .collect::<Vec<(Arc<AccountStorageEntry>, Slot)>>()
                })
                .flatten()
                .unzip()
        });

        m2.stop();

        debug!(
            "hash_total: get slots: {}, filter: {}",
            m.as_us(),
            m2.as_us(),
        );
        (result, slots)
    }

    /// Returns the latest full snapshot slot
    pub fn latest_full_snapshot_slot(&self) -> Option<Slot> {
        self.latest_full_snapshot_slot.read()
    }

    /// Sets the latest full snapshot slot to `slot`
    pub fn set_latest_full_snapshot_slot(&self, slot: Slot) {
        *self.latest_full_snapshot_slot.lock_write() = Some(slot);
    }

    /// return Some(lamports_to_top_off) if 'account' would collect rent
    fn stats_for_rent_payers(
        pubkey: &Pubkey,
        lamports: u64,
        account_data_len: usize,
        account_rent_epoch: Epoch,
        executable: bool,
        rent_collector: &RentCollector,
    ) -> Option<u64> {
        if lamports == 0 {
            return None;
        }
        (rent_collector.should_collect_rent(pubkey, executable)
            && !rent_collector
                .get_rent_due(lamports, account_data_len, account_rent_epoch)
                .is_exempt())
        .then(|| {
            let min_balance = rent_collector.rent.minimum_balance(account_data_len);
            // return lamports required to top off this account to make it rent exempt
            min_balance.saturating_sub(lamports)
        })
    }

    fn generate_index_for_slot(
        &self,
        storage: &AccountStorageEntry,
        slot: Slot,
        store_id: AccountsFileId,
        rent_collector: &RentCollector,
        storage_info: &StorageSizeAndCountMap,
    ) -> SlotIndexGenerationInfo {
        if storage.accounts.get_account_sizes(&[0]).is_empty() {
            return SlotIndexGenerationInfo::default();
        }
        let secondary = !self.account_indexes.is_empty();

        let mut rent_paying_accounts_by_partition = Vec::default();
        let mut accounts_data_len = 0;
        let mut num_accounts_rent_paying = 0;
        let mut amount_to_top_off_rent = 0;
        let mut stored_size_alive = 0;

        let (dirty_pubkeys, insert_time_us, mut generate_index_results) = {
            let mut items_local = Vec::default();
            storage.accounts.scan_index(|info| {
                stored_size_alive += info.stored_size_aligned;
                if info.index_info.lamports > 0 {
                    accounts_data_len += info.index_info.data_len;
                }
                items_local.push(info.index_info);
            });
            let items = items_local.into_iter().map(|info| {
                if let Some(amount_to_top_off_rent_this_account) = Self::stats_for_rent_payers(
                    &info.pubkey,
                    info.lamports,
                    info.data_len as usize,
                    info.rent_epoch,
                    info.executable,
                    rent_collector,
                ) {
                    amount_to_top_off_rent += amount_to_top_off_rent_this_account;
                    num_accounts_rent_paying += 1;
                    // remember this rent-paying account pubkey
                    rent_paying_accounts_by_partition.push(info.pubkey);
                }

                (
                    info.pubkey,
                    AccountInfo::new(
                        StorageLocation::AppendVec(store_id, info.offset), // will never be cached
                        info.lamports,
                    ),
                )
            });
            self.accounts_index
                .insert_new_if_missing_into_primary_index(
                    slot,
                    storage.approx_stored_count(),
                    items,
                )
        };
        if secondary {
            // scan storage a second time to update the secondary index
            storage.accounts.scan_accounts(|stored_account| {
                stored_size_alive += stored_account.stored_size();
                let pubkey = stored_account.pubkey();
                self.accounts_index.update_secondary_indexes(
                    pubkey,
                    &stored_account,
                    &self.account_indexes,
                );
            });
        }

        if let Some(duplicates_this_slot) = std::mem::take(&mut generate_index_results.duplicates) {
            // there were duplicate pubkeys in this same slot
            // Some were not inserted. This means some info like stored data is off.
            duplicates_this_slot
                .into_iter()
                .for_each(|(pubkey, (_slot, info))| {
                    storage
                        .accounts
                        .get_stored_account_meta_callback(info.offset(), |duplicate| {
                            assert_eq!(&pubkey, duplicate.pubkey());
                            stored_size_alive =
                                stored_size_alive.saturating_sub(duplicate.stored_size());
                            if !duplicate.is_zero_lamport() {
                                accounts_data_len =
                                    accounts_data_len.saturating_sub(duplicate.data().len() as u64);
                            }
                        });
                });
        }

        {
            // second, collect into the shared DashMap once we've figured out all the info per store_id
            let mut info = storage_info.entry(store_id).or_default();
            info.stored_size += stored_size_alive;
            info.count += generate_index_results.count;
        }

        // dirty_pubkeys will contain a pubkey if an item has multiple rooted entries for
        // a given pubkey. If there is just a single item, there is no cleaning to
        // be done on that pubkey. Use only those pubkeys with multiple updates.
        if !dirty_pubkeys.is_empty() {
            self.uncleaned_pubkeys.insert(slot, dirty_pubkeys);
        }
        SlotIndexGenerationInfo {
            insert_time_us,
            num_accounts: generate_index_results.count as u64,
            num_accounts_rent_paying,
            accounts_data_len,
            amount_to_top_off_rent,
            rent_paying_accounts_by_partition,
        }
    }

    pub fn generate_index(
        &self,
        limit_load_slot_count_from_snapshot: Option<usize>,
        verify: bool,
        genesis_config: &GenesisConfig,
    ) -> IndexGenerationInfo {
        let mut total_time = Measure::start("generate_index");
        let mut slots = self.storage.all_slots();
        slots.sort_unstable();
        if let Some(limit) = limit_load_slot_count_from_snapshot {
            slots.truncate(limit); // get rid of the newer slots and keep just the older
        }
        let max_slot = slots.last().cloned().unwrap_or_default();
        let schedule = &genesis_config.epoch_schedule;
        let rent_collector = RentCollector::new(
            schedule.get_epoch(max_slot),
            schedule.clone(),
            genesis_config.slots_per_year(),
            genesis_config.rent.clone(),
        );
        let accounts_data_len = AtomicU64::new(0);

        let rent_paying_accounts_by_partition =
            Mutex::new(RentPayingAccountsByPartition::new(schedule));

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
                // seems to be a good heuristic given varying # cpus for in-mem disk index
                8
            };
            let chunk_size = (outer_slots_len / (std::cmp::max(1, threads.saturating_sub(1)))) + 1; // approximately 400k slots in a snapshot
            let mut index_time = Measure::start("index");
            let insertion_time_us = AtomicU64::new(0);
            let rent_paying = AtomicUsize::new(0);
            let amount_to_top_off_rent = AtomicU64::new(0);
            let total_including_duplicates = AtomicU64::new(0);
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
                        let Some(storage) = self.storage.get_slot_storage_entry(*slot) else {
                            // no storage at this slot, no information to pull out
                            continue;
                        };
                        let store_id = storage.id();

                        scan_time.stop();
                        scan_time_sum += scan_time.as_us();

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
                            } = self.generate_index_for_slot(
                                &storage,
                                *slot,
                                store_id,
                                &rent_collector,
                                &storage_info,
                            );

                            rent_paying.fetch_add(rent_paying_this_slot, Ordering::Relaxed);
                            amount_to_top_off_rent
                                .fetch_add(amount_to_top_off_rent_this_slot, Ordering::Relaxed);
                            total_including_duplicates
                                .fetch_add(total_this_slot, Ordering::Relaxed);
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
                            storage.accounts.scan_accounts(|account_info| {
                                let key = account_info.pubkey();
                                let index_entry = self.accounts_index.get_cloned(key).unwrap();
                                let slot_list = index_entry.slot_list.read().unwrap();
                                let mut count = 0;
                                for (slot2, account_info2) in slot_list.iter() {
                                    if slot2 == slot {
                                        count += 1;
                                        let ai = AccountInfo::new(
                                            StorageLocation::AppendVec(
                                                store_id,
                                                account_info.offset(),
                                            ), // will never be cached
                                            account_info.lamports(),
                                        );
                                        assert_eq!(&ai, account_info2);
                                    }
                                }
                                assert_eq!(1, count);
                            });
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
            let (total_items, min_bin_size, max_bin_size) = self
                .accounts_index
                .account_maps
                .iter()
                .map(|map_bin| map_bin.len_for_stats())
                .fold((0, usize::MAX, usize::MIN), |acc, len| {
                    (
                        acc.0 + len,
                        std::cmp::min(acc.1, len),
                        std::cmp::max(acc.2, len),
                    )
                });

            let mut index_flush_us = 0;
            let total_duplicate_slot_keys = AtomicU64::default();
            let mut populate_duplicate_keys_us = 0;
            // outer vec is accounts index bin (determined by pubkey value)
            // inner vec is the pubkeys within that bin that are present in > 1 slot
            let unique_pubkeys_by_bin = Mutex::new(Vec::<Vec<Pubkey>>::default());
            if pass == 0 {
                // tell accounts index we are done adding the initial accounts at startup
                let mut m = Measure::start("accounts_index_idle_us");
                self.accounts_index.set_startup(Startup::Normal);
                m.stop();
                index_flush_us = m.as_us();

                populate_duplicate_keys_us = measure_us!({
                    // this has to happen before visit_duplicate_pubkeys_during_startup below
                    // get duplicate keys from acct idx. We have to wait until we've finished flushing.
                    self.accounts_index
                        .populate_and_retrieve_duplicate_keys_from_startup(|slot_keys| {
                            total_duplicate_slot_keys
                                .fetch_add(slot_keys.len() as u64, Ordering::Relaxed);
                            let unique_keys =
                                HashSet::<Pubkey>::from_iter(slot_keys.iter().map(|(_, key)| *key));
                            for (slot, key) in slot_keys {
                                self.uncleaned_pubkeys.entry(slot).or_default().push(key);
                            }
                            let unique_pubkeys_by_bin_inner =
                                unique_keys.into_iter().collect::<Vec<_>>();
                            // does not matter that this is not ordered by slot
                            unique_pubkeys_by_bin
                                .lock()
                                .unwrap()
                                .push(unique_pubkeys_by_bin_inner);
                        });
                })
                .1;
            }
            let unique_pubkeys_by_bin = unique_pubkeys_by_bin.into_inner().unwrap();

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
                total_duplicate_slot_keys: total_duplicate_slot_keys.load(Ordering::Relaxed),
                populate_duplicate_keys_us,
                total_including_duplicates: total_including_duplicates.load(Ordering::Relaxed),
                total_slots: slots.len() as u64,
                ..GenerateIndexTimings::default()
            };

            if pass == 0 {
                #[derive(Debug, Default)]
                struct DuplicatePubkeysVisitedInfo {
                    accounts_data_len_from_duplicates: u64,
                    uncleaned_roots: IntSet<Slot>,
                }
                impl DuplicatePubkeysVisitedInfo {
                    fn reduce(mut a: Self, mut b: Self) -> Self {
                        if a.uncleaned_roots.len() >= b.uncleaned_roots.len() {
                            a.merge(b);
                            a
                        } else {
                            b.merge(a);
                            b
                        }
                    }
                    fn merge(&mut self, other: Self) {
                        self.accounts_data_len_from_duplicates +=
                            other.accounts_data_len_from_duplicates;
                        self.uncleaned_roots.extend(other.uncleaned_roots);
                    }
                }

                // subtract data.len() from accounts_data_len for all old accounts that are in the index twice
                let mut accounts_data_len_dedup_timer =
                    Measure::start("handle accounts data len duplicates");
                let DuplicatePubkeysVisitedInfo {
                    accounts_data_len_from_duplicates,
                    uncleaned_roots,
                } = unique_pubkeys_by_bin
                    .par_iter()
                    .fold(
                        DuplicatePubkeysVisitedInfo::default,
                        |accum, pubkeys_by_bin| {
                            let intermediate = pubkeys_by_bin
                                .par_chunks(4096)
                                .fold(DuplicatePubkeysVisitedInfo::default, |accum, pubkeys| {
                                    let (accounts_data_len_from_duplicates, uncleaned_roots) = self
                                        .visit_duplicate_pubkeys_during_startup(
                                            pubkeys,
                                            &rent_collector,
                                            &timings,
                                        );
                                    let intermediate = DuplicatePubkeysVisitedInfo {
                                        accounts_data_len_from_duplicates,
                                        uncleaned_roots,
                                    };
                                    DuplicatePubkeysVisitedInfo::reduce(accum, intermediate)
                                })
                                .reduce(
                                    DuplicatePubkeysVisitedInfo::default,
                                    DuplicatePubkeysVisitedInfo::reduce,
                                );
                            DuplicatePubkeysVisitedInfo::reduce(accum, intermediate)
                        },
                    )
                    .reduce(
                        DuplicatePubkeysVisitedInfo::default,
                        DuplicatePubkeysVisitedInfo::reduce,
                    );
                accounts_data_len_dedup_timer.stop();
                timings.accounts_data_len_dedup_time_us = accounts_data_len_dedup_timer.as_us();
                timings.slots_to_clean = uncleaned_roots.len() as u64;

                self.accounts_index
                    .add_uncleaned_roots(uncleaned_roots.into_iter());
                accounts_data_len.fetch_sub(accounts_data_len_from_duplicates, Ordering::Relaxed);
                info!(
                    "accounts data len: {}",
                    accounts_data_len.load(Ordering::Relaxed)
                );
            }

            if pass == 0 {
                // Need to add these last, otherwise older updates will be cleaned
                for root in &slots {
                    self.accounts_index.add_root(*root);
                }

                self.set_storage_count_and_alive_bytes(storage_info, &mut timings);
            }
            total_time.stop();
            timings.total_time_us = total_time.as_us();
            timings.report(self.accounts_index.get_startup_stats());
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
    ) -> (u64, IntSet<Slot>) {
        let mut accounts_data_len_from_duplicates = 0;
        let mut uncleaned_slots = IntSet::default();
        let mut removed_rent_paying = 0;
        let mut removed_top_off = 0;
        self.accounts_index.scan(
            pubkeys.iter(),
            |pubkey, slots_refs, _entry| {
                if let Some((slot_list, _ref_count)) = slots_refs {
                    if slot_list.len() > 1 {
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
                            accessor.check_and_get_loaded_account(|loaded_account| {
                                let data_len = loaded_account.data_len();
                                accounts_data_len_from_duplicates += data_len;
                                if let Some(lamports_to_top_off) = Self::stats_for_rent_payers(
                                    pubkey,
                                    loaded_account.lamports(),
                                    data_len,
                                    loaded_account.rent_epoch(),
                                    loaded_account.executable(),
                                    rent_collector,
                                ) {
                                    removed_rent_paying += 1;
                                    removed_top_off += lamports_to_top_off;
                                }
                            });
                        });
                    }
                }
                AccountsIndexScanResult::OnlyKeepInMemoryIfDirty
            },
            None,
            false,
        );
        timings
            .rent_paying
            .fetch_sub(removed_rent_paying, Ordering::Relaxed);
        timings
            .amount_to_top_off_rent
            .fetch_sub(removed_top_off, Ordering::Relaxed);
        (accounts_data_len_from_duplicates as u64, uncleaned_slots)
    }

    fn set_storage_count_and_alive_bytes(
        &self,
        stored_sizes_and_counts: StorageSizeAndCountMap,
        timings: &mut GenerateIndexTimings,
    ) {
        // store count and size for each storage
        let mut storage_size_storages_time = Measure::start("storage_size_storages");
        for (_slot, store) in self.storage.iter() {
            let id = store.id();
            // Should be default at this point
            assert_eq!(store.alive_bytes(), 0);
            if let Some(entry) = stored_sizes_and_counts.get(&id) {
                trace!(
                    "id: {} setting count: {} cur: {}",
                    id,
                    entry.count,
                    store.count(),
                );
                {
                    let mut count_and_status = store.count_and_status.lock_write();
                    assert_eq!(count_and_status.0, 0);
                    count_and_status.0 = entry.count;
                }
                store.alive_bytes.store(entry.stored_size, Ordering::SeqCst);
                assert!(
                    store.approx_stored_count() >= entry.count,
                    "{}, {}",
                    store.approx_stored_count(),
                    entry.count
                );
            } else {
                trace!("id: {} clearing count", id);
                store.count_and_status.lock_write().0 = 0;
            }
        }
        storage_size_storages_time.stop();
        timings.storage_size_storages_us = storage_size_storages_time.as_us();
    }

    pub fn print_accounts_stats(&self, label: &str) {
        self.print_index(label);
        self.print_count_and_status(label);
    }

    fn print_index(&self, label: &str) {
        let mut alive_roots: Vec<_> = self.accounts_index.all_alive_roots();
        #[allow(clippy::stable_sort_primitive)]
        alive_roots.sort();
        info!("{}: accounts_index alive_roots: {:?}", label, alive_roots,);
        let full_pubkey_range = Pubkey::from([0; 32])..=Pubkey::from([0xff; 32]);

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

    pub fn print_count_and_status(&self, label: &str) {
        let mut slots: Vec<_> = self.storage.all_slots();
        #[allow(clippy::stable_sort_primitive)]
        slots.sort();
        info!("{}: count_and status for {} slots:", label, slots.len());
        for slot in &slots {
            let entry = self.storage.get_slot_storage_entry(*slot).unwrap();
            info!(
                "  slot: {} id: {} count_and_status: {:?} approx_store_count: {} len: {} capacity: {}",
                slot,
                entry.id(),
                entry.count_and_status.read(),
                entry.approx_store_count.load(Ordering::Relaxed),
                entry.accounts.len(),
                entry.accounts.capacity(),
            );
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

#[derive(Debug, Copy, Clone)]
enum HandleReclaims<'a> {
    ProcessDeadSlots(&'a PurgeStats),
    DoNotProcessDeadSlots,
}

/// Which accounts hash calculation is being performed?
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum CalcAccountsHashKind {
    Full,
    Incremental,
}

impl CalcAccountsHashKind {
    /// How should zero-lamport accounts be handled by this accounts hash calculation?
    fn zero_lamport_accounts(&self) -> ZeroLamportAccounts {
        match self {
            CalcAccountsHashKind::Full => ZeroLamportAccounts::Excluded,
            CalcAccountsHashKind::Incremental => ZeroLamportAccounts::Included,
        }
    }
}

pub(crate) enum UpdateIndexThreadSelection {
    /// Use current thread only
    Inline,
    /// Use a thread-pool if the number of updates exceeds a threshold
    PoolWithThreshold,
}

// These functions/fields are only usable from a dev context (i.e. tests and benches)
#[cfg(feature = "dev-context-only-utils")]
impl AccountStorageEntry {
    fn accounts_count(&self) -> usize {
        let mut count = 0;
        self.accounts.scan_accounts(|_| {
            count += 1;
        });
        count
    }
}

// These functions/fields are only usable from a dev context (i.e. tests and benches)
#[cfg(feature = "dev-context-only-utils")]
impl AccountsDb {
    pub fn load_without_fixed_root(
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

    pub fn accounts_delta_hashes(&self) -> &Mutex<HashMap<Slot, AccountsDeltaHash>> {
        &self.accounts_delta_hashes
    }

    pub fn accounts_hashes(&self) -> &Mutex<HashMap<Slot, (AccountsHash, /*capitalization*/ u64)>> {
        &self.accounts_hashes
    }

    pub fn assert_load_account(&self, slot: Slot, pubkey: Pubkey, expected_lamports: u64) {
        let ancestors = vec![(slot, 0)].into_iter().collect();
        let (account, slot) = self.load_without_fixed_root(&ancestors, &pubkey).unwrap();
        assert_eq!((account.lamports(), slot), (expected_lamports, slot));
    }

    pub fn assert_not_load_account(&self, slot: Slot, pubkey: Pubkey) {
        let ancestors = vec![(slot, 0)].into_iter().collect();
        let load = self.load_without_fixed_root(&ancestors, &pubkey);
        assert!(load.is_none(), "{load:?}");
    }

    pub fn check_accounts(&self, pubkeys: &[Pubkey], slot: Slot, num: usize, count: usize) {
        let ancestors = vec![(slot, 0)].into_iter().collect();
        for _ in 0..num {
            let idx = thread_rng().gen_range(0..num);
            let account = self.load_without_fixed_root(&ancestors, &pubkeys[idx]);
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

    /// callers used to call store_uncached. But, this is not allowed anymore.
    pub fn store_for_tests(&self, slot: Slot, accounts: &[(&Pubkey, &AccountSharedData)]) {
        self.store(
            (slot, accounts),
            &StoreTo::Cache,
            None,
            StoreReclaims::Default,
            UpdateIndexThreadSelection::PoolWithThreshold,
        );
    }

    #[allow(clippy::needless_range_loop)]
    pub fn modify_accounts(&self, pubkeys: &[Pubkey], slot: Slot, num: usize, count: usize) {
        for idx in 0..num {
            let account = AccountSharedData::new(
                (idx + count) as u64,
                0,
                AccountSharedData::default().owner(),
            );
            self.store_for_tests(slot, &[(&pubkeys[idx], &account)]);
        }
    }

    pub fn check_storage(&self, slot: Slot, count: usize) {
        assert!(self.storage.get_slot_storage_entry(slot).is_some());
        let store = self.storage.get_slot_storage_entry(slot).unwrap();
        let total_count = store.count();
        assert_eq!(store.status(), AccountStorageStatus::Available);
        assert_eq!(total_count, count);
        let (expected_store_count, actual_store_count): (usize, usize) =
            (store.approx_stored_count(), store.accounts_count());
        assert_eq!(expected_store_count, actual_store_count);
    }

    pub fn create_account(
        &self,
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
            assert!(self.load_without_fixed_root(&ancestors, &pubkey).is_none());
            self.store_for_tests(slot, &[(&pubkey, &account)]);
        }
        for t in 0..num_vote {
            let pubkey = solana_sdk::pubkey::new_rand();
            let account =
                AccountSharedData::new((num + t + 1) as u64, space, &solana_vote_program::id());
            pubkeys.push(pubkey);
            let ancestors = vec![(slot, 0)].into_iter().collect();
            assert!(self.load_without_fixed_root(&ancestors, &pubkey).is_none());
            self.store_for_tests(slot, &[(&pubkey, &account)]);
        }
    }

    pub fn sizes_of_accounts_in_storage_for_tests(&self, slot: Slot) -> Vec<usize> {
        let mut sizes = Vec::default();
        if let Some(storage) = self.storage.get_slot_storage_entry(slot) {
            storage.accounts.scan_accounts(|account| {
                sizes.push(account.stored_size());
            });
        }
        sizes
    }

    pub fn ref_count_for_pubkey(&self, pubkey: &Pubkey) -> RefCount {
        self.accounts_index.ref_count_from_storage(pubkey)
    }

    pub fn alive_account_count_in_slot(&self, slot: Slot) -> usize {
        self.storage
            .get_slot_storage_entry(slot)
            .map(|storage| storage.count())
            .unwrap_or(0)
            .saturating_add(
                self.accounts_cache
                    .slot_cache(slot)
                    .map(|slot_cache| slot_cache.len())
                    .unwrap_or_default(),
            )
    }

    /// useful to adapt tests written prior to introduction of the write cache
    /// to use the write cache
    pub fn add_root_and_flush_write_cache(&self, slot: Slot) {
        self.add_root(slot);
        self.flush_root_write_cache(slot);
    }

    /// useful to adapt tests written prior to introduction of the write cache
    /// to use the write cache
    pub fn flush_root_write_cache(&self, root: Slot) {
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

    pub fn all_account_count_in_accounts_file(&self, slot: Slot) -> usize {
        let store = self.storage.get_slot_storage_entry(slot);
        if let Some(store) = store {
            let count = store.accounts_count();
            let stored_count = store.approx_stored_count();
            assert_eq!(stored_count, count);
            count
        } else {
            0
        }
    }

    pub fn verify_accounts_hash_and_lamports_for_tests(
        &self,
        slot: Slot,
        total_lamports: u64,
        config: VerifyAccountsHashAndLamportsConfig,
    ) -> Result<(), AccountsHashVerificationError> {
        let snapshot_storages = self.get_snapshot_storages(..);
        let snapshot_storages_and_slots = (
            snapshot_storages.0.as_slice(),
            snapshot_storages.1.as_slice(),
        );
        self.verify_accounts_hash_and_lamports(
            snapshot_storages_and_slots,
            slot,
            total_lamports,
            None,
            config,
        )
    }
}

// These functions/fields are only usable from a dev context (i.e. tests and benches)
#[cfg(feature = "dev-context-only-utils")]
impl<'a> VerifyAccountsHashAndLamportsConfig<'a> {
    pub fn new_for_test(
        ancestors: &'a Ancestors,
        epoch_schedule: &'a EpochSchedule,
        rent_collector: &'a RentCollector,
    ) -> Self {
        Self {
            ancestors,
            test_hash_calculation: true,
            epoch_schedule,
            rent_collector,
            ignore_mismatch: false,
            store_detailed_debug_info: false,
            use_bg_thread_pool: false,
        }
    }
}

/// A set of utility functions used for testing and benchmarking
#[cfg(feature = "dev-context-only-utils")]
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
        if accounts
            .accounts_db
            .storage
            .get_slot_storage_entry(slot)
            .is_none()
        {
            // Some callers relied on old behavior where the the file size was rounded up to the
            // next page size because they append to the storage file after it was written.
            // This behavior is not supported by a normal running validator.  Since this function
            // is only called by tests/benches, add some extra capacity to the file to not break
            // the tests/benches.  Those tests/benches should be updated though!  Bypassing the
            // write cache in general is not supported.
            let bytes_required = num * aligned_stored_size(data_size) + 4096;
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
            let amount = thread_rng().gen_range(0..10);
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
            account_info::StoredSize,
            account_storage::meta::{AccountMeta, StoredMeta},
            accounts_file::AccountsFileProvider,
            accounts_hash::MERKLE_FANOUT,
            accounts_index::{tests::*, AccountSecondaryIndexesIncludeExclude},
            ancient_append_vecs,
            append_vec::{test_utils::TempFile, AppendVec, AppendVecStoredAccountMeta},
            storable_accounts::AccountForStorage,
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
            hash::DefaultHasher,
            iter::FromIterator,
            str::FromStr,
            sync::{atomic::AtomicBool, RwLock},
            thread::{self, Builder, JoinHandle},
        },
        test_case::test_case,
    };

    fn linear_ancestors(end_slot: u64) -> Ancestors {
        let mut ancestors: Ancestors = vec![(0, 0)].into_iter().collect();
        for i in 1..end_slot {
            ancestors.insert(i, (i - 1) as usize);
        }
        ancestors
    }

    impl AccountsDb {
        fn get_storage_for_slot(&self, slot: Slot) -> Option<Arc<AccountStorageEntry>> {
            self.storage.get_slot_storage_entry(slot)
        }
    }

    /// this tuple contains slot info PER account
    impl<'a, T: ReadableAccount + Sync> StorableAccounts<'a> for (Slot, &'a [(&'a Pubkey, &'a T, Slot)])
    where
        AccountForStorage<'a>: From<&'a T>,
    {
        fn account<Ret>(
            &self,
            index: usize,
            mut callback: impl for<'local> FnMut(AccountForStorage<'local>) -> Ret,
        ) -> Ret {
            callback(self.1[index].1.into())
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
    }

    impl AccountStorageEntry {
        fn add_account(&self, num_bytes: usize) {
            self.add_accounts(1, num_bytes)
        }
    }

    impl CurrentAncientAccountsFile {
        /// note this requires that 'slot_and_accounts_file' is Some
        fn id(&self) -> AccountsFileId {
            self.accounts_file().id()
        }
    }

    /// Helper macro to define accounts_db_test for both `AppendVec` and `HotStorage`.
    /// This macro supports creating both regular tests and tests that should panic.
    /// Usage:
    ///   For regular test, use the following syntax.
    ///     define_accounts_db_test!(TEST_NAME, |accounts_db| { TEST_BODY }); // regular test
    ///   For test that should panic, use the following syntax.
    ///     define_accounts_db_test!(TEST_NAME, panic = "PANIC_MSG", |accounts_db| { TEST_BODY });
    macro_rules! define_accounts_db_test {
        (@testfn $name:ident, $accounts_file_provider: ident, |$accounts_db:ident| $inner: tt) => {
                fn run_test($accounts_db: AccountsDb) {
                    $inner
                }
                let accounts_db =
                    AccountsDb::new_single_for_tests_with_provider($accounts_file_provider);
                run_test(accounts_db);

        };
        ($name:ident, |$accounts_db:ident| $inner: tt) => {
            #[test_case(AccountsFileProvider::AppendVec; "append_vec")]
            #[test_case(AccountsFileProvider::HotStorage; "hot_storage")]
            fn $name(accounts_file_provider: AccountsFileProvider) {
                define_accounts_db_test!(@testfn $name, accounts_file_provider, |$accounts_db| $inner);
            }
        };
        ($name:ident, panic = $panic_message:literal, |$accounts_db:ident| $inner: tt) => {
            #[test_case(AccountsFileProvider::AppendVec; "append_vec")]
            #[test_case(AccountsFileProvider::HotStorage; "hot_storage")]
            #[should_panic(expected = $panic_message)]
            fn $name(accounts_file_provider: AccountsFileProvider) {
                define_accounts_db_test!(@testfn $name, accounts_file_provider, |$accounts_db| $inner);
            }
        };
    }
    pub(crate) use define_accounts_db_test;

    fn run_generate_index_duplicates_within_slot_test(db: AccountsDb, reverse: bool) {
        let slot0 = 0;

        let pubkey = Pubkey::from([1; 32]);

        let append_vec = db.create_and_insert_store(slot0, 1000, "test");

        let mut account_small = AccountSharedData::default();
        account_small.set_data(vec![1]);
        account_small.set_lamports(1);
        let mut account_big = AccountSharedData::default();
        account_big.set_data(vec![5; 10]);
        account_big.set_lamports(2);
        assert_ne!(
            aligned_stored_size(account_big.data().len()),
            aligned_stored_size(account_small.data().len())
        );
        // same account twice with different data lens
        // Rules are the last one of each pubkey is the one that ends up in the index.
        let mut data = vec![(&pubkey, &account_big), (&pubkey, &account_small)];
        if reverse {
            data = data.into_iter().rev().collect();
        }
        let expected_accounts_data_len = data.last().unwrap().1.data().len();
        let expected_alive_bytes = aligned_stored_size(expected_accounts_data_len);
        let storable_accounts = (slot0, &data[..]);

        // construct append vec with account to generate an index from
        append_vec.accounts.append_accounts(&storable_accounts, 0);
        // append vecs set this at load
        append_vec
            .approx_store_count
            .store(data.len(), Ordering::Relaxed);

        let genesis_config = GenesisConfig::default();
        assert!(!db.accounts_index.contains(&pubkey));
        let result = db.generate_index(None, false, &genesis_config);
        // index entry should only contain a single entry for the pubkey since index cannot hold more than 1 entry per slot
        let entry = db.accounts_index.get_cloned(&pubkey).unwrap();
        assert_eq!(entry.slot_list.read().unwrap().len(), 1);
        if db.accounts_file_provider == AccountsFileProvider::AppendVec {
            // alive bytes doesn't match account size for tiered storage
            assert_eq!(append_vec.alive_bytes(), expected_alive_bytes);
        }
        // total # accounts in append vec
        assert_eq!(append_vec.approx_stored_count(), 2);
        // # alive accounts
        assert_eq!(append_vec.count(), 1);
        // all account data alive
        assert_eq!(
            result.accounts_data_len as usize, expected_accounts_data_len,
            "reverse: {reverse}"
        );
    }

    define_accounts_db_test!(test_generate_index_duplicates_within_slot, |db| {
        run_generate_index_duplicates_within_slot_test(db, false);
    });

    define_accounts_db_test!(test_generate_index_duplicates_within_slot_reverse, |db| {
        run_generate_index_duplicates_within_slot_test(db, true);
    });

    fn generate_sample_account_from_storage(i: u8) -> AccountFromStorage {
        // offset has to be 8 byte aligned
        let offset = (i as usize) * std::mem::size_of::<u64>();
        AccountFromStorage {
            index_info: AccountInfo::new(StorageLocation::AppendVec(i as u32, offset), i as u64),
            data_len: i as u64,
            pubkey: Pubkey::new_from_array([i; 32]),
        }
    }

    /// Reserve ancient storage size is not supported for TiredStorage
    #[test]
    fn test_sort_and_remove_dups() {
        // empty
        let mut test1 = vec![];
        let expected = test1.clone();
        AccountsDb::sort_and_remove_dups(&mut test1);
        assert_eq!(test1, expected);
        assert_eq!(test1, expected);
        // just 0
        let mut test1 = vec![generate_sample_account_from_storage(0)];
        let expected = test1.clone();
        AccountsDb::sort_and_remove_dups(&mut test1);
        assert_eq!(test1, expected);
        assert_eq!(test1, expected);
        // 0, 1
        let mut test1 = vec![
            generate_sample_account_from_storage(0),
            generate_sample_account_from_storage(1),
        ];
        let expected = test1.clone();
        AccountsDb::sort_and_remove_dups(&mut test1);
        assert_eq!(test1, expected);
        assert_eq!(test1, expected);
        // 1, 0. sort should reverse
        let mut test2 = vec![
            generate_sample_account_from_storage(1),
            generate_sample_account_from_storage(0),
        ];
        AccountsDb::sort_and_remove_dups(&mut test2);
        assert_eq!(test2, expected);
        assert_eq!(test2, expected);

        for insert_other_good in 0..2 {
            // 0 twice so it gets removed
            let mut test1 = vec![
                generate_sample_account_from_storage(0),
                generate_sample_account_from_storage(0),
            ];
            let mut expected = test1.clone();
            expected.truncate(1); // get rid of 1st duplicate
            test1.first_mut().unwrap().data_len = 2342342; // this one should be ignored, so modify the data_len so it will fail the compare below if it is used
            if insert_other_good < 2 {
                // insert another good one before or after the 2 bad ones
                test1.insert(insert_other_good, generate_sample_account_from_storage(1));
                // other good one should always be last since it is sorted after
                expected.push(generate_sample_account_from_storage(1));
            }
            AccountsDb::sort_and_remove_dups(&mut test1);
            assert_eq!(test1, expected);
            assert_eq!(test1, expected);
        }

        let mut test1 = [1, 0, 1, 0, 1u8]
            .into_iter()
            .map(generate_sample_account_from_storage)
            .collect::<Vec<_>>();
        test1.iter_mut().take(3).for_each(|entry| {
            entry.data_len = 2342342; // this one should be ignored, so modify the data_len so it will fail the compare below if it is used
            entry.index_info = AccountInfo::new(StorageLocation::Cached, 23434);
        });

        let expected = [0, 1u8]
            .into_iter()
            .map(generate_sample_account_from_storage)
            .collect::<Vec<_>>();
        AccountsDb::sort_and_remove_dups(&mut test1);
        assert_eq!(test1, expected);
        assert_eq!(test1, expected);
    }

    #[test]
    fn test_sort_and_remove_dups_random() {
        use rand::prelude::*;
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(1234);
        let accounts: Vec<_> =
            std::iter::repeat_with(|| generate_sample_account_from_storage(rng.gen::<u8>()))
                .take(1000)
                .collect();

        let mut accounts1 = accounts.clone();
        let num_dups1 = AccountsDb::sort_and_remove_dups(&mut accounts1);

        // Use BTreeMap to calculate sort and remove dups alternatively.
        let mut map = std::collections::BTreeMap::default();
        let mut num_dups2 = 0;
        for account in accounts.iter() {
            if map.insert(*account.pubkey(), *account).is_some() {
                num_dups2 += 1;
            }
        }
        let accounts2: Vec<_> = map.into_values().collect();
        assert_eq!(accounts1, accounts2);
        assert_eq!(num_dups1, num_dups2);
    }

    /// Reserve ancient storage size is not supported for TiredStorage
    #[test]
    fn test_create_ancient_accounts_file() {
        let ancient_append_vec_size = ancient_append_vecs::get_ancient_append_vec_capacity();
        let db = AccountsDb::new_single_for_tests();

        {
            // create an ancient appendvec from a small appendvec, the size of
            // the ancient appendvec should be the size of the ideal ancient
            // appendvec size.
            let mut current_ancient = CurrentAncientAccountsFile::default();
            let slot0 = 0;

            // there has to be an existing append vec at this slot for a new current ancient at the slot to make sense
            let _existing_append_vec = db.create_and_insert_store(slot0, 1000, "test");
            let _ = current_ancient.create_ancient_accounts_file(slot0, &db, 0);
            assert_eq!(
                current_ancient.accounts_file().capacity(),
                ancient_append_vec_size
            );
        }

        {
            // create an ancient appendvec from a large appendvec (bigger than
            // current ancient_append_vec_size), the ancient appendvec should be
            // the size of the bigger ancient appendvec size.
            let mut current_ancient = CurrentAncientAccountsFile::default();
            let slot1 = 1;
            // there has to be an existing append vec at this slot for a new current ancient at the slot to make sense
            let _existing_append_vec = db.create_and_insert_store(slot1, 1000, "test");
            let _ = current_ancient.create_ancient_accounts_file(
                slot1,
                &db,
                2 * ancient_append_vec_size as usize,
            );
            assert_eq!(
                current_ancient.accounts_file().capacity(),
                2 * ancient_append_vec_size
            );
        }
    }

    define_accounts_db_test!(test_maybe_unref_accounts_already_in_ancient, |db| {
        let slot0 = 0;
        let slot1 = 1;
        let available_bytes = 1_000_000;
        let mut current_ancient = CurrentAncientAccountsFile::default();

        // setup 'to_store'
        let pubkey = Pubkey::from([1; 32]);
        let account_size = 3;

        let account = AccountSharedData::default();

        let account_meta = AccountMeta {
            lamports: 1,
            owner: Pubkey::from([2; 32]),
            executable: false,
            rent_epoch: 0,
        };
        let offset = 3 * std::mem::size_of::<u64>();
        let hash = AccountHash(Hash::new(&[2; 32]));
        let stored_meta = StoredMeta {
            // global write version
            write_version_obsolete: 0,
            // key for the account
            pubkey,
            data_len: 43,
        };
        let account = StoredAccountMeta::AppendVec(AppendVecStoredAccountMeta {
            meta: &stored_meta,
            // account data
            account_meta: &account_meta,
            data: account.data(),
            offset,
            stored_size: account_size,
            hash: &hash,
        });
        let account_from_storage = AccountFromStorage::new(&account);
        let map_from_storage = vec![&account_from_storage];
        let alive_total_bytes = account.stored_size();
        let to_store =
            AccountsToStore::new(available_bytes, &map_from_storage, alive_total_bytes, slot0);
        // Done: setup 'to_store'

        // there has to be an existing append vec at this slot for a new current ancient at the slot to make sense
        let _existing_append_vec = db.create_and_insert_store(slot0, 1000, "test");
        {
            let _shrink_in_progress = current_ancient.create_ancient_accounts_file(slot0, &db, 0);
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
        let _shrink_in_progress = current_ancient.create_ancient_accounts_file(slot1, &db, 0);
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
    });

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
        let offset = 99 * std::mem::size_of::<u64>(); // offset needs to be 8 byte aligned
        let stored_size = 101;
        let hash = AccountHash(Hash::new_unique());
        let stored_account = StoredAccountMeta::AppendVec(AppendVecStoredAccountMeta {
            meta: &meta,
            account_meta: &account_meta,
            data: &data,
            offset,
            stored_size,
            hash: &hash,
        });
        let stored_account2 = StoredAccountMeta::AppendVec(AppendVecStoredAccountMeta {
            meta: &meta2,
            account_meta: &account_meta,
            data: &data,
            offset,
            stored_size,
            hash: &hash,
        });
        let stored_account3 = StoredAccountMeta::AppendVec(AppendVecStoredAccountMeta {
            meta: &meta3,
            account_meta: &account_meta,
            data: &data,
            offset,
            stored_size,
            hash: &hash,
        });
        let stored_account4 = StoredAccountMeta::AppendVec(AppendVecStoredAccountMeta {
            meta: &meta4,
            account_meta: &account_meta,
            data: &data,
            offset,
            stored_size,
            hash: &hash,
        });
        let mut existing_ancient_pubkeys = HashSet::default();
        let account_from_storage = AccountFromStorage::new(&stored_account);
        let accounts_from_storage = [&account_from_storage];
        // pubkey NOT in existing_ancient_pubkeys, so do NOT unref, but add to existing_ancient_pubkeys
        let unrefs = AccountsDb::get_keys_to_unref_ancient(
            &accounts_from_storage,
            &mut existing_ancient_pubkeys,
        );
        assert!(unrefs.is_empty());
        assert_eq!(
            existing_ancient_pubkeys.iter().collect::<Vec<_>>(),
            vec![&pubkey]
        );
        // pubkey already in existing_ancient_pubkeys, so DO unref
        let unrefs = AccountsDb::get_keys_to_unref_ancient(
            &accounts_from_storage,
            &mut existing_ancient_pubkeys,
        );
        assert_eq!(
            existing_ancient_pubkeys.iter().collect::<Vec<_>>(),
            vec![&pubkey]
        );
        assert_eq!(unrefs.iter().cloned().collect::<Vec<_>>(), vec![&pubkey]);
        // pubkey2 NOT in existing_ancient_pubkeys, so do NOT unref, but add to existing_ancient_pubkeys
        let account_from_storage2 = AccountFromStorage::new(&stored_account2);
        let accounts_from_storage = [&account_from_storage2];
        let unrefs = AccountsDb::get_keys_to_unref_ancient(
            &accounts_from_storage,
            &mut existing_ancient_pubkeys,
        );
        assert!(unrefs.is_empty());
        assert_eq!(
            existing_ancient_pubkeys.iter().sorted().collect::<Vec<_>>(),
            vec![&pubkey, &pubkey2]
                .into_iter()
                .sorted()
                .collect::<Vec<_>>()
        );
        // pubkey2 already in existing_ancient_pubkeys, so DO unref
        let unrefs = AccountsDb::get_keys_to_unref_ancient(
            &accounts_from_storage,
            &mut existing_ancient_pubkeys,
        );
        assert_eq!(
            existing_ancient_pubkeys.iter().sorted().collect::<Vec<_>>(),
            vec![&pubkey, &pubkey2]
                .into_iter()
                .sorted()
                .collect::<Vec<_>>()
        );
        assert_eq!(unrefs.iter().cloned().collect::<Vec<_>>(), vec![&pubkey2]);
        // pubkey3/4 NOT in existing_ancient_pubkeys, so do NOT unref, but add to existing_ancient_pubkeys
        let account_from_storage3 = AccountFromStorage::new(&stored_account3);
        let account_from_storage4 = AccountFromStorage::new(&stored_account4);
        let accounts_from_storage = [&account_from_storage3, &account_from_storage4];
        let unrefs = AccountsDb::get_keys_to_unref_ancient(
            &accounts_from_storage,
            &mut existing_ancient_pubkeys,
        );
        assert!(unrefs.is_empty());
        assert_eq!(
            existing_ancient_pubkeys.iter().sorted().collect::<Vec<_>>(),
            vec![&pubkey, &pubkey2, &pubkey3, &pubkey4]
                .into_iter()
                .sorted()
                .collect::<Vec<_>>()
        );
        // pubkey3/4 already in existing_ancient_pubkeys, so DO unref
        let unrefs = AccountsDb::get_keys_to_unref_ancient(
            &accounts_from_storage,
            &mut existing_ancient_pubkeys,
        );
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

    pub(crate) fn sample_storages_and_account_in_slot(
        slot: Slot,
        accounts: &AccountsDb,
    ) -> (
        Vec<Arc<AccountStorageEntry>>,
        Vec<CalculateHashIntermediate>,
    ) {
        let pubkey0 = Pubkey::from([0u8; 32]);
        let pubkey127 = Pubkey::from([0x7fu8; 32]);
        let pubkey128 = Pubkey::from([0x80u8; 32]);
        let pubkey255 = Pubkey::from([0xffu8; 32]);

        let mut raw_expected = vec![
            CalculateHashIntermediate {
                hash: AccountHash(Hash::default()),
                lamports: 1,
                pubkey: pubkey0,
            },
            CalculateHashIntermediate {
                hash: AccountHash(Hash::default()),
                lamports: 128,
                pubkey: pubkey127,
            },
            CalculateHashIntermediate {
                hash: AccountHash(Hash::default()),
                lamports: 129,
                pubkey: pubkey128,
            },
            CalculateHashIntermediate {
                hash: AccountHash(Hash::default()),
                lamports: 256,
                pubkey: pubkey255,
            },
        ];

        let expected_hashes = [
            AccountHash(Hash::from_str("EkyjPt4oL7KpRMEoAdygngnkhtVwCxqJ2MkwaGV4kUU4").unwrap()),
            AccountHash(Hash::from_str("4N7T4C2MK3GbHudqhfGsCyi2GpUU3roN6nhwViA41LYL").unwrap()),
            AccountHash(Hash::from_str("HzWMbUEnSfkrPiMdZeM6zSTdU5czEvGkvDcWBApToGC9").unwrap()),
            AccountHash(Hash::from_str("AsWzo1HphgrrgQ6V2zFUVDssmfaBipx2XfwGZRqcJjir").unwrap()),
        ];

        let mut raw_accounts = Vec::default();

        for i in 0..raw_expected.len() {
            raw_accounts.push(AccountSharedData::new(
                raw_expected[i].lamports,
                1,
                AccountSharedData::default().owner(),
            ));
            let hash = AccountsDb::hash_account(&raw_accounts[i], &raw_expected[i].pubkey);
            assert_eq!(hash, expected_hashes[i]);
            raw_expected[i].hash = hash;
        }

        let to_store = raw_accounts
            .iter()
            .zip(raw_expected.iter())
            .map(|(account, intermediate)| (&intermediate.pubkey, account))
            .collect::<Vec<_>>();

        accounts.store_for_tests(slot, &to_store[..]);
        accounts.add_root_and_flush_write_cache(slot);

        let (storages, slots) = accounts.get_snapshot_storages(..=slot);
        assert_eq!(storages.len(), slots.len());
        storages
            .iter()
            .zip(slots.iter())
            .for_each(|(storage, slot)| {
                assert_eq!(&storage.slot(), slot);
            });
        (storages, raw_expected)
    }

    pub(crate) fn sample_storages_and_accounts(
        accounts: &AccountsDb,
    ) -> (
        Vec<Arc<AccountStorageEntry>>,
        Vec<CalculateHashIntermediate>,
    ) {
        sample_storages_and_account_in_slot(1, accounts)
    }

    pub(crate) fn get_storage_refs(input: &[Arc<AccountStorageEntry>]) -> SortedStorages {
        SortedStorages::new(input)
    }

    define_accounts_db_test!(
        test_accountsdb_calculate_accounts_hash_from_storages_simple,
        |db| {
            let (storages, _size, _slot_expected) = sample_storage();

            let result = db.calculate_accounts_hash(
                &CalcAccountsHashConfig::default(),
                &get_storage_refs(&storages),
                HashStats::default(),
            );
            let expected_hash =
                Hash::from_str("GKot5hBsd81kMupNCXHaqbhv3huEbxAFMLnpcX2hniwn").unwrap();
            let expected_accounts_hash = AccountsHash(expected_hash);
            assert_eq!(result, (expected_accounts_hash, 0));
        }
    );

    define_accounts_db_test!(
        test_accountsdb_calculate_accounts_hash_from_storages,
        |db| {
            let (storages, raw_expected) = sample_storages_and_accounts(&db);
            let expected_hash = AccountsHasher::compute_merkle_root_loop(
                raw_expected.clone(),
                MERKLE_FANOUT,
                |item| &item.hash.0,
            );
            let sum = raw_expected.iter().map(|item| item.lamports).sum();
            let result = db.calculate_accounts_hash(
                &CalcAccountsHashConfig::default(),
                &get_storage_refs(&storages),
                HashStats::default(),
            );

            let expected_accounts_hash = AccountsHash(expected_hash);
            assert_eq!(result, (expected_accounts_hash, sum));
        }
    );

    fn sample_storage() -> (Vec<Arc<AccountStorageEntry>>, usize, Slot) {
        let (_temp_dirs, paths) = get_temp_accounts_paths(1).unwrap();
        let slot_expected: Slot = 0;
        let size: usize = 123;
        let data = AccountStorageEntry::new(
            &paths[0],
            slot_expected,
            0,
            size as u64,
            AccountsFileProvider::AppendVec,
        );

        let arc = Arc::new(data);
        let storages = vec![arc];
        (storages, size, slot_expected)
    }

    pub(crate) fn append_single_account_with_default_hash(
        storage: &AccountStorageEntry,
        pubkey: &Pubkey,
        account: &AccountSharedData,
        mark_alive: bool,
        add_to_index: Option<&AccountInfoAccountsIndex>,
    ) {
        let slot = storage.slot();
        let accounts = [(pubkey, account)];
        let slice = &accounts[..];
        let storable_accounts = (slot, slice);
        let stored_accounts_info = storage
            .accounts
            .append_accounts(&storable_accounts, 0)
            .unwrap();
        if mark_alive {
            // updates 'alive_bytes' on the storage
            storage.add_account(stored_accounts_info.size);
        }

        if let Some(index) = add_to_index {
            let account_info = AccountInfo::new(
                StorageLocation::AppendVec(storage.id(), stored_accounts_info.offsets[0]),
                account.lamports(),
            );
            index.upsert(
                slot,
                slot,
                pubkey,
                account,
                &AccountSecondaryIndexes::default(),
                account_info,
                &mut Vec::default(),
                UpsertReclaim::IgnoreReclaims,
            );
        }
    }

    fn append_sample_data_to_storage(
        storage: &AccountStorageEntry,
        pubkey: &Pubkey,
        mark_alive: bool,
        account_data_size: Option<u64>,
    ) {
        let acc = AccountSharedData::new(
            1,
            account_data_size.unwrap_or(48) as usize,
            AccountSharedData::default().owner(),
        );
        append_single_account_with_default_hash(storage, pubkey, &acc, mark_alive, None);
    }

    pub(crate) fn sample_storage_with_entries(
        tf: &TempFile,
        slot: Slot,
        pubkey: &Pubkey,
        mark_alive: bool,
    ) -> Arc<AccountStorageEntry> {
        sample_storage_with_entries_id(tf, slot, pubkey, 0, mark_alive, None)
    }

    fn sample_storage_with_entries_id_fill_percentage(
        tf: &TempFile,
        slot: Slot,
        pubkey: &Pubkey,
        id: AccountsFileId,
        mark_alive: bool,
        account_data_size: Option<u64>,
        fill_percentage: u64,
    ) -> Arc<AccountStorageEntry> {
        let (_temp_dirs, paths) = get_temp_accounts_paths(1).unwrap();
        let file_size = account_data_size.unwrap_or(123) * 100 / fill_percentage;
        let size_aligned: usize = aligned_stored_size(file_size as usize);
        let mut data = AccountStorageEntry::new(
            &paths[0],
            slot,
            id,
            size_aligned as u64,
            AccountsFileProvider::AppendVec,
        );
        let av = AccountsFile::AppendVec(AppendVec::new(
            &tf.path,
            true,
            (1024 * 1024).max(size_aligned),
        ));
        data.accounts = av;

        let arc = Arc::new(data);
        append_sample_data_to_storage(&arc, pubkey, mark_alive, account_data_size);
        arc
    }

    fn sample_storage_with_entries_id(
        tf: &TempFile,
        slot: Slot,
        pubkey: &Pubkey,
        id: AccountsFileId,
        mark_alive: bool,
        account_data_size: Option<u64>,
    ) -> Arc<AccountStorageEntry> {
        sample_storage_with_entries_id_fill_percentage(
            tf,
            slot,
            pubkey,
            id,
            mark_alive,
            account_data_size,
            100,
        )
    }

    define_accounts_db_test!(test_accountsdb_add_root, |db| {
        let key = Pubkey::default();
        let account0 = AccountSharedData::new(1, 0, &key);

        db.store_for_tests(0, &[(&key, &account0)]);
        db.add_root(0);
        let ancestors = vec![(1, 1)].into_iter().collect();
        assert_eq!(
            db.load_without_fixed_root(&ancestors, &key),
            Some((account0, 0))
        );
    });

    define_accounts_db_test!(test_accountsdb_latest_ancestor, |db| {
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
    });

    define_accounts_db_test!(test_accountsdb_latest_ancestor_with_root, |db| {
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
    });

    define_accounts_db_test!(test_accountsdb_root_one_slot, |db| {
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
    });

    define_accounts_db_test!(test_accountsdb_add_root_many, |db| {
        let mut pubkeys: Vec<Pubkey> = vec![];
        db.create_account(&mut pubkeys, 0, 100, 0, 0);
        for _ in 1..100 {
            let idx = thread_rng().gen_range(0..99);
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
            let idx = thread_rng().gen_range(0..99);
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
    });

    define_accounts_db_test!(test_accountsdb_count_stores, |db| {
        let mut pubkeys: Vec<Pubkey> = vec![];
        db.create_account(&mut pubkeys, 0, 2, DEFAULT_FILE_SIZE as usize / 3, 0);
        db.add_root_and_flush_write_cache(0);
        db.check_storage(0, 2);

        let pubkey = solana_sdk::pubkey::new_rand();
        let account = AccountSharedData::new(1, DEFAULT_FILE_SIZE as usize / 3, &pubkey);
        db.store_for_tests(1, &[(&pubkey, &account)]);
        db.store_for_tests(1, &[(&pubkeys[0], &account)]);
        // adding root doesn't change anything
        db.calculate_accounts_delta_hash(1);
        db.add_root_and_flush_write_cache(1);
        {
            let slot_0_store = &db.storage.get_slot_storage_entry(0).unwrap();
            let slot_1_store = &db.storage.get_slot_storage_entry(1).unwrap();
            assert_eq!(slot_0_store.count(), 2);
            assert_eq!(slot_1_store.count(), 2);
            assert_eq!(slot_0_store.approx_stored_count(), 2);
            assert_eq!(slot_1_store.approx_stored_count(), 2);
        }

        // overwrite old rooted account version; only the r_slot_0_stores.count() should be
        // decremented
        // slot 2 is not a root and should be ignored by clean
        db.store_for_tests(2, &[(&pubkeys[0], &account)]);
        db.clean_accounts_for_tests();
        {
            let slot_0_store = &db.storage.get_slot_storage_entry(0).unwrap();
            let slot_1_store = &db.storage.get_slot_storage_entry(1).unwrap();
            assert_eq!(slot_0_store.count(), 1);
            assert_eq!(slot_1_store.count(), 2);
            assert_eq!(slot_0_store.approx_stored_count(), 2);
            assert_eq!(slot_1_store.approx_stored_count(), 2);
        }
    });

    define_accounts_db_test!(test_accounts_unsquashed, |db0| {
        let key = Pubkey::default();

        // 1 token in the "root", i.e. db zero
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
    });

    fn run_test_remove_unrooted_slot(is_cached: bool, db: AccountsDb) {
        let unrooted_slot = 9;
        let unrooted_bank_id = 9;
        let key = Pubkey::default();
        let account0 = AccountSharedData::new(1, 0, &key);
        let ancestors = vec![(unrooted_slot, 1)].into_iter().collect();
        assert!(!db.accounts_index.contains(&key));
        if is_cached {
            db.store_cached((unrooted_slot, &[(&key, &account0)][..]), None);
        } else {
            db.store_for_tests(unrooted_slot, &[(&key, &account0)]);
        }
        assert!(db.get_bank_hash_stats(unrooted_slot).is_some());
        assert!(db.accounts_index.contains(&key));
        db.assert_load_account(unrooted_slot, key, 1);

        // Purge the slot
        db.remove_unrooted_slots(&[(unrooted_slot, unrooted_bank_id)]);
        assert!(db.load_without_fixed_root(&ancestors, &key).is_none());
        assert!(db.get_bank_hash_stats(unrooted_slot).is_none());
        assert!(db.accounts_cache.slot_cache(unrooted_slot).is_none());
        assert!(db.storage.get_slot_storage_entry(unrooted_slot).is_none());
        assert!(!db.accounts_index.contains(&key));

        // Test we can store for the same slot again and get the right information
        let account0 = AccountSharedData::new(2, 0, &key);
        db.store_for_tests(unrooted_slot, &[(&key, &account0)]);
        db.assert_load_account(unrooted_slot, key, 2);
    }

    define_accounts_db_test!(test_remove_unrooted_slot_cached, |db| {
        run_test_remove_unrooted_slot(true, db);
    });

    define_accounts_db_test!(test_remove_unrooted_slot_storage, |db| {
        run_test_remove_unrooted_slot(false, db);
    });

    fn update_accounts(accounts: &AccountsDb, pubkeys: &[Pubkey], slot: Slot, range: usize) {
        for _ in 1..1000 {
            let idx = thread_rng().gen_range(0..range);
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

    #[test]
    fn test_account_one() {
        let (_accounts_dirs, paths) = get_temp_accounts_paths(1).unwrap();
        let db = AccountsDb::new_for_tests(paths, &ClusterType::Development);
        let mut pubkeys: Vec<Pubkey> = vec![];
        db.create_account(&mut pubkeys, 0, 1, 0, 0);
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
        let db = AccountsDb::new_for_tests(paths, &ClusterType::Development);
        let mut pubkeys: Vec<Pubkey> = vec![];
        db.create_account(&mut pubkeys, 0, 100, 0, 0);
        db.check_accounts(&pubkeys, 0, 100, 1);
    }

    #[test]
    fn test_account_update() {
        let accounts = AccountsDb::new_single_for_tests();
        let mut pubkeys: Vec<Pubkey> = vec![];
        accounts.create_account(&mut pubkeys, 0, 100, 0, 0);
        update_accounts(&accounts, &pubkeys, 0, 99);
        accounts.add_root_and_flush_write_cache(0);
        accounts.check_storage(0, 100);
    }

    #[test]
    fn test_account_grow_many() {
        let (_accounts_dir, paths) = get_temp_accounts_paths(2).unwrap();
        let size = 4096;
        let accounts = AccountsDb {
            file_size: size,
            ..AccountsDb::new_for_tests(paths, &ClusterType::Development)
        };
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
        let mut all_slots = vec![];
        for slot_storage in accounts.storage.iter() {
            all_slots.push(slot_storage.0)
        }
        for slot in all_slots {
            *append_vec_histogram.entry(slot).or_insert(0) += 1;
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
                let store = &accounts.storage.get_slot_storage_entry(0).unwrap();
                assert_eq!(store.count(), 1);
                assert_eq!(store.status(), AccountStorageStatus::Available);
                continue;
            }

            let pubkey2 = solana_sdk::pubkey::new_rand();
            let account2 = AccountSharedData::new(1, DEFAULT_FILE_SIZE as usize / 2, &pubkey2);
            accounts.store_for_tests(0, &[(&pubkey2, &account2)]);

            if pass == 1 {
                accounts.add_root_and_flush_write_cache(0);
                assert_eq!(accounts.storage.len(), 1);
                let store = &accounts.storage.get_slot_storage_entry(0).unwrap();
                assert_eq!(store.count(), 2);
                assert_eq!(store.status(), AccountStorageStatus::Available);
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
                    let store = &accounts.storage.get_slot_storage_entry(0).unwrap();
                    assert_eq!(store.status(), status[0]);
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
        let accounts = AccountsDb::new_single_for_tests();
        let pubkey = solana_sdk::pubkey::new_rand();
        let account = AccountSharedData::new(1, 0, AccountSharedData::default().owner());
        //store an account
        accounts.store_for_tests(0, &[(&pubkey, &account)]);
        accounts.add_root_and_flush_write_cache(0);

        let ancestors = vec![(0, 0)].into_iter().collect();
        let id = accounts
            .accounts_index
            .get_with_and_then(
                &pubkey,
                Some(&ancestors),
                None,
                false,
                |(_slot, account_info)| account_info.store_id(),
            )
            .unwrap();
        accounts.calculate_accounts_delta_hash(0);

        //slot is still there, since gc is lazy
        assert_eq!(accounts.storage.get_slot_storage_entry(0).unwrap().id(), id);

        //store causes clean
        accounts.store_for_tests(1, &[(&pubkey, &account)]);

        // generate delta state for slot 1, so clean operates on it.
        accounts.calculate_accounts_delta_hash(1);

        //slot is gone
        accounts.print_accounts_stats("pre-clean");
        accounts.add_root_and_flush_write_cache(1);
        assert!(accounts.storage.get_slot_storage_entry(0).is_some());
        accounts.clean_accounts_for_tests();
        assert!(accounts.storage.get_slot_storage_entry(0).is_none());

        //new value is there
        let ancestors = vec![(1, 1)].into_iter().collect();
        assert_eq!(
            accounts.load_without_fixed_root(&ancestors, &pubkey),
            Some((account, 1))
        );
    }

    #[test]
    fn test_clean_zero_lamport_and_dead_slot() {
        solana_logger::setup();

        let accounts = AccountsDb::new_single_for_tests();
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
            .get_with_and_then(
                &pubkey1,
                Some(&ancestors),
                None,
                false,
                |(slot, account_info)| (slot, account_info),
            )
            .unwrap();
        let (slot2, account_info2) = accounts
            .accounts_index
            .get_with_and_then(
                &pubkey2,
                Some(&ancestors),
                None,
                false,
                |(slot, account_info)| (slot, account_info),
            )
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
        accounts.calculate_accounts_delta_hash(0);
        accounts.add_root_and_flush_write_cache(0);
        accounts.calculate_accounts_delta_hash(1);
        accounts.add_root_and_flush_write_cache(1);
        accounts.calculate_accounts_delta_hash(2);
        accounts.add_root_and_flush_write_cache(2);

        // Slot 1 should be removed, slot 0 cannot be removed because it still has
        // the latest update for pubkey 2
        accounts.clean_accounts_for_tests();
        assert!(accounts.storage.get_slot_storage_entry(0).is_some());
        assert!(accounts.storage.get_slot_storage_entry(1).is_none());

        // Slot 1 should be cleaned because all it's accounts are
        // zero lamports, and are not present in any other slot's
        // storage entries
        assert_eq!(accounts.alive_account_count_in_slot(1), 0);
    }

    #[test]
    fn test_clean_multiple_zero_lamport_decrements_index_ref_count() {
        solana_logger::setup();

        let accounts = AccountsDb::new_single_for_tests();
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
        accounts.calculate_accounts_delta_hash(0);
        accounts.add_root_and_flush_write_cache(0);
        accounts.calculate_accounts_delta_hash(1);
        accounts.add_root_and_flush_write_cache(1);
        accounts.calculate_accounts_delta_hash(2);
        accounts.add_root_and_flush_write_cache(2);

        // Account ref counts should match how many slots they were stored in
        // Account 1 = 3 slots; account 2 = 1 slot
        assert_eq!(accounts.accounts_index.ref_count_from_storage(&pubkey1), 3);
        assert_eq!(accounts.accounts_index.ref_count_from_storage(&pubkey2), 1);

        accounts.clean_accounts_for_tests();
        // Slots 0 and 1 should each have been cleaned because all of their
        // accounts are zero lamports
        assert!(accounts.storage.get_slot_storage_entry(0).is_none());
        assert!(accounts.storage.get_slot_storage_entry(1).is_none());
        // Slot 2 only has a zero lamport account as well. But, calc_delete_dependencies()
        // should exclude slot 2 from the clean due to changes in other slots
        assert!(accounts.storage.get_slot_storage_entry(2).is_some());
        // Index ref counts should be consistent with the slot stores. Account 1 ref count
        // should be 1 since slot 2 is the only alive slot; account 2 should have a ref
        // count of 0 due to slot 0 being dead
        assert_eq!(accounts.accounts_index.ref_count_from_storage(&pubkey1), 1);
        assert_eq!(accounts.accounts_index.ref_count_from_storage(&pubkey2), 0);

        accounts.clean_accounts_for_tests();
        // Slot 2 will now be cleaned, which will leave account 1 with a ref count of 0
        assert!(accounts.storage.get_slot_storage_entry(2).is_none());
        assert_eq!(accounts.accounts_index.ref_count_from_storage(&pubkey1), 0);
    }

    #[test]
    fn test_clean_zero_lamport_and_old_roots() {
        solana_logger::setup();

        let accounts = AccountsDb::new_single_for_tests();
        let pubkey = solana_sdk::pubkey::new_rand();
        let account = AccountSharedData::new(1, 0, AccountSharedData::default().owner());
        let zero_lamport_account =
            AccountSharedData::new(0, 0, AccountSharedData::default().owner());

        // Store a zero-lamport account
        accounts.store_for_tests(0, &[(&pubkey, &account)]);
        accounts.store_for_tests(1, &[(&pubkey, &zero_lamport_account)]);

        // Simulate rooting the zero-lamport account, should be a
        // candidate for cleaning
        accounts.calculate_accounts_delta_hash(0);
        accounts.add_root_and_flush_write_cache(0);
        accounts.calculate_accounts_delta_hash(1);
        accounts.add_root_and_flush_write_cache(1);

        // Slot 0 should be removed, and
        // zero-lamport account should be cleaned
        accounts.clean_accounts_for_tests();

        assert!(accounts.storage.get_slot_storage_entry(0).is_none());
        assert!(accounts.storage.get_slot_storage_entry(1).is_none());

        // Slot 0 should be cleaned because all it's accounts have been
        // updated in the rooted slot 1
        assert_eq!(accounts.alive_account_count_in_slot(0), 0);

        // Slot 1 should be cleaned because all it's accounts are
        // zero lamports, and are not present in any other slot's
        // storage entries
        assert_eq!(accounts.alive_account_count_in_slot(1), 0);

        // zero lamport account, should no longer exist in accounts index
        // because it has been removed
        assert!(!accounts.accounts_index.contains_with(&pubkey, None, None));
    }

    #[test]
    fn test_clean_old_with_normal_account() {
        solana_logger::setup();

        let accounts = AccountsDb::new_single_for_tests();
        let pubkey = solana_sdk::pubkey::new_rand();
        let account = AccountSharedData::new(1, 0, AccountSharedData::default().owner());
        //store an account
        accounts.store_for_tests(0, &[(&pubkey, &account)]);
        accounts.store_for_tests(1, &[(&pubkey, &account)]);

        // simulate slots are rooted after while
        accounts.calculate_accounts_delta_hash(0);
        accounts.add_root_and_flush_write_cache(0);
        accounts.calculate_accounts_delta_hash(1);
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

        let accounts = AccountsDb::new_single_for_tests();
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
        accounts.calculate_accounts_delta_hash(0);
        accounts.add_root_and_flush_write_cache(0);
        accounts.calculate_accounts_delta_hash(1);
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

        let mut accounts = AccountsDb {
            account_indexes: spl_token_mint_index_enabled(),
            ..AccountsDb::new_single_for_tests()
        };
        let pubkey1 = solana_sdk::pubkey::new_rand();
        let pubkey2 = solana_sdk::pubkey::new_rand();

        // Set up account to be added to secondary index
        let mint_key = Pubkey::new_unique();
        let mut account_data_with_mint =
            vec![0; solana_inline_spl::token::Account::get_packed_len()];
        account_data_with_mint[..PUBKEY_BYTES].clone_from_slice(&(mint_key.to_bytes()));

        let mut normal_account = AccountSharedData::new(1, 0, AccountSharedData::default().owner());
        normal_account.set_owner(solana_inline_spl::token::id());
        normal_account.set_data(account_data_with_mint.clone());
        let mut zero_account = AccountSharedData::new(0, 0, AccountSharedData::default().owner());
        zero_account.set_owner(solana_inline_spl::token::id());
        zero_account.set_data(account_data_with_mint);

        //store an account
        accounts.store_for_tests(0, &[(&pubkey1, &normal_account)]);
        accounts.store_for_tests(0, &[(&pubkey1, &normal_account)]);
        accounts.store_for_tests(1, &[(&pubkey1, &zero_account)]);
        accounts.store_for_tests(0, &[(&pubkey2, &normal_account)]);
        accounts.store_for_tests(2, &[(&pubkey2, &normal_account)]);

        //simulate slots are rooted after while
        accounts.calculate_accounts_delta_hash(0);
        accounts.add_root_and_flush_write_cache(0);
        accounts.calculate_accounts_delta_hash(1);
        accounts.add_root_and_flush_write_cache(1);
        accounts.calculate_accounts_delta_hash(2);
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
        assert!(!accounts.accounts_index.contains_with(&pubkey1, None, None));

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

        let accounts = AccountsDb::new_single_for_tests();
        let pubkey = solana_sdk::pubkey::new_rand();
        let account = AccountSharedData::new(1, 0, AccountSharedData::default().owner());
        let zero_account = AccountSharedData::new(0, 0, AccountSharedData::default().owner());

        // store an account, make it a zero lamport account
        // in slot 1
        accounts.store_for_tests(0, &[(&pubkey, &account)]);
        accounts.store_for_tests(1, &[(&pubkey, &zero_account)]);

        // simulate slots are rooted after while
        accounts.calculate_accounts_delta_hash(0);
        accounts.add_root_and_flush_write_cache(0);
        accounts.calculate_accounts_delta_hash(1);
        accounts.add_root_and_flush_write_cache(1);

        // Only clean up to account 0, should not purge slot 0 based on
        // updates in later slots in slot 1
        assert_eq!(accounts.alive_account_count_in_slot(0), 1);
        assert_eq!(accounts.alive_account_count_in_slot(1), 1);
        accounts.clean_accounts(Some(0), false, &EpochSchedule::default());
        assert_eq!(accounts.alive_account_count_in_slot(0), 1);
        assert_eq!(accounts.alive_account_count_in_slot(1), 1);
        assert!(accounts.accounts_index.contains_with(&pubkey, None, None));

        // Now the account can be cleaned up
        accounts.clean_accounts(Some(1), false, &EpochSchedule::default());
        assert_eq!(accounts.alive_account_count_in_slot(0), 0);
        assert_eq!(accounts.alive_account_count_in_slot(1), 0);

        // The zero lamport account, should no longer exist in accounts index
        // because it has been removed
        assert!(!accounts.accounts_index.contains_with(&pubkey, None, None));
    }

    #[test]
    fn test_uncleaned_roots_with_account() {
        solana_logger::setup();

        let accounts = AccountsDb::new_single_for_tests();
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

        let accounts = AccountsDb::new_single_for_tests();

        assert_eq!(accounts.accounts_index.uncleaned_roots_len(), 0);

        // simulate slots are rooted after while
        accounts.add_root_and_flush_write_cache(0);
        assert_eq!(accounts.accounts_index.uncleaned_roots_len(), 1);

        //now uncleaned roots are cleaned up
        accounts.clean_accounts_for_tests();
        assert_eq!(accounts.accounts_index.uncleaned_roots_len(), 0);
    }

    fn assert_no_stores(accounts: &AccountsDb, slot: Slot) {
        let store = accounts.storage.get_slot_storage_entry(slot);
        assert!(store.is_none());
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
        accounts.calculate_accounts_delta_hash(0);
        accounts.add_root_and_flush_write_cache(0);

        // Step A
        let mut current_slot = 1;
        accounts.store_for_tests(current_slot, &[(&pubkey, &account)]);
        // Store another live account to slot 1 which will prevent any purge
        // since the store count will not be zero
        accounts.store_for_tests(current_slot, &[(&pubkey2, &account2)]);
        accounts.calculate_accounts_delta_hash(current_slot);
        accounts.add_root_and_flush_write_cache(current_slot);
        let (slot1, account_info1) = accounts
            .accounts_index
            .get_with_and_then(&pubkey, None, None, false, |(slot, account_info)| {
                (slot, account_info)
            })
            .unwrap();
        let (slot2, account_info2) = accounts
            .accounts_index
            .get_with_and_then(&pubkey2, None, None, false, |(slot, account_info)| {
                (slot, account_info)
            })
            .unwrap();
        assert_eq!(slot1, current_slot);
        assert_eq!(slot1, slot2);
        assert_eq!(account_info1.store_id(), account_info2.store_id());

        // Step B
        current_slot += 1;
        let zero_lamport_slot = current_slot;
        accounts.store_for_tests(current_slot, &[(&pubkey, &zero_lamport_account)]);
        accounts.calculate_accounts_delta_hash(current_slot);
        accounts.add_root_and_flush_write_cache(current_slot);

        accounts.assert_load_account(current_slot, pubkey, zero_lamport);

        current_slot += 1;
        accounts.calculate_accounts_delta_hash(current_slot);
        accounts.add_root_and_flush_write_cache(current_slot);

        accounts.print_accounts_stats("pre_purge");

        accounts.clean_accounts_for_tests();

        accounts.print_accounts_stats("post_purge");

        // The earlier entry for pubkey in the account index is purged,
        let (slot_list_len, index_slot) = {
            let account_entry = accounts.accounts_index.get_cloned(&pubkey).unwrap();
            let slot_list = account_entry.slot_list.read().unwrap();
            (slot_list.len(), slot_list[0].0)
        };
        assert_eq!(slot_list_len, 1);
        // Zero lamport entry was not the one purged
        assert_eq!(index_slot, zero_lamport_slot);
        // The ref count should still be 2 because no slots were purged
        assert_eq!(accounts.ref_count_for_pubkey(&pubkey), 2);

        // storage for slot 1 had 2 accounts, now has 1 after pubkey 1
        // was reclaimed
        accounts.check_storage(1, 1);
        // storage for slot 2 had 1 accounts, now has 1
        accounts.check_storage(2, 1);
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
        accounts.store_for_tests(current_slot, &[(&pubkey, &account)]);
        accounts.calculate_accounts_delta_hash(current_slot);
        accounts.add_root_and_flush_write_cache(current_slot);

        current_slot += 1;
        accounts.store_for_tests(current_slot, &[(&pubkey, &zero_lamport_account)]);
        accounts.calculate_accounts_delta_hash(current_slot);
        accounts.add_root_and_flush_write_cache(current_slot);

        accounts.assert_load_account(current_slot, pubkey, zero_lamport);

        // Otherwise slot 2 will not be removed
        current_slot += 1;
        accounts.calculate_accounts_delta_hash(current_slot);
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
        assert!(!accounts.accounts_index.contains(&pubkey));

        // slot 1 & 2 should not have any stores
        assert_no_stores(&accounts, 1);
        assert_no_stores(&accounts, 2);
    }

    #[test]
    #[ignore]
    fn test_store_account_stress() {
        let slot = 42;
        let num_threads = 2;

        let min_file_bytes = std::mem::size_of::<StoredMeta>() + std::mem::size_of::<AccountMeta>();

        let db = Arc::new(AccountsDb {
            file_size: min_file_bytes as u64,
            ..AccountsDb::new_single_for_tests()
        });

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
                            let account_bal = thread_rng().gen_range(1..99);
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
        let db = AccountsDb::new_single_for_tests();
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
        let purge_keys = [(key1, slots)];
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
        let db = AccountsDb::new_single_for_tests();

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
        let offset = 99 * std::mem::size_of::<u64>(); // offset needs to be 8 byte aligned
        let stored_size = 101;
        let hash = AccountHash(Hash::new_unique());
        let stored_account = StoredAccountMeta::AppendVec(AppendVecStoredAccountMeta {
            meta: &meta,
            account_meta: &account_meta,
            data: &data,
            offset,
            stored_size,
            hash: &hash,
        });
        assert!(accounts_equal(&account, &stored_account));
    }

    /// A place holder stored size for a cached entry. We don't need to store the size for cached entries, but we have to pass something.
    /// stored size is only used for shrinking. We don't shrink items in the write cache.
    const CACHE_VIRTUAL_STORED_SIZE: StoredSize = 0;

    #[test]
    fn test_hash_stored_account() {
        // Number are just sequential.
        let meta = StoredMeta {
            write_version_obsolete: 0x09_0a_0b_0c_0d_0e_0f_10,
            data_len: 0x11_12_13_14_15_16_17_18,
            pubkey: Pubkey::from([
                0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26,
                0x27, 0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x2d, 0x2e, 0x2f, 0x30, 0x31, 0x32, 0x33, 0x34,
                0x35, 0x36, 0x37, 0x38,
            ]),
        };
        let account_meta = AccountMeta {
            lamports: 0x39_3a_3b_3c_3d_3e_3f_40,
            rent_epoch: 0x41_42_43_44_45_46_47_48,
            owner: Pubkey::from([
                0x49, 0x4a, 0x4b, 0x4c, 0x4d, 0x4e, 0x4f, 0x50, 0x51, 0x52, 0x53, 0x54, 0x55, 0x56,
                0x57, 0x58, 0x59, 0x5a, 0x5b, 0x5c, 0x5d, 0x5e, 0x5f, 0x60, 0x61, 0x62, 0x63, 0x64,
                0x65, 0x66, 0x67, 0x68,
            ]),
            executable: false,
        };
        const ACCOUNT_DATA_LEN: usize = 3;
        let data: [u8; ACCOUNT_DATA_LEN] = [0x69, 0x6a, 0x6b];
        let offset: usize = 0x6c_6d_6e_6f_70_71_72_73;
        let hash = AccountHash(Hash::from([
            0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x7b, 0x7c, 0x7d, 0x7e, 0x7f, 0x80, 0x81,
            0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8a, 0x8b, 0x8c, 0x8d, 0x8e, 0x8f,
            0x90, 0x91, 0x92, 0x93,
        ]));

        let stored_account = StoredAccountMeta::AppendVec(AppendVecStoredAccountMeta {
            meta: &meta,
            account_meta: &account_meta,
            data: &data,
            offset,
            stored_size: CACHE_VIRTUAL_STORED_SIZE as usize,
            hash: &hash,
        });
        let account = stored_account.to_account_shared_data();

        let expected_account_hash =
            AccountHash(Hash::from_str("4xuaE8UfH8EYsPyDZvJXUScoZSyxUJf2BpzVMLTFh497").unwrap());

        assert_eq!(
            AccountsDb::hash_account(&stored_account, stored_account.pubkey(),),
            expected_account_hash,
            "StoredAccountMeta's data layout might be changed; update hashing if needed."
        );
        assert_eq!(
            AccountsDb::hash_account(&account, stored_account.pubkey(),),
            expected_account_hash,
            "Account-based hashing must be consistent with StoredAccountMeta-based one."
        );
    }

    #[test]
    fn test_bank_hash_stats() {
        solana_logger::setup();
        let db = AccountsDb::new_single_for_tests();

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

        let stats = db.get_bank_hash_stats(some_slot).unwrap();
        assert_eq!(stats.num_updated_accounts, 1);
        assert_eq!(stats.num_removed_accounts, 1);
        assert_eq!(stats.num_lamports_stored, 1);
        assert_eq!(stats.total_data_len, 2 * some_data_len as u64);
        assert_eq!(stats.num_executable_accounts, 1);
    }

    // something we can get a ref to
    lazy_static! {
        pub static ref EPOCH_SCHEDULE: EpochSchedule = EpochSchedule::default();
        pub static ref RENT_COLLECTOR: RentCollector = RentCollector::default();
    }

    impl<'a> CalcAccountsHashConfig<'a> {
        pub(crate) fn default() -> Self {
            Self {
                use_bg_thread_pool: false,
                ancestors: None,
                epoch_schedule: &EPOCH_SCHEDULE,
                rent_collector: &RENT_COLLECTOR,
                store_detailed_debug_info_on_failure: false,
            }
        }
    }

    #[test]
    fn test_verify_accounts_hash() {
        solana_logger::setup();
        let db = AccountsDb::new_single_for_tests();

        let key = solana_sdk::pubkey::new_rand();
        let some_data_len = 0;
        let some_slot: Slot = 0;
        let account = AccountSharedData::new(1, some_data_len, &key);
        let ancestors = vec![(some_slot, 0)].into_iter().collect();
        let epoch_schedule = EpochSchedule::default();
        let rent_collector = RentCollector::default();

        db.store_for_tests(some_slot, &[(&key, &account)]);
        db.add_root_and_flush_write_cache(some_slot);
        let (_, capitalization) =
            db.update_accounts_hash_for_tests(some_slot, &ancestors, true, true);

        let config = VerifyAccountsHashAndLamportsConfig::new_for_test(
            &ancestors,
            &epoch_schedule,
            &rent_collector,
        );

        assert_matches!(
            db.verify_accounts_hash_and_lamports_for_tests(some_slot, 1, config.clone()),
            Ok(_)
        );

        db.accounts_hashes.lock().unwrap().remove(&some_slot);

        assert_matches!(
            db.verify_accounts_hash_and_lamports_for_tests(some_slot, 1, config.clone()),
            Err(AccountsHashVerificationError::MissingAccountsHash)
        );

        db.set_accounts_hash(
            some_slot,
            (AccountsHash(Hash::new(&[0xca; HASH_BYTES])), capitalization),
        );

        assert_matches!(
            db.verify_accounts_hash_and_lamports_for_tests(some_slot, 1, config),
            Err(AccountsHashVerificationError::MismatchedAccountsHash)
        );
    }

    #[test]
    fn test_verify_bank_capitalization() {
        for pass in 0..2 {
            solana_logger::setup();
            let db = AccountsDb::new_single_for_tests();

            let key = solana_sdk::pubkey::new_rand();
            let some_data_len = 0;
            let some_slot: Slot = 0;
            let account = AccountSharedData::new(1, some_data_len, &key);
            let ancestors = vec![(some_slot, 0)].into_iter().collect();
            let epoch_schedule = EpochSchedule::default();
            let rent_collector = RentCollector::default();
            let config = VerifyAccountsHashAndLamportsConfig::new_for_test(
                &ancestors,
                &epoch_schedule,
                &rent_collector,
            );

            db.store_for_tests(some_slot, &[(&key, &account)]);
            if pass == 0 {
                db.add_root_and_flush_write_cache(some_slot);
                db.update_accounts_hash_for_tests(some_slot, &ancestors, true, true);

                assert_matches!(
                    db.verify_accounts_hash_and_lamports_for_tests(some_slot, 1, config.clone()),
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
                db.verify_accounts_hash_and_lamports_for_tests(some_slot, 2, config.clone()),
                Ok(_)
            );

            assert_matches!(
                db.verify_accounts_hash_and_lamports_for_tests(some_slot, 10, config),
                Err(AccountsHashVerificationError::MismatchedTotalLamports(expected, actual)) if expected == 2 && actual == 10
            );
        }
    }

    #[test]
    fn test_verify_accounts_hash_no_account() {
        solana_logger::setup();
        let db = AccountsDb::new_single_for_tests();

        let some_slot: Slot = 0;
        let ancestors = vec![(some_slot, 0)].into_iter().collect();

        db.add_root(some_slot);
        db.update_accounts_hash_for_tests(some_slot, &ancestors, true, true);

        let epoch_schedule = EpochSchedule::default();
        let rent_collector = RentCollector::default();
        let config = VerifyAccountsHashAndLamportsConfig::new_for_test(
            &ancestors,
            &epoch_schedule,
            &rent_collector,
        );

        assert_matches!(
            db.verify_accounts_hash_and_lamports_for_tests(some_slot, 0, config),
            Ok(_)
        );
    }

    #[test]
    fn test_verify_accounts_hash_bad_account_hash() {
        solana_logger::setup();
        let db = AccountsDb::new_single_for_tests();

        let key = Pubkey::default();
        let some_data_len = 0;
        let some_slot: Slot = 0;
        let account = AccountSharedData::new(1, some_data_len, &key);
        let ancestors = vec![(some_slot, 0)].into_iter().collect();

        let accounts = &[(&key, &account)][..];
        db.update_accounts_hash_for_tests(some_slot, &ancestors, false, false);

        // provide bogus account hashes
        db.store_accounts_unfrozen(
            (some_slot, accounts),
            &StoreTo::Storage(&db.find_storage_candidate(some_slot)),
            None,
            StoreReclaims::Default,
            UpdateIndexThreadSelection::PoolWithThreshold,
        );
        db.add_root(some_slot);

        let epoch_schedule = EpochSchedule::default();
        let rent_collector = RentCollector::default();
        let config = VerifyAccountsHashAndLamportsConfig::new_for_test(
            &ancestors,
            &epoch_schedule,
            &rent_collector,
        );

        assert_matches!(
            db.verify_accounts_hash_and_lamports_for_tests(some_slot, 1, config),
            Err(AccountsHashVerificationError::MismatchedAccountsHash)
        );
    }

    #[test]
    fn test_storage_finder() {
        solana_logger::setup();
        let db = AccountsDb {
            file_size: 16 * 1024,
            ..AccountsDb::new_single_for_tests()
        };
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
        let db = AccountsDb::new_single_for_tests();
        assert!(db.get_snapshot_storages(..=0).0.is_empty());
    }

    #[test]
    fn test_get_snapshot_storages_only_older_than_or_equal_to_snapshot_slot() {
        let db = AccountsDb::new_single_for_tests();

        let key = Pubkey::default();
        let account = AccountSharedData::new(1, 0, &key);
        let before_slot = 0;
        let base_slot = before_slot + 1;
        let after_slot = base_slot + 1;

        db.store_for_tests(base_slot, &[(&key, &account)]);
        db.add_root_and_flush_write_cache(base_slot);
        assert!(db.get_snapshot_storages(..=before_slot).0.is_empty());

        assert_eq!(1, db.get_snapshot_storages(..=base_slot).0.len());
        assert_eq!(1, db.get_snapshot_storages(..=after_slot).0.len());
    }

    #[test]
    fn test_get_snapshot_storages_only_non_empty() {
        for pass in 0..2 {
            let db = AccountsDb::new_single_for_tests();

            let key = Pubkey::default();
            let account = AccountSharedData::new(1, 0, &key);
            let base_slot = 0;
            let after_slot = base_slot + 1;

            db.store_for_tests(base_slot, &[(&key, &account)]);
            if pass == 0 {
                db.add_root_and_flush_write_cache(base_slot);
                db.storage.remove(&base_slot, false);
                assert!(db.get_snapshot_storages(..=after_slot).0.is_empty());
                continue;
            }

            db.store_for_tests(base_slot, &[(&key, &account)]);
            db.add_root_and_flush_write_cache(base_slot);
            assert_eq!(1, db.get_snapshot_storages(..=after_slot).0.len());
        }
    }

    #[test]
    fn test_get_snapshot_storages_only_roots() {
        let db = AccountsDb::new_single_for_tests();

        let key = Pubkey::default();
        let account = AccountSharedData::new(1, 0, &key);
        let base_slot = 0;
        let after_slot = base_slot + 1;

        db.store_for_tests(base_slot, &[(&key, &account)]);
        assert!(db.get_snapshot_storages(..=after_slot).0.is_empty());

        db.add_root_and_flush_write_cache(base_slot);
        assert_eq!(1, db.get_snapshot_storages(..=after_slot).0.len());
    }

    #[test]
    fn test_get_snapshot_storages_exclude_empty() {
        let db = AccountsDb::new_single_for_tests();

        let key = Pubkey::default();
        let account = AccountSharedData::new(1, 0, &key);
        let base_slot = 0;
        let after_slot = base_slot + 1;

        db.store_for_tests(base_slot, &[(&key, &account)]);
        db.add_root_and_flush_write_cache(base_slot);
        assert_eq!(1, db.get_snapshot_storages(..=after_slot).0.len());

        db.storage
            .get_slot_storage_entry(0)
            .unwrap()
            .remove_accounts(0, true, 1);
        assert!(db.get_snapshot_storages(..=after_slot).0.is_empty());
    }

    #[test]
    fn test_get_snapshot_storages_with_base_slot() {
        let db = AccountsDb::new_single_for_tests();

        let key = Pubkey::default();
        let account = AccountSharedData::new(1, 0, &key);

        let slot = 10;
        db.store_for_tests(slot, &[(&key, &account)]);
        db.add_root_and_flush_write_cache(slot);
        assert_eq!(0, db.get_snapshot_storages(slot + 1..=slot + 1).0.len());
        assert_eq!(1, db.get_snapshot_storages(slot..=slot + 1).0.len());
    }

    define_accounts_db_test!(
        test_storage_remove_account_double_remove,
        panic = "double remove of account in slot: 0/store: 0!!",
        |accounts| {
            let pubkey = solana_sdk::pubkey::new_rand();
            let account = AccountSharedData::new(1, 0, AccountSharedData::default().owner());
            accounts.store_for_tests(0, &[(&pubkey, &account)]);
            accounts.add_root_and_flush_write_cache(0);
            let storage_entry = accounts.storage.get_slot_storage_entry(0).unwrap();
            storage_entry.remove_accounts(0, true, 1);
            storage_entry.remove_accounts(0, true, 1);
        }
    );

    fn do_full_clean_refcount(mut accounts: AccountsDb, store1_first: bool, store_size: u64) {
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
        accounts.file_size = store_size;

        // A: Initialize AccountsDb with pubkey1 and pubkey2
        current_slot += 1;
        if store1_first {
            accounts.store_for_tests(current_slot, &[(&pubkey1, &account)]);
            accounts.store_for_tests(current_slot, &[(&pubkey2, &account)]);
        } else {
            accounts.store_for_tests(current_slot, &[(&pubkey2, &account)]);
            accounts.store_for_tests(current_slot, &[(&pubkey1, &account)]);
        }
        accounts.calculate_accounts_delta_hash(current_slot);
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
        accounts.calculate_accounts_delta_hash(current_slot);
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
        accounts.calculate_accounts_delta_hash(current_slot);

        info!("post C");

        accounts.print_accounts_stats("Post-C");

        // D: Make all keys 0-lamport, cleans all keys
        current_slot += 1;
        assert_eq!(3, accounts.ref_count_for_pubkey(&pubkey1));
        accounts.store_for_tests(current_slot, &[(&pubkey1, &zero_lamport_account)]);
        accounts.store_for_tests(current_slot, &[(&pubkey2, &zero_lamport_account)]);
        accounts.store_for_tests(current_slot, &[(&pubkey3, &zero_lamport_account)]);

        let snapshot_stores = accounts.get_snapshot_storages(..=current_slot).0;
        let total_accounts: usize = snapshot_stores.iter().map(|s| s.accounts_count()).sum();
        assert!(!snapshot_stores.is_empty());
        assert!(total_accounts > 0);

        info!("post D");
        accounts.print_accounts_stats("Post-D");

        accounts.calculate_accounts_delta_hash(current_slot);
        accounts.add_root_and_flush_write_cache(current_slot);
        accounts.clean_accounts_for_tests();

        accounts.print_accounts_stats("Post-D clean");

        let total_accounts_post_clean: usize =
            snapshot_stores.iter().map(|s| s.accounts_count()).sum();
        assert_eq!(total_accounts, total_accounts_post_clean);

        // should clean all 3 pubkeys
        assert_eq!(accounts.ref_count_for_pubkey(&pubkey1), 0);
        assert_eq!(accounts.ref_count_for_pubkey(&pubkey2), 0);
        assert_eq!(accounts.ref_count_for_pubkey(&pubkey3), 0);
    }

    // Setup 3 scenarios which try to differentiate between pubkey1 being in an
    // Available slot or a Full slot which would cause a different reset behavior
    // when pubkey1 is cleaned and therefore cause the ref count to be incorrect
    // preventing a removal of that key.
    //
    // do stores with a 4mb size so only 1 store is created per slot
    define_accounts_db_test!(test_full_clean_refcount_no_first_4m, |accounts| {
        do_full_clean_refcount(accounts, false, 4 * 1024 * 1024);
    });

    // do stores with a 4k size and store pubkey1 first
    define_accounts_db_test!(test_full_clean_refcount_no_first_4k, |accounts| {
        do_full_clean_refcount(accounts, false, 4 * 1024);
    });

    // do stores with a 4k size and store pubkey1 2nd
    define_accounts_db_test!(test_full_clean_refcount_first_4k, |accounts| {
        do_full_clean_refcount(accounts, true, 4 * 1024);
    });

    #[test]
    fn test_clean_stored_dead_slots_empty() {
        let accounts = AccountsDb::new_single_for_tests();
        let mut dead_slots = IntSet::default();
        dead_slots.insert(10);
        accounts.clean_stored_dead_slots(&dead_slots, None, &HashSet::default());
    }

    #[test]
    fn test_shrink_all_slots_none() {
        let epoch_schedule = EpochSchedule::default();
        for startup in &[false, true] {
            let accounts = AccountsDb::new_single_for_tests();

            for _ in 0..10 {
                accounts.shrink_candidate_slots(&epoch_schedule);
            }

            accounts.shrink_all_slots(*startup, &EpochSchedule::default(), None);
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
        accounts.calculate_accounts_delta_hash(current_slot);
        accounts.add_root_and_flush_write_cache(current_slot);

        current_slot += 1;
        let pubkey_count_after_shrink = 25000;
        let updated_pubkeys = &pubkeys[0..pubkey_count - pubkey_count_after_shrink];

        for pubkey in updated_pubkeys {
            accounts.store_for_tests(current_slot, &[(pubkey, &account)]);
        }
        accounts.calculate_accounts_delta_hash(current_slot);
        accounts.add_root_and_flush_write_cache(current_slot);
        accounts.clean_accounts_for_tests();

        assert_eq!(
            pubkey_count,
            accounts.all_account_count_in_accounts_file(shrink_slot)
        );

        // Only, try to shrink stale slots, nothing happens because shrink ratio
        // is not small enough to do a shrink
        // Note this shrink ratio had to change because we are WAY over-allocating append vecs when we flush the write cache at the moment.
        accounts.shrink_ratio = AccountShrinkThreshold::TotalSpace { shrink_ratio: 0.4 };
        accounts.shrink_candidate_slots(&EpochSchedule::default());
        assert_eq!(
            pubkey_count,
            accounts.all_account_count_in_accounts_file(shrink_slot)
        );

        // Now, do full-shrink.
        accounts.shrink_all_slots(false, &EpochSchedule::default(), None);
        assert_eq!(
            pubkey_count_after_shrink,
            accounts.all_account_count_in_accounts_file(shrink_slot)
        );
    }

    #[test]
    fn test_select_candidates_by_total_usage_no_candidates() {
        // no input candidates -- none should be selected
        solana_logger::setup();
        let candidates = ShrinkCandidates::default();
        let db = AccountsDb::new_single_for_tests();

        let (selected_candidates, next_candidates) =
            db.select_candidates_by_total_usage(&candidates, DEFAULT_ACCOUNTS_SHRINK_RATIO, None);

        assert_eq!(0, selected_candidates.len());
        assert_eq!(0, next_candidates.len());
    }

    #[test]
    fn test_select_candidates_by_total_usage_3_way_split_condition() {
        // three candidates, one selected for shrink, one is put back to the candidate list and one is ignored
        solana_logger::setup();
        let mut candidates = ShrinkCandidates::default();
        let db = AccountsDb::new_single_for_tests();

        let common_store_path = Path::new("");
        let store_file_size = 100;

        let store1_slot = 11;
        let store1 = Arc::new(AccountStorageEntry::new(
            common_store_path,
            store1_slot,
            store1_slot as AccountsFileId,
            store_file_size,
            AccountsFileProvider::AppendVec,
        ));
        db.storage.insert(store1_slot, Arc::clone(&store1));
        store1.alive_bytes.store(0, Ordering::Release);
        candidates.insert(store1_slot);

        let store2_slot = 22;
        let store2 = Arc::new(AccountStorageEntry::new(
            common_store_path,
            store2_slot,
            store2_slot as AccountsFileId,
            store_file_size,
            AccountsFileProvider::AppendVec,
        ));
        db.storage.insert(store2_slot, Arc::clone(&store2));
        store2
            .alive_bytes
            .store(store_file_size as usize / 2, Ordering::Release);
        candidates.insert(store2_slot);

        let store3_slot = 33;
        let store3 = Arc::new(AccountStorageEntry::new(
            common_store_path,
            store3_slot,
            store3_slot as AccountsFileId,
            store_file_size,
            AccountsFileProvider::AppendVec,
        ));
        db.storage.insert(store3_slot, Arc::clone(&store3));
        store3
            .alive_bytes
            .store(store_file_size as usize, Ordering::Release);
        candidates.insert(store3_slot);

        // Set the target alive ratio to 0.6 so that we can just get rid of store1, the remaining two stores
        // alive ratio can be > the target ratio: the actual ratio is 0.75 because of 150 alive bytes / 200 total bytes.
        // The target ratio is also set to larger than store2's alive ratio: 0.5 so that it would be added
        // to the candidates list for next round.
        let target_alive_ratio = 0.6;
        let (selected_candidates, next_candidates) =
            db.select_candidates_by_total_usage(&candidates, target_alive_ratio, None);
        assert_eq!(1, selected_candidates.len());
        assert!(selected_candidates.contains(&store1_slot));
        assert_eq!(1, next_candidates.len());
        assert!(next_candidates.contains(&store2_slot));
    }

    #[test]
    fn test_select_candidates_by_total_usage_2_way_split_condition() {
        // three candidates, 2 are selected for shrink, one is ignored
        solana_logger::setup();
        let db = AccountsDb::new_single_for_tests();
        let mut candidates = ShrinkCandidates::default();

        let common_store_path = Path::new("");
        let store_file_size = 100;

        let store1_slot = 11;
        let store1 = Arc::new(AccountStorageEntry::new(
            common_store_path,
            store1_slot,
            store1_slot as AccountsFileId,
            store_file_size,
            AccountsFileProvider::AppendVec,
        ));
        db.storage.insert(store1_slot, Arc::clone(&store1));
        store1.alive_bytes.store(0, Ordering::Release);
        candidates.insert(store1_slot);

        let store2_slot = 22;
        let store2 = Arc::new(AccountStorageEntry::new(
            common_store_path,
            store2_slot,
            store2_slot as AccountsFileId,
            store_file_size,
            AccountsFileProvider::AppendVec,
        ));
        db.storage.insert(store2_slot, Arc::clone(&store2));
        store2
            .alive_bytes
            .store(store_file_size as usize / 2, Ordering::Release);
        candidates.insert(store2_slot);

        let store3_slot = 33;
        let store3 = Arc::new(AccountStorageEntry::new(
            common_store_path,
            store3_slot,
            store3_slot as AccountsFileId,
            store_file_size,
            AccountsFileProvider::AppendVec,
        ));
        db.storage.insert(store3_slot, Arc::clone(&store3));
        store3
            .alive_bytes
            .store(store_file_size as usize, Ordering::Release);
        candidates.insert(store3_slot);

        // Set the target ratio to default (0.8), both store1 and store2 must be selected and store3 is ignored.
        let target_alive_ratio = DEFAULT_ACCOUNTS_SHRINK_RATIO;
        let (selected_candidates, next_candidates) =
            db.select_candidates_by_total_usage(&candidates, target_alive_ratio, None);
        assert_eq!(2, selected_candidates.len());
        assert!(selected_candidates.contains(&store1_slot));
        assert!(selected_candidates.contains(&store2_slot));
        assert_eq!(0, next_candidates.len());
    }

    #[test]
    fn test_select_candidates_by_total_usage_all_clean() {
        // 2 candidates, they must be selected to achieve the target alive ratio
        solana_logger::setup();
        let db = AccountsDb::new_single_for_tests();
        let mut candidates = ShrinkCandidates::default();

        let common_store_path = Path::new("");
        let store_file_size = 100;

        let store1_slot = 11;
        let store1 = Arc::new(AccountStorageEntry::new(
            common_store_path,
            store1_slot,
            store1_slot as AccountsFileId,
            store_file_size,
            AccountsFileProvider::AppendVec,
        ));
        db.storage.insert(store1_slot, Arc::clone(&store1));
        store1
            .alive_bytes
            .store(store_file_size as usize / 4, Ordering::Release);
        candidates.insert(store1_slot);

        let store2_slot = 22;
        let store2 = Arc::new(AccountStorageEntry::new(
            common_store_path,
            store2_slot,
            store2_slot as AccountsFileId,
            store_file_size,
            AccountsFileProvider::AppendVec,
        ));
        db.storage.insert(store2_slot, Arc::clone(&store2));
        store2
            .alive_bytes
            .store(store_file_size as usize / 2, Ordering::Release);
        candidates.insert(store2_slot);

        for newest_ancient_slot in [None, Some(store1_slot), Some(store2_slot)] {
            // Set the target ratio to default (0.8), both stores from the two different slots must be selected.
            let target_alive_ratio = DEFAULT_ACCOUNTS_SHRINK_RATIO;
            let (selected_candidates, next_candidates) = db.select_candidates_by_total_usage(
                &candidates,
                target_alive_ratio,
                newest_ancient_slot.map(|newest_ancient_slot| newest_ancient_slot + 1),
            );
            assert_eq!(
                if newest_ancient_slot == Some(store1_slot) {
                    1
                } else if newest_ancient_slot == Some(store2_slot) {
                    0
                } else {
                    2
                },
                selected_candidates.len()
            );
            assert_eq!(
                newest_ancient_slot.is_none(),
                selected_candidates.contains(&store1_slot)
            );

            if newest_ancient_slot != Some(store2_slot) {
                assert!(selected_candidates.contains(&store2_slot));
            }
            assert_eq!(0, next_candidates.len());
        }
    }

    const UPSERT_POPULATE_RECLAIMS: UpsertReclaim = UpsertReclaim::PopulateReclaims;

    #[test]
    fn test_delete_dependencies() {
        solana_logger::setup();
        let accounts_index = AccountsIndex::<AccountInfo, AccountInfo>::default_for_tests();
        let key0 = Pubkey::new_from_array([0u8; 32]);
        let key1 = Pubkey::new_from_array([1u8; 32]);
        let key2 = Pubkey::new_from_array([2u8; 32]);
        let info0 = AccountInfo::new(StorageLocation::AppendVec(0, 0), 0);
        let info1 = AccountInfo::new(StorageLocation::AppendVec(1, 0), 0);
        let info2 = AccountInfo::new(StorageLocation::AppendVec(2, 0), 0);
        let info3 = AccountInfo::new(StorageLocation::AppendVec(3, 0), 0);
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
        for key in [&key0, &key1, &key2] {
            let index_entry = accounts_index.get_cloned(key).unwrap();
            let rooted_entries = accounts_index
                .get_rooted_entries(index_entry.slot_list.read().unwrap().as_slice(), None);
            let ref_count = index_entry.ref_count();
            purges.insert(*key, (rooted_entries, ref_count));
        }
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
            AccountsDb::checked_sum_for_capitalization(vec![1, u64::MAX].into_iter()),
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
        let store = accounts.storage.get_slot_storage_entry(0).unwrap();
        let total_len = store.accounts.len();
        info!("total: {}", total_len);
        assert_eq!(total_len, STORE_META_OVERHEAD);
    }

    #[test]
    fn test_store_clean_after_shrink() {
        solana_logger::setup();
        let accounts = AccountsDb::new_single_for_tests();
        let epoch_schedule = EpochSchedule::default();

        let account = AccountSharedData::new(1, 16 * 4096, &Pubkey::default());
        let pubkey1 = solana_sdk::pubkey::new_rand();
        accounts.store_cached((0, &[(&pubkey1, &account)][..]), None);

        let pubkey2 = solana_sdk::pubkey::new_rand();
        accounts.store_cached((0, &[(&pubkey2, &account)][..]), None);

        let zero_account = AccountSharedData::new(0, 1, &Pubkey::default());
        accounts.store_cached((1, &[(&pubkey1, &zero_account)][..]), None);

        // Add root 0 and flush separately
        accounts.calculate_accounts_delta_hash(0);
        accounts.add_root(0);
        accounts.flush_accounts_cache(true, None);

        // clear out the dirty keys
        accounts.clean_accounts_for_tests();

        // flush 1
        accounts.calculate_accounts_delta_hash(1);
        accounts.add_root(1);
        accounts.flush_accounts_cache(true, None);

        accounts.print_accounts_stats("pre-clean");

        // clean to remove pubkey1 from 0,
        // shrink to shrink pubkey1 from 0
        // then another clean to remove pubkey1 from slot 1
        accounts.clean_accounts_for_tests();

        accounts.shrink_candidate_slots(&epoch_schedule);

        accounts.clean_accounts_for_tests();

        accounts.print_accounts_stats("post-clean");
        assert_eq!(accounts.accounts_index.ref_count_from_storage(&pubkey1), 0);
    }

    #[test]
    #[should_panic(expected = "We've run out of storage ids!")]
    fn test_wrapping_storage_id() {
        let db = AccountsDb::new_single_for_tests();

        let zero_lamport_account =
            AccountSharedData::new(0, 0, AccountSharedData::default().owner());

        // set 'next' id to the max possible value
        db.next_id.store(AccountsFileId::MAX, Ordering::Release);
        let slots = 3;
        let keys = (0..slots).map(|_| Pubkey::new_unique()).collect::<Vec<_>>();
        // write unique keys to successive slots
        keys.iter().enumerate().for_each(|(slot, key)| {
            let slot = slot as Slot;
            db.store_for_tests(slot, &[(key, &zero_lamport_account)]);
            db.calculate_accounts_delta_hash(slot);
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
    fn test_reuse_storage_id() {
        solana_logger::setup();
        let db = AccountsDb::new_single_for_tests();

        let zero_lamport_account =
            AccountSharedData::new(0, 0, AccountSharedData::default().owner());

        // set 'next' id to the max possible value
        db.next_id.store(AccountsFileId::MAX, Ordering::Release);
        let slots = 3;
        let keys = (0..slots).map(|_| Pubkey::new_unique()).collect::<Vec<_>>();
        // write unique keys to successive slots
        keys.iter().enumerate().for_each(|(slot, key)| {
            let slot = slot as Slot;
            db.store_for_tests(slot, &[(key, &zero_lamport_account)]);
            db.calculate_accounts_delta_hash(slot);
            db.add_root_and_flush_write_cache(slot);
            // reset next_id to what it was previously to cause us to re-use the same id
            db.next_id.store(AccountsFileId::MAX, Ordering::Release);
        });
        let ancestors = Ancestors::default();
        keys.iter().for_each(|key| {
            assert!(db.load_without_fixed_root(&ancestors, key).is_some());
        });
    }

    #[test]
    fn test_zero_lamport_new_root_not_cleaned() {
        let db = AccountsDb::new_single_for_tests();
        let account_key = Pubkey::new_unique();
        let zero_lamport_account =
            AccountSharedData::new(0, 0, AccountSharedData::default().owner());

        // Store zero lamport account into slots 0 and 1, root both slots
        db.store_for_tests(0, &[(&account_key, &zero_lamport_account)]);
        db.store_for_tests(1, &[(&account_key, &zero_lamport_account)]);
        db.calculate_accounts_delta_hash(0);
        db.add_root_and_flush_write_cache(0);
        db.calculate_accounts_delta_hash(1);
        db.add_root_and_flush_write_cache(1);

        // Only clean zero lamport accounts up to slot 0
        db.clean_accounts(Some(0), false, &EpochSchedule::default());

        // Should still be able to find zero lamport account in slot 1
        assert_eq!(
            db.load_without_fixed_root(&Ancestors::default(), &account_key),
            Some((zero_lamport_account, 1))
        );
    }

    #[test]
    fn test_store_load_cached() {
        let db = AccountsDb::new_single_for_tests();
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
        let db = AccountsDb::new_single_for_tests();
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
        let db = AccountsDb::new_single_for_tests();
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
        assert!(db.accounts_index.contains(&unrooted_key));
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
        let mut db = AccountsDb::new_single_for_tests();
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

    #[test]
    fn test_read_only_accounts_cache() {
        let db = Arc::new(AccountsDb::new_single_for_tests());

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

    #[test]
    fn test_load_with_read_only_accounts_cache() {
        let db = Arc::new(AccountsDb::new_single_for_tests());

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
        let (account, slot) = db
            .load_account_with(&Ancestors::default(), &account_key, |_| false)
            .unwrap();
        assert_eq!(account.lamports(), 1);
        assert_eq!(db.read_only_accounts_cache.cache_len(), 0);
        assert_eq!(slot, 1);

        let (account, slot) = db
            .load_account_with(&Ancestors::default(), &account_key, |_| true)
            .unwrap();
        assert_eq!(account.lamports(), 1);
        assert_eq!(db.read_only_accounts_cache.cache_len(), 1);
        assert_eq!(slot, 1);

        db.store_cached((2, &[(&account_key, &zero_lamport_account)][..]), None);
        let account = db.load_account_with(&Ancestors::default(), &account_key, |_| false);
        assert!(account.is_none());
        assert_eq!(db.read_only_accounts_cache.cache_len(), 1);

        db.read_only_accounts_cache.reset_for_tests();
        assert_eq!(db.read_only_accounts_cache.cache_len(), 0);
        let account = db.load_account_with(&Ancestors::default(), &account_key, |_| true);
        assert!(account.is_none());
        assert_eq!(db.read_only_accounts_cache.cache_len(), 0);

        let slot2_account = AccountSharedData::new(2, 1, AccountSharedData::default().owner());
        db.store_cached((2, &[(&account_key, &slot2_account)][..]), None);
        let (account, slot) = db
            .load_account_with(&Ancestors::default(), &account_key, |_| false)
            .unwrap();
        assert_eq!(account.lamports(), 2);
        assert_eq!(db.read_only_accounts_cache.cache_len(), 0);
        assert_eq!(slot, 2);

        let slot2_account = AccountSharedData::new(2, 1, AccountSharedData::default().owner());
        db.store_cached((2, &[(&account_key, &slot2_account)][..]), None);
        let (account, slot) = db
            .load_account_with(&Ancestors::default(), &account_key, |_| true)
            .unwrap();
        assert_eq!(account.lamports(), 2);
        // The account shouldn't be added to read_only_cache because it is in write_cache.
        assert_eq!(db.read_only_accounts_cache.cache_len(), 0);
        assert_eq!(slot, 2);
    }

    #[test]
    fn test_account_matches_owners() {
        let db = Arc::new(AccountsDb::new_single_for_tests());

        let owners: Vec<Pubkey> = (0..2).map(|_| Pubkey::new_unique()).collect();

        let account1_key = Pubkey::new_unique();
        let account1 = AccountSharedData::new(321, 10, &owners[0]);

        let account2_key = Pubkey::new_unique();
        let account2 = AccountSharedData::new(1, 1, &owners[1]);

        let account3_key = Pubkey::new_unique();
        let account3 = AccountSharedData::new(1, 1, &Pubkey::new_unique());

        // Account with 0 lamports
        let account4_key = Pubkey::new_unique();
        let account4 = AccountSharedData::new(0, 1, &owners[1]);

        db.store_cached((0, &[(&account1_key, &account1)][..]), None);
        db.store_cached((1, &[(&account2_key, &account2)][..]), None);
        db.store_cached((2, &[(&account3_key, &account3)][..]), None);
        db.store_cached((3, &[(&account4_key, &account4)][..]), None);

        db.add_root(0);
        db.add_root(1);
        db.add_root(2);
        db.add_root(3);

        // Flush the cache so that the account meta will be read from the storage
        db.flush_accounts_cache(true, None);
        db.clean_accounts_for_tests();

        assert_eq!(
            db.account_matches_owners(&Ancestors::default(), &account1_key, &owners),
            Ok(0)
        );
        assert_eq!(
            db.account_matches_owners(&Ancestors::default(), &account2_key, &owners),
            Ok(1)
        );
        assert_eq!(
            db.account_matches_owners(&Ancestors::default(), &account3_key, &owners),
            Err(MatchAccountOwnerError::NoMatch)
        );
        assert_eq!(
            db.account_matches_owners(&Ancestors::default(), &account4_key, &owners),
            Err(MatchAccountOwnerError::NoMatch)
        );
        assert_eq!(
            db.account_matches_owners(&Ancestors::default(), &Pubkey::new_unique(), &owners),
            Err(MatchAccountOwnerError::UnableToLoad)
        );

        // Flush the cache and load account1 (so that it's in the cache)
        db.flush_accounts_cache(true, None);
        db.clean_accounts_for_tests();
        let _ = db
            .do_load(
                &Ancestors::default(),
                &account1_key,
                Some(0),
                LoadHint::Unspecified,
                LoadZeroLamports::SomeWithZeroLamportAccountForTests,
            )
            .unwrap();

        assert_eq!(
            db.account_matches_owners(&Ancestors::default(), &account1_key, &owners),
            Ok(0)
        );
        assert_eq!(
            db.account_matches_owners(&Ancestors::default(), &account2_key, &owners),
            Ok(1)
        );
        assert_eq!(
            db.account_matches_owners(&Ancestors::default(), &account3_key, &owners),
            Err(MatchAccountOwnerError::NoMatch)
        );
        assert_eq!(
            db.account_matches_owners(&Ancestors::default(), &account4_key, &owners),
            Err(MatchAccountOwnerError::NoMatch)
        );
        assert_eq!(
            db.account_matches_owners(&Ancestors::default(), &Pubkey::new_unique(), &owners),
            Err(MatchAccountOwnerError::UnableToLoad)
        );
    }

    /// a test that will accept either answer
    const LOAD_ZERO_LAMPORTS_ANY_TESTS: LoadZeroLamports = LoadZeroLamports::None;

    #[test]
    fn test_flush_cache_clean() {
        let db = Arc::new(AccountsDb::new_single_for_tests());

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
        let db = Arc::new(AccountsDb::new_single_for_tests());

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
        assert!(db.storage.get_slot_storage_entry(0).is_some());

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
        let db = Arc::new(AccountsDb::new_single_for_tests());
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
        db.calculate_accounts_delta_hash(0);

        let max_scan_root = 0;
        db.add_root(max_scan_root);
        let scan_ancestors: Arc<Ancestors> = Arc::new(vec![(0, 1), (1, 1)].into_iter().collect());
        let bank_id = 0;
        let scan_tracker = setup_scan(db.clone(), scan_ancestors.clone(), bank_id, account_key2);

        // Add a new root 2
        let new_root = 2;
        db.calculate_accounts_delta_hash(new_root);
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
            self.storage.get_slot_storage_entry(slot).unwrap()
        }
    }

    define_accounts_db_test!(test_alive_bytes, |accounts_db| {
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

        storage0.accounts.scan_accounts(|account| {
            let before_size = storage0.alive_bytes.load(Ordering::Acquire);
            let account_info = accounts_db
                .accounts_index
                .get_cloned(account.pubkey())
                .unwrap()
                .slot_list
                .read()
                .unwrap()
                // Should only be one entry per key, since every key was only stored to slot 0
                [0];
            assert_eq!(account_info.0, slot);
            let reclaims = [account_info];
            accounts_db.remove_dead_accounts(reclaims.iter(), None, true);
            let after_size = storage0.alive_bytes.load(Ordering::Acquire);
            if storage0.count() == 0
                && AccountsFileProvider::HotStorage == accounts_db.accounts_file_provider
            {
                // when `remove_dead_accounts` reaches 0 accounts, all bytes are marked as dead
                assert_eq!(after_size, 0);
            } else {
                assert_eq!(before_size, after_size + account.stored_size());
            }
        });
    });

    fn setup_accounts_db_cache_clean(
        num_slots: usize,
        scan_slot: Option<Slot>,
        write_cache_limit_bytes: Option<u64>,
    ) -> (Arc<AccountsDb>, Vec<Pubkey>, Vec<Slot>, Option<ScanTracker>) {
        let mut accounts_db = AccountsDb::new_single_for_tests();
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
                |slot_accounts: &DashSet<Pubkey>, loaded_account: &LoadedAccount, _data| {
                    slot_accounts.insert(*loaded_account.pubkey());
                },
                ScanAccountStorageData::NoData,
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
                |slot_account: &RwLock<Pubkey>, loaded_account: &LoadedAccount, _data| {
                    *slot_account.write().unwrap() = *loaded_account.pubkey();
                },
                ScanAccountStorageData::NoData,
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
                |loaded_account: &LoadedAccount| {
                    assert!(
                        !is_cache_at_limit,
                        "When cache is at limit, all roots should have been flushed to storage"
                    );
                    // All slots <= requested_flush_root should have been flushed, regardless
                    // of ongoing scans
                    assert!(*slot > requested_flush_root);
                    Some(*loaded_account.pubkey())
                },
                |slot_accounts: &DashSet<Pubkey>, loaded_account: &LoadedAccount, _data| {
                    slot_accounts.insert(*loaded_account.pubkey());
                    if !is_cache_at_limit {
                        // Only true when the limit hasn't been reached and there are still
                        // slots left in the cache
                        assert!(*slot <= requested_flush_root);
                    }
                },
                ScanAccountStorageData::NoData,
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
                    |slot_account: &DashSet<Pubkey>, loaded_account: &LoadedAccount, _data| {
                        slot_account.insert(*loaded_account.pubkey());
                    },
                    ScanAccountStorageData::NoData,
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
        let db = AccountsDb::new_single_for_tests();
        let epoch_schedule = EpochSchedule::default();
        let account_key1 = Pubkey::new_unique();
        let account_key2 = Pubkey::new_unique();
        let account1 = AccountSharedData::new(1, 0, AccountSharedData::default().owner());

        // Store into slot 0
        // This has to be done uncached since we are trying to add another account to the append vec AFTER it has been flushed.
        // This doesn't work if the flush creates an append vec of exactly the right size.
        // Normal operations NEVER write the same account to the same append vec twice during a write cache flush.
        db.store_uncached(0, &[(&account_key1, &account1)][..]);
        db.store_uncached(0, &[(&account_key2, &account1)][..]);
        db.add_root(0);
        if !do_intra_cache_clean {
            // Add an additional ref within the same slot to pubkey 1
            db.store_uncached(0, &[(&account_key1, &account1)]);
        }

        // Make account_key1 in slot 0 outdated by updating in rooted slot 1
        db.store_cached((1, &[(&account_key1, &account1)][..]), None);
        db.add_root(1);
        // Flushes all roots
        db.flush_accounts_cache(true, None);
        db.calculate_accounts_delta_hash(0);
        db.calculate_accounts_delta_hash(1);

        // Clean to remove outdated entry from slot 0
        db.clean_accounts(Some(1), false, &EpochSchedule::default());

        // Shrink Slot 0
        {
            let mut shrink_candidate_slots = db.shrink_candidate_slots.lock().unwrap();
            shrink_candidate_slots.insert(0);
        }
        db.shrink_candidate_slots(&epoch_schedule);

        // Make slot 0 dead by updating the remaining key
        db.store_cached((2, &[(&account_key2, &account1)][..]), None);
        db.add_root(2);

        // Flushes all roots
        db.flush_accounts_cache(true, None);

        // Should be one store before clean for slot 0
        db.get_and_assert_single_storage(0);
        db.calculate_accounts_delta_hash(2);
        db.clean_accounts(Some(2), false, &EpochSchedule::default());

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

    define_accounts_db_test!(test_partial_clean, |db| {
        let account_key1 = Pubkey::new_unique();
        let account_key2 = Pubkey::new_unique();
        let account1 = AccountSharedData::new(1, 0, AccountSharedData::default().owner());
        let account2 = AccountSharedData::new(2, 0, AccountSharedData::default().owner());
        let account3 = AccountSharedData::new(3, 0, AccountSharedData::default().owner());
        let account4 = AccountSharedData::new(4, 0, AccountSharedData::default().owner());

        // Store accounts into slots 0 and 1
        db.store_uncached(0, &[(&account_key1, &account1), (&account_key2, &account1)]);
        db.store_uncached(1, &[(&account_key1, &account2)]);
        db.calculate_accounts_delta_hash(0);
        db.calculate_accounts_delta_hash(1);
        db.print_accounts_stats("pre-clean1");

        // clean accounts - no accounts should be cleaned, since no rooted slots
        //
        // Checking that the uncleaned_pubkeys are not pre-maturely removed
        // such that when the slots are rooted, and can actually be cleaned, then the
        // delta keys are still there.
        db.clean_accounts_for_tests();

        db.print_accounts_stats("post-clean1");
        // Check stores > 0
        assert!(!db.storage.is_empty_entry(0));
        assert!(!db.storage.is_empty_entry(1));

        // root slot 0
        db.add_root_and_flush_write_cache(0);

        // store into slot 2
        db.store_uncached(2, &[(&account_key2, &account3), (&account_key1, &account3)]);
        db.calculate_accounts_delta_hash(2);
        db.clean_accounts_for_tests();
        db.print_accounts_stats("post-clean2");

        // root slots 1
        db.add_root_and_flush_write_cache(1);
        db.clean_accounts_for_tests();

        db.print_accounts_stats("post-clean3");

        db.store_uncached(3, &[(&account_key2, &account4)]);
        db.calculate_accounts_delta_hash(3);
        db.add_root_and_flush_write_cache(3);

        // Check that we can clean where max_root=3 and slot=2 is not rooted
        db.clean_accounts_for_tests();

        assert!(db.uncleaned_pubkeys.is_empty());

        db.print_accounts_stats("post-clean4");

        assert!(db.storage.is_empty_entry(0));
        assert!(!db.storage.is_empty_entry(1));
    });

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
                        .store(thread_rng().gen_range(0..10) as u64, Ordering::Relaxed);

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

        let mut db = AccountsDb::new_single_for_tests();
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
        let mut db = AccountsDb::new_single_for_tests();
        let epoch_schedule = EpochSchedule::default();
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
                    db.shrink_candidate_slots.lock().unwrap().insert(slot);
                    db.shrink_candidate_slots(&epoch_schedule);
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
        let mut db = AccountsDb::new_single_for_tests();
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
        let db = AccountsDb::new_single_for_tests();
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
                assert!(!db.accounts_index.contains(&account_in_slot));
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
                // Clear for next iteration so that `assert!(self.storage.get_slot_storage_entry(purged_slot).is_none());`
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
        let db = AccountsDb::new_single_for_tests();

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
        let db = AccountsDb::new_single_for_tests();

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
        let db = AccountsDb::new_single_for_tests();

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
        let path = Path::new("");
        let file_size = 100;
        let slot = 11;

        let store = Arc::new(AccountStorageEntry::new(
            path,
            slot,
            slot as AccountsFileId,
            file_size,
            AccountsFileProvider::AppendVec,
        ));
        store.add_account(file_size as usize);
        assert!(!AccountsDb::is_shrinking_productive(slot, &store));

        let store = Arc::new(AccountStorageEntry::new(
            path,
            slot,
            slot as AccountsFileId,
            file_size,
            AccountsFileProvider::AppendVec,
        ));
        store.add_account(file_size as usize / 2);
        store.add_account(file_size as usize / 4);
        store.remove_accounts(file_size as usize / 4, false, 1);
        assert!(AccountsDb::is_shrinking_productive(slot, &store));

        store.add_account(file_size as usize / 2);
        assert!(!AccountsDb::is_shrinking_productive(slot, &store));
    }

    #[test]
    fn test_is_candidate_for_shrink() {
        solana_logger::setup();

        let mut accounts = AccountsDb::new_single_for_tests();
        let common_store_path = Path::new("");
        let store_file_size = 100_000;
        let entry = Arc::new(AccountStorageEntry::new(
            common_store_path,
            0,
            1,
            store_file_size,
            AccountsFileProvider::AppendVec,
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

        entry
            .alive_bytes
            .store(store_file_size as usize - 1, Ordering::Release);
        assert!(accounts.is_candidate_for_shrink(&entry));
        entry
            .alive_bytes
            .store(store_file_size as usize, Ordering::Release);
        assert!(!accounts.is_candidate_for_shrink(&entry));

        let shrink_ratio = 0.3;
        let file_size_shrink_limit = (store_file_size as f64 * shrink_ratio) as usize;
        entry
            .alive_bytes
            .store(file_size_shrink_limit + 1, Ordering::Release);
        accounts.shrink_ratio = AccountShrinkThreshold::TotalSpace { shrink_ratio };
        assert!(accounts.is_candidate_for_shrink(&entry));
        accounts.shrink_ratio = AccountShrinkThreshold::IndividualStore { shrink_ratio };
        assert!(!accounts.is_candidate_for_shrink(&entry));
    }

    define_accounts_db_test!(test_calculate_storage_count_and_alive_bytes, |accounts| {
        accounts.accounts_index.set_startup(Startup::Startup);
        let shared_key = solana_sdk::pubkey::new_rand();
        let account = AccountSharedData::new(1, 1, AccountSharedData::default().owner());
        let slot0 = 0;

        accounts.accounts_index.set_startup(Startup::Startup);

        let storage = accounts.create_and_insert_store(slot0, 4_000, "flush_slot_cache");
        storage
            .accounts
            .append_accounts(&(slot0, &[(&shared_key, &account)][..]), 0);

        let storage = accounts.storage.get_slot_storage_entry(slot0).unwrap();
        let storage_info = StorageSizeAndCountMap::default();
        accounts.generate_index_for_slot(
            &storage,
            slot0,
            0,
            &RentCollector::default(),
            &storage_info,
        );
        assert_eq!(storage_info.len(), 1);
        for entry in storage_info.iter() {
            let expected_stored_size =
                if accounts.accounts_file_provider == AccountsFileProvider::HotStorage {
                    33
                } else {
                    144
                };
            assert_eq!(
                (entry.key(), entry.value().count, entry.value().stored_size),
                (&0, 1, expected_stored_size)
            );
        }
        accounts.accounts_index.set_startup(Startup::Normal);
    });

    define_accounts_db_test!(
        test_calculate_storage_count_and_alive_bytes_0_accounts,
        |accounts| {
            // empty store
            let storage = accounts.create_and_insert_store(0, 1, "test");
            let storage_info = StorageSizeAndCountMap::default();
            accounts.generate_index_for_slot(
                &storage,
                0,
                0,
                &RentCollector::default(),
                &storage_info,
            );
            assert!(storage_info.is_empty());
        }
    );

    define_accounts_db_test!(
        test_calculate_storage_count_and_alive_bytes_2_accounts,
        |accounts| {
            let keys = [
                solana_sdk::pubkey::Pubkey::from([0; 32]),
                solana_sdk::pubkey::Pubkey::from([255; 32]),
            ];
            accounts.accounts_index.set_startup(Startup::Startup);

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
            let storage = accounts.create_and_insert_store(slot0, 4_000, "flush_slot_cache");
            storage.accounts.append_accounts(
                &(slot0, &[(&keys[0], &account), (&keys[1], &account_big)][..]),
                0,
            );

            let storage_info = StorageSizeAndCountMap::default();
            accounts.generate_index_for_slot(
                &storage,
                0,
                0,
                &RentCollector::default(),
                &storage_info,
            );
            assert_eq!(storage_info.len(), 1);
            for entry in storage_info.iter() {
                let expected_stored_size =
                    if accounts.accounts_file_provider == AccountsFileProvider::HotStorage {
                        1065
                    } else {
                        1280
                    };
                assert_eq!(
                    (entry.key(), entry.value().count, entry.value().stored_size),
                    (&0, 2, expected_stored_size)
                );
            }
            accounts.accounts_index.set_startup(Startup::Normal);
        }
    );

    define_accounts_db_test!(test_set_storage_count_and_alive_bytes, |accounts| {
        // make sure we have storage 0
        let shared_key = solana_sdk::pubkey::new_rand();
        let account = AccountSharedData::new(1, 1, AccountSharedData::default().owner());
        let slot0 = 0;
        accounts.store_for_tests(slot0, &[(&shared_key, &account)]);
        accounts.add_root_and_flush_write_cache(slot0);

        // fake out the store count to avoid the assert
        for (_, store) in accounts.storage.iter() {
            store.alive_bytes.store(0, Ordering::Release);
            let mut count_and_status = store.count_and_status.lock_write();
            count_and_status.0 = 0;
        }

        // count needs to be <= approx stored count in store.
        // approx stored count is 1 in store since we added a single account.
        let count = 1;

        // populate based on made up hash data
        let dashmap = DashMap::default();
        dashmap.insert(
            0,
            StorageSizeAndCount {
                stored_size: 2,
                count,
            },
        );

        for (_, store) in accounts.storage.iter() {
            assert_eq!(store.count_and_status.read().0, 0);
            assert_eq!(store.alive_bytes.load(Ordering::Acquire), 0);
        }
        accounts.set_storage_count_and_alive_bytes(dashmap, &mut GenerateIndexTimings::default());
        assert_eq!(accounts.storage.len(), 1);
        for (_, store) in accounts.storage.iter() {
            assert_eq!(store.id(), 0);
            assert_eq!(store.count_and_status.read().0, count);
            assert_eq!(store.alive_bytes.load(Ordering::Acquire), 2);
        }
    });

    define_accounts_db_test!(test_purge_alive_unrooted_slots_after_clean, |accounts| {
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
        accounts.calculate_accounts_delta_hash(slot0);

        // On the next *rooted* slot, update the `shared_key` account to zero lamports
        let zero_lamport_account =
            AccountSharedData::new(0, 0, AccountSharedData::default().owner());
        accounts.store_for_tests(slot1, &[(&shared_key, &zero_lamport_account)]);

        // Simulate adding dirty pubkeys on bank freeze, set root
        accounts.calculate_accounts_delta_hash(slot1);
        accounts.add_root_and_flush_write_cache(slot1);

        // The later rooted zero-lamport update to `shared_key` cannot be cleaned
        // because it is kept alive by the unrooted slot.
        accounts.clean_accounts_for_tests();
        assert!(accounts.accounts_index.contains(&shared_key));

        // Simulate purge_slot() all from AccountsBackgroundService
        accounts.purge_slot(slot0, 0, true);

        // Now clean should clean up the remaining key
        accounts.clean_accounts_for_tests();
        assert!(!accounts.accounts_index.contains(&shared_key));
        assert_no_storages_at_slot(&accounts, slot0);
    });

    /// asserts that not only are there 0 append vecs, but there is not even an entry in the storage map for 'slot'
    fn assert_no_storages_at_slot(db: &AccountsDb, slot: Slot) {
        assert!(db.storage.get_slot_storage_entry(slot).is_none());
    }

    // Test to make sure `clean_accounts()` works properly with `latest_full_snapshot_slot`
    //
    // Basically:
    //
    // - slot 1: set Account1's balance to non-zero
    // - slot 2: set Account1's balance to a different non-zero amount
    // - slot 3: set Account1's balance to zero
    // - call `clean_accounts()` with `max_clean_root` set to 2
    //     - ensure Account1 has *not* been purged
    //     - ensure the store from slot 1 is cleaned up
    // - call `clean_accounts()` with `latest_full_snapshot_slot` set to 2
    //     - ensure Account1 has *not* been purged
    // - call `clean_accounts()` with `latest_full_snapshot_slot` set to 3
    //     - ensure Account1 *has* been purged
    define_accounts_db_test!(
        test_clean_accounts_with_latest_full_snapshot_slot,
        |accounts_db| {
            let pubkey = solana_sdk::pubkey::new_rand();
            let owner = solana_sdk::pubkey::new_rand();
            let space = 0;

            let slot1: Slot = 1;
            let account = AccountSharedData::new(111, space, &owner);
            accounts_db.store_cached((slot1, &[(&pubkey, &account)][..]), None);
            accounts_db.calculate_accounts_delta_hash(slot1);
            accounts_db.add_root_and_flush_write_cache(slot1);

            let slot2: Slot = 2;
            let account = AccountSharedData::new(222, space, &owner);
            accounts_db.store_cached((slot2, &[(&pubkey, &account)][..]), None);
            accounts_db.calculate_accounts_delta_hash(slot2);
            accounts_db.add_root_and_flush_write_cache(slot2);

            let slot3: Slot = 3;
            let account = AccountSharedData::new(0, space, &owner);
            accounts_db.store_cached((slot3, &[(&pubkey, &account)][..]), None);
            accounts_db.calculate_accounts_delta_hash(slot3);
            accounts_db.add_root_and_flush_write_cache(slot3);

            assert_eq!(accounts_db.ref_count_for_pubkey(&pubkey), 3);

            accounts_db.set_latest_full_snapshot_slot(slot2);
            accounts_db.clean_accounts(Some(slot2), false, &EpochSchedule::default());
            assert_eq!(accounts_db.ref_count_for_pubkey(&pubkey), 2);

            accounts_db.set_latest_full_snapshot_slot(slot2);
            accounts_db.clean_accounts(None, false, &EpochSchedule::default());
            assert_eq!(accounts_db.ref_count_for_pubkey(&pubkey), 1);

            accounts_db.set_latest_full_snapshot_slot(slot3);
            accounts_db.clean_accounts(None, false, &EpochSchedule::default());
            assert_eq!(accounts_db.ref_count_for_pubkey(&pubkey), 0);
        }
    );

    #[test]
    fn test_filter_zero_lamport_clean_for_incremental_snapshots() {
        solana_logger::setup();
        let slot = 10;

        struct TestParameters {
            latest_full_snapshot_slot: Option<Slot>,
            max_clean_root: Option<Slot>,
            should_contain: bool,
        }

        let do_test = |test_params: TestParameters| {
            let account_info = AccountInfo::new(StorageLocation::AppendVec(42, 128), 0);
            let pubkey = solana_sdk::pubkey::new_rand();
            let mut key_set = HashSet::default();
            key_set.insert(pubkey);
            let store_count = 0;
            let mut store_counts = HashMap::default();
            store_counts.insert(slot, (store_count, key_set));
            let mut purges_zero_lamports = HashMap::default();
            purges_zero_lamports.insert(pubkey, (vec![(slot, account_info)], 1));

            let accounts_db = AccountsDb::new_single_for_tests();
            if let Some(latest_full_snapshot_slot) = test_params.latest_full_snapshot_slot {
                accounts_db.set_latest_full_snapshot_slot(latest_full_snapshot_slot);
            }
            accounts_db.filter_zero_lamport_clean_for_incremental_snapshots(
                test_params.max_clean_root,
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
            let latest_full_snapshot_slot = None;

            do_test(TestParameters {
                latest_full_snapshot_slot,
                max_clean_root: Some(slot),
                should_contain: true,
            });

            do_test(TestParameters {
                latest_full_snapshot_slot,
                max_clean_root: None,
                should_contain: true,
            });
        }

        // Scenario 2: last full snapshot is GREATER THAN zero lamport account slot
        // In this scenario always purge, and just test the various permutations of
        // `should_filter_for_incremental_snapshots` based on `max_clean_root`.
        {
            let latest_full_snapshot_slot = Some(slot + 1);

            do_test(TestParameters {
                latest_full_snapshot_slot,
                max_clean_root: latest_full_snapshot_slot,
                should_contain: true,
            });

            do_test(TestParameters {
                latest_full_snapshot_slot,
                max_clean_root: latest_full_snapshot_slot.map(|s| s + 1),
                should_contain: true,
            });

            do_test(TestParameters {
                latest_full_snapshot_slot,
                max_clean_root: None,
                should_contain: true,
            });
        }

        // Scenario 3: last full snapshot is EQUAL TO zero lamport account slot
        // In this scenario always purge, as it's the same as Scenario 2.
        {
            let latest_full_snapshot_slot = Some(slot);

            do_test(TestParameters {
                latest_full_snapshot_slot,
                max_clean_root: latest_full_snapshot_slot,
                should_contain: true,
            });

            do_test(TestParameters {
                latest_full_snapshot_slot,
                max_clean_root: latest_full_snapshot_slot.map(|s| s + 1),
                should_contain: true,
            });

            do_test(TestParameters {
                latest_full_snapshot_slot,
                max_clean_root: None,
                should_contain: true,
            });
        }

        // Scenario 4: last full snapshot is LESS THAN zero lamport account slot
        // In this scenario do *not* purge, except when `should_filter_for_incremental_snapshots`
        // is false
        {
            let latest_full_snapshot_slot = Some(slot - 1);

            do_test(TestParameters {
                latest_full_snapshot_slot,
                max_clean_root: latest_full_snapshot_slot,
                should_contain: true,
            });

            do_test(TestParameters {
                latest_full_snapshot_slot,
                max_clean_root: latest_full_snapshot_slot.map(|s| s + 1),
                should_contain: false,
            });

            do_test(TestParameters {
                latest_full_snapshot_slot,
                max_clean_root: None,
                should_contain: false,
            });
        }
    }

    impl AccountsDb {
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
        let pk1 = Pubkey::from([1; 32]);
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
            let pk1 = Pubkey::from([1; 32]);
            let pk2 = Pubkey::from([2; 32]);
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
                for (pk, slots) in [(pk1, vec![slot1, slot2]), (pk2, vec![slot1])] {
                    let result = purged_stored_account_slots.remove(&pk).unwrap();
                    assert_eq!(result, slots.into_iter().collect::<HashSet<_>>());
                }
                assert!(purged_stored_account_slots.is_empty());
                assert_eq!(db.accounts_index.ref_count_from_storage(&pk1), 0);
                assert_eq!(db.accounts_index.ref_count_from_storage(&pk2), 1);
            }
        }
    }

    define_accounts_db_test!(test_many_unrefs, |db| {
        let mut purged_stored_account_slots = AccountSlots::default();
        let mut reclaims = SlotList::default();
        let pk1 = Pubkey::from([1; 32]);
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
    });

    #[test_case(CreateAncientStorage::Append; "append")]
    #[test_case(CreateAncientStorage::Pack; "pack")]
    fn test_get_oldest_non_ancient_slot_for_hash_calc_scan(
        create_ancient_storage: CreateAncientStorage,
    ) {
        let expected = |v| {
            if create_ancient_storage == CreateAncientStorage::Append {
                Some(v)
            } else {
                None
            }
        };

        let mut db = AccountsDb::new_single_for_tests();
        db.create_ancient_storage = create_ancient_storage;

        let config = CalcAccountsHashConfig::default();
        let slot = config.epoch_schedule.slots_per_epoch;
        let slots_per_epoch = config.epoch_schedule.slots_per_epoch;
        assert_ne!(slot, 0);
        let offset = 10;
        // no ancient append vecs, so always 0
        assert_eq!(
            db.get_oldest_non_ancient_slot_for_hash_calc_scan(slots_per_epoch + offset, &config),
            expected(0)
        );
        // ancient append vecs enabled (but at 0 offset), so can be non-zero
        db.ancient_append_vec_offset = Some(0);
        // 0..=(slots_per_epoch - 1) are all non-ancient
        assert_eq!(
            db.get_oldest_non_ancient_slot_for_hash_calc_scan(slots_per_epoch - 1, &config),
            expected(0)
        );
        // 1..=slots_per_epoch are all non-ancient, so 1 is oldest non ancient
        assert_eq!(
            db.get_oldest_non_ancient_slot_for_hash_calc_scan(slots_per_epoch, &config),
            expected(1)
        );
        assert_eq!(
            db.get_oldest_non_ancient_slot_for_hash_calc_scan(slots_per_epoch + offset, &config),
            expected(offset + 1)
        );
    }

    define_accounts_db_test!(test_mark_dirty_dead_stores_empty, |db| {
        let slot = 0;
        for add_dirty_stores in [false, true] {
            let dead_storages = db.mark_dirty_dead_stores(slot, add_dirty_stores, None, false);
            assert!(dead_storages.is_empty());
            assert!(db.dirty_stores.is_empty());
        }
    });

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
            let old_id = existing_store.id();
            let dead_storages = db.mark_dirty_dead_stores(slot, add_dirty_stores, None, false);
            assert!(db.storage.get_slot_storage_entry(slot).is_none());
            assert_eq!(dead_storages.len(), 1);
            assert_eq!(dead_storages.first().unwrap().id(), old_id);
            if add_dirty_stores {
                assert_eq!(1, db.dirty_stores.len());
                let dirty_store = db.dirty_stores.get(&slot).unwrap();
                assert_eq!(dirty_store.id(), old_id);
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
            let old_id = old_store.id();
            let shrink_in_progress = db.get_store_for_shrink(slot, 100);
            let dead_storages =
                db.mark_dirty_dead_stores(slot, add_dirty_stores, Some(shrink_in_progress), false);
            assert!(db.storage.get_slot_storage_entry(slot).is_some());
            assert_eq!(dead_storages.len(), 1);
            assert_eq!(dead_storages.first().unwrap().id(), old_id);
            if add_dirty_stores {
                assert_eq!(1, db.dirty_stores.len());
                let dirty_store = db.dirty_stores.get(&slot).unwrap();
                assert_eq!(dirty_store.id(), old_id);
            } else {
                assert!(db.dirty_stores.is_empty());
            }
            assert!(db.storage.get_slot_storage_entry(slot).is_some());
        }
    }

    #[test]
    fn test_split_storages_ancient_chunks() {
        let storages = SortedStorages::empty();
        assert_eq!(storages.max_slot_inclusive(), 0);
        let result = SplitAncientStorages::new(Some(0), &storages);
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

    define_accounts_db_test!(test_add_uncleaned_pubkeys_after_shrink, |db| {
        let slot = 0;
        let pubkey = Pubkey::from([1; 32]);
        db.add_uncleaned_pubkeys_after_shrink(slot, vec![pubkey].into_iter());
        assert_eq!(&*db.uncleaned_pubkeys.get(&slot).unwrap(), &vec![pubkey]);
    });

    define_accounts_db_test!(test_get_ancient_slots, |db| {
        let slot1 = 1;

        // there has to be an existing append vec at this slot for a new current ancient at the slot to make sense
        let storages = (0..3)
            .map(|i| db.create_and_insert_store(slot1 + (i as Slot), 1000, "test"))
            .collect::<Vec<_>>();

        for count in 1..4 {
            // use subset of storages
            let mut raw_storages = storages.clone();
            raw_storages.truncate(count);
            let snapshot_storages = SortedStorages::new(&raw_storages);
            // 0 = all storages are non-ancient
            // 1 = all storages are non-ancient
            // 2 = ancient slots: 1
            // 3 = ancient slots: 1, 2
            // 4 = ancient slots: 1, 2, 3
            // 5 = ...
            for all_are_large in [false, true] {
                for oldest_non_ancient_slot in 0..6 {
                    let ancient_slots = SplitAncientStorages::get_ancient_slots(
                        oldest_non_ancient_slot,
                        &snapshot_storages,
                        |_storage| all_are_large,
                    );

                    if all_are_large {
                        assert_eq!(
                            raw_storages
                                .iter()
                                .filter_map(|storage| {
                                    let slot = storage.slot();
                                    (slot < oldest_non_ancient_slot).then_some(slot)
                                })
                                .collect::<Vec<_>>(),
                            ancient_slots,
                            "count: {count}"
                        );
                    } else {
                        // none are treated as ancient since none were deemed large enough append vecs.
                        assert!(ancient_slots.is_empty());
                    }
                }
            }
        }
    });

    define_accounts_db_test!(test_get_ancient_slots_one_large, |db| {
        let slot1 = 1;

        // there has to be an existing append vec at this slot for a new current ancient at the slot to make sense
        let storages = (0..3)
            .map(|i| db.create_and_insert_store(slot1 + (i as Slot), 1000, "test"))
            .collect::<Vec<_>>();

        for count in 1..4 {
            // use subset of storages
            let mut raw_storages = storages.clone();
            raw_storages.truncate(count);
            let snapshot_storages = SortedStorages::new(&raw_storages);
            // 0 = all storages are non-ancient
            // 1 = all storages are non-ancient
            // 2 = ancient slots: 1
            // 3 = ancient slots: 1, 2
            // 4 = ancient slots: 1, 2 (except 2 is large, 3 is not, so treat 3 as non-ancient)
            // 5 = ...
            for oldest_non_ancient_slot in 0..6 {
                let ancient_slots = SplitAncientStorages::get_ancient_slots(
                    oldest_non_ancient_slot,
                    &snapshot_storages,
                    |storage| storage.slot() == 2,
                );
                let mut expected = raw_storages
                    .iter()
                    .filter_map(|storage| {
                        let slot = storage.slot();
                        (slot < oldest_non_ancient_slot).then_some(slot)
                    })
                    .collect::<Vec<_>>();
                if expected.len() >= 2 {
                    // slot 3 is not considered ancient since slot 3 is a small append vec.
                    // slot 2 is the only large append vec, so 1 by itself is not ancient. [1, 2] is ancient, [1,2,3] becomes just [1,2]
                    expected.truncate(2);
                } else {
                    // we're not asking about the big append vec at 2, so nothing
                    expected.clear();
                }
                assert_eq!(expected, ancient_slots, "count: {count}");
            }
        }
    });

    #[test]
    fn test_hash_storage_info() {
        {
            let hasher = DefaultHasher::new();
            let hash = hasher.finish();
            assert_eq!(15130871412783076140, hash);
        }
        {
            let mut hasher = DefaultHasher::new();
            let slot: Slot = 0;
            let tf = crate::append_vec::test_utils::get_append_vec_path("test_hash_storage_info");
            let pubkey1 = solana_sdk::pubkey::new_rand();
            let mark_alive = false;
            let storage = sample_storage_with_entries(&tf, slot, &pubkey1, mark_alive);

            let load = AccountsDb::hash_storage_info(&mut hasher, &storage, slot);
            let hash = hasher.finish();
            // can't assert hash here - it is a function of mod date
            assert!(load);
            let slot = 2; // changed this
            let mut hasher = DefaultHasher::new();
            let load = AccountsDb::hash_storage_info(&mut hasher, &storage, slot);
            let hash2 = hasher.finish();
            assert_ne!(hash, hash2); // slot changed, these should be different
                                     // can't assert hash here - it is a function of mod date
            assert!(load);
            let mut hasher = DefaultHasher::new();
            append_sample_data_to_storage(&storage, &solana_sdk::pubkey::new_rand(), false, None);
            let load = AccountsDb::hash_storage_info(&mut hasher, &storage, slot);
            let hash3 = hasher.finish();
            assert_ne!(hash2, hash3); // moddate and written size changed
                                      // can't assert hash here - it is a function of mod date
            assert!(load);
            let mut hasher = DefaultHasher::new();
            let load = AccountsDb::hash_storage_info(&mut hasher, &storage, slot);
            let hash4 = hasher.finish();
            assert_eq!(hash4, hash3); // same
                                      // can't assert hash here - it is a function of mod date
            assert!(load);
        }
    }

    #[test]
    fn test_sweep_get_oldest_non_ancient_slot_max() {
        let epoch_schedule = EpochSchedule::default();
        // way into future
        for ancient_append_vec_offset in [
            epoch_schedule.slots_per_epoch,
            epoch_schedule.slots_per_epoch + 1,
            epoch_schedule.slots_per_epoch * 2,
        ] {
            let db = AccountsDb::new_with_config(
                Vec::new(),
                &ClusterType::Development,
                AccountSecondaryIndexes::default(),
                AccountShrinkThreshold::default(),
                Some(AccountsDbConfig {
                    ancient_append_vec_offset: Some(ancient_append_vec_offset as i64),
                    ..ACCOUNTS_DB_CONFIG_FOR_TESTING
                }),
                None,
                Arc::default(),
            );
            // before any roots are added, we expect the oldest non-ancient slot to be 0
            assert_eq!(0, db.get_oldest_non_ancient_slot(&epoch_schedule));
            for max_root_inclusive in [
                0,
                epoch_schedule.slots_per_epoch,
                epoch_schedule.slots_per_epoch * 2,
                epoch_schedule.slots_per_epoch * 10,
            ] {
                db.add_root(max_root_inclusive);
                // oldest non-ancient will never exceed max_root_inclusive, even if the offset is so large it would mathematically move ancient PAST the newest root
                assert_eq!(
                    max_root_inclusive,
                    db.get_oldest_non_ancient_slot(&epoch_schedule)
                );
            }
        }
    }

    #[test]
    fn test_sweep_get_oldest_non_ancient_slot() {
        let epoch_schedule = EpochSchedule::default();
        let ancient_append_vec_offset = 50_000;
        let db = AccountsDb::new_with_config(
            Vec::new(),
            &ClusterType::Development,
            AccountSecondaryIndexes::default(),
            AccountShrinkThreshold::default(),
            Some(AccountsDbConfig {
                ancient_append_vec_offset: Some(ancient_append_vec_offset),
                ..ACCOUNTS_DB_CONFIG_FOR_TESTING
            }),
            None,
            Arc::default(),
        );
        // before any roots are added, we expect the oldest non-ancient slot to be 0
        assert_eq!(0, db.get_oldest_non_ancient_slot(&epoch_schedule));
        // adding roots until slots_per_epoch +/- ancient_append_vec_offset should still saturate to 0 as oldest non ancient slot
        let max_root_inclusive = AccountsDb::apply_offset_to_slot(0, ancient_append_vec_offset - 1);
        db.add_root(max_root_inclusive);
        // oldest non-ancient will never exceed max_root_inclusive
        assert_eq!(0, db.get_oldest_non_ancient_slot(&epoch_schedule));
        for offset in 0..3u64 {
            let max_root_inclusive = ancient_append_vec_offset as u64 + offset;
            db.add_root(max_root_inclusive);
            assert_eq!(
                0,
                db.get_oldest_non_ancient_slot(&epoch_schedule),
                "offset: {offset}"
            );
        }
        for offset in 0..3u64 {
            let max_root_inclusive = AccountsDb::apply_offset_to_slot(
                epoch_schedule.slots_per_epoch - 1,
                -ancient_append_vec_offset,
            ) + offset;
            db.add_root(max_root_inclusive);
            assert_eq!(
                offset,
                db.get_oldest_non_ancient_slot(&epoch_schedule),
                "offset: {offset}, max_root_inclusive: {max_root_inclusive}"
            );
        }
    }

    #[test]
    fn test_sweep_get_oldest_non_ancient_slot2() {
        // note that this test has to worry about saturation at 0 as we subtract `slots_per_epoch` and `ancient_append_vec_offset`
        let epoch_schedule = EpochSchedule::default();
        for ancient_append_vec_offset in [-10_000i64, 50_000] {
            // at `starting_slot_offset`=0, with a negative `ancient_append_vec_offset`, we expect saturation to 0
            // big enough to avoid all saturation issues.
            let avoid_saturation = 1_000_000;
            assert!(
                avoid_saturation
                    > epoch_schedule.slots_per_epoch + ancient_append_vec_offset.unsigned_abs()
            );
            for starting_slot_offset in [0, avoid_saturation] {
                let db = AccountsDb::new_with_config(
                    Vec::new(),
                    &ClusterType::Development,
                    AccountSecondaryIndexes::default(),
                    AccountShrinkThreshold::default(),
                    Some(AccountsDbConfig {
                        ancient_append_vec_offset: Some(ancient_append_vec_offset),
                        ..ACCOUNTS_DB_CONFIG_FOR_TESTING
                    }),
                    None,
                    Arc::default(),
                );
                // before any roots are added, we expect the oldest non-ancient slot to be 0
                assert_eq!(0, db.get_oldest_non_ancient_slot(&epoch_schedule));

                let ancient_append_vec_offset = db.ancient_append_vec_offset.unwrap();
                assert_ne!(ancient_append_vec_offset, 0);
                // try a few values to simulate a real validator
                for inc in [0, 1, 2, 3, 4, 5, 8, 10, 10, 11, 200, 201, 1_000] {
                    // oldest non-ancient slot is 1 greater than first ancient slot
                    let completed_slot =
                        epoch_schedule.slots_per_epoch + inc + starting_slot_offset;

                    // test get_oldest_non_ancient_slot, which is based off the largest root
                    db.add_root(completed_slot);
                    let expected_oldest_non_ancient_slot = AccountsDb::apply_offset_to_slot(
                        AccountsDb::apply_offset_to_slot(
                            completed_slot,
                            -((epoch_schedule.slots_per_epoch as i64).saturating_sub(1)),
                        ),
                        ancient_append_vec_offset,
                    );
                    assert_eq!(
                        expected_oldest_non_ancient_slot,
                        db.get_oldest_non_ancient_slot(&epoch_schedule)
                    );
                }
            }
        }
    }

    #[test]
    #[should_panic(expected = "called `Option::unwrap()` on a `None` value")]
    fn test_current_ancient_slot_assert() {
        let current_ancient = CurrentAncientAccountsFile::default();
        _ = current_ancient.slot();
    }

    #[test]
    #[should_panic(expected = "called `Option::unwrap()` on a `None` value")]
    fn test_current_ancient_append_vec_assert() {
        let current_ancient = CurrentAncientAccountsFile::default();
        _ = current_ancient.accounts_file();
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
            let mut current_ancient = CurrentAncientAccountsFile::new(slot, append_vec.clone());
            assert_eq!(current_ancient.slot(), slot);
            assert_eq!(current_ancient.id(), append_vec.id());
            assert_eq!(current_ancient.accounts_file().id(), append_vec.id());

            let _shrink_in_progress = current_ancient.create_if_necessary(slot2, &db, 0);
            assert_eq!(current_ancient.slot(), slot);
            assert_eq!(current_ancient.id(), append_vec.id());
        }

        {
            // create_if_necessary
            let db = AccountsDb::new_single_for_tests();
            // there has to be an existing append vec at this slot for a new current ancient at the slot to make sense
            let _existing_append_vec = db.create_and_insert_store(slot2, 1000, "test");

            let mut current_ancient = CurrentAncientAccountsFile::default();
            let mut _shrink_in_progress = current_ancient.create_if_necessary(slot2, &db, 0);
            let id = current_ancient.id();
            assert_eq!(current_ancient.slot(), slot2);
            assert!(is_ancient(&current_ancient.accounts_file().accounts));
            let slot3 = 3;
            // should do nothing
            let _shrink_in_progress = current_ancient.create_if_necessary(slot3, &db, 0);
            assert_eq!(current_ancient.slot(), slot2);
            assert_eq!(current_ancient.id(), id);
            assert!(is_ancient(&current_ancient.accounts_file().accounts));
        }

        {
            // create_ancient_append_vec
            let db = AccountsDb::new_single_for_tests();
            let mut current_ancient = CurrentAncientAccountsFile::default();
            // there has to be an existing append vec at this slot for a new current ancient at the slot to make sense
            let _existing_append_vec = db.create_and_insert_store(slot2, 1000, "test");

            {
                let _shrink_in_progress =
                    current_ancient.create_ancient_accounts_file(slot2, &db, 0);
            }
            let id = current_ancient.id();
            assert_eq!(current_ancient.slot(), slot2);
            assert!(is_ancient(&current_ancient.accounts_file().accounts));

            // there has to be an existing append vec at this slot for a new current ancient at the slot to make sense
            let _existing_append_vec = db.create_and_insert_store(slot3, 1000, "test");

            let mut _shrink_in_progress =
                current_ancient.create_ancient_accounts_file(slot3, &db, 0);
            assert_eq!(current_ancient.slot(), slot3);
            assert!(is_ancient(&current_ancient.accounts_file().accounts));
            assert_ne!(current_ancient.id(), id);
        }
    }

    define_accounts_db_test!(test_get_sorted_potential_ancient_slots, |db| {
        let ancient_append_vec_offset = db.ancient_append_vec_offset.unwrap();
        let epoch_schedule = EpochSchedule::default();
        let oldest_non_ancient_slot = db.get_oldest_non_ancient_slot(&epoch_schedule);
        assert!(db
            .get_sorted_potential_ancient_slots(oldest_non_ancient_slot)
            .is_empty());
        let root0 = 0;
        db.add_root(root0);
        let root1 = 1;
        db.add_root(root1);
        let oldest_non_ancient_slot = db.get_oldest_non_ancient_slot(&epoch_schedule);
        assert!(db
            .get_sorted_potential_ancient_slots(oldest_non_ancient_slot)
            .is_empty());
        let completed_slot = epoch_schedule.slots_per_epoch;
        db.accounts_index.add_root(completed_slot);
        let oldest_non_ancient_slot = db.get_oldest_non_ancient_slot(&epoch_schedule);
        // get_sorted_potential_ancient_slots uses 'less than' as opposed to 'less or equal'
        // so, we need to get more than an epoch away to get the first valid root
        assert!(db
            .get_sorted_potential_ancient_slots(oldest_non_ancient_slot)
            .is_empty());
        let completed_slot = epoch_schedule.slots_per_epoch + root0;
        db.accounts_index.add_root(AccountsDb::apply_offset_to_slot(
            completed_slot,
            -ancient_append_vec_offset,
        ));
        let oldest_non_ancient_slot = db.get_oldest_non_ancient_slot(&epoch_schedule);
        assert_eq!(
            db.get_sorted_potential_ancient_slots(oldest_non_ancient_slot),
            vec![root0]
        );
        let completed_slot = epoch_schedule.slots_per_epoch + root1;
        db.accounts_index.add_root(AccountsDb::apply_offset_to_slot(
            completed_slot,
            -ancient_append_vec_offset,
        ));
        let oldest_non_ancient_slot = db.get_oldest_non_ancient_slot(&epoch_schedule);
        assert_eq!(
            db.get_sorted_potential_ancient_slots(oldest_non_ancient_slot),
            vec![root0, root1]
        );
        db.accounts_index
            .roots_tracker
            .write()
            .unwrap()
            .alive_roots
            .remove(&root0);
        let oldest_non_ancient_slot = db.get_oldest_non_ancient_slot(&epoch_schedule);
        assert_eq!(
            db.get_sorted_potential_ancient_slots(oldest_non_ancient_slot),
            vec![root1]
        );
    });

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
                                let unique_accounts = db
                                    .get_unique_accounts_from_storage_for_shrink(
                                        &storage,
                                        &ShrinkStats::default(),
                                    );

                                let shrink_collect = db.shrink_collect::<AliveAccounts<'_>>(
                                    &storage,
                                    &unique_accounts,
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

                                assert_eq!(shrink_collect.slot, slot5);

                                assert_eq!(
                                    shrink_collect
                                        .alive_accounts
                                        .accounts
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
                                    assert_eq!(
                                        shrink_collect.alive_total_bytes,
                                        alive_total_one_account
                                    );
                                } else {
                                    assert_eq!(shrink_collect.alive_total_bytes, 0);
                                }
                                // expected_capacity is determined by what size append vec gets created when the write cache is flushed to an append vec.
                                let mut expected_capacity =
                                    (account_count * aligned_stored_size(space)) as u64;
                                if append_opposite_zero_lamport_account && space != 0 {
                                    // zero lamport accounts always write space = 0
                                    expected_capacity -= space as u64;
                                }

                                assert_eq!(shrink_collect.capacity, expected_capacity);
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

    pub(crate) const CAN_RANDOMLY_SHRINK_FALSE: bool = false;

    define_accounts_db_test!(test_combine_ancient_slots_empty, |db| {
        // empty slots
        db.combine_ancient_slots(Vec::default(), CAN_RANDOMLY_SHRINK_FALSE);
    });

    #[test]
    fn test_combine_ancient_slots_simple() {
        for alive in [false, true] {
            _ = get_one_ancient_append_vec_and_others(alive, 0);
        }
    }

    fn get_all_accounts_from_storages<'a>(
        storages: impl Iterator<Item = &'a Arc<AccountStorageEntry>>,
    ) -> Vec<(Pubkey, AccountSharedData)> {
        storages
            .flat_map(|storage| {
                let mut vec = Vec::default();
                storage.accounts.scan_accounts(|account| {
                    vec.push((*account.pubkey(), account.to_account_shared_data()));
                });
                // make sure scan_pubkeys results match
                // Note that we assume traversals are both in the same order, but this doesn't have to be true.
                let mut compare = Vec::default();
                storage.accounts.scan_pubkeys(|k| {
                    compare.push(*k);
                });
                assert_eq!(compare, vec.iter().map(|(k, _)| *k).collect::<Vec<_>>());
                vec
            })
            .collect::<Vec<_>>()
    }

    pub(crate) fn get_all_accounts(
        db: &AccountsDb,
        slots: impl Iterator<Item = Slot>,
    ) -> Vec<(Pubkey, AccountSharedData)> {
        slots
            .filter_map(|slot| {
                let storage = db.storage.get_slot_storage_entry(slot);
                storage.map(|storage| get_all_accounts_from_storages(std::iter::once(&storage)))
            })
            .flatten()
            .collect::<Vec<_>>()
    }

    pub(crate) fn compare_all_accounts(
        one: &[(Pubkey, AccountSharedData)],
        two: &[(Pubkey, AccountSharedData)],
    ) {
        let mut failures = 0;
        let mut two_indexes = (0..two.len()).collect::<Vec<_>>();
        one.iter().for_each(|(pubkey, account)| {
            for i in 0..two_indexes.len() {
                let pubkey2 = two[two_indexes[i]].0;
                if pubkey2 == *pubkey {
                    if !accounts_equal(account, &two[two_indexes[i]].1) {
                        failures += 1;
                    }
                    two_indexes.remove(i);
                    break;
                }
            }
        });
        // helper method to reduce the volume of logged data to help identify differences
        // modify this when you hit a failure
        let clean = |accounts: &[(Pubkey, AccountSharedData)]| {
            accounts
                .iter()
                .map(|(_pubkey, account)| account.lamports())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            failures,
            0,
            "one: {:?}, two: {:?}, two_indexes: {:?}",
            clean(one),
            clean(two),
            two_indexes,
        );
        assert!(
            two_indexes.is_empty(),
            "one: {one:?}, two: {two:?}, two_indexes: {two_indexes:?}"
        );
    }

    #[test]
    fn test_shrink_ancient_overflow_with_min_size() {
        solana_logger::setup();

        let ideal_av_size = ancient_append_vecs::get_ancient_append_vec_capacity();
        let num_normal_slots = 2;

        // build an ancient append vec at slot 'ancient_slot' with one `fat`
        // account that's larger than the ideal size of ancient append vec to
        // simulate the *oversized* append vec for shrinking.
        let account_size = (1.5 * ideal_av_size as f64) as u64;
        let (db, ancient_slot) = get_one_ancient_append_vec_and_others_with_account_size(
            true,
            num_normal_slots,
            Some(account_size),
        );

        let max_slot_inclusive = ancient_slot + (num_normal_slots as Slot);
        let initial_accounts = get_all_accounts(&db, ancient_slot..(max_slot_inclusive + 1));

        let ancient = db.storage.get_slot_storage_entry(ancient_slot).unwrap();

        // assert that the min_size, which about 1.5 * ideal_av_size, kicked in
        // and result that the ancient append vec capacity exceeds the ideal_av_size
        assert!(ancient.capacity() > ideal_av_size);

        // combine 1 normal append vec into existing oversize ancient append vec.
        db.combine_ancient_slots(
            (ancient_slot..max_slot_inclusive).collect(),
            CAN_RANDOMLY_SHRINK_FALSE,
        );

        compare_all_accounts(
            &initial_accounts,
            &get_all_accounts(&db, ancient_slot..max_slot_inclusive),
        );

        // the append vec at max_slot_inclusive-1 should NOT have been removed
        // since the append vec is already oversized and we created an ancient
        // append vec there.
        let ancient2 = db
            .storage
            .get_slot_storage_entry(max_slot_inclusive - 1)
            .unwrap();
        assert!(is_ancient(&ancient2.accounts));
        assert!(ancient2.capacity() > ideal_av_size); // min_size kicked in, which cause the appendvec to be larger than the ideal_av_size

        // Combine normal append vec(s) into existing ancient append vec this
        // will overflow the original ancient append vec because of the oversized
        // ancient append vec is full.
        db.combine_ancient_slots(
            (ancient_slot..=max_slot_inclusive).collect(),
            CAN_RANDOMLY_SHRINK_FALSE,
        );

        compare_all_accounts(
            &initial_accounts,
            &get_all_accounts(&db, ancient_slot..(max_slot_inclusive + 1)),
        );

        // Nothing should be combined because the append vec are oversized.
        // min_size kicked in, which cause the appendvecs to be larger than the ideal_av_size.
        let ancient = db.storage.get_slot_storage_entry(ancient_slot).unwrap();
        assert!(is_ancient(&ancient.accounts));
        assert!(ancient.capacity() > ideal_av_size);

        let ancient2 = db
            .storage
            .get_slot_storage_entry(max_slot_inclusive - 1)
            .unwrap();
        assert!(is_ancient(&ancient2.accounts));
        assert!(ancient2.capacity() > ideal_av_size);

        let ancient3 = db
            .storage
            .get_slot_storage_entry(max_slot_inclusive)
            .unwrap();
        assert!(is_ancient(&ancient3.accounts));
        assert!(ancient3.capacity() > ideal_av_size);
    }

    #[test]
    fn test_shink_overflow_too_much() {
        let num_normal_slots = 2;
        let ideal_av_size = ancient_append_vecs::get_ancient_append_vec_capacity();
        let fat_account_size = (1.5 * ideal_av_size as f64) as u64;

        // Prepare 3 append vecs to combine [small, big, small]
        let account_data_sizes = vec![100, fat_account_size, 100];
        let (db, slot1) = create_db_with_storages_and_index_with_customized_account_size_per_slot(
            true,
            num_normal_slots + 1,
            account_data_sizes,
        );
        let storage = db.get_storage_for_slot(slot1).unwrap();
        let created_accounts = db.get_unique_accounts_from_storage(&storage);

        // Adjust alive_ratio for slot2 to test it is shrinkable and is a
        // candidate for squashing into the previous ancient append vec.
        // However, due to the fact that this append vec is `oversized`, it can't
        // be squashed into the ancient append vec at previous slot (exceeds the
        // size limit). Therefore, a new "oversized" ancient append vec is
        // created at slot2 as the overflow. This is where the "min_bytes" in
        // `fn create_ancient_append_vec` is used.
        let slot2 = slot1 + 1;
        let storage2 = db.storage.get_slot_storage_entry(slot2).unwrap();
        let original_cap_slot2 = storage2.accounts.capacity();
        storage2
            .accounts
            .set_current_len_for_tests(original_cap_slot2 as usize);

        // Combine append vec into ancient append vec.
        let slots_to_combine: Vec<Slot> = (slot1..slot1 + (num_normal_slots + 1) as Slot).collect();
        db.combine_ancient_slots(slots_to_combine, CAN_RANDOMLY_SHRINK_FALSE);

        // slot2 is too big to fit into ideal ancient append vec at slot1. So slot2 won't be merged into slot1.
        // slot1 will have its own ancient append vec.
        assert!(db.storage.get_slot_storage_entry(slot1).is_some());
        let ancient = db.get_storage_for_slot(slot1).unwrap();
        assert!(is_ancient(&ancient.accounts));
        assert_eq!(ancient.capacity(), ideal_av_size);

        let after_store = db.get_storage_for_slot(slot1).unwrap();
        let GetUniqueAccountsResult {
            stored_accounts: after_stored_accounts,
            capacity: after_capacity,
            ..
        } = db.get_unique_accounts_from_storage(&after_store);
        assert!(created_accounts.capacity <= after_capacity);
        assert_eq!(created_accounts.stored_accounts.len(), 1);
        assert_eq!(after_stored_accounts.len(), 1);

        // slot2, even after shrinking, is still oversized. Therefore, slot 2
        // exists as an ancient append vec.
        let storage2_after = db.storage.get_slot_storage_entry(slot2).unwrap();
        assert!(is_ancient(&storage2_after.accounts));
        assert!(storage2_after.capacity() > ideal_av_size);
        let after_store = db.get_storage_for_slot(slot2).unwrap();
        let GetUniqueAccountsResult {
            stored_accounts: after_stored_accounts,
            capacity: after_capacity,
            ..
        } = db.get_unique_accounts_from_storage(&after_store);
        assert!(created_accounts.capacity <= after_capacity);
        assert_eq!(created_accounts.stored_accounts.len(), 1);
        assert_eq!(after_stored_accounts.len(), 1);
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
        assert!(db
            .storage
            .get_slot_storage_entry(max_slot_inclusive)
            .is_none());
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
        let next_slot = max_slot_inclusive + 1;
        create_storages_and_update_index(&db, None, next_slot, num_normal_slots, true, None);
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
        let mut current_ancient = CurrentAncientAccountsFile::new(
            ancient_slot,
            db.get_storage_for_slot(ancient_slot).unwrap(),
        );
        let mut dropped_roots = Vec::default();
        db.combine_one_store_into_ancient(
            next_slot,
            &db.get_storage_for_slot(next_slot).unwrap(),
            &mut current_ancient,
            &mut AncientSlotPubkeys::default(),
            &mut dropped_roots,
        );
        assert!(db.storage.is_empty_entry(next_slot));
        // this removes the storages entry completely from the hashmap for 'next_slot'.
        // Otherwise, we have a zero length vec in that hashmap
        db.handle_dropped_roots_for_ancient(dropped_roots.into_iter());
        assert!(db.storage.get_slot_storage_entry(next_slot).is_none());

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
        assert!(db.storage.get_slot_storage_entry(ancient_slot).is_some());
        assert!(is_ancient(
            &db.storage
                .get_slot_storage_entry(ancient_slot)
                .unwrap()
                .accounts
        ));
        ((ancient_slot + 1)..=max_slot_inclusive)
            .for_each(|slot| assert!(db.storage.get_slot_storage_entry(slot).is_none()));
    }

    pub fn get_account_from_account_from_storage(
        account: &AccountFromStorage,
        db: &AccountsDb,
        slot: Slot,
    ) -> AccountSharedData {
        let storage = db
            .storage
            .get_slot_storage_entry_shrinking_in_progress_ok(slot)
            .unwrap();
        storage
            .accounts
            .get_account_shared_data(account.index_info.offset())
            .unwrap()
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
                        let original_pubkey = original
                            .accounts
                            .get_stored_account_meta_callback(0, |account| *account.pubkey())
                            .unwrap();
                        let slot = ancient_slot + 1 + (count_marked_dead as Slot);
                        _ = db.purge_keys_exact(
                            [(
                                original_pubkey,
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
                assert!(db.storage.get_slot_storage_entry(ancient_slot).is_some());
                let ancient = db.get_storage_for_slot(ancient_slot).unwrap();
                assert!(is_ancient(&ancient.accounts));
                let first_alive = ancient_slot + 1 + (dead_accounts as Slot);
                for slot in first_alive..=max_slot_inclusive {
                    assert!(db.storage.get_slot_storage_entry(slot).is_none());
                }

                let GetUniqueAccountsResult {
                    stored_accounts: mut after_stored_accounts,
                    ..
                } = db.get_unique_accounts_from_storage(&ancient);
                assert_eq!(
                    after_stored_accounts.len(),
                    num_normal_slots + 1 - dead_accounts,
                    "normal_slots: {num_normal_slots}, dead_accounts: {dead_accounts}"
                );
                for original in &originals {
                    let i = original
                        .accounts
                        .get_stored_account_meta_callback(0, |original| {
                            after_stored_accounts
                                .iter()
                                .enumerate()
                                .find_map(|(i, stored_ancient)| {
                                    (stored_ancient.pubkey() == original.pubkey()).then_some({
                                        assert!(accounts_equal(
                                            &get_account_from_account_from_storage(
                                                stored_ancient,
                                                &db,
                                                ancient_slot
                                            ),
                                            &original
                                        ));
                                        i
                                    })
                                })
                                .expect("did not find account")
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
                storage.accounts.scan_accounts(|account| {
                    let info = AccountInfo::new(
                        StorageLocation::AppendVec(storage.id(), account.offset()),
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

    pub(crate) fn remove_account_for_tests(
        storage: &AccountStorageEntry,
        num_bytes: usize,
        reset_accounts: bool,
    ) {
        storage.remove_accounts(num_bytes, reset_accounts, 1);
    }

    pub(crate) fn create_storages_and_update_index_with_customized_account_size_per_slot(
        db: &AccountsDb,
        tf: Option<&TempFile>,
        starting_slot: Slot,
        num_slots: usize,
        alive: bool,
        account_data_sizes: Vec<u64>,
    ) {
        if num_slots == 0 {
            return;
        }
        assert!(account_data_sizes.len() == num_slots);
        let local_tf = (tf.is_none()).then(|| {
            crate::append_vec::test_utils::get_append_vec_path("create_storages_and_update_index")
        });
        let tf = tf.unwrap_or_else(|| local_tf.as_ref().unwrap());

        let starting_id = db
            .storage
            .iter()
            .map(|storage| storage.1.id())
            .max()
            .unwrap_or(999);
        for (i, account_data_size) in account_data_sizes.iter().enumerate().take(num_slots) {
            let id = starting_id + (i as AccountsFileId);
            let pubkey1 = solana_sdk::pubkey::new_rand();
            let storage = sample_storage_with_entries_id_fill_percentage(
                tf,
                starting_slot + (i as Slot),
                &pubkey1,
                id,
                alive,
                Some(*account_data_size),
                50,
            );
            insert_store(db, Arc::clone(&storage));
        }

        let storage = db.get_storage_for_slot(starting_slot).unwrap();
        let created_accounts = db.get_unique_accounts_from_storage(&storage);
        assert_eq!(created_accounts.stored_accounts.len(), 1);

        if alive {
            populate_index(db, starting_slot..(starting_slot + (num_slots as Slot) + 1));
        }
    }

    pub(crate) fn create_storages_and_update_index(
        db: &AccountsDb,
        tf: Option<&TempFile>,
        starting_slot: Slot,
        num_slots: usize,
        alive: bool,
        account_data_size: Option<u64>,
    ) {
        if num_slots == 0 {
            return;
        }

        let local_tf = (tf.is_none()).then(|| {
            crate::append_vec::test_utils::get_append_vec_path("create_storages_and_update_index")
        });
        let tf = tf.unwrap_or_else(|| local_tf.as_ref().unwrap());

        let starting_id = db
            .storage
            .iter()
            .map(|storage| storage.1.id())
            .max()
            .unwrap_or(999);
        for i in 0..num_slots {
            let id = starting_id + (i as AccountsFileId);
            let pubkey1 = solana_sdk::pubkey::new_rand();
            let storage = sample_storage_with_entries_id(
                tf,
                starting_slot + (i as Slot),
                &pubkey1,
                id,
                alive,
                account_data_size,
            );
            insert_store(db, Arc::clone(&storage));
        }

        let storage = db.get_storage_for_slot(starting_slot).unwrap();
        let created_accounts = db.get_unique_accounts_from_storage(&storage);
        assert_eq!(created_accounts.stored_accounts.len(), 1);

        if alive {
            populate_index(db, starting_slot..(starting_slot + (num_slots as Slot) + 1));
        }
    }

    pub(crate) fn create_db_with_storages_and_index(
        alive: bool,
        num_slots: usize,
        account_data_size: Option<u64>,
    ) -> (AccountsDb, Slot) {
        solana_logger::setup();

        let db = AccountsDb::new_single_for_tests();

        // create a single append vec with a single account in a slot
        // add the pubkey to index if alive
        // call combine_ancient_slots with the slot
        // verify we create an ancient appendvec that has alive accounts and does not have dead accounts

        let slot1 = 1;
        create_storages_and_update_index(&db, None, slot1, num_slots, alive, account_data_size);

        let slot1 = slot1 as Slot;
        (db, slot1)
    }

    pub(crate) fn create_db_with_storages_and_index_with_customized_account_size_per_slot(
        alive: bool,
        num_slots: usize,
        account_data_size: Vec<u64>,
    ) -> (AccountsDb, Slot) {
        solana_logger::setup();

        let db = AccountsDb::new_single_for_tests();

        // create a single append vec with a single account in a slot
        // add the pubkey to index if alive
        // call combine_ancient_slots with the slot
        // verify we create an ancient appendvec that has alive accounts and does not have dead accounts

        let slot1 = 1;
        create_storages_and_update_index_with_customized_account_size_per_slot(
            &db,
            None,
            slot1,
            num_slots,
            alive,
            account_data_size,
        );

        let slot1 = slot1 as Slot;
        (db, slot1)
    }

    fn get_one_ancient_append_vec_and_others_with_account_size(
        alive: bool,
        num_normal_slots: usize,
        account_data_size: Option<u64>,
    ) -> (AccountsDb, Slot) {
        let (db, slot1) =
            create_db_with_storages_and_index(alive, num_normal_slots + 1, account_data_size);
        let storage = db.get_storage_for_slot(slot1).unwrap();
        let created_accounts = db.get_unique_accounts_from_storage(&storage);

        db.combine_ancient_slots(vec![slot1], CAN_RANDOMLY_SHRINK_FALSE);
        assert!(db.storage.get_slot_storage_entry(slot1).is_some());
        let ancient = db.get_storage_for_slot(slot1).unwrap();
        assert_eq!(alive, is_ancient(&ancient.accounts));
        let after_store = db.get_storage_for_slot(slot1).unwrap();
        let GetUniqueAccountsResult {
            stored_accounts: after_stored_accounts,
            capacity: after_capacity,
            ..
        } = db.get_unique_accounts_from_storage(&after_store);
        if alive {
            assert!(created_accounts.capacity <= after_capacity);
        } else {
            assert_eq!(created_accounts.capacity, after_capacity);
        }
        assert_eq!(created_accounts.stored_accounts.len(), 1);
        // always 1 account: either we leave the append vec alone if it is all dead
        // or we create a new one and copy into it if account is alive
        assert_eq!(after_stored_accounts.len(), 1);
        (db, slot1)
    }

    fn get_one_ancient_append_vec_and_others(
        alive: bool,
        num_normal_slots: usize,
    ) -> (AccountsDb, Slot) {
        get_one_ancient_append_vec_and_others_with_account_size(alive, num_normal_slots, None)
    }

    #[test]
    fn test_handle_dropped_roots_for_ancient() {
        solana_logger::setup();
        let db = AccountsDb::new_single_for_tests();
        db.handle_dropped_roots_for_ancient(std::iter::empty::<Slot>());
        let slot0 = 0;
        let dropped_roots = vec![slot0];
        db.accounts_index.add_root(slot0);
        db.accounts_index.add_uncleaned_roots([slot0]);
        assert!(db.accounts_index.is_uncleaned_root(slot0));
        assert!(db.accounts_index.is_alive_root(slot0));
        db.handle_dropped_roots_for_ancient(dropped_roots.into_iter());
        assert!(!db.accounts_index.is_uncleaned_root(slot0));
        assert!(!db.accounts_index.is_alive_root(slot0));
    }

    fn insert_store(db: &AccountsDb, append_vec: Arc<AccountStorageEntry>) {
        db.storage.insert(append_vec.slot(), append_vec);
    }

    #[test]
    #[should_panic(expected = "self.storage.remove")]
    fn test_handle_dropped_roots_for_ancient_assert() {
        solana_logger::setup();
        let common_store_path = Path::new("");
        let store_file_size = 10_000;
        let entry = Arc::new(AccountStorageEntry::new(
            common_store_path,
            0,
            1,
            store_file_size,
            AccountsFileProvider::AppendVec,
        ));
        let db = AccountsDb::new_single_for_tests();
        let slot0 = 0;
        let dropped_roots = vec![slot0];
        insert_store(&db, entry);
        db.handle_dropped_roots_for_ancient(dropped_roots.into_iter());
    }

    #[test]
    fn test_should_move_to_ancient_accounts_file() {
        solana_logger::setup();
        let db = AccountsDb::new_single_for_tests();
        let slot5 = 5;
        let tf = crate::append_vec::test_utils::get_append_vec_path(
            "test_should_move_to_ancient_append_vec",
        );
        let pubkey1 = solana_sdk::pubkey::new_rand();
        let storage = sample_storage_with_entries(&tf, slot5, &pubkey1, false);
        let mut current_ancient = CurrentAncientAccountsFile::default();

        let should_move = db.should_move_to_ancient_accounts_file(
            &storage,
            &mut current_ancient,
            slot5,
            CAN_RANDOMLY_SHRINK_FALSE,
        );
        assert!(current_ancient.slot_and_accounts_file.is_none());
        // slot is not ancient, so it is good to move
        assert!(should_move);

        current_ancient = CurrentAncientAccountsFile::new(slot5, Arc::clone(&storage)); // just 'some', contents don't matter
        let should_move = db.should_move_to_ancient_accounts_file(
            &storage,
            &mut current_ancient,
            slot5,
            CAN_RANDOMLY_SHRINK_FALSE,
        );
        // should have kept the same 'current_ancient'
        assert_eq!(current_ancient.slot(), slot5);
        assert_eq!(current_ancient.accounts_file().slot(), slot5);
        assert_eq!(current_ancient.id(), storage.id());

        // slot is not ancient, so it is good to move
        assert!(should_move);

        // now, create an ancient slot and make sure that it does NOT think it needs to be moved and that it becomes the ancient append vec to use
        let mut current_ancient = CurrentAncientAccountsFile::default();
        let slot1_ancient = 1;
        // there has to be an existing append vec at this slot for a new current ancient at the slot to make sense
        let _existing_append_vec = db.create_and_insert_store(slot1_ancient, 1000, "test");
        let ancient1 = db
            .get_store_for_shrink(slot1_ancient, get_ancient_append_vec_capacity())
            .new_storage()
            .clone();
        let should_move = db.should_move_to_ancient_accounts_file(
            &ancient1,
            &mut current_ancient,
            slot1_ancient,
            CAN_RANDOMLY_SHRINK_FALSE,
        );
        assert!(!should_move);
        assert_eq!(current_ancient.id(), ancient1.id());
        assert_eq!(current_ancient.slot(), slot1_ancient);

        // current is ancient1
        // try to move ancient2
        // current should become ancient2
        let slot2_ancient = 2;
        let mut current_ancient = CurrentAncientAccountsFile::new(slot1_ancient, ancient1.clone());
        // there has to be an existing append vec at this slot for a new current ancient at the slot to make sense
        let _existing_append_vec = db.create_and_insert_store(slot2_ancient, 1000, "test");
        let ancient2 = db
            .get_store_for_shrink(slot2_ancient, get_ancient_append_vec_capacity())
            .new_storage()
            .clone();
        let should_move = db.should_move_to_ancient_accounts_file(
            &ancient2,
            &mut current_ancient,
            slot2_ancient,
            CAN_RANDOMLY_SHRINK_FALSE,
        );
        assert!(!should_move);
        assert_eq!(current_ancient.id(), ancient2.id());
        assert_eq!(current_ancient.slot(), slot2_ancient);

        // now try a full ancient append vec
        // current is None
        let slot3_full_ancient = 3;
        let mut current_ancient = CurrentAncientAccountsFile::default();
        // there has to be an existing append vec at this slot for a new current ancient at the slot to make sense
        let _existing_append_vec = db.create_and_insert_store(slot3_full_ancient, 1000, "test");
        let full_ancient_3 = make_full_ancient_accounts_file(&db, slot3_full_ancient, false);
        let should_move = db.should_move_to_ancient_accounts_file(
            &full_ancient_3.new_storage().clone(),
            &mut current_ancient,
            slot3_full_ancient,
            CAN_RANDOMLY_SHRINK_FALSE,
        );
        assert!(!should_move);
        assert_eq!(current_ancient.id(), full_ancient_3.new_storage().id());
        assert_eq!(current_ancient.slot(), slot3_full_ancient);

        // now set current_ancient to something
        let mut current_ancient = CurrentAncientAccountsFile::new(slot1_ancient, ancient1.clone());
        let should_move = db.should_move_to_ancient_accounts_file(
            &full_ancient_3.new_storage().clone(),
            &mut current_ancient,
            slot3_full_ancient,
            CAN_RANDOMLY_SHRINK_FALSE,
        );
        assert!(!should_move);
        assert_eq!(current_ancient.id(), full_ancient_3.new_storage().id());
        assert_eq!(current_ancient.slot(), slot3_full_ancient);

        // now mark the full ancient as candidate for shrink
        adjust_alive_bytes(full_ancient_3.new_storage(), 0);

        // should shrink here, returning none for current
        let mut current_ancient = CurrentAncientAccountsFile::default();
        let should_move = db.should_move_to_ancient_accounts_file(
            &full_ancient_3.new_storage().clone(),
            &mut current_ancient,
            slot3_full_ancient,
            CAN_RANDOMLY_SHRINK_FALSE,
        );
        assert!(should_move);
        assert!(current_ancient.slot_and_accounts_file.is_none());

        // should return true here, returning current from prior
        // now set current_ancient to something and see if it still goes to None
        let mut current_ancient = CurrentAncientAccountsFile::new(slot1_ancient, ancient1.clone());
        let should_move = db.should_move_to_ancient_accounts_file(
            &Arc::clone(full_ancient_3.new_storage()),
            &mut current_ancient,
            slot3_full_ancient,
            CAN_RANDOMLY_SHRINK_FALSE,
        );
        assert!(should_move);
        assert_eq!(current_ancient.id(), ancient1.id());
        assert_eq!(current_ancient.slot(), slot1_ancient);
    }

    fn adjust_alive_bytes(storage: &AccountStorageEntry, alive_bytes: usize) {
        storage.alive_bytes.store(alive_bytes, Ordering::Release);
    }

    /// cause 'ancient' to appear to contain 'len' bytes
    fn adjust_append_vec_len_for_tests(ancient: &AccountStorageEntry, len: usize) {
        assert!(is_ancient(&ancient.accounts));
        ancient.accounts.set_current_len_for_tests(len);
        adjust_alive_bytes(ancient, len);
    }

    fn make_ancient_append_vec_full(ancient: &AccountStorageEntry, mark_alive: bool) {
        for _ in 0..100 {
            append_sample_data_to_storage(ancient, &Pubkey::default(), mark_alive, None);
        }
        // since we're not adding to the index, this is how we specify that all these accounts are alive
        adjust_alive_bytes(ancient, ancient.capacity() as usize);
    }

    fn make_full_ancient_accounts_file(
        db: &AccountsDb,
        slot: Slot,
        mark_alive: bool,
    ) -> ShrinkInProgress<'_> {
        let full = db.get_store_for_shrink(slot, get_ancient_append_vec_capacity());
        make_ancient_append_vec_full(full.new_storage(), mark_alive);
        full
    }

    define_accounts_db_test!(test_calculate_incremental_accounts_hash, |accounts_db| {
        let owner = Pubkey::new_unique();
        let mut accounts: Vec<_> = (0..10)
            .map(|_| (Pubkey::new_unique(), AccountSharedData::new(0, 0, &owner)))
            .collect();

        // store some accounts into slot 0
        let slot = 0;
        {
            accounts[0].1.set_lamports(0);
            accounts[1].1.set_lamports(1);
            accounts[2].1.set_lamports(10);
            accounts[3].1.set_lamports(100);
            //accounts[4].1.set_lamports(1_000); <-- will be added next slot

            let accounts = vec![
                (&accounts[0].0, &accounts[0].1),
                (&accounts[1].0, &accounts[1].1),
                (&accounts[2].0, &accounts[2].1),
                (&accounts[3].0, &accounts[3].1),
            ];
            accounts_db.store_cached((slot, accounts.as_slice()), None);
            accounts_db.add_root_and_flush_write_cache(slot);
        }

        // store some accounts into slot 1
        let slot = slot + 1;
        {
            //accounts[0].1.set_lamports(0);      <-- unchanged
            accounts[1].1.set_lamports(0); /*     <-- drain account */
            //accounts[2].1.set_lamports(10);     <-- unchanged
            //accounts[3].1.set_lamports(100);    <-- unchanged
            accounts[4].1.set_lamports(1_000); /* <-- add account */

            let accounts = vec![
                (&accounts[1].0, &accounts[1].1),
                (&accounts[4].0, &accounts[4].1),
            ];
            accounts_db.store_cached((slot, accounts.as_slice()), None);
            accounts_db.add_root_and_flush_write_cache(slot);
        }

        // calculate the full accounts hash
        let full_accounts_hash = {
            accounts_db.clean_accounts(Some(slot - 1), false, &EpochSchedule::default());
            let (storages, _) = accounts_db.get_snapshot_storages(..=slot);
            let storages = SortedStorages::new(&storages);
            accounts_db.calculate_accounts_hash(
                &CalcAccountsHashConfig::default(),
                &storages,
                HashStats::default(),
            )
        };
        assert_eq!(full_accounts_hash.1, 1_110);
        let full_accounts_hash_slot = slot;

        // Calculate the expected full accounts hash here and ensure it matches.
        // Ensure the zero-lamport accounts are NOT included in the full accounts hash.
        let full_account_hashes = [(2, 0), (3, 0), (4, 1)].into_iter().map(|(index, _slot)| {
            let (pubkey, account) = &accounts[index];
            AccountsDb::hash_account(account, pubkey).0
        });
        let expected_accounts_hash = AccountsHash(compute_merkle_root(full_account_hashes));
        assert_eq!(full_accounts_hash.0, expected_accounts_hash);

        // store accounts into slot 2
        let slot = slot + 1;
        {
            //accounts[0].1.set_lamports(0);         <-- unchanged
            //accounts[1].1.set_lamports(0);         <-- unchanged
            accounts[2].1.set_lamports(0); /*        <-- drain account */
            //accounts[3].1.set_lamports(100);       <-- unchanged
            //accounts[4].1.set_lamports(1_000);     <-- unchanged
            accounts[5].1.set_lamports(10_000); /*   <-- add account */
            accounts[6].1.set_lamports(100_000); /*  <-- add account */
            //accounts[7].1.set_lamports(1_000_000); <-- will be added next slot

            let accounts = vec![
                (&accounts[2].0, &accounts[2].1),
                (&accounts[5].0, &accounts[5].1),
                (&accounts[6].0, &accounts[6].1),
            ];
            accounts_db.store_cached((slot, accounts.as_slice()), None);
            accounts_db.add_root_and_flush_write_cache(slot);
        }

        // store accounts into slot 3
        let slot = slot + 1;
        {
            //accounts[0].1.set_lamports(0);          <-- unchanged
            //accounts[1].1.set_lamports(0);          <-- unchanged
            //accounts[2].1.set_lamports(0);          <-- unchanged
            accounts[3].1.set_lamports(0); /*         <-- drain account */
            //accounts[4].1.set_lamports(1_000);      <-- unchanged
            accounts[5].1.set_lamports(0); /*         <-- drain account */
            //accounts[6].1.set_lamports(100_000);    <-- unchanged
            accounts[7].1.set_lamports(1_000_000); /* <-- add account */

            let accounts = vec![
                (&accounts[3].0, &accounts[3].1),
                (&accounts[5].0, &accounts[5].1),
                (&accounts[7].0, &accounts[7].1),
            ];
            accounts_db.store_cached((slot, accounts.as_slice()), None);
            accounts_db.add_root_and_flush_write_cache(slot);
        }

        // calculate the incremental accounts hash
        let incremental_accounts_hash = {
            accounts_db.set_latest_full_snapshot_slot(full_accounts_hash_slot);
            accounts_db.clean_accounts(Some(slot - 1), false, &EpochSchedule::default());
            let (storages, _) =
                accounts_db.get_snapshot_storages(full_accounts_hash_slot + 1..=slot);
            let storages = SortedStorages::new(&storages);
            accounts_db.calculate_incremental_accounts_hash(
                &CalcAccountsHashConfig::default(),
                &storages,
                HashStats::default(),
            )
        };
        assert_eq!(incremental_accounts_hash.1, 1_100_000);

        // Ensure the zero-lamport accounts are included in the IAH.
        // Accounts 2, 3, and 5 are all zero-lamports.
        let incremental_account_hashes =
            [(2, 2), (3, 3), (5, 3), (6, 2), (7, 3)]
                .into_iter()
                .map(|(index, _slot)| {
                    let (pubkey, account) = &accounts[index];
                    if account.is_zero_lamport() {
                        // For incremental accounts hash, the hash of a zero lamport account is the hash of its pubkey.
                        // Ensure this implementation detail remains in sync with AccountsHasher::de_dup_in_parallel().
                        let hash = blake3::hash(bytemuck::bytes_of(pubkey));
                        Hash::new_from_array(hash.into())
                    } else {
                        AccountsDb::hash_account(account, pubkey).0
                    }
                });
        let expected_accounts_hash =
            IncrementalAccountsHash(compute_merkle_root(incremental_account_hashes));
        assert_eq!(incremental_accounts_hash.0, expected_accounts_hash);
    });

    fn compute_merkle_root(hashes: impl IntoIterator<Item = Hash>) -> Hash {
        let hashes = hashes.into_iter().collect();
        AccountsHasher::compute_merkle_root_recurse(hashes, MERKLE_FANOUT)
    }
}

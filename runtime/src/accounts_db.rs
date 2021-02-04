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
//! a "write_version".  A single global atomic `AccountsDB::write_version`
//! tracks the number of commits to the entire data store. So the latest
//! commit for each slot entry would be indexed.

use crate::{
    accounts_cache::{AccountsCache, CachedAccount, SlotCache},
    accounts_index::{
        AccountIndex, AccountsIndex, AccountsIndexRootsStats, Ancestors, IndexKey, IsCached,
        SlotList, SlotSlice, ZeroLamport,
    },
    append_vec::{AppendVec, StoredAccountMeta, StoredMeta},
    contains::Contains,
};
use blake3::traits::digest::Digest;
use dashmap::{
    mapref::entry::Entry::{Occupied, Vacant},
    DashMap, DashSet,
};
use lazy_static::lazy_static;
use log::*;
use rand::{prelude::SliceRandom, thread_rng, Rng};
use rayon::{prelude::*, ThreadPool};
use serde::{Deserialize, Serialize};
use solana_measure::measure::Measure;
use solana_rayon_threadlimit::get_thread_count;
use solana_sdk::{
    account::Account,
    clock::{Epoch, Slot},
    genesis_config::ClusterType,
    hash::{Hash, Hasher},
    pubkey::Pubkey,
};
use solana_vote_program::vote_state::MAX_LOCKOUT_HISTORY;
use std::{
    borrow::Cow,
    boxed::Box,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    convert::{TryFrom, TryInto},
    io::{Error as IOError, Result as IOResult},
    ops::RangeBounds,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    sync::{Arc, Mutex, MutexGuard, RwLock},
    time::Instant,
};
use tempfile::TempDir;

const PAGE_SIZE: u64 = 4 * 1024;
const MAX_RECYCLE_STORES: usize = 1000;
const STORE_META_OVERHEAD: usize = 256;
const MAX_CACHE_SLOTS: usize = 200;
const FLUSH_CACHE_RANDOM_THRESHOLD: usize = MAX_LOCKOUT_HISTORY;
const SCAN_SLOT_PAR_ITER_THRESHOLD: usize = 4000;

pub const DEFAULT_FILE_SIZE: u64 = PAGE_SIZE * 1024;
pub const DEFAULT_NUM_THREADS: u32 = 8;
pub const DEFAULT_NUM_DIRS: u32 = 4;
pub const SHRINK_RATIO: f64 = 0.80;

// A specially reserved storage id just for entries in the cache, so that
// operations that take a storage entry can maintain a common interface
// when interacting with cached accounts. This id is "virtual" in that it
// doesn't actually refer to an actual storage entry.
const CACHE_VIRTUAL_STORAGE_ID: usize = AppendVecId::MAX;

// A specially reserved write version (identifier for ordering writes in an AppendVec)
// for entries in the cache, so that  operations that take a storage entry can maintain
// a common interface when interacting with cached accounts. This version is "virtual" in
// that it doesn't actually map to an entry in an AppendVec.
const CACHE_VIRTUAL_WRITE_VERSION: u64 = 0;

// A specially reserved offset (represents an offset into an AppendVec)
// for entries in the cache, so that  operations that take a storage entry can maintain
// a common interface when interacting with cached accounts. This version is "virtual" in
// that it doesn't actually map to an entry in an AppendVec.
const CACHE_VIRTUAL_OFFSET: usize = 0;
const CACHE_VIRTUAL_STORED_SIZE: usize = 0;

const MERKLE_FANOUT: usize = 16;

type DashMapVersionHash = DashMap<Pubkey, (u64, Hash)>;

lazy_static! {
    // FROZEN_ACCOUNT_PANIC is used to signal local_cluster that an AccountsDB panic has occurred,
    // as |cargo test| cannot observe panics in other threads
    pub static ref FROZEN_ACCOUNT_PANIC: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
}

pub enum ScanStorageResult<R, B> {
    Cached(Vec<R>),
    Stored(B),
}

#[derive(Debug, Default)]
pub struct ErrorCounters {
    pub total: usize,
    pub account_in_use: usize,
    pub account_loaded_twice: usize,
    pub account_not_found: usize,
    pub blockhash_not_found: usize,
    pub blockhash_too_old: usize,
    pub call_chain_too_deep: usize,
    pub duplicate_signature: usize,
    pub instruction_error: usize,
    pub insufficient_funds: usize,
    pub invalid_account_for_fee: usize,
    pub invalid_account_index: usize,
    pub invalid_program_for_execution: usize,
    pub not_allowed_during_cluster_maintenance: usize,
}

#[derive(Default, Debug, PartialEq, Clone)]
pub struct AccountInfo {
    /// index identifying the append storage
    store_id: AppendVecId,

    /// offset into the storage
    offset: usize,

    /// needed to track shrink candidacy in bytes. Used to update the number
    /// of alive bytes in an AppendVec as newer slots purge outdated entries
    stored_size: usize,

    /// lamports in the account used when squashing kept for optimization
    /// purposes to remove accounts with zero balance.
    lamports: u64,
}
impl IsCached for AccountInfo {
    fn is_cached(&self) -> bool {
        self.store_id == CACHE_VIRTUAL_STORAGE_ID
    }
}

impl ZeroLamport for AccountInfo {
    fn is_zero_lamport(&self) -> bool {
        self.lamports == 0
    }
}

/// An offset into the AccountsDB::storage vector
pub type AppendVecId = usize;
pub type SnapshotStorage = Vec<Arc<AccountStorageEntry>>;
pub type SnapshotStorages = Vec<SnapshotStorage>;

// Each slot has a set of storage entries.
pub(crate) type SlotStores = Arc<RwLock<HashMap<usize, Arc<AccountStorageEntry>>>>;

type AccountSlots = HashMap<Pubkey, HashSet<Slot>>;
type AppendVecOffsets = HashMap<AppendVecId, HashSet<usize>>;
type ReclaimResult = (AccountSlots, AppendVecOffsets);
type StorageFinder<'a> = Box<dyn Fn(Slot, usize) -> Arc<AccountStorageEntry> + 'a>;
type ShrinkCandidates = HashMap<Slot, HashMap<AppendVecId, Arc<AccountStorageEntry>>>;

#[derive(Default, Debug, PartialEq, Clone)]
struct CalculateHashIntermediate {
    pub version: u64,
    pub hash: Hash,
    pub lamports: u64,
    pub raw_lamports: u64,
    pub slot: Slot,
}

impl CalculateHashIntermediate {
    pub fn new(version: u64, hash: Hash, lamports: u64, raw_lamports: u64, slot: Slot) -> Self {
        Self {
            version,
            hash,
            lamports,
            raw_lamports,
            slot,
        }
    }
}

trait Versioned {
    fn version(&self) -> u64;
}

impl Versioned for (u64, Hash) {
    fn version(&self) -> u64 {
        self.0
    }
}

impl Versioned for CalculateHashIntermediate {
    fn version(&self) -> u64 {
        self.version
    }
}

impl Versioned for (u64, AccountInfo) {
    fn version(&self) -> u64 {
        self.0
    }
}

pub enum LoadedAccountAccessor<'a> {
    Stored(Option<(Arc<AccountStorageEntry>, usize)>),
    Cached((&'a AccountsCache, Slot, &'a Pubkey)),
}

impl<'a> LoadedAccountAccessor<'a> {
    fn get_loaded_account(&self) -> Option<LoadedAccount> {
        match self {
            LoadedAccountAccessor::Stored(storage_entry) => {
                // May not be present if slot was cleaned up in between
                storage_entry.as_ref().and_then(|(storage_entry, offset)| {
                    storage_entry
                        .get_stored_account_meta(*offset)
                        .map(LoadedAccount::Stored)
                })
            }
            LoadedAccountAccessor::Cached((cache, slot, pubkey)) => {
                // May not be present if slot was cleaned up in between
                cache.load(*slot, pubkey).map(|cached_account| {
                    LoadedAccount::Cached((**pubkey, Cow::Owned(cached_account)))
                })
            }
        }
    }
}

pub enum LoadedAccount<'a> {
    Stored(StoredAccountMeta<'a>),
    Cached((Pubkey, Cow<'a, CachedAccount>)),
}

impl<'a> LoadedAccount<'a> {
    pub fn owner(&self) -> &Pubkey {
        match self {
            LoadedAccount::Stored(stored_account_meta) => &stored_account_meta.account_meta.owner,
            LoadedAccount::Cached((_, cached_account)) => &cached_account.account.owner,
        }
    }

    pub fn executable(&self) -> bool {
        match self {
            LoadedAccount::Stored(stored_account_meta) => {
                stored_account_meta.account_meta.executable
            }
            LoadedAccount::Cached((_, cached_account)) => cached_account.account.executable,
        }
    }

    pub fn loaded_hash(&self) -> &Hash {
        match self {
            LoadedAccount::Stored(stored_account_meta) => &stored_account_meta.hash,
            LoadedAccount::Cached((_, cached_account)) => &cached_account.hash,
        }
    }

    pub fn pubkey(&self) -> &Pubkey {
        match self {
            LoadedAccount::Stored(stored_account_meta) => &stored_account_meta.meta.pubkey,
            LoadedAccount::Cached((pubkey, _)) => &pubkey,
        }
    }

    pub fn write_version(&self) -> u64 {
        match self {
            LoadedAccount::Stored(stored_account_meta) => stored_account_meta.meta.write_version,
            LoadedAccount::Cached(_) => CACHE_VIRTUAL_WRITE_VERSION,
        }
    }

    pub fn compute_hash(&self, slot: Slot, cluster_type: &ClusterType, pubkey: &Pubkey) -> Hash {
        match self {
            LoadedAccount::Stored(stored_account_meta) => {
                AccountsDB::hash_stored_account(slot, &stored_account_meta, cluster_type)
            }
            LoadedAccount::Cached((_, cached_account)) => {
                AccountsDB::hash_account(slot, &cached_account.account, pubkey, cluster_type)
            }
        }
    }

    pub fn stored_size(&self) -> usize {
        match self {
            LoadedAccount::Stored(stored_account_meta) => stored_account_meta.stored_size,
            LoadedAccount::Cached(_) => CACHE_VIRTUAL_STORED_SIZE,
        }
    }

    pub fn lamports(&self) -> u64 {
        match self {
            LoadedAccount::Stored(stored_account_meta) => stored_account_meta.account_meta.lamports,
            LoadedAccount::Cached((_, cached_account)) => cached_account.account.lamports,
        }
    }

    pub fn account(self) -> Account {
        match self {
            LoadedAccount::Stored(stored_account_meta) => stored_account_meta.clone_account(),
            LoadedAccount::Cached((_, cached_account)) => match cached_account {
                Cow::Owned(cached_account) => cached_account.account,
                Cow::Borrowed(cached_account) => cached_account.account.clone(),
            },
        }
    }
}

#[derive(Clone, Default, Debug)]
pub struct AccountStorage(pub DashMap<Slot, SlotStores>);

impl AccountStorage {
    fn get_account_storage_entry(
        &self,
        slot: Slot,
        store_id: AppendVecId,
    ) -> Option<Arc<AccountStorageEntry>> {
        self.get_slot_stores(slot)
            .and_then(|storage_map| storage_map.read().unwrap().get(&store_id).cloned())
    }

    fn get_slot_stores(&self, slot: Slot) -> Option<SlotStores> {
        self.0.get(&slot).map(|result| result.value().clone())
    }

    fn slot_store_count(&self, slot: Slot, store_id: AppendVecId) -> Option<usize> {
        self.get_account_storage_entry(slot, store_id)
            .map(|store| store.count())
    }

    fn all_slots(&self) -> Vec<Slot> {
        self.0.iter().map(|iter_item| *iter_item.key()).collect()
    }
}

#[derive(Debug, Eq, PartialEq, Copy, Clone, Deserialize, Serialize, AbiExample, AbiEnumVisitor)]
pub enum AccountStorageStatus {
    Available = 0,
    Full = 1,
    Candidate = 2,
}

impl Default for AccountStorageStatus {
    fn default() -> Self {
        Self::Available
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
    zero_lamport_key_clone_us: u64,
    delta_key_count: u64,
    zero_lamport_count: u64,
}

/// Persistent storage structure holding the accounts
#[derive(Debug)]
pub struct AccountStorageEntry {
    pub(crate) id: AtomicUsize,

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
    pub fn new(path: &Path, slot: Slot, id: usize, file_size: u64) -> Self {
        let tail = AppendVec::new_relative_path(slot, id);
        let path = Path::new(path).join(&tail);
        let accounts = AppendVec::new(&path, true, file_size as usize);

        Self {
            id: AtomicUsize::new(id),
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
            id: AtomicUsize::new(id),
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

    pub fn recycle(&self, slot: Slot, id: usize) {
        let mut count_and_status = self.count_and_status.write().unwrap();
        self.accounts.reset();
        *count_and_status = (0, AccountStorageStatus::Available);
        self.slot.store(slot, Ordering::Release);
        self.id.store(id, Ordering::Relaxed);
        self.approx_store_count.store(0, Ordering::Relaxed);
        self.alive_bytes.store(0, Ordering::Relaxed);
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
        self.id.load(Ordering::Relaxed)
    }

    pub fn flush(&self) -> Result<(), IOError> {
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

    pub fn get_relative_path(&self) -> Option<PathBuf> {
        AppendVec::get_relative_path(self.accounts.get_path())
    }

    pub fn get_path(&self) -> PathBuf {
        self.accounts.get_path()
    }
}

pub fn get_temp_accounts_paths(count: u32) -> IOResult<(Vec<TempDir>, Vec<PathBuf>)> {
    let temp_dirs: IOResult<Vec<TempDir>> = (0..count).map(|_| TempDir::new()).collect();
    let temp_dirs = temp_dirs?;
    let paths: Vec<PathBuf> = temp_dirs.iter().map(|t| t.path().to_path_buf()).collect();
    Ok((temp_dirs, paths))
}

#[derive(Clone, Default, Debug, Serialize, Deserialize, PartialEq, AbiExample)]
pub struct BankHashStats {
    pub num_updated_accounts: u64,
    pub num_removed_accounts: u64,
    pub num_lamports_stored: u64,
    pub total_data_len: u64,
    pub num_executable_accounts: u64,
}

impl BankHashStats {
    pub fn update(&mut self, account: &Account) {
        if account.lamports == 0 {
            self.num_removed_accounts += 1;
        } else {
            self.num_updated_accounts += 1;
        }
        self.total_data_len = self.total_data_len.wrapping_add(account.data.len() as u64);
        if account.executable {
            self.num_executable_accounts += 1;
        }
        self.num_lamports_stored = self.num_lamports_stored.wrapping_add(account.lamports);
    }

    pub fn merge(&mut self, other: &BankHashStats) {
        self.num_updated_accounts += other.num_updated_accounts;
        self.num_removed_accounts += other.num_removed_accounts;
        self.total_data_len = self.total_data_len.wrapping_add(other.total_data_len);
        self.num_lamports_stored = self
            .num_lamports_stored
            .wrapping_add(other.num_lamports_stored);
        self.num_executable_accounts += other.num_executable_accounts;
    }
}

#[derive(Clone, Default, Debug, Serialize, Deserialize, PartialEq, AbiExample)]
pub struct BankHashInfo {
    pub hash: Hash,
    pub snapshot_hash: Hash,
    pub stats: BankHashStats,
}

#[derive(Debug)]
struct FrozenAccountInfo {
    pub hash: Hash,    // Hash generated by hash_frozen_account_data()
    pub lamports: u64, // Account balance cannot be lower than this amount
}

#[derive(Default)]
pub struct StoreAccountsTiming {
    store_accounts_elapsed: u64,
    update_index_elapsed: u64,
    handle_reclaims_elapsed: u64,
}
// This structure handles the load/store of the accounts
#[derive(Debug)]
pub struct AccountsDB {
    /// Keeps tracks of index into AppendVec on a per slot basis
    pub accounts_index: AccountsIndex<AccountInfo>,

    pub storage: AccountStorage,

    pub accounts_cache: AccountsCache,

    recycle_stores: RwLock<Vec<Arc<AccountStorageEntry>>>,

    /// distribute the accounts across storage lists
    pub next_id: AtomicUsize,

    /// Set of shrinkable stores organized by map of slot to append_vec_id
    pub shrink_candidate_slots: Mutex<ShrinkCandidates>,

    /// Legacy shrink slots to support non-cached code-path.
    pub shrink_candidate_slots_v1: Mutex<Vec<Slot>>,

    pub(crate) write_version: AtomicU64,

    /// Set of storage paths to pick from
    pub(crate) paths: Vec<PathBuf>,

    pub shrink_paths: RwLock<Option<Vec<PathBuf>>>,

    /// Directory of paths this accounts_db needs to hold/remove
    temp_paths: Option<Vec<TempDir>>,

    /// Starting file size of appendvecs
    file_size: u64,

    /// Accounts that will cause a panic! if data modified or lamports decrease
    frozen_accounts: HashMap<Pubkey, FrozenAccountInfo>,

    /// Thread pool used for par_iter
    pub thread_pool: ThreadPool,

    pub thread_pool_clean: ThreadPool,

    /// Number of append vecs to create to maximize parallelism when scanning
    /// the accounts
    min_num_stores: usize,

    pub bank_hashes: RwLock<HashMap<Slot, BankHashInfo>>,

    stats: AccountsStats,

    clean_accounts_stats: CleanAccountsStats,

    // Stats for purges called outside of clean_accounts()
    external_purge_slots_stats: PurgeStats,

    shrink_stats: ShrinkStats,

    pub cluster_type: Option<ClusterType>,

    pub account_indexes: HashSet<AccountIndex>,

    pub caching_enabled: bool,

    /// Set of unique keys per slot which is used
    /// to drive clean_accounts
    /// Generated by get_accounts_delta_hash
    uncleaned_pubkeys: DashMap<Slot, Vec<Pubkey>>,
}

#[derive(Debug, Default)]
struct AccountsStats {
    delta_hash_scan_time_total_us: AtomicU64,
    delta_hash_accumulate_time_total_us: AtomicU64,
    delta_hash_num: AtomicU64,

    last_store_report: AtomicU64,
    store_hash_accounts: AtomicU64,
    store_accounts: AtomicU64,
    store_update_index: AtomicU64,
    store_handle_reclaims: AtomicU64,
    store_append_accounts: AtomicU64,
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
struct PurgeStats {
    last_report: AtomicU64,
    safety_checks_elapsed: AtomicU64,
    remove_storages_elapsed: AtomicU64,
    drop_storage_entries_elapsed: AtomicU64,
    num_cached_slots_removed: AtomicUsize,
    num_stored_slots_removed: AtomicUsize,
    total_removed_storage_entries: AtomicUsize,
    total_removed_cached_bytes: AtomicU64,
    total_removed_stored_bytes: AtomicU64,
    recycle_stores_write_elapsed: AtomicU64,
}

impl PurgeStats {
    fn report(&self, metric_name: &'static str, report_interval_ms: Option<u64>) {
        let should_report = report_interval_ms
            .map(|report_interval_ms| {
                let last = self.last_report.load(Ordering::Relaxed);
                let now = solana_sdk::timing::timestamp();
                now.saturating_sub(last) > report_interval_ms
                    && self.last_report.compare_exchange(
                        last,
                        now,
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                    ) == Ok(last)
            })
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
                    "remove_storages_elapsed",
                    self.remove_storages_elapsed.swap(0, Ordering::Relaxed) as i64,
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
            );
        }
    }
}

#[derive(Debug, Default)]
struct LatestAccountsIndexRootsStats {
    roots_len: AtomicUsize,
    uncleaned_roots_len: AtomicUsize,
    previous_uncleaned_roots_len: AtomicUsize,
}

impl LatestAccountsIndexRootsStats {
    fn update(&self, accounts_index_roots_stats: &AccountsIndexRootsStats) {
        self.roots_len
            .store(accounts_index_roots_stats.roots_len, Ordering::Relaxed);
        self.uncleaned_roots_len.store(
            accounts_index_roots_stats.uncleaned_roots_len,
            Ordering::Relaxed,
        );
        self.previous_uncleaned_roots_len.store(
            accounts_index_roots_stats.previous_uncleaned_roots_len,
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
                "previous_uncleaned_roots_len",
                self.previous_uncleaned_roots_len.load(Ordering::Relaxed) as i64,
                i64
            ),
        );

        // Don't need to reset since this tracks the latest updates, not a cumulative total
    }
}

#[derive(Debug, Default)]
struct CleanAccountsStats {
    purge_stats: PurgeStats,
    latest_accounts_index_roots_stats: LatestAccountsIndexRootsStats,
}

impl CleanAccountsStats {
    fn report(&self) {
        self.purge_stats.report("clean_purge_slots_stats", None);
        self.latest_accounts_index_roots_stats.report();
    }
}

#[derive(Debug, Default)]
struct ShrinkStats {
    last_report: AtomicU64,
    num_slots_shrunk: AtomicUsize,
    storage_read_elapsed: AtomicU64,
    index_read_elapsed: AtomicU64,
    find_alive_elapsed: AtomicU64,
    create_and_insert_store_elapsed: AtomicU64,
    store_accounts_elapsed: AtomicU64,
    update_index_elapsed: AtomicU64,
    handle_reclaims_elapsed: AtomicU64,
    write_storage_elapsed: AtomicU64,
    rewrite_elapsed: AtomicU64,
    drop_storage_entries_elapsed: AtomicU64,
    recycle_stores_write_elapsed: AtomicU64,
    accounts_removed: AtomicUsize,
    bytes_removed: AtomicU64,
}

impl ShrinkStats {
    fn report(&self) {
        let last = self.last_report.load(Ordering::Relaxed);
        let now = solana_sdk::timing::timestamp();

        let should_report = now.saturating_sub(last) > 1000
            && self
                .last_report
                .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
                == Ok(last);

        if should_report {
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
                    "find_alive_elapsed",
                    self.find_alive_elapsed.swap(0, Ordering::Relaxed) as i64,
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
                    "write_storage_elapsed",
                    self.write_storage_elapsed.swap(0, Ordering::Relaxed) as i64,
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
            );
        }
    }
}

fn make_min_priority_thread_pool() -> ThreadPool {
    // Use lower thread count to reduce priority.
    let num_threads = std::cmp::max(2, num_cpus::get() / 4);
    rayon::ThreadPoolBuilder::new()
        .thread_name(|i| format!("solana-accounts-cleanup-{}", i))
        .num_threads(num_threads)
        .build()
        .unwrap()
}

#[cfg(all(test, RUSTC_WITH_SPECIALIZATION))]
impl solana_frozen_abi::abi_example::AbiExample for AccountsDB {
    fn example() -> Self {
        let accounts_db = AccountsDB::new_single();
        let key = Pubkey::default();
        let some_data_len = 5;
        let some_slot: Slot = 0;
        let account = Account::new(1, some_data_len, &key);
        accounts_db.store_uncached(some_slot, &[(&key, &account)]);
        accounts_db.add_root(0);

        accounts_db
    }
}

impl Default for AccountsDB {
    fn default() -> Self {
        let num_threads = get_thread_count();

        let mut bank_hashes = HashMap::new();
        bank_hashes.insert(0, BankHashInfo::default());
        AccountsDB {
            accounts_index: AccountsIndex::default(),
            storage: AccountStorage::default(),
            accounts_cache: AccountsCache::default(),
            recycle_stores: RwLock::new(Vec::new()),
            uncleaned_pubkeys: DashMap::new(),
            next_id: AtomicUsize::new(0),
            shrink_candidate_slots_v1: Mutex::new(Vec::new()),
            shrink_candidate_slots: Mutex::new(HashMap::new()),
            write_version: AtomicU64::new(0),
            paths: vec![],
            shrink_paths: RwLock::new(None),
            temp_paths: None,
            file_size: DEFAULT_FILE_SIZE,
            thread_pool: rayon::ThreadPoolBuilder::new()
                .num_threads(num_threads)
                .thread_name(|i| format!("solana-accounts-db-{}", i))
                .build()
                .unwrap(),
            thread_pool_clean: make_min_priority_thread_pool(),
            min_num_stores: num_threads,
            bank_hashes: RwLock::new(bank_hashes),
            frozen_accounts: HashMap::new(),
            external_purge_slots_stats: PurgeStats::default(),
            clean_accounts_stats: CleanAccountsStats::default(),
            shrink_stats: ShrinkStats::default(),
            stats: AccountsStats::default(),
            cluster_type: None,
            account_indexes: HashSet::new(),
            caching_enabled: false,
        }
    }
}

impl AccountsDB {
    pub fn new(paths: Vec<PathBuf>, cluster_type: &ClusterType) -> Self {
        AccountsDB::new_with_config(paths, cluster_type, HashSet::new(), false)
    }

    pub fn new_with_config(
        paths: Vec<PathBuf>,
        cluster_type: &ClusterType,
        account_indexes: HashSet<AccountIndex>,
        caching_enabled: bool,
    ) -> Self {
        let new = if !paths.is_empty() {
            Self {
                paths,
                temp_paths: None,
                cluster_type: Some(*cluster_type),
                account_indexes,
                caching_enabled,
                ..Self::default()
            }
        } else {
            // Create a temporary set of accounts directories, used primarily
            // for testing
            let (temp_dirs, paths) = get_temp_accounts_paths(DEFAULT_NUM_DIRS).unwrap();
            Self {
                paths,
                temp_paths: Some(temp_dirs),
                cluster_type: Some(*cluster_type),
                account_indexes,
                caching_enabled,
                ..Self::default()
            }
        };
        {
            for path in new.paths.iter() {
                std::fs::create_dir_all(path).expect("Create directory failed.");
            }
        }
        new
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

    pub fn new_single() -> Self {
        AccountsDB {
            min_num_stores: 0,
            ..AccountsDB::new(Vec::new(), &ClusterType::Development)
        }
    }

    fn new_storage_entry(&self, slot: Slot, path: &Path, size: u64) -> AccountStorageEntry {
        AccountStorageEntry::new(
            path,
            slot,
            self.next_id.fetch_add(1, Ordering::Relaxed),
            size,
        )
    }

    // Reclaim older states of rooted accounts for AccountsDB bloat mitigation
    fn clean_old_rooted_accounts(
        &self,
        purges_in_root: Vec<Pubkey>,
        max_clean_root: Option<Slot>,
    ) -> ReclaimResult {
        if purges_in_root.is_empty() {
            return (HashMap::new(), HashMap::new());
        }
        // This number isn't carefully chosen; just guessed randomly such that
        // the hot loop will be the order of ~Xms.
        const INDEX_CLEAN_BULK_COUNT: usize = 4096;

        let mut clean_rooted = Measure::start("clean_old_root-ms");
        let reclaim_vecs =
            purges_in_root
                .par_chunks(INDEX_CLEAN_BULK_COUNT)
                .map(|pubkeys: &[Pubkey]| {
                    let mut reclaims = Vec::new();
                    for pubkey in pubkeys {
                        self.accounts_index.clean_rooted_entries(
                            &pubkey,
                            &mut reclaims,
                            max_clean_root,
                            &self.account_indexes,
                        );
                    }
                    reclaims
                });
        let reclaims: Vec<_> = reclaim_vecs.flatten().collect();
        clean_rooted.stop();
        inc_new_counter_info!("clean-old-root-par-clean-ms", clean_rooted.as_ms() as usize);

        let mut measure = Measure::start("clean_old_root_reclaims");

        // Don't reset from clean, since the pubkeys in those stores may need to be unref'ed
        // and those stores may be used for background hashing.
        let reset_accounts = false;

        let mut reclaim_result = (HashMap::new(), HashMap::new());
        self.handle_reclaims(
            &reclaims,
            None,
            false,
            Some(&mut reclaim_result),
            reset_accounts,
        );
        measure.stop();
        debug!("{} {}", clean_rooted, measure);
        inc_new_counter_info!("clean-old-root-reclaim-ms", measure.as_ms() as usize);
        reclaim_result
    }

    fn do_reset_uncleaned_roots(&self, max_clean_root: Option<Slot>) {
        self.accounts_index.reset_uncleaned_roots(max_clean_root);
    }

    fn calc_delete_dependencies(
        purges: &HashMap<Pubkey, (SlotList<AccountInfo>, u64)>,
        store_counts: &mut HashMap<AppendVecId, (usize, HashSet<Pubkey>)>,
    ) {
        // Another pass to check if there are some filtered accounts which
        // do not match the criteria of deleting all appendvecs which contain them
        // then increment their storage count.
        let mut already_counted = HashSet::new();
        for (pubkey, (account_infos, ref_count_from_storage)) in purges.iter() {
            let no_delete = if account_infos.len() as u64 != *ref_count_from_storage {
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
                true
            } else {
                let mut no_delete = false;
                for (_slot, account_info) in account_infos {
                    debug!(
                        "calc_delete_dependencies()
                        storage id: {},
                        count len: {}",
                        account_info.store_id,
                        store_counts.get(&account_info.store_id).unwrap().0,
                    );
                    if store_counts.get(&account_info.store_id).unwrap().0 != 0 {
                        no_delete = true;
                        break;
                    }
                }
                no_delete
            };
            if no_delete {
                let mut pending_store_ids: HashSet<usize> = HashSet::new();
                for (_slot_id, account_info) in account_infos {
                    if !already_counted.contains(&account_info.store_id) {
                        pending_store_ids.insert(account_info.store_id);
                    }
                }
                while !pending_store_ids.is_empty() {
                    let id = pending_store_ids.iter().next().cloned().unwrap();
                    pending_store_ids.remove(&id);
                    if already_counted.contains(&id) {
                        continue;
                    }
                    store_counts.get_mut(&id).unwrap().0 += 1;
                    already_counted.insert(id);

                    let affected_pubkeys = &store_counts.get(&id).unwrap().1;
                    for key in affected_pubkeys {
                        for (_slot, account_info) in &purges.get(&key).unwrap().0 {
                            if !already_counted.contains(&account_info.store_id) {
                                pending_store_ids.insert(account_info.store_id);
                            }
                        }
                    }
                }
            }
        }
    }

    fn purge_keys_exact<'a, C: 'a>(
        &'a self,
        pubkey_to_slot_set: &'a [(Pubkey, C)],
    ) -> Vec<(u64, AccountInfo)>
    where
        C: Contains<'a, Slot>,
    {
        let mut reclaims = Vec::new();
        let mut dead_keys = Vec::new();

        for (pubkey, slots_set) in pubkey_to_slot_set {
            let is_empty = self.accounts_index.purge_exact(
                &pubkey,
                slots_set,
                &mut reclaims,
                &self.account_indexes,
            );
            if is_empty {
                dead_keys.push(pubkey);
            }
        }

        self.accounts_index
            .handle_dead_keys(&dead_keys, &self.account_indexes);
        reclaims
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

    fn collect_uncleaned_pubkeys_to_slot(&self, max_slot: Slot) -> (Vec<Vec<Pubkey>>, Slot) {
        let mut max_slot_in_uncleaned_pubkeys = 0;
        let slots: Vec<Slot> = self
            .uncleaned_pubkeys
            .iter()
            .filter_map(|entry| {
                let slot = entry.key();
                max_slot_in_uncleaned_pubkeys = max_slot_in_uncleaned_pubkeys.max(*slot);
                if *slot <= max_slot {
                    Some(*slot)
                } else {
                    None
                }
            })
            .collect();
        (
            slots
                .into_iter()
                .filter_map(|slot| {
                    let maybe_slot_keys = self.uncleaned_pubkeys.remove(&slot);
                    if self.accounts_index.is_root(slot) {
                        // Safe to unwrap on rooted slots since this is called from clean_accounts
                        // and only clean_accounts operates on rooted slots. purge_slots only
                        // operates on uncleaned_pubkeys
                        let (_slot, keys) = maybe_slot_keys.expect("Root slot should exist");
                        Some(keys)
                    } else {
                        None
                    }
                })
                .collect(),
            max_slot_in_uncleaned_pubkeys,
        )
    }

    // Construct a vec of pubkeys for cleaning from:
    //   uncleaned_pubkeys - the delta set of updated pubkeys in rooted slots from the last clean
    //   zero_lamport_pubkeys - set of all alive pubkeys containing 0-lamport updates
    fn construct_candidate_clean_keys(
        &self,
        max_clean_root: Option<Slot>,
        timings: &mut CleanKeyTimings,
    ) -> Vec<Pubkey> {
        let mut zero_lamport_key_clone = Measure::start("zero_lamport_key");
        let pubkeys = self.accounts_index.zero_lamport_pubkeys().clone();
        timings.zero_lamport_count = pubkeys.len() as u64;
        zero_lamport_key_clone.stop();
        timings.zero_lamport_key_clone_us += zero_lamport_key_clone.as_us();

        let mut collect_delta_keys = Measure::start("key_create");
        let max_slot = max_clean_root.unwrap_or_else(|| self.accounts_index.max_root());
        let (delta_keys, _max_slot) = self.collect_uncleaned_pubkeys_to_slot(max_slot);
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
        let pubkeys: Vec<Pubkey> = pubkeys.into_iter().collect();
        hashset_to_vec.stop();
        timings.hashset_to_vec_us += hashset_to_vec.as_us();

        pubkeys
    }

    // Purge zero lamport accounts and older rooted account states as garbage
    // collection
    // Only remove those accounts where the entire rooted history of the account
    // can be purged because there are no live append vecs in the ancestors
    pub fn clean_accounts(&self, max_clean_root: Option<Slot>) {
        let max_clean_root = self.max_clean_root(max_clean_root);

        // hold a lock to prevent slot shrinking from running because it might modify some rooted
        // slot storages which can not happen as long as we're cleaning accounts because we're also
        // modifying the rooted slot storages!
        let mut candidates_v1 = self.shrink_candidate_slots_v1.lock().unwrap();
        self.report_store_stats();

        let mut key_timings = CleanKeyTimings::default();
        let pubkeys = self.construct_candidate_clean_keys(max_clean_root, &mut key_timings);

        let total_keys_count = pubkeys.len();
        let mut accounts_scan = Measure::start("accounts_scan");
        // parallel scan the index.
        let (mut purges, purges_in_root) = {
            self.thread_pool_clean.install(|| {
                pubkeys
                    .par_chunks(4096)
                    .map(|pubkeys: &[Pubkey]| {
                        let mut purges_in_root = Vec::new();
                        let mut purges = HashMap::new();
                        for pubkey in pubkeys {
                            if let Some((locked_entry, index)) =
                                self.accounts_index.get(pubkey, None, max_clean_root)
                            {
                                let slot_list = locked_entry.slot_list();
                                let (slot, account_info) = &slot_list[index];
                                if account_info.lamports == 0 {
                                    purges.insert(
                                        *pubkey,
                                        self.accounts_index
                                            .roots_and_ref_count(&locked_entry, max_clean_root),
                                    );
                                }

                                // prune zero_lamport_pubkey set which should contain all 0-lamport
                                // keys whether rooted or not. A 0-lamport update may become rooted
                                // in the future.
                                let has_zero_lamport_accounts = slot_list
                                    .iter()
                                    .any(|(_slot, account_info)| account_info.lamports == 0);
                                if !has_zero_lamport_accounts {
                                    self.accounts_index.remove_zero_lamport_key(pubkey);
                                }

                                // Release the lock
                                let slot = *slot;
                                drop(locked_entry);

                                if self.accounts_index.is_uncleaned_root(slot) {
                                    // Assertion enforced by `accounts_index.get()`, the latest slot
                                    // will not be greater than the given `max_clean_root`
                                    if let Some(max_clean_root) = max_clean_root {
                                        assert!(slot <= max_clean_root);
                                    }
                                    purges_in_root.push(*pubkey);
                                }
                            } else {
                                let r_accounts_index =
                                    self.accounts_index.account_maps.read().unwrap();
                                if !r_accounts_index.contains_key(pubkey) {
                                    self.accounts_index.remove_zero_lamport_key(pubkey);
                                }
                            }
                        }
                        (purges, purges_in_root)
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
            })
        };
        accounts_scan.stop();

        let mut clean_old_rooted = Measure::start("clean_old_roots");
        let (purged_account_slots, removed_accounts) =
            self.clean_old_rooted_accounts(purges_in_root, max_clean_root);

        if self.caching_enabled {
            self.do_reset_uncleaned_roots(max_clean_root);
        } else {
            self.do_reset_uncleaned_roots_v1(&mut candidates_v1, max_clean_root);
        }
        clean_old_rooted.stop();

        let mut store_counts_time = Measure::start("store_counts");

        // Calculate store counts as if everything was purged
        // Then purge if we can
        let mut store_counts: HashMap<AppendVecId, (usize, HashSet<Pubkey>)> = HashMap::new();
        for (key, (account_infos, ref_count)) in purges.iter_mut() {
            if purged_account_slots.contains_key(&key) {
                *ref_count = self.accounts_index.ref_count_from_storage(&key);
            }
            account_infos.retain(|(slot, account_info)| {
                let was_slot_purged = purged_account_slots
                    .get(&key)
                    .map(|slots_removed| slots_removed.contains(slot))
                    .unwrap_or(false);
                if was_slot_purged {
                    // No need to look up the slot storage below if the entire
                    // slot was purged
                    return false;
                }
                // Check if this update in `slot` to the account with `key` was reclaimed earlier by
                // `clean_old_rooted_accounts()`
                let was_reclaimed = removed_accounts
                    .get(&account_info.store_id)
                    .map(|store_removed| store_removed.contains(&account_info.offset))
                    .unwrap_or(false);
                if was_reclaimed {
                    return false;
                }
                if let Some(store_count) = store_counts.get_mut(&account_info.store_id) {
                    store_count.0 -= 1;
                    store_count.1.insert(*key);
                } else {
                    let mut key_set = HashSet::new();
                    key_set.insert(*key);
                    let count = self
                        .storage
                        .slot_store_count(*slot, account_info.store_id)
                        .unwrap()
                        - 1;
                    debug!(
                        "store_counts, inserting slot: {}, store id: {}, count: {}",
                        slot, account_info.store_id, count
                    );
                    store_counts.insert(account_info.store_id, (count, key_set));
                }
                true
            });
        }
        store_counts_time.stop();

        let mut calc_deps_time = Measure::start("calc_deps");
        Self::calc_delete_dependencies(&purges, &mut store_counts);
        calc_deps_time.stop();

        // Only keep purges where the entire history of the account in the root set
        // can be purged. All AppendVecs for those updates are dead.
        let mut purge_filter = Measure::start("purge_filter");
        purges.retain(|_pubkey, (account_infos, _ref_count)| {
            for (_slot, account_info) in account_infos.iter() {
                if store_counts.get(&account_info.store_id).unwrap().0 != 0 {
                    return false;
                }
            }
            true
        });
        purge_filter.stop();

        let mut reclaims_time = Measure::start("reclaims");
        // Recalculate reclaims with new purge set
        let pubkey_to_slot_set: Vec<_> = purges
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

        let reclaims = self.purge_keys_exact(&pubkey_to_slot_set);

        // Don't reset from clean, since the pubkeys in those stores may need to be unref'ed
        // and those stores may be used for background hashing.
        let reset_accounts = false;
        self.handle_reclaims(&reclaims, None, false, None, reset_accounts);

        reclaims_time.stop();

        self.clean_accounts_stats.report();
        datapoint_info!(
            "clean_accounts",
            (
                "collect_delta_keys_us",
                key_timings.collect_delta_keys_us,
                i64
            ),
            (
                "zero_lamport_key_clone_us",
                key_timings.zero_lamport_key_clone_us,
                i64
            ),
            ("accounts_scan", accounts_scan.as_us() as i64, i64),
            ("clean_old_rooted", clean_old_rooted.as_us() as i64, i64),
            ("store_counts", store_counts_time.as_us() as i64, i64),
            ("purge_filter", purge_filter.as_us() as i64, i64),
            ("calc_deps", calc_deps_time.as_us() as i64, i64),
            ("reclaims", reclaims_time.as_us() as i64, i64),
            ("delta_key_count", key_timings.delta_key_count, i64),
            ("zero_lamport_count", key_timings.zero_lamport_count, i64),
            ("total_keys_count", total_keys_count, i64),
        );
    }

    /// Removes the accounts in the input `reclaims` from the tracked "count" of
    /// their corresponding  storage entries. Note this does not actually free
    /// the memory from the storage entries until all the storage entries for
    /// a given slot `S` are empty, at which point `process_dead_slots` will
    /// remove all the storage entries for `S`.
    ///
    /// # Arguments
    /// * `reclaims` - The accounts to remove from storage entries' "count"
    ///
    /// * `expected_single_dead_slot` - A correctness assertion. If this is equal to `Some(S)`,
    ///    then the function will check that the only slot being cleaned up in `reclaims`
    ///    is the slot == `S`. This is true for instance when `handle_reclaims` is called
    ///    from store or slot shrinking, as those should only touch the slot they are
    ///    currently storing to or shrinking.
    ///
    /// * `no_dead_slot` - A correctness assertion. If this is equal to
    ///    `false`, the function will check that no slots are cleaned up/removed via
    ///    `process_dead_slots`. For instance, on store, no slots should be cleaned up,
    ///    but during the background clean accounts purges accounts from old rooted slots,
    ///    so outdated slots may be removed.
    /// * `reclaim_result` - Information about accounts that were removed from storage, does
    ///    not include accounts that were removed from the cache
    /// * `reset_accounts` - Reset the append_vec store when the store is dead (count==0)
    ///    From the clean and shrink paths it should be false since there may be an in-progress
    ///    hash operation and the stores may hold accounts that need to be unref'ed.
    fn handle_reclaims(
        &self,
        reclaims: SlotSlice<AccountInfo>,
        expected_single_dead_slot: Option<Slot>,
        no_dead_slot: bool,
        reclaim_result: Option<&mut ReclaimResult>,
        reset_accounts: bool,
    ) {
        if reclaims.is_empty() {
            return;
        }
        let (purged_account_slots, reclaimed_offsets) =
            if let Some((ref mut x, ref mut y)) = reclaim_result {
                (Some(x), Some(y))
            } else {
                (None, None)
            };
        let dead_slots = self.remove_dead_accounts(
            reclaims,
            expected_single_dead_slot,
            reclaimed_offsets,
            reset_accounts,
        );
        if no_dead_slot {
            assert!(dead_slots.is_empty());
        } else if let Some(expected_single_dead_slot) = expected_single_dead_slot {
            assert!(dead_slots.len() <= 1);
            if dead_slots.len() == 1 {
                assert!(dead_slots.contains(&expected_single_dead_slot));
            }
        }
        self.process_dead_slots(&dead_slots, purged_account_slots);
    }

    // Must be kept private!, does sensitive cleanup that should only be called from
    // supported pipelines in AccountsDb
    fn process_dead_slots(
        &self,
        dead_slots: &HashSet<Slot>,
        purged_account_slots: Option<&mut AccountSlots>,
    ) {
        if dead_slots.is_empty() {
            return;
        }
        let mut clean_dead_slots = Measure::start("reclaims::clean_dead_slots");
        self.clean_stored_dead_slots(&dead_slots, purged_account_slots);
        clean_dead_slots.stop();

        let mut purge_removed_slots = Measure::start("reclaims::purge_removed_slots");
        self.purge_storage_slots(&dead_slots);
        purge_removed_slots.stop();

        // If the slot is dead, remove the need to shrink the storages as
        // the storage entries will be purged.
        for slot in dead_slots {
            self.shrink_candidate_slots.lock().unwrap().remove(slot);
        }

        debug!(
            "process_dead_slots({}): {} {} {:?}",
            dead_slots.len(),
            clean_dead_slots,
            purge_removed_slots,
            dead_slots,
        );
    }

    fn do_shrink_slot_stores<'a, I>(&'a self, slot: Slot, stores: I)
    where
        I: Iterator<Item = &'a Arc<AccountStorageEntry>>,
    {
        debug!("do_shrink_slot_stores: slot: {}", slot);
        let mut stored_accounts = vec![];
        let mut original_bytes = 0;
        for store in stores {
            let mut start = 0;
            original_bytes += store.total_bytes();
            while let Some((account, next)) = store.accounts.get_account(start) {
                stored_accounts.push((
                    account.meta.pubkey,
                    account.clone_account(),
                    *account.hash,
                    next - start,
                    (store.append_vec_id(), account.offset),
                    account.meta.write_version,
                ));
                start = next;
            }
        }

        let mut index_read_elapsed = Measure::start("index_read_elapsed");
        let alive_accounts: Vec<_> = {
            stored_accounts
                .iter()
                .filter(
                    |(
                        pubkey,
                        _account,
                        _account_hash,
                        _storage_size,
                        (store_id, offset),
                        _write_version,
                    )| {
                        if let Some((locked_entry, _)) = self.accounts_index.get(pubkey, None, None)
                        {
                            locked_entry
                                .slot_list()
                                .iter()
                                .any(|(_slot, i)| i.store_id == *store_id && i.offset == *offset)
                        } else {
                            false
                        }
                    },
                )
                .collect()
        };
        index_read_elapsed.stop();

        let alive_total: u64 = alive_accounts
            .iter()
            .map(
                |(_pubkey, _account, _account_hash, account_size, _location, _write_version)| {
                    *account_size as u64
                },
            )
            .sum();
        let aligned_total: u64 = self.page_align(alive_total);

        let total_starting_accounts = stored_accounts.len();
        let total_accounts_after_shrink = alive_accounts.len();
        debug!(
            "shrinking: slot: {}, total_starting_accounts: {} => total_accounts_after_shrink: {} ({} bytes; aligned to: {})",
            slot,
            total_starting_accounts,
            total_accounts_after_shrink,
            alive_total,
            aligned_total
        );

        let mut rewrite_elapsed = Measure::start("rewrite_elapsed");
        let mut dead_storages = vec![];
        let mut find_alive_elapsed = 0;
        let mut create_and_insert_store_elapsed = 0;
        let mut write_storage_elapsed = 0;
        let mut store_accounts_timing = StoreAccountsTiming::default();
        if aligned_total > 0 {
            let mut start = Measure::start("find_alive_elapsed");
            let mut accounts = Vec::with_capacity(alive_accounts.len());
            let mut hashes = Vec::with_capacity(alive_accounts.len());
            let mut write_versions = Vec::with_capacity(alive_accounts.len());

            for (pubkey, account, account_hash, _size, _location, write_version) in &alive_accounts
            {
                accounts.push((pubkey, account));
                hashes.push(*account_hash);
                write_versions.push(*write_version);
            }
            start.stop();
            find_alive_elapsed = start.as_us();

            let mut start = Measure::start("create_and_insert_store_elapsed");
            let shrunken_store = if let Some(new_store) =
                self.try_recycle_and_insert_store(slot, aligned_total, aligned_total + 1024)
            {
                new_store
            } else {
                let maybe_shrink_paths = self.shrink_paths.read().unwrap();
                if let Some(ref shrink_paths) = *maybe_shrink_paths {
                    self.create_and_insert_store_with_paths(
                        slot,
                        aligned_total,
                        "shrink-w-path",
                        shrink_paths,
                    )
                } else {
                    self.create_and_insert_store(slot, aligned_total, "shrink")
                }
            };
            start.stop();
            create_and_insert_store_elapsed = start.as_us();

            // here, we're writing back alive_accounts. That should be an atomic operation
            // without use of rather wide locks in this whole function, because we're
            // mutating rooted slots; There should be no writers to them.
            store_accounts_timing = self.store_accounts_frozen(
                slot,
                &accounts,
                &hashes,
                Some(Box::new(move |_, _| shrunken_store.clone())),
                Some(Box::new(write_versions.into_iter())),
            );

            // `store_accounts_frozen()` above may have purged accounts from some
            // other storage entries (the ones that were just overwritten by this
            // new storage entry). This means some of those stores might have caused
            // this slot to be readded to `self.shrink_candidate_slots`, so delete
            // those here
            self.shrink_candidate_slots.lock().unwrap().remove(&slot);

            // Purge old, overwritten storage entries
            let mut start = Measure::start("write_storage_elapsed");
            if let Some(slot_stores) = self.storage.get_slot_stores(slot) {
                slot_stores.write().unwrap().retain(|_key, store| {
                    if store.count() == 0 {
                        dead_storages.push(store.clone());
                    }
                    store.count() > 0
                });
            }
            start.stop();
            write_storage_elapsed = start.as_us();
        }
        rewrite_elapsed.stop();

        let mut recycle_stores_write_elapsed = Measure::start("recycle_stores_write_time");
        let mut recycle_stores = self.recycle_stores.write().unwrap();
        recycle_stores_write_elapsed.stop();

        let mut drop_storage_entries_elapsed = Measure::start("drop_storage_entries_elapsed");
        if recycle_stores.len() < MAX_RECYCLE_STORES {
            recycle_stores.extend(dead_storages);
            drop(recycle_stores);
        } else {
            self.stats
                .dropped_stores
                .fetch_add(recycle_stores.len() as u64, Ordering::Relaxed);
            drop(recycle_stores);
            drop(dead_storages);
        }
        drop_storage_entries_elapsed.stop();

        self.shrink_stats
            .num_slots_shrunk
            .fetch_add(1, Ordering::Relaxed);
        self.shrink_stats
            .index_read_elapsed
            .fetch_add(index_read_elapsed.as_us(), Ordering::Relaxed);
        self.shrink_stats
            .find_alive_elapsed
            .fetch_add(find_alive_elapsed, Ordering::Relaxed);
        self.shrink_stats
            .create_and_insert_store_elapsed
            .fetch_add(create_and_insert_store_elapsed, Ordering::Relaxed);
        self.shrink_stats.store_accounts_elapsed.fetch_add(
            store_accounts_timing.store_accounts_elapsed,
            Ordering::Relaxed,
        );
        self.shrink_stats.update_index_elapsed.fetch_add(
            store_accounts_timing.update_index_elapsed,
            Ordering::Relaxed,
        );
        self.shrink_stats.handle_reclaims_elapsed.fetch_add(
            store_accounts_timing.handle_reclaims_elapsed,
            Ordering::Relaxed,
        );
        self.shrink_stats
            .write_storage_elapsed
            .fetch_add(write_storage_elapsed, Ordering::Relaxed);
        self.shrink_stats
            .rewrite_elapsed
            .fetch_add(rewrite_elapsed.as_us(), Ordering::Relaxed);
        self.shrink_stats
            .drop_storage_entries_elapsed
            .fetch_add(drop_storage_entries_elapsed.as_us(), Ordering::Relaxed);
        self.shrink_stats
            .recycle_stores_write_elapsed
            .fetch_add(recycle_stores_write_elapsed.as_us(), Ordering::Relaxed);
        self.shrink_stats.accounts_removed.fetch_add(
            total_starting_accounts - total_accounts_after_shrink,
            Ordering::Relaxed,
        );
        self.shrink_stats.bytes_removed.fetch_add(
            original_bytes.saturating_sub(aligned_total),
            Ordering::Relaxed,
        );
        self.shrink_stats.report();
    }

    // Reads all accounts in given slot's AppendVecs and filter only to alive,
    // then create a minimum AppendVec filled with the alive.
    fn shrink_slot_forced(&self, slot: Slot) -> usize {
        debug!("shrink_slot_forced: slot: {}", slot);

        if let Some(stores_lock) = self.storage.get_slot_stores(slot) {
            let stores: Vec<Arc<AccountStorageEntry>> =
                stores_lock.read().unwrap().values().cloned().collect();
            let mut alive_count = 0;
            let mut stored_count = 0;
            for store in &stores {
                alive_count += store.count();
                stored_count += store.approx_stored_count();
            }
            if alive_count == stored_count && stores.len() == 1 {
                trace!(
                    "shrink_slot_forced ({}): not able to shrink at all: alive/stored: {} / {}",
                    slot,
                    alive_count,
                    stored_count,
                );
                return 0;
            }
            self.do_shrink_slot_stores(slot, stores.iter());
            alive_count
        } else {
            0
        }
    }

    fn all_slots_in_storage(&self) -> Vec<Slot> {
        self.storage.all_slots()
    }

    fn all_root_slots_in_index(&self) -> Vec<Slot> {
        self.accounts_index.all_roots()
    }

    pub fn shrink_candidate_slots(&self) -> usize {
        let shrink_slots = std::mem::replace(
            &mut *self.shrink_candidate_slots.lock().unwrap(),
            HashMap::new(),
        );
        let num_candidates = shrink_slots.len();
        for (slot, slot_shrink_candidates) in shrink_slots {
            let mut measure = Measure::start("shrink_candidate_slots-ms");
            self.do_shrink_slot_stores(slot, slot_shrink_candidates.values());
            measure.stop();
            inc_new_counter_info!("shrink_candidate_slots-ms", measure.as_ms() as usize);
        }
        num_candidates
    }

    pub fn shrink_all_slots(&self) {
        for slot in self.all_slots_in_storage() {
            if self.caching_enabled {
                self.shrink_slot_forced(slot);
            } else {
                self.do_shrink_slot_forced_v1(slot);
            }
        }
    }

    pub fn scan_accounts<F, A>(&self, ancestors: &Ancestors, scan_func: F) -> A
    where
        F: Fn(&mut A, Option<(&Pubkey, Account, Slot)>),
        A: Default,
    {
        let mut collector = A::default();
        self.accounts_index
            .scan_accounts(ancestors, |pubkey, (account_info, slot)| {
                let account_slot = self
                    .get_account_accessor_from_cache_or_storage(
                        slot,
                        pubkey,
                        account_info.store_id,
                        account_info.offset,
                    )
                    .get_loaded_account()
                    .map(|loaded_account| (pubkey, loaded_account.account(), slot));
                scan_func(&mut collector, account_slot)
            });
        collector
    }

    pub fn unchecked_scan_accounts<F, A>(
        &self,
        metric_name: &'static str,
        ancestors: &Ancestors,
        scan_func: F,
    ) -> A
    where
        F: Fn(&mut A, (&Pubkey, LoadedAccount, Slot)),
        A: Default,
    {
        let mut collector = A::default();
        self.accounts_index.unchecked_scan_accounts(
            metric_name,
            ancestors,
            |pubkey, (account_info, slot)| {
                if let Some(loaded_account) = self
                    .get_account_accessor_from_cache_or_storage(
                        slot,
                        pubkey,
                        account_info.store_id,
                        account_info.offset,
                    )
                    .get_loaded_account()
                {
                    scan_func(&mut collector, (pubkey, loaded_account, slot));
                }
            },
        );
        collector
    }

    pub fn range_scan_accounts<F, A, R>(
        &self,
        metric_name: &'static str,
        ancestors: &Ancestors,
        range: R,
        scan_func: F,
    ) -> A
    where
        F: Fn(&mut A, Option<(&Pubkey, Account, Slot)>),
        A: Default,
        R: RangeBounds<Pubkey>,
    {
        let mut collector = A::default();
        self.accounts_index.range_scan_accounts(
            metric_name,
            ancestors,
            range,
            |pubkey, (account_info, slot)| {
                let account_slot = self
                    .get_account_accessor_from_cache_or_storage(
                        slot,
                        pubkey,
                        account_info.store_id,
                        account_info.offset,
                    )
                    .get_loaded_account()
                    .map(|loaded_account| (pubkey, loaded_account.account(), slot));
                scan_func(&mut collector, account_slot)
            },
        );
        collector
    }

    pub fn index_scan_accounts<F, A>(
        &self,
        ancestors: &Ancestors,
        index_key: IndexKey,
        scan_func: F,
    ) -> A
    where
        F: Fn(&mut A, Option<(&Pubkey, Account, Slot)>),
        A: Default,
    {
        let mut collector = A::default();
        self.accounts_index.index_scan_accounts(
            ancestors,
            index_key,
            |pubkey, (account_info, slot)| {
                let account_slot = self
                    .get_account_accessor_from_cache_or_storage(
                        slot,
                        pubkey,
                        account_info.store_id,
                        account_info.offset,
                    )
                    .get_loaded_account()
                    .map(|loaded_account| (pubkey, loaded_account.account(), slot));
                scan_func(&mut collector, account_slot)
            },
        );
        collector
    }

    /// Scan a specific slot through all the account storage in parallel
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
                            cache_map_func(LoadedAccount::Cached((
                                *cached_account.key(),
                                Cow::Borrowed(cached_account.value()),
                            )))
                        })
                        .collect()
                }))
            } else {
                ScanStorageResult::Cached(
                    slot_cache
                        .iter()
                        .filter_map(|cached_account| {
                            cache_map_func(LoadedAccount::Cached((
                                *cached_account.key(),
                                Cow::Borrowed(cached_account.value()),
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
            let storage_maps: Vec<Arc<AccountStorageEntry>> = self
                .storage
                .get_slot_stores(slot)
                .map(|res| res.read().unwrap().values().cloned().collect())
                .unwrap_or_default();
            self.thread_pool.install(|| {
                storage_maps
                    .par_iter()
                    .flat_map(|storage| storage.all_accounts())
                    .for_each(|account| storage_scan_func(&retval, LoadedAccount::Stored(account)));
            });

            ScanStorageResult::Stored(retval)
        }
    }

    pub fn set_hash(&self, slot: Slot, parent_slot: Slot) {
        let mut bank_hashes = self.bank_hashes.write().unwrap();
        if bank_hashes.get(&slot).is_some() {
            error!(
                "set_hash: already exists; multiple forks with shared slot {} as child (parent: {})!?",
                slot, parent_slot,
            );
            return;
        }

        let new_hash_info = BankHashInfo {
            hash: Hash::default(),
            snapshot_hash: Hash::default(),
            stats: BankHashStats::default(),
        };
        bank_hashes.insert(slot, new_hash_info);
    }

    pub fn load(&self, ancestors: &Ancestors, pubkey: &Pubkey) -> Option<(Account, Slot)> {
        self.do_load(ancestors, pubkey, None)
    }

    fn do_load(
        &self,
        ancestors: &Ancestors,
        pubkey: &Pubkey,
        max_root: Option<Slot>,
    ) -> Option<(Account, Slot)> {
        let (slot, store_id, offset) = {
            let (lock, index) = self.accounts_index.get(pubkey, Some(ancestors), max_root)?;
            let slot_list = lock.slot_list();
            let (
                slot,
                AccountInfo {
                    store_id, offset, ..
                },
            ) = slot_list[index];
            (slot, store_id, offset)
            // `lock` released here
        };

        //TODO: thread this as a ref
        self.get_account_accessor_from_cache_or_storage(slot, pubkey, store_id, offset)
            .get_loaded_account()
            .map(|loaded_account| (loaded_account.account(), slot))
    }

    pub fn load_account_hash(&self, ancestors: &Ancestors, pubkey: &Pubkey) -> Hash {
        let (slot, store_id, offset) = {
            let (lock, index) = self
                .accounts_index
                .get(pubkey, Some(ancestors), None)
                .unwrap();
            let slot_list = lock.slot_list();
            let (
                slot,
                AccountInfo {
                    store_id, offset, ..
                },
            ) = slot_list[index];
            (slot, store_id, offset)
            // lock released here
        };

        self.get_account_accessor_from_cache_or_storage(slot, pubkey, store_id, offset)
            .get_loaded_account()
            .map(|loaded_account| *loaded_account.loaded_hash())
            .unwrap()
    }

    pub fn load_slow(&self, ancestors: &Ancestors, pubkey: &Pubkey) -> Option<(Account, Slot)> {
        self.load(ancestors, pubkey)
    }

    // Only safe to use the `get_account_accessor_from_cache_or_storage() -> get_loaded_account()`
    // pattern if you're holding the AccountIndex lock for the `pubkey`, otherwise, a cache
    // flush could happen between `get_account_accessor_from_cache_or_storage()` and
    //`get_loaded_account()`, and the `LoadedAccountAccessor::Cached((&self.accounts_cache, slot, pubkey))`
    // returned here won't be able to find a slot cache entry for that `slot`.
    fn get_account_accessor_from_cache_or_storage<'a>(
        &'a self,
        slot: Slot,
        pubkey: &'a Pubkey,
        store_id: usize,
        offset: usize,
    ) -> LoadedAccountAccessor<'a> {
        if store_id == CACHE_VIRTUAL_STORAGE_ID {
            LoadedAccountAccessor::Cached((&self.accounts_cache, slot, pubkey))
        } else {
            let account_storage_entry = self.storage.get_account_storage_entry(slot, store_id);
            LoadedAccountAccessor::Stored(
                account_storage_entry.map(|account_storage_entry| (account_storage_entry, offset)),
            )
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
        for (i, store) in recycle_stores.iter().enumerate() {
            if Arc::strong_count(store) == 1 {
                max = std::cmp::max(store.accounts.capacity(), max);
                min = std::cmp::min(store.accounts.capacity(), min);
                avail += 1;

                if store.accounts.capacity() >= min_size && store.accounts.capacity() < max_size {
                    let ret = recycle_stores.swap_remove(i);
                    drop(recycle_stores);
                    let old_id = ret.append_vec_id();
                    ret.recycle(slot, self.next_id.fetch_add(1, Ordering::Relaxed));
                    debug!(
                        "recycling store: {} {:?} old_id: {}",
                        ret.append_vec_id(),
                        ret.get_path(),
                        old_id
                    );
                    return Some(ret);
                }
            }
        }
        debug!(
            "no recycle stores max: {} min: {} len: {} looking: {}, {} avail: {}",
            max,
            min,
            recycle_stores.len(),
            min_size,
            max_size,
            avail,
        );
        None
    }

    fn find_storage_candidate(&self, slot: Slot, size: usize) -> Arc<AccountStorageEntry> {
        let mut create_extra = false;
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
                if slot_stores.len() <= self.min_num_stores {
                    let mut total_accounts = 0;
                    for store in slot_stores.values() {
                        total_accounts += store.count();
                    }

                    // Create more stores so that when scanning the storage all CPUs have work
                    if (total_accounts / 16) >= slot_stores.len() {
                        create_extra = true;
                    }
                }

                // pick an available store at random by iterating from a random point
                let to_skip = thread_rng().gen_range(0, slot_stores.len());

                for (i, store) in slot_stores.values().cycle().skip(to_skip).enumerate() {
                    if store.try_available() {
                        let ret = store.clone();
                        drop(slot_stores);
                        if create_extra {
                            if self
                                .try_recycle_and_insert_store(slot, size as u64, std::u64::MAX)
                                .is_none()
                            {
                                self.stats
                                    .create_store_count
                                    .fetch_add(1, Ordering::Relaxed);
                                self.create_and_insert_store(slot, self.file_size, "store extra");
                            } else {
                                self.stats
                                    .recycle_store_count
                                    .fetch_add(1, Ordering::Relaxed);
                            }
                        }
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
            self.stats
                .recycle_store_count
                .fetch_add(1, Ordering::Relaxed);
            store
        } else {
            self.stats
                .create_store_count
                .fetch_add(1, Ordering::Relaxed);
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

    fn page_align(&self, size: u64) -> u64 {
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
        let path_index = thread_rng().gen_range(0, paths.len());
        let store = Arc::new(self.new_storage_entry(
            slot,
            &Path::new(&paths[path_index]),
            self.page_align(size),
        ));

        if store.append_vec_id() == CACHE_VIRTUAL_STORAGE_ID {
            panic!("We've run out of storage ids!");
        }

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
        let slot_storages: SlotStores = self.storage.get_slot_stores(slot).unwrap_or_else(||
            // DashMap entry.or_insert() returns a RefMut, essentially a write lock,
            // which is dropped after this block ends, minimizing time held by the lock.
            // However, we still want to persist the reference to the `SlotStores` behind
            // the lock, hence we clone it out, (`SlotStores` is an Arc so is cheap to clone).
            self.storage
                .0
                .entry(slot)
                .or_insert(Arc::new(RwLock::new(HashMap::new())))
                .clone());

        assert!(slot_storages
            .write()
            .unwrap()
            .insert(store.append_vec_id(), store)
            .is_none());
    }

    pub fn purge_slot(&self, slot: Slot) {
        let mut slots = HashSet::new();
        slots.insert(slot);
        self.purge_slots(&slots);
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
                if recycle_stores.len() > MAX_RECYCLE_STORES {
                    let dropped_count = total_removed_storage_entries - recycled_count;
                    self.stats
                        .dropped_stores
                        .fetch_add(dropped_count as u64, Ordering::Relaxed);
                    return recycle_stores_write_elapsed.as_us();
                }
                recycle_stores.push(stores.clone());
                recycled_count += 1;
            }
        }
        recycle_stores_write_elapsed.as_us()
    }

    fn do_purge_slots_from_cache_and_store<'a>(
        &'a self,
        can_exist_in_cache: bool,
        removed_slots: impl Iterator<Item = &'a Slot>,
        purge_stats: &PurgeStats,
    ) {
        let mut remove_storages_elapsed = Measure::start("remove_storages_elapsed");
        let mut all_removed_slot_storages = vec![];
        let mut num_cached_slots_removed = 0;
        let mut total_removed_cached_bytes = 0;
        let mut total_removed_storage_entries = 0;
        let mut total_removed_stored_bytes = 0;
        for remove_slot in removed_slots {
            if let Some(slot_cache) = self.accounts_cache.remove_slot(*remove_slot) {
                // If the slot is still in the cache, remove the backing storages for
                // the slot and from the Accounts Index
                if !can_exist_in_cache {
                    panic!("The removed slot must alrady have been flushed from the cache");
                }
                num_cached_slots_removed += 1;
                total_removed_cached_bytes += slot_cache.total_bytes();
                self.purge_slot_cache(*remove_slot, slot_cache);
            } else if let Some((_, slot_removed_storages)) = self.storage.0.remove(&remove_slot) {
                // Because AccountsBackgroundService synchronously flushes from the accounts cache
                // and handles all Bank::drop() (the cleanup function that leads to this
                // function call), then we don't need to worry above an overlapping cache flush
                // with this function call. This means, if we get into this case, we can be
                // confident that the entire state for this slot has been flushed to the storage
                // already.

                // Note this only cleans up the storage entries. The accounts index cleaning
                // (removing from the slot list, decrementing the account ref count), is handled in
                // clean_accounts() -> purge_older_root_entries()
                {
                    let r_slot_removed_storages = slot_removed_storages.read().unwrap();
                    total_removed_storage_entries += r_slot_removed_storages.len();
                    total_removed_stored_bytes += r_slot_removed_storages
                        .values()
                        .map(|i| i.accounts.capacity())
                        .sum::<u64>();
                }
                all_removed_slot_storages.push(slot_removed_storages.clone());
            }

            // It should not be possible that a slot is neither in the cache or storage. Even in
            // a slot with all ticks, `Bank::new_from_parent()` immediately stores some sysvars
            // on bank creation.

            // Remove any delta pubkey set if existing.
            self.uncleaned_pubkeys.remove(remove_slot);
        }
        remove_storages_elapsed.stop();

        let num_stored_slots_removed = all_removed_slot_storages.len();

        let recycle_stores_write_elapsed =
            self.recycle_slot_stores(total_removed_storage_entries, &all_removed_slot_storages);

        let mut drop_storage_entries_elapsed = Measure::start("drop_storage_entries_elapsed");
        // Backing mmaps for removed storages entries explicitly dropped here outside
        // of any locks
        drop(all_removed_slot_storages);
        drop_storage_entries_elapsed.stop();

        purge_stats
            .remove_storages_elapsed
            .fetch_add(remove_storages_elapsed.as_us(), Ordering::Relaxed);
        purge_stats
            .drop_storage_entries_elapsed
            .fetch_add(drop_storage_entries_elapsed.as_us(), Ordering::Relaxed);
        purge_stats
            .num_cached_slots_removed
            .fetch_add(num_cached_slots_removed, Ordering::Relaxed);
        purge_stats
            .total_removed_cached_bytes
            .fetch_add(total_removed_cached_bytes, Ordering::Relaxed);
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

    fn purge_storage_slots(&self, removed_slots: &HashSet<Slot>) {
        // Check all slots `removed_slots` are no longer rooted
        let mut safety_checks_elapsed = Measure::start("safety_checks_elapsed");
        for slot in removed_slots.iter() {
            assert!(!self.accounts_index.is_root(*slot))
        }
        safety_checks_elapsed.stop();
        self.clean_accounts_stats
            .purge_stats
            .safety_checks_elapsed
            .fetch_add(safety_checks_elapsed.as_us(), Ordering::Relaxed);
        self.do_purge_slots_from_cache_and_store(
            false,
            removed_slots.iter(),
            &self.clean_accounts_stats.purge_stats,
        );
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
        self.purge_slot_cache_pubkeys(purged_slot, purged_slot_pubkeys, pubkey_to_slot_set, true);
    }

    fn purge_slot_cache_pubkeys(
        &self,
        purged_slot: Slot,
        purged_slot_pubkeys: HashSet<(Slot, Pubkey)>,
        pubkey_to_slot_set: Vec<(Pubkey, Slot)>,
        is_dead: bool,
    ) {
        // Slot purged from cache should not exist in the backing store
        assert!(self.storage.get_slot_stores(purged_slot).is_none());
        let num_purged_keys = pubkey_to_slot_set.len();
        let reclaims = self.purge_keys_exact(&pubkey_to_slot_set);
        assert_eq!(reclaims.len(), num_purged_keys);
        if is_dead {
            self.finalize_dead_slot_removal(
                std::iter::once(&purged_slot),
                purged_slot_pubkeys,
                None,
            );
        }
    }

    fn purge_slots(&self, slots: &HashSet<Slot>) {
        // `add_root()` should be called first
        let mut safety_checks_elapsed = Measure::start("safety_checks_elapsed");
        let non_roots: Vec<&Slot> = slots
            .iter()
            .filter(|slot| !self.accounts_index.is_root(**slot))
            .collect();
        safety_checks_elapsed.stop();
        self.external_purge_slots_stats
            .safety_checks_elapsed
            .fetch_add(safety_checks_elapsed.as_us(), Ordering::Relaxed);
        self.do_purge_slots_from_cache_and_store(
            true,
            non_roots.into_iter(),
            &self.external_purge_slots_stats,
        );
        self.external_purge_slots_stats
            .report("external_purge_slots_stats", Some(1000));
    }

    // TODO: This is currently:
    // 1. Unsafe with scan because it can remove a slot in the middle
    // of a scan.
    // 2. Doesn't handle cache flushes that happen during the slot deletion (see comment below).
    pub fn remove_unrooted_slot(&self, remove_slot: Slot) {
        if self.accounts_index.is_root(remove_slot) {
            panic!("Trying to remove accounts for rooted slot {}", remove_slot);
        }

        if let Some(slot_cache) = self.accounts_cache.remove_slot(remove_slot) {
            // If the slot is still in the cache, remove it from the cache
            self.purge_slot_cache(remove_slot, slot_cache);
        }

        // TODO: Handle if the slot was flushed to storage while we were removing the cached
        // slot above, i.e. it's possible the storage contains partial version of the current
        // slot. One way to handle this is to augment slots to contain a "version", That way,
        // 1) We clean older versions via the natural clean() pipeline
        // without having to call this function out of band.
        // 2) This deletion doesn't have to block on scan
        // Reads will then always read the latest version of a slot. Scans will also know
        // which version their parents because banks will also be augmented with this version,
        // which handles cases where a deletion of one version happens in the middle of the scan.
        let scan_result: ScanStorageResult<Pubkey, DashSet<Pubkey>> = self.scan_account_storage(
            remove_slot,
            |loaded_account: LoadedAccount| Some(*loaded_account.pubkey()),
            |accum: &DashSet<Pubkey>, loaded_account: LoadedAccount| {
                accum.insert(*loaded_account.pubkey());
            },
        );

        // Purge this slot from the accounts index
        let purge_slot: HashSet<Slot> = vec![remove_slot].into_iter().collect();
        let mut reclaims = vec![];
        match scan_result {
            ScanStorageResult::Cached(cached_keys) => {
                for pubkey in cached_keys.iter() {
                    self.accounts_index.purge_exact(
                        pubkey,
                        &purge_slot,
                        &mut reclaims,
                        &self.account_indexes,
                    );
                }
            }
            ScanStorageResult::Stored(stored_keys) => {
                for set_ref in stored_keys.iter() {
                    self.accounts_index.purge_exact(
                        set_ref.key(),
                        &purge_slot,
                        &mut reclaims,
                        &self.account_indexes,
                    );
                }
            }
        }

        self.handle_reclaims(&reclaims, Some(remove_slot), false, None, false);

        // After handling the reclaimed entries, this slot's
        // storage entries should be purged from self.storage
        assert!(self.storage.get_slot_stores(remove_slot).is_none());
    }

    fn include_owner(cluster_type: &ClusterType, slot: Slot) -> bool {
        // When devnet was moved to stable release channel, it was done without
        // hashing account.owner. That's because devnet's slot was lower than
        // 5_800_000 and the release channel's gating lacked ClusterType at the time...
        match cluster_type {
            ClusterType::Devnet => slot >= 5_800_000,
            _ => true,
        }
    }

    pub fn hash_stored_account(
        slot: Slot,
        account: &StoredAccountMeta,
        cluster_type: &ClusterType,
    ) -> Hash {
        let include_owner = Self::include_owner(cluster_type, slot);

        if slot > Self::get_blake3_slot(cluster_type) {
            Self::blake3_hash_account_data(
                slot,
                account.account_meta.lamports,
                &account.account_meta.owner,
                account.account_meta.executable,
                account.account_meta.rent_epoch,
                account.data,
                &account.meta.pubkey,
                include_owner,
            )
        } else {
            Self::hash_account_data(
                slot,
                account.account_meta.lamports,
                &account.account_meta.owner,
                account.account_meta.executable,
                account.account_meta.rent_epoch,
                account.data,
                &account.meta.pubkey,
                include_owner,
            )
        }
    }

    pub fn hash_account(
        slot: Slot,
        account: &Account,
        pubkey: &Pubkey,
        cluster_type: &ClusterType,
    ) -> Hash {
        let include_owner = Self::include_owner(cluster_type, slot);

        if slot > Self::get_blake3_slot(cluster_type) {
            Self::blake3_hash_account_data(
                slot,
                account.lamports,
                &account.owner,
                account.executable,
                account.rent_epoch,
                &account.data,
                pubkey,
                include_owner,
            )
        } else {
            Self::hash_account_data(
                slot,
                account.lamports,
                &account.owner,
                account.executable,
                account.rent_epoch,
                &account.data,
                pubkey,
                include_owner,
            )
        }
    }

    fn hash_frozen_account_data(account: &Account) -> Hash {
        let mut hasher = Hasher::default();

        hasher.hash(&account.data);
        hasher.hash(&account.owner.as_ref());

        if account.executable {
            hasher.hash(&[1u8; 1]);
        } else {
            hasher.hash(&[0u8; 1]);
        }

        hasher.result()
    }

    pub fn hash_account_data(
        slot: Slot,
        lamports: u64,
        owner: &Pubkey,
        executable: bool,
        rent_epoch: Epoch,
        data: &[u8],
        pubkey: &Pubkey,
        include_owner: bool,
    ) -> Hash {
        if lamports == 0 {
            return Hash::default();
        }

        let mut hasher = Hasher::default();

        hasher.hash(&lamports.to_le_bytes());

        hasher.hash(&slot.to_le_bytes());

        hasher.hash(&rent_epoch.to_le_bytes());

        hasher.hash(&data);

        if executable {
            hasher.hash(&[1u8; 1]);
        } else {
            hasher.hash(&[0u8; 1]);
        }

        if include_owner {
            hasher.hash(&owner.as_ref());
        }
        hasher.hash(&pubkey.as_ref());

        hasher.result()
    }

    pub fn blake3_hash_account_data(
        slot: Slot,
        lamports: u64,
        owner: &Pubkey,
        executable: bool,
        rent_epoch: Epoch,
        data: &[u8],
        pubkey: &Pubkey,
        include_owner: bool,
    ) -> Hash {
        if lamports == 0 {
            return Hash::default();
        }

        let mut hasher = blake3::Hasher::new();

        hasher.update(&lamports.to_le_bytes());

        hasher.update(&slot.to_le_bytes());

        hasher.update(&rent_epoch.to_le_bytes());

        hasher.update(&data);

        if executable {
            hasher.update(&[1u8; 1]);
        } else {
            hasher.update(&[0u8; 1]);
        }

        if include_owner {
            hasher.update(&owner.as_ref());
        }
        hasher.update(&pubkey.as_ref());

        Hash(<[u8; solana_sdk::hash::HASH_BYTES]>::try_from(hasher.finalize().as_slice()).unwrap())
    }

    fn get_blake3_slot(cluster_type: &ClusterType) -> Slot {
        match cluster_type {
            ClusterType::Development => 0,
            // Epoch 400
            ClusterType::Devnet => 3_276_800,
            // Epoch 78
            ClusterType::MainnetBeta => 33_696_000,
            // Epoch 95
            ClusterType::Testnet => 35_516_256,
        }
    }

    fn bulk_assign_write_version(&self, count: usize) -> u64 {
        self.write_version
            .fetch_add(count as u64, Ordering::Relaxed)
    }

    fn write_accounts_to_storage<F: FnMut(Slot, usize) -> Arc<AccountStorageEntry>>(
        &self,
        slot: Slot,
        hashes: &[Hash],
        mut storage_finder: F,
        accounts_and_meta_to_store: &[(StoredMeta, &Account)],
    ) -> Vec<AccountInfo> {
        assert_eq!(hashes.len(), accounts_and_meta_to_store.len());
        let mut infos: Vec<AccountInfo> = Vec::with_capacity(accounts_and_meta_to_store.len());
        let mut total_append_accounts_us = 0;
        let mut total_storage_find_us = 0;
        while infos.len() < accounts_and_meta_to_store.len() {
            let mut storage_find = Measure::start("storage_finder");
            let storage = storage_finder(
                slot,
                accounts_and_meta_to_store[infos.len()].1.data.len() + STORE_META_OVERHEAD,
            );
            storage_find.stop();
            total_storage_find_us += storage_find.as_us();
            let mut append_accounts = Measure::start("append_accounts");
            let rvs = storage.accounts.append_accounts(
                &accounts_and_meta_to_store[infos.len()..],
                &hashes[infos.len()..],
            );
            assert!(!rvs.is_empty());
            append_accounts.stop();
            total_append_accounts_us += append_accounts.as_us();
            if rvs.len() == 1 {
                storage.set_status(AccountStorageStatus::Full);

                // See if an account overflows the append vecs in the slot.
                let data_len = (accounts_and_meta_to_store[infos.len()].1.data.len()
                    + STORE_META_OVERHEAD) as u64;
                if !self.has_space_available(slot, data_len) {
                    let special_store_size = std::cmp::max(data_len * 2, self.file_size);
                    if self
                        .try_recycle_and_insert_store(slot, special_store_size, std::u64::MAX)
                        .is_none()
                    {
                        self.stats
                            .create_store_count
                            .fetch_add(1, Ordering::Relaxed);
                        self.create_and_insert_store(slot, special_store_size, "large create");
                    } else {
                        self.stats
                            .recycle_store_count
                            .fetch_add(1, Ordering::Relaxed);
                    }
                }
                continue;
            }

            for (offsets, (_, account)) in rvs
                .windows(2)
                .zip(&accounts_and_meta_to_store[infos.len()..])
            {
                let stored_size = offsets[1] - offsets[0];
                storage.add_account(stored_size);
                infos.push(AccountInfo {
                    store_id: storage.append_vec_id(),
                    offset: offsets[0],
                    stored_size,
                    lamports: account.lamports,
                });
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

    // `force_flush` flushes all the cached roots `<= requested_flush_root`. It also then
    // flushes:
    // 1) Any remaining roots if there are > MAX_CACHE_SLOTS remaining slots in the cache,
    // 2) It there are still > MAX_CACHE_SLOTS remaining slots in the cache, the excess
    // unrooted slots
    pub fn flush_accounts_cache(&self, force_flush: bool, requested_flush_root: Option<Slot>) {
        #[cfg(not(test))]
        assert!(requested_flush_root.is_some());

        if !force_flush && self.accounts_cache.num_slots() <= MAX_CACHE_SLOTS {
            return;
        }

        // Flush only the roots <= requested_flush_root, so that snapshotting has all
        // the relevant roots in storage.
        let mut flush_roots_elapsed = Measure::start("flush_roots_elapsed");
        let mut account_bytes_saved = 0;
        let mut num_accounts_saved = 0;

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

        // If there are > MAX_CACHE_SLOTS, then flush the excess ones to storage
        let (total_new_excess_roots, num_excess_roots_flushed) =
            if self.accounts_cache.num_slots() > MAX_CACHE_SLOTS {
                // Start by flushing the roots
                //
                // Cannot do any cleaning on roots past `requested_flush_root` because future
                // snapshots may need updates from those later slots, hence we pass `None`
                // for `should_clean`.
                self.flush_rooted_accounts_cache(None, None)
            } else {
                (0, 0)
            };
        let old_slots = self.accounts_cache.find_older_frozen_slots(MAX_CACHE_SLOTS);
        let excess_slot_count = old_slots.len();
        let mut unflushable_unrooted_slot_count = 0;
        let max_flushed_root = self.accounts_cache.fetch_max_flush_root();
        for old_slot in old_slots {
            // Don't flush slots that are known to be unrooted
            if old_slot > max_flushed_root {
                self.flush_slot_cache(old_slot, None::<&mut fn(&_, &_) -> bool>);
            } else {
                unflushable_unrooted_slot_count += 1;
            }
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

        // Flush a random slot out after every force flush to catch any inconsistencies
        // between cache and written state (i.e. should cause a hash mismatch between validators
        // that flush and don't flush if such a bug exists).
        let num_slots_remaining = self.accounts_cache.num_slots();
        if force_flush && num_slots_remaining >= FLUSH_CACHE_RANDOM_THRESHOLD {
            // Don't flush slots that are known to be unrooted
            let mut frozen_slots = self.accounts_cache.find_older_frozen_slots(0);
            frozen_slots.retain(|s| *s > max_flushed_root);
            // Remove a random index 0 <= i < `frozen_slots.len()`
            let rand_slot = frozen_slots.choose(&mut thread_rng());
            if let Some(rand_slot) = rand_slot {
                info!(
                    "Flushing random slot: {}, num_remaining: {}",
                    *rand_slot, num_slots_remaining
                );
                self.flush_slot_cache(*rand_slot, None::<&mut fn(&_, &_) -> bool>);
            }
        }
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

        // Use HashMap because HashSet doesn't provide Entry api
        let mut written_accounts = HashMap::new();

        // If `should_clean` is None, then`should_flush_f` is also None, which will cause
        // `flush_slot_cache` to flush all accounts to storage without cleaning any accounts.
        let mut should_flush_f = should_clean.map(|(account_bytes_saved, num_accounts_saved)| {
            move |&pubkey: &Pubkey, account: &Account| {
                use std::collections::hash_map::Entry::{Occupied, Vacant};
                let should_flush = match written_accounts.entry(pubkey) {
                    Vacant(vacant_entry) => {
                        vacant_entry.insert(());
                        true
                    }
                    Occupied(_occupied_entry) => {
                        *account_bytes_saved += account.data.len();
                        *num_accounts_saved += 1;
                        // If a later root already wrote this account, no point
                        // in flushing it
                        false
                    }
                };
                should_flush
            }
        });

        // Always flush up to `requested_flush_root`, which is necessary for things like snapshotting.
        let cached_roots: BTreeSet<Slot> = self.accounts_cache.clear_roots(requested_flush_root);

        // Iterate from highest to lowest so that we don't need to flush earlier
        // outdated updates in earlier roots
        let mut num_roots_flushed = 0;
        for &root in cached_roots.iter().rev() {
            let should_flush_f = if let Some(max_clean_root) = max_clean_root {
                if root > max_clean_root {
                    // Only if the root is greater than the `max_clean_root` do we
                    // have to prevent cleaning, otherwise, just default to `should_flush_f`
                    // for any slots <= `max_clean_root`
                    None
                } else {
                    should_flush_f.as_mut()
                }
            } else {
                should_flush_f.as_mut()
            };

            if self.flush_slot_cache(root, should_flush_f) {
                num_roots_flushed += 1;
            }

            // Regardless of whether this slot was *just* flushed from the cache by the above
            // `flush_slot_cache()`, we should update the `max_flush_root`.
            // This is because some rooted slots may be flushed to storage *before* they are marked as root.
            // This can occur for instance when:
            // 1) The cache is overwhelmed, we we flushed some yet to be rooted frozen slots
            // 2) Random evictions
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

    // `should_flush_f` is an optional closure that determines wehther a given
    // account should be flushed. Passing `None` will by default flush all
    // accounts
    fn flush_slot_cache(
        &self,
        slot: Slot,
        mut should_flush_f: Option<&mut impl FnMut(&Pubkey, &Account) -> bool>,
    ) -> bool {
        info!("flush_slot_cache slot: {}", slot);
        let slot_cache = self.accounts_cache.slot_cache(slot);
        if let Some(slot_cache) = slot_cache {
            let iter_items: Vec<_> = slot_cache.iter().collect();
            let mut total_size = 0;
            let mut purged_slot_pubkeys: HashSet<(Slot, Pubkey)> = HashSet::new();
            let mut pubkey_to_slot_set: Vec<(Pubkey, Slot)> = vec![];
            let (accounts, hashes): (Vec<(&Pubkey, &Account)>, Vec<Hash>) = iter_items
                .iter()
                .filter_map(|iter_item| {
                    let key = iter_item.key();
                    let account = &iter_item.value().account;
                    let should_flush = should_flush_f
                        .as_mut()
                        .map(|should_flush_f| should_flush_f(key, account))
                        .unwrap_or(true);
                    if should_flush {
                        let hash = iter_item.value().hash;
                        total_size += (account.data.len() + STORE_META_OVERHEAD) as u64;
                        Some(((key, account), hash))
                    } else {
                        // If we don't flush, we have to remove the entry from the
                        // index, since it's equivalent to purging
                        purged_slot_pubkeys.insert((slot, *key));
                        pubkey_to_slot_set.push((*key, slot));
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
            );

            if !is_dead_slot {
                let aligned_total_size = self.page_align(total_size);
                // This ensures that all updates are written to an AppendVec, before any
                // updates to the index happen, so anybody that sees a real entry in the index,
                // will be able to find the account in storage
                let flushed_store =
                    self.create_and_insert_store(slot, aligned_total_size, "flush_slot_cache");
                self.store_accounts_frozen(
                    slot,
                    &accounts,
                    &hashes,
                    Some(Box::new(move |_, _| flushed_store.clone())),
                    None,
                );
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

            // Remove this slot from the cache, which will to AccountsDb readers should look like an
            // atomic switch from the cache to storage
            assert!(self.accounts_cache.remove_slot(slot).is_some());
            true
        } else {
            false
        }
    }

    fn write_accounts_to_cache(
        &self,
        slot: Slot,
        hashes: &[Hash],
        accounts_and_meta_to_store: &[(StoredMeta, &Account)],
    ) -> Vec<AccountInfo> {
        assert_eq!(hashes.len(), accounts_and_meta_to_store.len());
        accounts_and_meta_to_store
            .iter()
            .zip(hashes)
            .map(|((meta, account), hash)| {
                self.accounts_cache
                    .store(slot, &meta.pubkey, (**account).clone(), *hash);
                AccountInfo {
                    store_id: CACHE_VIRTUAL_STORAGE_ID,
                    offset: CACHE_VIRTUAL_OFFSET,
                    stored_size: CACHE_VIRTUAL_STORED_SIZE,
                    lamports: account.lamports,
                }
            })
            .collect()
    }

    fn store_accounts_to<
        F: FnMut(Slot, usize) -> Arc<AccountStorageEntry>,
        P: Iterator<Item = u64>,
    >(
        &self,
        slot: Slot,
        accounts: &[(&Pubkey, &Account)],
        hashes: &[Hash],
        storage_finder: F,
        mut write_version_producer: P,
        is_cached_store: bool,
    ) -> Vec<AccountInfo> {
        let default_account = Account::default();
        let accounts_and_meta_to_store: Vec<(StoredMeta, &Account)> = accounts
            .iter()
            .map(|(pubkey, account)| {
                let account = if account.lamports == 0 {
                    &default_account
                } else {
                    *account
                };
                let data_len = account.data.len() as u64;
                let meta = StoredMeta {
                    write_version: write_version_producer.next().unwrap(),
                    pubkey: **pubkey,
                    data_len,
                };
                (meta, account)
            })
            .collect();

        if self.caching_enabled && is_cached_store {
            self.write_accounts_to_cache(slot, hashes, &accounts_and_meta_to_store)
        } else {
            self.write_accounts_to_storage(
                slot,
                hashes,
                storage_finder,
                &accounts_and_meta_to_store,
            )
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
        for iter_item in self.storage.0.iter() {
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
        }
        info!("total_stores: {}, newest_slot: {}, oldest_slot: {}, max_slot: {} (num={}), min_slot: {} (num={})",
              total_count, newest_slot, oldest_slot, max_slot, max, min_slot, min);
        datapoint_info!(
            "accounts_db-stores",
            ("total_count", total_count, i64),
            (
                "recycle_count",
                self.recycle_stores.read().unwrap().len() as u64,
                i64
            ),
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

    pub fn compute_merkle_root_and_capitalization(
        hashes: Vec<(Pubkey, Hash, u64)>,
        fanout: usize,
    ) -> (Hash, u64) {
        Self::compute_merkle_root_and_capitalization_loop(hashes, fanout, |t| (t.1, t.2))
    }

    // this function avoids an infinite recursion compiler error
    fn compute_merkle_root_and_capitalization_recurse(
        hashes: Vec<(Hash, u64)>,
        fanout: usize,
    ) -> (Hash, u64) {
        Self::compute_merkle_root_and_capitalization_loop(hashes, fanout, |t: &(Hash, u64)| {
            (t.0, t.1)
        })
    }

    // For the first iteration, there could be more items in the tuple than just hash and lamports.
    // Using extractor allows us to avoid an unnecessary array copy on the first iteration.
    fn compute_merkle_root_and_capitalization_loop<T, F>(
        hashes: Vec<T>,
        fanout: usize,
        extractor: F,
    ) -> (Hash, u64)
    where
        F: Fn(&T) -> (Hash, u64) + std::marker::Sync,
        T: std::marker::Sync,
    {
        if hashes.is_empty() {
            return (Hasher::default().result(), 0);
        }

        let mut time = Measure::start("time");

        let total_hashes = hashes.len();
        // we need div_ceil here
        let mut chunks = total_hashes / fanout;
        if total_hashes % fanout != 0 {
            chunks += 1;
        }

        let result: Vec<_> = (0..chunks)
            .into_par_iter()
            .map(|i| {
                let start_index = i * fanout;
                let end_index = std::cmp::min(start_index + fanout, total_hashes);

                let mut hasher = Hasher::default();
                let mut this_sum = 0u128;
                for item in hashes.iter().take(end_index).skip(start_index) {
                    let (h, l) = extractor(&item);
                    this_sum += l as u128;
                    hasher.hash(h.as_ref());
                }

                (
                    hasher.result(),
                    Self::checked_cast_for_capitalization(this_sum),
                )
            })
            .collect();
        time.stop();
        debug!("hashing {} {}", total_hashes, time);

        if result.len() == 1 {
            result[0]
        } else {
            Self::compute_merkle_root_and_capitalization_recurse(result, fanout)
        }
    }

    fn accumulate_account_hashes(
        hashes: Vec<(Pubkey, Hash, u64)>,
        slot: Slot,
        debug: bool,
    ) -> Hash {
        let ((hash, ..), ..) =
            Self::accumulate_account_hashes_and_capitalization(hashes, slot, debug);
        hash
    }

    fn sort_hashes_by_pubkey(hashes: &mut Vec<(Pubkey, Hash, u64)>) {
        hashes.par_sort_unstable_by(|a, b| a.0.cmp(&b.0));
    }

    fn accumulate_account_hashes_and_capitalization(
        mut hashes: Vec<(Pubkey, Hash, u64)>,
        slot: Slot,
        debug: bool,
    ) -> ((Hash, u64), (Measure, Measure)) {
        let mut sort_time = Measure::start("sort");
        Self::sort_hashes_by_pubkey(&mut hashes);
        sort_time.stop();

        if debug {
            for (key, hash, _lamports) in &hashes {
                info!("slot: {} key {} hash {}", slot, key, hash);
            }
        }

        let mut hash_time = Measure::start("hash");
        let res = Self::compute_merkle_root_and_capitalization(hashes, MERKLE_FANOUT);
        hash_time.stop();

        (res, (sort_time, hash_time))
    }

    pub fn checked_cast_for_capitalization(balance: u128) -> u64 {
        balance
            .try_into()
            .expect("overflow is detected while summing capitalization")
    }

    pub fn checked_iterative_sum_for_capitalization(total_cap: u64, new_cap: u64) -> u64 {
        let new_total = total_cap as u128 + new_cap as u128;
        Self::checked_cast_for_capitalization(new_total)
    }

    pub fn checked_sum_for_capitalization<T: Iterator<Item = u64>>(balances: T) -> u64 {
        Self::checked_cast_for_capitalization(balances.map(|b| b as u128).sum::<u128>())
    }

    pub fn account_balance_for_capitalization(
        lamports: u64,
        owner: &Pubkey,
        executable: bool,
        simple_capitalization_enabled: bool,
    ) -> u64 {
        if simple_capitalization_enabled {
            return lamports;
        }

        let is_specially_retained = (solana_sdk::native_loader::check_id(owner) && executable)
            || solana_sdk::sysvar::check_id(owner);

        if is_specially_retained {
            // specially retained accounts always have an initial 1 lamport
            // balance, but could be modified by transfers which increase
            // the balance but don't affect the capitalization.
            lamports - 1
        } else {
            lamports
        }
    }

    fn calculate_accounts_hash(
        &self,
        slot: Slot,
        ancestors: &Ancestors,
        check_hash: bool,
        simple_capitalization_enabled: bool,
    ) -> Result<(Hash, u64), BankHashVerificationError> {
        use BankHashVerificationError::*;
        let mut scan = Measure::start("scan");
        let keys: Vec<_> = self
            .accounts_index
            .account_maps
            .read()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        let mismatch_found = AtomicU64::new(0);
        let hashes: Vec<(Hash, u64)> = {
            self.thread_pool_clean.install(|| {
                keys.par_iter()
                    .filter_map(|pubkey| {
                        if let Some((lock, index)) =
                            self.accounts_index.get(pubkey, Some(ancestors), Some(slot))
                        {
                            let (slot, account_info) = &lock.slot_list()[index];
                            if account_info.lamports != 0 {
                                self.get_account_accessor_from_cache_or_storage(
                                    *slot,
                                    pubkey,
                                    account_info.store_id,
                                    account_info.offset,
                                )
                                .get_loaded_account()
                                .and_then(|loaded_account| {
                                    let loaded_hash = loaded_account.loaded_hash();
                                    let balance = Self::account_balance_for_capitalization(
                                        account_info.lamports,
                                        loaded_account.owner(),
                                        loaded_account.executable(),
                                        simple_capitalization_enabled,
                                    );

                                    if check_hash {
                                        let computed_hash = loaded_account.compute_hash(
                                            *slot,
                                            &self.cluster_type.expect(
                                                "Cluster type must be set at initialization",
                                            ),
                                            pubkey,
                                        );
                                        if computed_hash != *loaded_hash {
                                            mismatch_found.fetch_add(1, Ordering::Relaxed);
                                            return None;
                                        }
                                    }

                                    Some((*loaded_hash, balance))
                                })
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                    .collect()
            })
        };
        if mismatch_found.load(Ordering::Relaxed) > 0 {
            warn!(
                "{} mismatched account hash(es) found",
                mismatch_found.load(Ordering::Relaxed)
            );
            return Err(MismatchedAccountHash);
        }

        scan.stop();
        let hash_total = hashes.len();
        let mut hash_time = Measure::start("hash");
        let (accumulated_hash, total_lamports) =
            Self::compute_merkle_root_and_capitalization_recurse(hashes, MERKLE_FANOUT);
        hash_time.stop();
        datapoint_info!(
            "update_accounts_hash",
            ("accounts_scan", scan.as_us(), i64),
            ("hash", hash_time.as_us(), i64),
            ("hash_total", hash_total, i64),
        );
        Ok((accumulated_hash, total_lamports))
    }

    pub fn get_accounts_hash(&self, slot: Slot) -> Hash {
        let bank_hashes = self.bank_hashes.read().unwrap();
        let bank_hash_info = bank_hashes.get(&slot).unwrap();
        bank_hash_info.snapshot_hash
    }

    pub fn update_accounts_hash(
        &self,
        slot: Slot,
        ancestors: &Ancestors,
        simple_capitalization_enabled: bool,
    ) -> (Hash, u64) {
        self.update_accounts_hash_with_index_option(
            false,
            false,
            slot,
            ancestors,
            simple_capitalization_enabled,
        )
    }

    pub fn update_accounts_hash_test(
        &self,
        slot: Slot,
        ancestors: &Ancestors,
        simple_capitalization_enabled: bool,
    ) -> (Hash, u64) {
        self.update_accounts_hash_with_index_option(
            false,
            true,
            slot,
            ancestors,
            simple_capitalization_enabled,
        )
    }

    /// Scan through all the account storage in parallel
    fn scan_account_storage_no_bank<F, B>(
        snapshot_storages: SnapshotStorages,
        scan_func: F,
    ) -> Vec<B>
    where
        F: Fn(LoadedAccount, AppendVecId, &mut B, Slot) + Send + Sync,
        B: Send + Default,
    {
        snapshot_storages
            .into_par_iter()
            .flatten()
            .map(|storage| {
                let accounts = storage.accounts.accounts(0);
                let mut retval = B::default();
                accounts.into_iter().for_each(|stored_account| {
                    scan_func(
                        LoadedAccount::Stored(stored_account),
                        storage.append_vec_id(),
                        &mut retval,
                        storage.slot(),
                    )
                });
                retval
            })
            .collect()
    }

    fn remove_zero_balance_accounts(
        account_maps: DashMap<Pubkey, CalculateHashIntermediate>,
    ) -> Vec<(Pubkey, Hash, u64)> {
        type ShardType = dashmap::lock::RwLock<
            std::collections::HashMap<
                solana_sdk::pubkey::Pubkey,
                dashmap::SharedValue<CalculateHashIntermediate>,
            >,
        >;
        let shards: &[ShardType] = account_maps.shards();

        let hashes: Vec<_> = shards
            .par_iter()
            .map(|x| {
                let a: dashmap::lock::RwLockReadGuard<HashMap<_, _>> = x.read();
                let res: Vec<_> = a
                    .iter()
                    .filter_map(|inp| {
                        let (pubkey, sv) = inp;
                        let item = sv.get();
                        if item.raw_lamports != 0 {
                            Some((*pubkey, item.hash, item.lamports))
                        } else {
                            None
                        }
                    })
                    .collect();
                res
            })
            .flatten()
            .collect();
        hashes
    }

    fn rest_of_hash_calculation(
        accounts: (DashMap<Pubkey, CalculateHashIntermediate>, Measure),
    ) -> (Hash, u64) {
        let (account_maps, time_scan) = accounts;

        let mut zeros = Measure::start("eliminate zeros");
        let hashes = Self::remove_zero_balance_accounts(account_maps);
        zeros.stop();
        let hash_total = hashes.len();
        let (ret, (sort_time, hash_time)) =
            Self::accumulate_account_hashes_and_capitalization(hashes, Slot::default(), false);
        datapoint_info!(
            "calculate_accounts_hash_without_index",
            ("accounts_scan", time_scan.as_us(), i64),
            ("eliminate_zeros", zeros.as_us(), i64),
            ("hash", hash_time.as_us(), i64),
            ("sort", sort_time.as_us(), i64),
            ("hash_total", hash_total, i64),
        );

        ret
    }

    fn calculate_accounts_hash_helper(
        &self,
        do_not_use_index: bool,
        slot: Slot,
        ancestors: &Ancestors,
        simple_capitalization_enabled: bool,
    ) -> (Hash, u64) {
        if do_not_use_index {
            let combined_maps = self.get_snapshot_storages(slot);

            Self::calculate_accounts_hash_without_index(
                combined_maps,
                simple_capitalization_enabled,
            )
        } else {
            self.calculate_accounts_hash(slot, ancestors, false, simple_capitalization_enabled)
                .unwrap()
        }
    }

    pub fn update_accounts_hash_with_index_option(
        &self,
        do_not_use_index: bool,
        debug_verify: bool,
        slot: Slot,
        ancestors: &Ancestors,
        simple_capitalization_enabled: bool,
    ) -> (Hash, u64) {
        let (hash, total_lamports) = self.calculate_accounts_hash_helper(
            do_not_use_index,
            slot,
            ancestors,
            simple_capitalization_enabled,
        );
        if debug_verify {
            // calculate the other way (store or non-store) and verify results match.
            let (hash_other, total_lamports_other) = self.calculate_accounts_hash_helper(
                !do_not_use_index,
                slot,
                ancestors,
                simple_capitalization_enabled,
            );

            assert_eq!(hash, hash_other);
            assert_eq!(total_lamports, total_lamports_other);
        }
        let mut bank_hashes = self.bank_hashes.write().unwrap();
        let mut bank_hash_info = bank_hashes.get_mut(&slot).unwrap();
        bank_hash_info.snapshot_hash = hash;
        (hash, total_lamports)
    }

    fn handle_one_loaded_account(
        key: &Pubkey,
        found_item: CalculateHashIntermediate,
        map: &DashMap<Pubkey, CalculateHashIntermediate>,
    ) {
        match map.entry(*key) {
            Occupied(mut dest_item) => {
                let contents = dest_item.get();
                if contents.slot < found_item.slot
                    || (contents.slot == found_item.slot
                        && contents.version() <= found_item.version())
                {
                    // replace the item
                    dest_item.insert(found_item);
                }
            }
            Vacant(v) => {
                v.insert(found_item);
            }
        };
    }

    fn scan_snapshot_stores(
        storage: SnapshotStorages,
        simple_capitalization_enabled: bool,
    ) -> (DashMap<Pubkey, CalculateHashIntermediate>, Measure) {
        let map: DashMap<Pubkey, CalculateHashIntermediate> = DashMap::new();
        let mut time = Measure::start("scan all accounts");
        Self::scan_account_storage_no_bank(
            storage,
            |loaded_account: LoadedAccount,
             _store_id: AppendVecId,
             _accum: &mut Vec<(Pubkey, CalculateHashIntermediate)>,
             slot: Slot| {
                let version = loaded_account.write_version();
                let raw_lamports = loaded_account.lamports();
                let balance = Self::account_balance_for_capitalization(
                    raw_lamports,
                    loaded_account.owner(),
                    loaded_account.executable(),
                    simple_capitalization_enabled,
                );

                let source_item = CalculateHashIntermediate::new(
                    version,
                    *loaded_account.loaded_hash(),
                    balance,
                    raw_lamports,
                    slot,
                );
                Self::handle_one_loaded_account(loaded_account.pubkey(), source_item, &map);
            },
        );
        time.stop();

        (map, time)
    }

    // modeled after get_accounts_delta_hash
    // intended to be faster than calculate_accounts_hash
    pub fn calculate_accounts_hash_without_index(
        storages: SnapshotStorages,
        simple_capitalization_enabled: bool,
    ) -> (Hash, u64) {
        let result = Self::scan_snapshot_stores(storages, simple_capitalization_enabled);

        Self::rest_of_hash_calculation(result)
    }

    pub fn verify_bank_hash_and_lamports(
        &self,
        slot: Slot,
        ancestors: &Ancestors,
        total_lamports: u64,
        simple_capitalization_enabled: bool,
    ) -> Result<(), BankHashVerificationError> {
        use BankHashVerificationError::*;

        let (calculated_hash, calculated_lamports) =
            self.calculate_accounts_hash(slot, ancestors, true, simple_capitalization_enabled)?;

        if calculated_lamports != total_lamports {
            warn!(
                "Mismatched total lamports: {} calculated: {}",
                total_lamports, calculated_lamports
            );
            return Err(MismatchedTotalLamports(calculated_lamports, total_lamports));
        }

        let bank_hashes = self.bank_hashes.read().unwrap();
        if let Some(found_hash_info) = bank_hashes.get(&slot) {
            if calculated_hash == found_hash_info.snapshot_hash {
                Ok(())
            } else {
                warn!(
                    "mismatched bank hash for slot {}: {} (calculated) != {} (expected)",
                    slot, calculated_hash, found_hash_info.snapshot_hash
                );
                Err(MismatchedBankHash)
            }
        } else {
            Err(MissingBankHash)
        }
    }

    pub fn get_accounts_delta_hash(&self, slot: Slot) -> Hash {
        let mut scan = Measure::start("scan");

        let scan_result: ScanStorageResult<(Pubkey, Hash, u64), DashMapVersionHash> = self
            .scan_account_storage(
                slot,
                |loaded_account: LoadedAccount| {
                    // Cache only has one version per key, don't need to worry about versioning
                    Some((
                        *loaded_account.pubkey(),
                        *loaded_account.loaded_hash(),
                        CACHE_VIRTUAL_WRITE_VERSION,
                    ))
                },
                |accum: &DashMap<Pubkey, (u64, Hash)>, loaded_account: LoadedAccount| {
                    let loaded_write_version = loaded_account.write_version();
                    let loaded_hash = *loaded_account.loaded_hash();
                    let should_insert =
                        if let Some(existing_entry) = accum.get(loaded_account.pubkey()) {
                            loaded_write_version > existing_entry.value().version()
                        } else {
                            true
                        };
                    if should_insert {
                        // Detected insertion is necessary, grabs the write lock to commit the write,
                        match accum.entry(*loaded_account.pubkey()) {
                            // Double check in case another thread interleaved a write between the read + write.
                            Occupied(mut occupied_entry) => {
                                if loaded_write_version > occupied_entry.get().version() {
                                    occupied_entry.insert((loaded_write_version, loaded_hash));
                                }
                            }

                            Vacant(vacant_entry) => {
                                vacant_entry.insert((loaded_write_version, loaded_hash));
                            }
                        }
                    }
                },
            );
        scan.stop();

        let mut accumulate = Measure::start("accumulate");
        let hashes: Vec<_> = match scan_result {
            ScanStorageResult::Cached(cached_result) => cached_result,
            ScanStorageResult::Stored(stored_result) => stored_result
                .into_iter()
                .map(|(pubkey, (_latest_write_version, hash))| (pubkey, hash, 0))
                .collect(),
        };
        let dirty_keys = hashes
            .iter()
            .map(|(pubkey, _hash, _lamports)| *pubkey)
            .collect();
        let ret = Self::accumulate_account_hashes(hashes, slot, false);
        accumulate.stop();
        let mut uncleaned_time = Measure::start("uncleaned_index");
        self.uncleaned_pubkeys.insert(slot, dirty_keys);
        uncleaned_time.stop();
        self.stats
            .store_uncleaned_update
            .fetch_add(uncleaned_time.as_us(), Ordering::Relaxed);

        self.stats
            .delta_hash_scan_time_total_us
            .fetch_add(scan.as_us(), Ordering::Relaxed);
        self.stats
            .delta_hash_accumulate_time_total_us
            .fetch_add(accumulate.as_us(), Ordering::Relaxed);
        self.stats.delta_hash_num.fetch_add(1, Ordering::Relaxed);
        ret
    }

    fn update_index(
        &self,
        slot: Slot,
        infos: Vec<AccountInfo>,
        accounts: &[(&Pubkey, &Account)],
    ) -> SlotList<AccountInfo> {
        let mut reclaims = SlotList::<AccountInfo>::with_capacity(infos.len() * 2);
        for (info, pubkey_account) in infos.into_iter().zip(accounts.iter()) {
            let pubkey = pubkey_account.0;
            self.accounts_index.upsert(
                slot,
                pubkey,
                &pubkey_account.1.owner,
                &pubkey_account.1.data,
                &self.account_indexes,
                info,
                &mut reclaims,
            );
        }
        reclaims
    }

    fn remove_dead_accounts(
        &self,
        reclaims: SlotSlice<AccountInfo>,
        expected_slot: Option<Slot>,
        mut reclaimed_offsets: Option<&mut AppendVecOffsets>,
        reset_accounts: bool,
    ) -> HashSet<Slot> {
        let mut dead_slots = HashSet::new();
        let mut new_shrink_candidates: ShrinkCandidates = HashMap::new();
        for (slot, account_info) in reclaims {
            // No cached accounts should make it here
            assert_ne!(account_info.store_id, CACHE_VIRTUAL_STORAGE_ID);
            if let Some(ref mut reclaimed_offsets) = reclaimed_offsets {
                reclaimed_offsets
                    .entry(account_info.store_id)
                    .or_default()
                    .insert(account_info.offset);
            }
            if let Some(expected_slot) = expected_slot {
                assert_eq!(*slot, expected_slot);
            }
            if let Some(store) = self
                .storage
                .get_account_storage_entry(*slot, account_info.store_id)
            {
                assert_eq!(
                    *slot, store.slot(),
                    "AccountDB::accounts_index corrupted. Storage pointed to: {}, expected: {}, should only point to one slot",
                    store.slot(), *slot
                );
                let count = store.remove_account(account_info.stored_size, reset_accounts);
                if count == 0 {
                    dead_slots.insert(*slot);
                } else if self.caching_enabled
                    && (self.page_align(store.alive_bytes() as u64) as f64
                        / store.total_bytes() as f64)
                        < SHRINK_RATIO
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

        if self.caching_enabled {
            {
                let mut shrink_candidate_slots = self.shrink_candidate_slots.lock().unwrap();
                for (slot, slot_shrink_candidates) in new_shrink_candidates {
                    for (store_id, store) in slot_shrink_candidates {
                        shrink_candidate_slots
                            .entry(slot)
                            .or_default()
                            .insert(store_id, store);
                    }
                }
            }
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

    fn finalize_dead_slot_removal<'a>(
        &'a self,
        dead_slots_iter: impl Iterator<Item = &'a Slot> + Clone,
        purged_slot_pubkeys: HashSet<(Slot, Pubkey)>,
        mut purged_account_slots: Option<&mut AccountSlots>,
    ) {
        for (slot, pubkey) in purged_slot_pubkeys {
            if let Some(ref mut purged_account_slots) = purged_account_slots {
                purged_account_slots.entry(pubkey).or_default().insert(slot);
            }
            self.accounts_index.unref_from_storage(&pubkey);
        }

        let mut accounts_index_root_stats = AccountsIndexRootsStats::default();
        for slot in dead_slots_iter.clone() {
            if let Some(latest) = self.accounts_index.clean_dead_slot(*slot) {
                accounts_index_root_stats = latest;
            }
        }

        self.clean_accounts_stats
            .latest_accounts_index_roots_stats
            .update(&accounts_index_root_stats);

        {
            let mut bank_hashes = self.bank_hashes.write().unwrap();
            for slot in dead_slots_iter {
                bank_hashes.remove(slot);
            }
        }
    }

    fn clean_stored_dead_slots(
        &self,
        dead_slots: &HashSet<Slot>,
        purged_account_slots: Option<&mut AccountSlots>,
    ) {
        let mut measure = Measure::start("clean_stored_dead_slots-ms");
        let mut stores: Vec<Arc<AccountStorageEntry>> = vec![];
        for slot in dead_slots.iter() {
            if let Some(slot_storage) = self.storage.get_slot_stores(*slot) {
                for store in slot_storage.read().unwrap().values() {
                    stores.push(store.clone());
                }
            }
        }
        let purged_slot_pubkeys: HashSet<(Slot, Pubkey)> = {
            self.thread_pool_clean.install(|| {
                stores
                    .into_par_iter()
                    .map(|store| {
                        let accounts = store.all_accounts();
                        accounts
                            .into_iter()
                            .map(|account| (store.slot(), account.meta.pubkey))
                            .collect::<HashSet<(Slot, Pubkey)>>()
                    })
                    .reduce(HashSet::new, |mut reduced, store_pubkeys| {
                        reduced.extend(store_pubkeys);
                        reduced
                    })
            })
        };
        self.finalize_dead_slot_removal(
            dead_slots.iter(),
            purged_slot_pubkeys,
            purged_account_slots,
        );
        measure.stop();
        inc_new_counter_info!("clean_stored_dead_slots-ms", measure.as_ms() as usize);
    }

    fn hash_accounts(
        &self,
        slot: Slot,
        accounts: &[(&Pubkey, &Account)],
        cluster_type: &ClusterType,
    ) -> Vec<Hash> {
        let mut stats = BankHashStats::default();
        let mut total_data = 0;
        let hashes: Vec<_> = accounts
            .iter()
            .map(|(pubkey, account)| {
                total_data += account.data.len();
                stats.update(account);
                Self::hash_account(slot, account, pubkey, cluster_type)
            })
            .collect();

        self.stats
            .store_total_data
            .fetch_add(total_data as u64, Ordering::Relaxed);

        let mut bank_hashes = self.bank_hashes.write().unwrap();
        let slot_info = bank_hashes
            .entry(slot)
            .or_insert_with(BankHashInfo::default);
        slot_info.stats.merge(&stats);

        hashes
    }

    pub(crate) fn freeze_accounts(&mut self, ancestors: &Ancestors, account_pubkeys: &[Pubkey]) {
        for account_pubkey in account_pubkeys {
            if let Some((account, _slot)) = self.load_slow(ancestors, &account_pubkey) {
                let frozen_account_info = FrozenAccountInfo {
                    hash: Self::hash_frozen_account_data(&account),
                    lamports: account.lamports,
                };
                warn!(
                    "Account {} is now frozen at lamports={}, hash={}",
                    account_pubkey, frozen_account_info.lamports, frozen_account_info.hash
                );
                self.frozen_accounts
                    .insert(*account_pubkey, frozen_account_info);
            } else {
                panic!(
                    "Unable to freeze an account that does not exist: {}",
                    account_pubkey
                );
            }
        }
    }

    /// Cause a panic if frozen accounts would be affected by data in `accounts`
    fn assert_frozen_accounts(&self, accounts: &[(&Pubkey, &Account)]) {
        if self.frozen_accounts.is_empty() {
            return;
        }
        for (account_pubkey, account) in accounts.iter() {
            if let Some(frozen_account_info) = self.frozen_accounts.get(*account_pubkey) {
                if account.lamports < frozen_account_info.lamports {
                    FROZEN_ACCOUNT_PANIC.store(true, Ordering::Relaxed);
                    panic!(
                        "Frozen account {} modified.  Lamports decreased from {} to {}",
                        account_pubkey, frozen_account_info.lamports, account.lamports,
                    )
                }

                let hash = Self::hash_frozen_account_data(&account);
                if hash != frozen_account_info.hash {
                    FROZEN_ACCOUNT_PANIC.store(true, Ordering::Relaxed);
                    panic!(
                        "Frozen account {} modified.  Hash changed from {} to {}",
                        account_pubkey, frozen_account_info.hash, hash,
                    )
                }
            }
        }
    }

    pub fn store_cached(&self, slot: Slot, accounts: &[(&Pubkey, &Account)]) {
        self.store(slot, accounts, self.caching_enabled);
    }

    /// Store the account update.
    pub fn store_uncached(&self, slot: Slot, accounts: &[(&Pubkey, &Account)]) {
        self.store(slot, accounts, false);
    }

    fn store(&self, slot: Slot, accounts: &[(&Pubkey, &Account)], is_cached_store: bool) {
        // If all transactions in a batch are errored,
        // it's possible to get a store with no accounts.
        if accounts.is_empty() {
            return;
        }
        self.assert_frozen_accounts(accounts);
        let mut hash_time = Measure::start("hash_accounts");
        let hashes = self.hash_accounts(
            slot,
            accounts,
            &self
                .cluster_type
                .expect("Cluster type must be set at initialization"),
        );
        hash_time.stop();
        self.stats
            .store_hash_accounts
            .fetch_add(hash_time.as_us(), Ordering::Relaxed);
        self.store_accounts_unfrozen(slot, accounts, &hashes, is_cached_store);
        self.report_store_timings();
    }

    fn report_store_timings(&self) {
        let last = self.stats.last_store_report.load(Ordering::Relaxed);
        let now = solana_sdk::timing::timestamp();

        if now.saturating_sub(last) > 1000
            && self.stats.last_store_report.compare_exchange(
                last,
                now,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) == Ok(last)
        {
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
            );

            datapoint_info!(
                "accounts_db_store_timings2",
                (
                    "recycle_store_count",
                    self.stats.recycle_store_count.swap(0, Ordering::Relaxed),
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

    fn store_accounts_unfrozen(
        &self,
        slot: Slot,
        accounts: &[(&Pubkey, &Account)],
        hashes: &[Hash],
        is_cached_store: bool,
    ) {
        // This path comes from a store to a non-frozen slot.
        // If a store is dead here, then a newer update for
        // each pubkey in the store must exist in another
        // store in the slot. Thus it is safe to reset the store and
        // re-use it for a future store op. The pubkey ref counts should still
        // hold just 1 ref from this slot.
        let reset_accounts = true;

        self.store_accounts_custom(
            slot,
            accounts,
            hashes,
            None::<StorageFinder>,
            None::<Box<dyn Iterator<Item = u64>>>,
            is_cached_store,
            reset_accounts,
        );
    }

    fn store_accounts_frozen<'a>(
        &'a self,
        slot: Slot,
        accounts: &[(&Pubkey, &Account)],
        hashes: &[Hash],
        storage_finder: Option<StorageFinder<'a>>,
        write_version_producer: Option<Box<dyn Iterator<Item = u64>>>,
    ) -> StoreAccountsTiming {
        // stores on a frozen slot should not reset
        // the append vec so that hashing could happen on the store
        // and accounts in the append_vec can be unrefed correctly
        let reset_accounts = false;
        let is_cached_store = false;
        self.store_accounts_custom(
            slot,
            accounts,
            hashes,
            storage_finder,
            write_version_producer,
            is_cached_store,
            reset_accounts,
        )
    }

    fn store_accounts_custom<'a>(
        &'a self,
        slot: Slot,
        accounts: &[(&Pubkey, &Account)],
        hashes: &[Hash],
        storage_finder: Option<StorageFinder<'a>>,
        write_version_producer: Option<Box<dyn Iterator<Item = u64>>>,
        is_cached_store: bool,
        reset_accounts: bool,
    ) -> StoreAccountsTiming {
        let storage_finder: StorageFinder<'a> = storage_finder
            .unwrap_or_else(|| Box::new(move |slot, size| self.find_storage_candidate(slot, size)));

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
            slot,
            accounts,
            hashes,
            storage_finder,
            write_version_producer,
            is_cached_store,
        );
        store_accounts_time.stop();
        self.stats
            .store_accounts
            .fetch_add(store_accounts_time.as_us(), Ordering::Relaxed);
        let mut update_index_time = Measure::start("update_index");

        // If the cache was flushed, then because `update_index` occurs
        // after the account are stored by the above `store_accounts_to`
        // call and all the accounts are stored, all reads after this point
        // will know to not check the cache anymore
        let mut reclaims = self.update_index(slot, infos, accounts);

        // For each updated account, `reclaims` should only have at most one
        // item (if the account was previously updated in this slot).
        // filter out the cached reclaims as those don't actually map
        // to anything that needs to be cleaned in the backing storage
        // entries
        if self.caching_enabled {
            reclaims.retain(|(_, r)| r.store_id != CACHE_VIRTUAL_STORAGE_ID);

            if is_cached_store {
                assert!(reclaims.is_empty());
            }
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
        // From 1) and 2) we guarantee passing Some(slot), true is safe
        let mut handle_reclaims_time = Measure::start("handle_reclaims");
        self.handle_reclaims(&reclaims, Some(slot), true, None, reset_accounts);
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

    pub fn add_root(&self, slot: Slot) {
        self.accounts_index.add_root(slot, self.caching_enabled);
        if self.caching_enabled {
            self.accounts_cache.add_root(slot);
        }
    }

    pub fn get_snapshot_storages(&self, snapshot_slot: Slot) -> SnapshotStorages {
        self.storage
            .0
            .iter()
            .filter(|iter_item| {
                let slot = *iter_item.key();
                slot <= snapshot_slot && self.accounts_index.is_root(slot)
            })
            .map(|iter_item| {
                iter_item
                    .value()
                    .read()
                    .unwrap()
                    .values()
                    .filter(|x| x.has_accounts())
                    .cloned()
                    .collect()
            })
            .filter(|snapshot_storage: &SnapshotStorage| !snapshot_storage.is_empty())
            .collect()
    }

    pub fn generate_index(&self) {
        type AccountsMap<'a> = HashMap<Pubkey, BTreeMap<u64, (AppendVecId, StoredAccountMeta<'a>)>>;
        let mut slots = self.storage.all_slots();
        #[allow(clippy::stable_sort_primitive)]
        slots.sort();

        let mut last_log_update = Instant::now();
        for (index, slot) in slots.iter().enumerate() {
            let now = Instant::now();
            if now.duration_since(last_log_update).as_secs() >= 2 {
                info!("generating index: {}/{} slots...", index, slots.len());
                last_log_update = now;
            }
            let storage_maps: Vec<Arc<AccountStorageEntry>> = self
                .storage
                .get_slot_stores(*slot)
                .map(|res| res.read().unwrap().values().cloned().collect())
                .unwrap_or_default();
            let num_accounts = storage_maps
                .iter()
                .map(|storage| storage.approx_stored_count())
                .sum();
            let mut accounts_map: AccountsMap = AccountsMap::with_capacity(num_accounts);
            storage_maps.iter().for_each(|storage| {
                let accounts = storage.all_accounts();
                accounts.into_iter().for_each(|stored_account| {
                    let entry = accounts_map
                        .entry(stored_account.meta.pubkey)
                        .or_insert_with(BTreeMap::new);
                    assert!(
                        // There should only be one update per write version for a specific slot
                        // and account
                        entry
                            .insert(
                                stored_account.meta.write_version,
                                (storage.append_vec_id(), stored_account)
                            )
                            .is_none()
                    );
                })
            });
            // Need to restore indexes even with older write versions which may
            // be shielding other accounts. When they are then purged, the
            // original non-shielded account value will be visible when the account
            // is restored from the append-vec
            if !accounts_map.is_empty() {
                let mut _reclaims: Vec<(u64, AccountInfo)> = vec![];
                let dirty_keys = accounts_map.iter().map(|(pubkey, _info)| *pubkey).collect();
                self.uncleaned_pubkeys.insert(*slot, dirty_keys);
                for (pubkey, account_infos) in accounts_map.into_iter() {
                    for (_, (store_id, stored_account)) in account_infos.into_iter() {
                        let account_info = AccountInfo {
                            store_id,
                            offset: stored_account.offset,
                            stored_size: stored_account.stored_size,
                            lamports: stored_account.account_meta.lamports,
                        };
                        self.accounts_index.insert_new_if_missing(
                            *slot,
                            &pubkey,
                            &stored_account.account_meta.owner,
                            &stored_account.data,
                            &self.account_indexes,
                            account_info,
                            &mut _reclaims,
                        );
                    }
                }
            }
        }

        // Need to add these last, otherwise older updates will be cleaned
        for slot in slots {
            self.accounts_index.add_root(slot, false);
        }

        let mut stored_sizes_and_counts = HashMap::new();
        for account_entry in self.accounts_index.account_maps.read().unwrap().values() {
            for (_slot, account_entry) in account_entry.slot_list.read().unwrap().iter() {
                let storage_entry_meta = stored_sizes_and_counts
                    .entry(account_entry.store_id)
                    .or_insert((0, 0));
                storage_entry_meta.0 += account_entry.stored_size;
                storage_entry_meta.1 += 1;
            }
        }
        for slot_stores in self.storage.0.iter() {
            for (id, store) in slot_stores.value().read().unwrap().iter() {
                // Should be default at this point
                assert_eq!(store.alive_bytes(), 0);
                if let Some((stored_size, count)) = stored_sizes_and_counts.get(&id) {
                    trace!("id: {} setting count: {} cur: {}", id, count, store.count(),);
                    store.count_and_status.write().unwrap().0 = *count;
                    store.alive_bytes.store(*stored_size, Ordering::SeqCst);
                } else {
                    trace!("id: {} clearing count", id);
                    store.count_and_status.write().unwrap().0 = 0;
                }
            }
        }
    }

    pub(crate) fn print_accounts_stats(&self, label: &str) {
        self.print_index(label);
        self.print_count_and_status(label);
        info!("recycle_stores:");
        let recycle_stores = self.recycle_stores.read().unwrap();
        for entry in recycle_stores.iter() {
            info!(
                "  slot: {} id: {} count_and_status: {:?} approx_store_count: {} len: {} capacity: {}",
                entry.slot(),
                entry.append_vec_id(),
                *entry.count_and_status.read().unwrap(),
                entry.approx_store_count.load(Ordering::Relaxed),
                entry.accounts.len(),
                entry.accounts.capacity(),
            );
        }
    }

    fn print_index(&self, label: &str) {
        let mut roots: Vec<_> = self.accounts_index.all_roots();
        #[allow(clippy::stable_sort_primitive)]
        roots.sort();
        info!("{}: accounts_index roots: {:?}", label, roots,);
        for (pubkey, account_entry) in self.accounts_index.account_maps.read().unwrap().iter() {
            info!("  key: {} ref_count: {}", pubkey, account_entry.ref_count(),);
            info!(
                "      slots: {:?}",
                *account_entry.slot_list.read().unwrap()
            );
        }
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

#[cfg(test)]
impl AccountsDB {
    pub fn new_sized(paths: Vec<PathBuf>, file_size: u64) -> Self {
        AccountsDB {
            file_size,
            ..AccountsDB::new(paths, &ClusterType::Development)
        }
    }

    pub fn new_sized_no_extra_stores(paths: Vec<PathBuf>, file_size: u64) -> Self {
        AccountsDB {
            file_size,
            min_num_stores: 0,
            ..AccountsDB::new(paths, &ClusterType::Development)
        }
    }

    pub fn get_append_vec_id(&self, pubkey: &Pubkey, slot: Slot) -> Option<AppendVecId> {
        let ancestors = vec![(slot, 1)].into_iter().collect();
        let result = self.accounts_index.get(&pubkey, Some(&ancestors), None);
        result.map(|(list, index)| list.slot_list()[index].1.store_id)
    }

    pub fn alive_account_count_in_slot(&self, slot: Slot) -> usize {
        self.storage
            .get_slot_stores(slot)
            .map(|storages| storages.read().unwrap().values().map(|s| s.count()).sum())
            .unwrap_or(0)
    }
}

/// Legacy shrink functions to support non-cached path.
/// Should be able to be deleted after cache path is the only path.
impl AccountsDB {
    // Reads all accounts in given slot's AppendVecs and filter only to alive,
    // then create a minimum AppendVec filled with the alive.
    // v1 path shrinks all stores in the slot
    //
    // Requires all stores in the slot to be re-written otherwise the accounts_index
    // store ref count could become incorrect.
    fn do_shrink_slot_v1(&self, slot: Slot, forced: bool) -> usize {
        trace!("shrink_stale_slot: slot: {}", slot);

        let mut stored_accounts = vec![];
        let mut storage_read_elapsed = Measure::start("storage_read_elapsed");
        {
            if let Some(stores_lock) = self.storage.get_slot_stores(slot) {
                let stores = stores_lock.read().unwrap();
                let mut alive_count = 0;
                let mut stored_count = 0;
                let mut written_bytes = 0;
                let mut total_bytes = 0;
                for store in stores.values() {
                    alive_count += store.count();
                    stored_count += store.approx_stored_count();
                    written_bytes += store.written_bytes();
                    total_bytes += store.total_bytes();
                }
                if alive_count == stored_count && stores.values().len() == 1 {
                    trace!(
                        "shrink_stale_slot ({}): not able to shrink at all: alive/stored: {} / {} {}",
                        slot,
                        alive_count,
                        stored_count,
                        if forced { " (forced)" } else { "" },
                    );
                    return 0;
                } else if !forced {
                    let sparse_by_count = (alive_count as f32 / stored_count as f32) <= 0.8;
                    let sparse_by_bytes = (written_bytes as f32 / total_bytes as f32) <= 0.8;
                    let not_sparse = !sparse_by_count && !sparse_by_bytes;
                    let too_small_to_shrink = total_bytes <= PAGE_SIZE;
                    if not_sparse || too_small_to_shrink {
                        return 0;
                    }
                    info!(
                        "shrink_stale_slot ({}): not_sparse: {} count: {}/{} byte: {}/{}",
                        slot, not_sparse, alive_count, stored_count, written_bytes, total_bytes,
                    );
                }
                for store in stores.values() {
                    let mut start = 0;
                    while let Some((account, next)) = store.accounts.get_account(start) {
                        stored_accounts.push((
                            account.meta.pubkey,
                            account.clone_account(),
                            *account.hash,
                            next - start,
                            (store.append_vec_id(), account.offset),
                            account.meta.write_version,
                        ));
                        start = next;
                    }
                }
            }
        }
        storage_read_elapsed.stop();

        let mut index_read_elapsed = Measure::start("index_read_elapsed");
        let alive_accounts: Vec<_> = {
            stored_accounts
                .iter()
                .filter(
                    |(
                        pubkey,
                        _account,
                        _account_hash,
                        _storage_size,
                        (store_id, offset),
                        _write_version,
                    )| {
                        if let Some((locked_entry, _)) = self.accounts_index.get(pubkey, None, None)
                        {
                            locked_entry
                                .slot_list()
                                .iter()
                                .any(|(_slot, i)| i.store_id == *store_id && i.offset == *offset)
                        } else {
                            false
                        }
                    },
                )
                .collect()
        };
        index_read_elapsed.stop();

        let alive_total: u64 = alive_accounts
            .iter()
            .map(
                |(_pubkey, _account, _account_hash, account_size, _location, _write_verion)| {
                    *account_size as u64
                },
            )
            .sum();
        let aligned_total: u64 = self.page_align(alive_total);

        debug!(
            "shrinking: slot: {}, stored_accounts: {} => alive_accounts: {} ({} bytes; aligned to: {})",
            slot,
            stored_accounts.len(),
            alive_accounts.len(),
            alive_total,
            aligned_total
        );

        let mut rewrite_elapsed = Measure::start("rewrite_elapsed");
        let mut dead_storages = vec![];
        let mut find_alive_elapsed = 0;
        let mut create_and_insert_store_elapsed = 0;
        let mut write_storage_elapsed = 0;
        let mut store_accounts_timing = StoreAccountsTiming::default();
        if aligned_total > 0 {
            let mut start = Measure::start("find_alive_elapsed");
            let mut accounts = Vec::with_capacity(alive_accounts.len());
            let mut hashes = Vec::with_capacity(alive_accounts.len());
            let mut write_versions = Vec::with_capacity(alive_accounts.len());

            for (pubkey, account, account_hash, _size, _location, write_version) in &alive_accounts
            {
                accounts.push((pubkey, account));
                hashes.push(*account_hash);
                write_versions.push(*write_version);
            }
            start.stop();
            find_alive_elapsed = start.as_us();

            let mut start = Measure::start("create_and_insert_store_elapsed");
            let shrunken_store = if let Some(new_store) =
                self.try_recycle_and_insert_store(slot, aligned_total, aligned_total + 1024)
            {
                new_store
            } else {
                let maybe_shrink_paths = self.shrink_paths.read().unwrap();
                if let Some(ref shrink_paths) = *maybe_shrink_paths {
                    self.create_and_insert_store_with_paths(
                        slot,
                        aligned_total,
                        "shrink-w-path",
                        shrink_paths,
                    )
                } else {
                    self.create_and_insert_store(slot, aligned_total, "shrink")
                }
            };
            start.stop();
            create_and_insert_store_elapsed = start.as_us();

            // here, we're writing back alive_accounts. That should be an atomic operation
            // without use of rather wide locks in this whole function, because we're
            // mutating rooted slots; There should be no writers to them.
            store_accounts_timing = self.store_accounts_frozen(
                slot,
                &accounts,
                &hashes,
                Some(Box::new(move |_, _| shrunken_store.clone())),
                Some(Box::new(write_versions.into_iter())),
            );

            let mut start = Measure::start("write_storage_elapsed");
            if let Some(slot_stores) = self.storage.get_slot_stores(slot) {
                slot_stores.write().unwrap().retain(|_key, store| {
                    if store.count() == 0 {
                        dead_storages.push(store.clone());
                    }
                    store.count() > 0
                });
            }
            start.stop();
            write_storage_elapsed = start.as_us();
        }
        rewrite_elapsed.stop();

        let mut recycle_stores_write_elapsed = Measure::start("recycle_stores_write_elapsed");
        let mut recycle_stores = self.recycle_stores.write().unwrap();
        recycle_stores_write_elapsed.stop();

        let mut drop_storage_entries_elapsed = Measure::start("drop_storage_entries_elapsed");
        if recycle_stores.len() < MAX_RECYCLE_STORES {
            recycle_stores.extend(dead_storages);
            drop(recycle_stores);
        } else {
            self.stats
                .dropped_stores
                .fetch_add(recycle_stores.len() as u64, Ordering::Relaxed);
            drop(recycle_stores);
            drop(dead_storages);
        }
        drop_storage_entries_elapsed.stop();

        self.shrink_stats
            .num_slots_shrunk
            .fetch_add(1, Ordering::Relaxed);
        self.shrink_stats
            .storage_read_elapsed
            .fetch_add(storage_read_elapsed.as_us(), Ordering::Relaxed);
        self.shrink_stats
            .index_read_elapsed
            .fetch_add(index_read_elapsed.as_us(), Ordering::Relaxed);
        self.shrink_stats
            .find_alive_elapsed
            .fetch_add(find_alive_elapsed, Ordering::Relaxed);
        self.shrink_stats
            .create_and_insert_store_elapsed
            .fetch_add(create_and_insert_store_elapsed, Ordering::Relaxed);
        self.shrink_stats.store_accounts_elapsed.fetch_add(
            store_accounts_timing.store_accounts_elapsed,
            Ordering::Relaxed,
        );
        self.shrink_stats.update_index_elapsed.fetch_add(
            store_accounts_timing.update_index_elapsed,
            Ordering::Relaxed,
        );
        self.shrink_stats.handle_reclaims_elapsed.fetch_add(
            store_accounts_timing.handle_reclaims_elapsed,
            Ordering::Relaxed,
        );
        self.shrink_stats
            .write_storage_elapsed
            .fetch_add(write_storage_elapsed, Ordering::Relaxed);
        self.shrink_stats
            .rewrite_elapsed
            .fetch_add(rewrite_elapsed.as_us(), Ordering::Relaxed);
        self.shrink_stats
            .drop_storage_entries_elapsed
            .fetch_add(drop_storage_entries_elapsed.as_us(), Ordering::Relaxed);
        self.shrink_stats
            .recycle_stores_write_elapsed
            .fetch_add(recycle_stores_write_elapsed.as_us(), Ordering::Relaxed);
        self.shrink_stats.report();

        alive_accounts.len()
    }

    fn do_reset_uncleaned_roots_v1(
        &self,
        candidates: &mut MutexGuard<Vec<Slot>>,
        max_clean_root: Option<Slot>,
    ) {
        let previous_roots = self.accounts_index.reset_uncleaned_roots(max_clean_root);
        candidates.extend(previous_roots);
    }

    #[cfg(test)]
    fn reset_uncleaned_roots_v1(&self) {
        self.do_reset_uncleaned_roots_v1(&mut self.shrink_candidate_slots_v1.lock().unwrap(), None);
    }

    fn do_shrink_stale_slot_v1(&self, slot: Slot) -> usize {
        self.do_shrink_slot_v1(slot, false)
    }
    fn do_shrink_slot_forced_v1(&self, slot: Slot) {
        self.do_shrink_slot_v1(slot, true);
    }

    fn shrink_stale_slot_v1(&self, candidates: &mut MutexGuard<Vec<Slot>>) -> usize {
        let mut shrunken_account_total = 0;
        let mut shrunk_slot_count = 0;
        let start = Instant::now();
        let num_roots = self.accounts_index.num_roots();
        loop {
            if let Some(slot) = self.do_next_shrink_slot_v1(candidates) {
                shrunken_account_total += self.do_shrink_stale_slot_v1(slot);
            } else {
                return 0;
            }
            if start.elapsed().as_millis() > 100 || shrunk_slot_count > num_roots / 10 {
                debug!(
                    "do_shrink_stale_slot_v1: {} {} {}us",
                    shrunk_slot_count,
                    candidates.len(),
                    start.elapsed().as_micros()
                );
                break;
            }
            shrunk_slot_count += 1;
        }
        shrunken_account_total
    }

    // Infinitely returns rooted roots in cyclic order
    fn do_next_shrink_slot_v1(&self, candidates: &mut MutexGuard<Vec<Slot>>) -> Option<Slot> {
        // At this point, a lock (= candidates) is ensured to be held to keep
        // do_reset_uncleaned_roots() (in clean_accounts()) from updating candidates.
        // Also, candidates in the lock may be swapped here if it's empty.
        let next = candidates.pop();

        if next.is_some() {
            next
        } else {
            let mut new_all_slots = self.all_root_slots_in_index();
            let next = new_all_slots.pop();
            // refresh candidates for later calls!
            **candidates = new_all_slots;

            next
        }
    }

    #[cfg(test)]
    fn next_shrink_slot_v1(&self) -> Option<Slot> {
        let mut candidates = self.shrink_candidate_slots_v1.lock().unwrap();
        self.do_next_shrink_slot_v1(&mut candidates)
    }

    pub fn process_stale_slot_v1(&self) -> usize {
        let mut measure = Measure::start("stale_slot_shrink-ms");
        let candidates = self.shrink_candidate_slots_v1.try_lock();
        if candidates.is_err() {
            // skip and return immediately if locked by clean_accounts()
            // the calling background thread will just retry later.
            return 0;
        }
        // hold this lock as long as this shrinking process is running to avoid conflicts
        // with clean_accounts().
        let mut candidates = candidates.unwrap();

        let count = self.shrink_stale_slot_v1(&mut candidates);
        measure.stop();
        inc_new_counter_info!("stale_slot_shrink-ms", measure.as_ms() as usize);

        count
    }

    #[cfg(test)]
    fn shrink_all_stale_slots_v1(&self) {
        for slot in self.all_slots_in_storage() {
            self.do_shrink_stale_slot_v1(slot);
        }
    }
}

#[cfg(test)]
pub mod tests {
    // TODO: all the bank tests are bank specific, issue: 2194
    use super::*;
    use crate::{
        accounts_index::tests::*, accounts_index::RefCount, append_vec::AccountMeta,
        inline_spl_token_v2_0,
    };
    use assert_matches::assert_matches;
    use rand::{thread_rng, Rng};
    use solana_sdk::{account::Account, hash::HASH_BYTES, pubkey::PUBKEY_BYTES};
    use std::{
        iter::FromIterator,
        str::FromStr,
        thread::{self, sleep, Builder, JoinHandle},
        time::Duration,
    };

    fn linear_ancestors(end_slot: u64) -> Ancestors {
        let mut ancestors: Ancestors = vec![(0, 0)].into_iter().collect();
        for i in 1..end_slot {
            ancestors.insert(i, (i - 1) as usize);
        }
        ancestors
    }

    #[test]
    fn test_accountsdb_rest_of_hash_calculation() {
        solana_logger::setup();

        let key = Pubkey::new(&[11u8; 32]);
        let account_maps: DashMap<Pubkey, CalculateHashIntermediate> = DashMap::new();
        let hash = Hash::new(&[1u8; 32]);
        let val = CalculateHashIntermediate::new(0, hash, 88, 490, Slot::default());
        account_maps.insert(key, val);

        // 2nd key - zero lamports, so will be removed
        let key = Pubkey::new(&[12u8; 32]);
        let hash = Hash::new(&[2u8; 32]);
        let val = CalculateHashIntermediate::new(0, hash, 1, 0, Slot::default());
        account_maps.insert(key, val);

        let result =
            AccountsDB::rest_of_hash_calculation((account_maps.clone(), Measure::start("")));
        let expected_hash = Hash::from_str("8j9ARGFv4W2GfML7d3sVJK2MePwrikqYnu6yqer28cCa").unwrap();
        assert_eq!(result, (expected_hash, 88));

        // 3rd key - with pubkey value before 1st key so it will be sorted first
        let key = Pubkey::new(&[10u8; 32]);
        let hash = Hash::new(&[2u8; 32]);
        let val = CalculateHashIntermediate::new(0, hash, 20, 20, Slot::default());
        account_maps.insert(key, val);

        let result = AccountsDB::rest_of_hash_calculation((account_maps, Measure::start("")));
        let expected_hash = Hash::from_str("EHv9C5vX7xQjjMpsJMzudnDTzoTSRwYkqLzY8tVMihGj").unwrap();
        assert_eq!(result, (expected_hash, 108));
    }

    #[test]
    fn test_accountsdb_handle_one_loaded_account() {
        solana_logger::setup();

        let account_maps: DashMap<Pubkey, CalculateHashIntermediate> = DashMap::new();
        let key = Pubkey::new_unique();
        let hash = Hash::new_unique();
        let val = CalculateHashIntermediate::new(1, hash, 1, 2, 1);

        AccountsDB::handle_one_loaded_account(&key, val.clone(), &account_maps);
        assert_eq!(*account_maps.get(&key).unwrap(), val);

        // slot same, version <
        let hash2 = Hash::new_unique();
        let val2 = CalculateHashIntermediate::new(0, hash2, 4, 5, 1);
        AccountsDB::handle_one_loaded_account(&key, val2, &account_maps);
        assert_eq!(*account_maps.get(&key).unwrap(), val);

        // slot same, vers =
        let hash3 = Hash::new_unique();
        let val3 = CalculateHashIntermediate::new(1, hash3, 2, 3, 1);
        AccountsDB::handle_one_loaded_account(&key, val3.clone(), &account_maps);
        assert_eq!(*account_maps.get(&key).unwrap(), val3);

        // slot same, vers >
        let hash4 = Hash::new_unique();
        let val4 = CalculateHashIntermediate::new(2, hash4, 6, 7, 1);
        AccountsDB::handle_one_loaded_account(&key, val4.clone(), &account_maps);
        assert_eq!(*account_maps.get(&key).unwrap(), val4);

        // slot >, version <
        let hash5 = Hash::new_unique();
        let val5 = CalculateHashIntermediate::new(0, hash5, 8, 9, 2);
        AccountsDB::handle_one_loaded_account(&key, val5.clone(), &account_maps);
        assert_eq!(*account_maps.get(&key).unwrap(), val5);
    }

    #[test]
    fn test_accountsdb_remove_zero_balance_accounts() {
        solana_logger::setup();

        let key = Pubkey::new_unique();
        let hash = Hash::new_unique();
        let account_maps: DashMap<Pubkey, CalculateHashIntermediate> = DashMap::new();
        let val = CalculateHashIntermediate::new(0, hash, 1, 2, Slot::default());
        account_maps.insert(key, val.clone());

        let result = AccountsDB::remove_zero_balance_accounts(account_maps);
        assert_eq!(result, vec![(key, val.hash, val.lamports)]);

        // zero original lamports
        let account_maps: DashMap<Pubkey, CalculateHashIntermediate> = DashMap::new();
        let val = CalculateHashIntermediate::new(0, hash, 1, 0, Slot::default());
        account_maps.insert(key, val);

        let result = AccountsDB::remove_zero_balance_accounts(account_maps);
        assert_eq!(result, vec![]);
    }

    #[test]
    fn test_accountsdb_calculate_accounts_hash_without_index_simple() {
        solana_logger::setup();

        let (storages, _size, _slot_expected) = sample_storage();
        let result = AccountsDB::calculate_accounts_hash_without_index(storages, true);
        let expected_hash = Hash::from_str("GKot5hBsd81kMupNCXHaqbhv3huEbxAFMLnpcX2hniwn").unwrap();
        assert_eq!(result, (expected_hash, 0));
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
        let acc = Account::new(1, 48, &Account::default().owner);
        let sm = StoredMeta {
            data_len: 1,
            pubkey,
            write_version: 1,
        };
        storages[0][0]
            .accounts
            .append_accounts(&[(sm, &acc)], &[Hash::default()]);

        let calls = AtomicU64::new(0);
        let result = AccountsDB::scan_account_storage_no_bank(
            storages,
            |loaded_account: LoadedAccount,
             _store_id: AppendVecId,
             accum: &mut Vec<u64>,
             slot: Slot| {
                calls.fetch_add(1, Ordering::Relaxed);
                assert_eq!(loaded_account.pubkey(), &pubkey);
                assert_eq!(slot_expected, slot);
                accum.push(expected);
            },
        );
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert_eq!(result, vec![vec![expected]]);
    }

    #[test]
    fn test_accountsdb_compute_merkle_root_and_capitalization() {
        solana_logger::setup();

        let expected_results = vec![
            (0, 0, "GKot5hBsd81kMupNCXHaqbhv3huEbxAFMLnpcX2hniwn", 0),
            (0, 1, "8unXKJYTxrR423HgQxbDmx29mFri1QNrzVKKDxEfc6bj", 0),
            (0, 2, "6QfkevXLLqbfAaR1kVjvMLFtEXvNUVrpmkwXqgsYtCFW", 1),
            (0, 3, "G3FrJd9JrXcMiqChTSfvEdBL2sCPny3ebiUy9Xxbn7a2", 3),
            (0, 4, "G3sZXHhwoCFuNyWy7Efffr47RBW33ibEp7b2hqNDmXdu", 6),
            (0, 5, "78atJJYpokAPKMJwHxUW8SBDvPkkSpTBV7GiB27HwosJ", 10),
            (0, 6, "7c9SM2BmCRVVXdrEdKcMK91MviPqXqQMd8QAb77tgLEy", 15),
            (0, 7, "3hsmnZPhf22UvBLiZ4dVa21Qsdh65CCrtYXsb8MxoVAa", 21),
            (0, 8, "5bwXUiC6RCRhb8fqvjvUXT6waU25str3UXA3a6Aq1jux", 28),
            (0, 9, "3NNtQKH6PaYpCnFBtyi2icK9eYX3YM5pqA3SKaXtUNzu", 36),
            (1, 0, "GKot5hBsd81kMupNCXHaqbhv3huEbxAFMLnpcX2hniwn", 0),
            (1, 1, "4GWVCsnEu1iRyxjAB3F7J7C4MMvcoxFWtP9ihvwvDgxY", 0),
            (1, 2, "8ML8Te6Uw2mipFr2v9sMZDcziXzhVqJo2qeMJohg1CJx", 1),
            (1, 3, "AMEuC3AgqAeRBGBhSfTmuMdfbAiXJnGmKv99kHmcAE1H", 3),
            (1, 4, "HEnDuJLHpsQfrApimGrovTqPEF6Vkrx2dKFr3BDtYzWx", 6),
            (1, 5, "6rH69iP2yM1o565noZN1EqjySW4PhYUskz3c5tXePUfV", 10),
            (1, 6, "7qEQMEXdfSPjbZ3q4cuuZwebDMvTvuaQ3dBiHoDUKo9a", 15),
            (1, 7, "GDJz7LSKYjqqz6ujCaaQRJRmQ7TLNCwYJhdT84qT4qwk", 21),
            (1, 8, "HT9krPLVTo3rr5WZQBQFrbqWs8SbYScXfnt8EVuobboM", 28),
            (1, 9, "8y2pMgqMdRsvqw6BQXm6wtz3qxGPss72i6H6gVpPyeda", 36),
        ];

        let mut expected_index = 0;
        let start = 0;
        let default_fanout = 2;
        // test 0..3 recursions (at fanout = 2) and 1 item remainder. The internals have 1 special case first loop and subsequent loops are the same types.
        let iterations = default_fanout * default_fanout * default_fanout + 2;
        for pass in 0..2 {
            let fanout = if pass == 0 {
                default_fanout
            } else {
                MERKLE_FANOUT
            };
            for count in start..iterations {
                let mut input: Vec<_> = (0..count)
                    .map(|i| {
                        let key = Pubkey::new(&[(pass * iterations + count) as u8; 32]);
                        let hash = Hash::new(&[(pass * iterations + count + i + 1) as u8; 32]);
                        (key, hash, i as u64)
                    })
                    .collect();
                let result;
                if pass == 0 {
                    result =
                        AccountsDB::compute_merkle_root_and_capitalization(input.clone(), fanout);
                } else {
                    result = AccountsDB::accumulate_account_hashes_and_capitalization(
                        input.clone(),
                        Slot::default(),
                        false,
                    )
                    .0;
                    assert_eq!(
                        AccountsDB::accumulate_account_hashes(
                            input.clone(),
                            Slot::default(),
                            false
                        ),
                        result.0
                    );
                    AccountsDB::sort_hashes_by_pubkey(&mut input);
                }
                let mut expected = 0;
                if count > 0 {
                    let count = count as u64;
                    let last_number = count - 1;
                    expected = count * last_number / 2;
                }

                // compare against calculated result for lamports
                assert_eq!(
                    result.1,
                    expected,
                    "failed at size: {}, with inputs: {:?}",
                    count,
                    input.into_iter().map(|x| x.2).collect::<Vec<u64>>()
                );

                // compare against captured, expected results for hash (and lamports)
                assert_eq!(
                    (pass, count, &*(result.0.to_string()), result.1),
                    expected_results[expected_index]
                );
                expected_index += 1;
            }
        }
    }

    #[test]
    #[should_panic(expected = "overflow is detected while summing capitalization")]
    fn test_accountsdb_compute_merkle_root_and_capitalization_overflow() {
        solana_logger::setup();

        let fanout = 2;
        let input = vec![
            (Pubkey::new_unique(), Hash::new_unique(), u64::MAX),
            (Pubkey::new_unique(), Hash::new_unique(), 1),
        ];
        AccountsDB::compute_merkle_root_and_capitalization(input, fanout);
    }

    #[test]
    fn test_accountsdb_add_root() {
        solana_logger::setup();
        let db = AccountsDB::new(Vec::new(), &ClusterType::Development);
        let key = Pubkey::default();
        let account0 = Account::new(1, 0, &key);

        db.store_uncached(0, &[(&key, &account0)]);
        db.add_root(0);
        let ancestors = vec![(1, 1)].into_iter().collect();
        assert_eq!(db.load_slow(&ancestors, &key), Some((account0, 0)));
    }

    #[test]
    fn test_accountsdb_latest_ancestor() {
        solana_logger::setup();
        let db = AccountsDB::new(Vec::new(), &ClusterType::Development);
        let key = Pubkey::default();
        let account0 = Account::new(1, 0, &key);

        db.store_uncached(0, &[(&key, &account0)]);

        let account1 = Account::new(0, 0, &key);
        db.store_uncached(1, &[(&key, &account1)]);

        let ancestors = vec![(1, 1)].into_iter().collect();
        assert_eq!(&db.load_slow(&ancestors, &key).unwrap().0, &account1);

        let ancestors = vec![(1, 1), (0, 0)].into_iter().collect();
        assert_eq!(&db.load_slow(&ancestors, &key).unwrap().0, &account1);

        let accounts: Vec<Account> =
            db.unchecked_scan_accounts("", &ancestors, |accounts: &mut Vec<Account>, option| {
                accounts.push(option.1.account());
            });
        assert_eq!(accounts, vec![account1]);
    }

    #[test]
    fn test_accountsdb_latest_ancestor_with_root() {
        solana_logger::setup();
        let db = AccountsDB::new(Vec::new(), &ClusterType::Development);
        let key = Pubkey::default();
        let account0 = Account::new(1, 0, &key);

        db.store_uncached(0, &[(&key, &account0)]);

        let account1 = Account::new(0, 0, &key);
        db.store_uncached(1, &[(&key, &account1)]);
        db.add_root(0);

        let ancestors = vec![(1, 1)].into_iter().collect();
        assert_eq!(&db.load_slow(&ancestors, &key).unwrap().0, &account1);

        let ancestors = vec![(1, 1), (0, 0)].into_iter().collect();
        assert_eq!(&db.load_slow(&ancestors, &key).unwrap().0, &account1);
    }

    #[test]
    fn test_accountsdb_root_one_slot() {
        solana_logger::setup();
        let db = AccountsDB::new(Vec::new(), &ClusterType::Development);

        let key = Pubkey::default();
        let account0 = Account::new(1, 0, &key);

        // store value 1 in the "root", i.e. db zero
        db.store_uncached(0, &[(&key, &account0)]);

        // now we have:
        //
        //                       root0 -> key.lamports==1
        //                        / \
        //                       /   \
        //  key.lamports==0 <- slot1    \
        //                             slot2 -> key.lamports==1
        //                                       (via root0)

        // store value 0 in one child
        let account1 = Account::new(0, 0, &key);
        db.store_uncached(1, &[(&key, &account1)]);

        // masking accounts is done at the Accounts level, at accountsDB we see
        // original account (but could also accept "None", which is implemented
        // at the Accounts level)
        let ancestors = vec![(0, 0), (1, 1)].into_iter().collect();
        assert_eq!(&db.load_slow(&ancestors, &key).unwrap().0, &account1);

        // we should see 1 token in slot 2
        let ancestors = vec![(0, 0), (2, 2)].into_iter().collect();
        assert_eq!(&db.load_slow(&ancestors, &key).unwrap().0, &account0);

        db.add_root(0);

        let ancestors = vec![(1, 1)].into_iter().collect();
        assert_eq!(db.load_slow(&ancestors, &key), Some((account1, 1)));
        let ancestors = vec![(2, 2)].into_iter().collect();
        assert_eq!(db.load_slow(&ancestors, &key), Some((account0, 0))); // original value
    }

    #[test]
    fn test_accountsdb_add_root_many() {
        let db = AccountsDB::new(Vec::new(), &ClusterType::Development);

        let mut pubkeys: Vec<Pubkey> = vec![];
        create_account(&db, &mut pubkeys, 0, 100, 0, 0);
        for _ in 1..100 {
            let idx = thread_rng().gen_range(0, 99);
            let ancestors = vec![(0, 0)].into_iter().collect();
            let account = db.load_slow(&ancestors, &pubkeys[idx]).unwrap();
            let default_account = Account {
                lamports: (idx + 1) as u64,
                ..Account::default()
            };
            assert_eq!((default_account, 0), account);
        }

        db.add_root(0);

        // check that all the accounts appear with a new root
        for _ in 1..100 {
            let idx = thread_rng().gen_range(0, 99);
            let ancestors = vec![(0, 0)].into_iter().collect();
            let account0 = db.load_slow(&ancestors, &pubkeys[idx]).unwrap();
            let ancestors = vec![(1, 1)].into_iter().collect();
            let account1 = db.load_slow(&ancestors, &pubkeys[idx]).unwrap();
            let default_account = Account {
                lamports: (idx + 1) as u64,
                ..Account::default()
            };
            assert_eq!(&default_account, &account0.0);
            assert_eq!(&default_account, &account1.0);
        }
    }

    #[test]
    fn test_accountsdb_count_stores() {
        solana_logger::setup();
        let db = AccountsDB::new_single();

        let mut pubkeys: Vec<Pubkey> = vec![];
        create_account(&db, &mut pubkeys, 0, 2, DEFAULT_FILE_SIZE as usize / 3, 0);
        assert!(check_storage(&db, 0, 2));

        let pubkey = solana_sdk::pubkey::new_rand();
        let account = Account::new(1, DEFAULT_FILE_SIZE as usize / 3, &pubkey);
        db.store_uncached(1, &[(&pubkey, &account)]);
        db.store_uncached(1, &[(&pubkeys[0], &account)]);
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

        // adding root doesn't change anything
        db.get_accounts_delta_hash(1);
        db.add_root(1);
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
        db.store_uncached(2, &[(&pubkeys[0], &account)]);
        db.clean_accounts(None);
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
        let db0 = AccountsDB::new(Vec::new(), &ClusterType::Development);
        let account0 = Account::new(1, 0, &key);
        db0.store_uncached(0, &[(&key, &account0)]);

        // 0 lamports in the child
        let account1 = Account::new(0, 0, &key);
        db0.store_uncached(1, &[(&key, &account1)]);

        // masking accounts is done at the Accounts level, at accountsDB we see
        // original account
        let ancestors = vec![(0, 0), (1, 1)].into_iter().collect();
        assert_eq!(db0.load_slow(&ancestors, &key), Some((account1, 1)));
        let ancestors = vec![(0, 0)].into_iter().collect();
        assert_eq!(db0.load_slow(&ancestors, &key), Some((account0, 0)));
    }

    #[test]
    fn test_remove_unrooted_slot() {
        let unrooted_slot = 9;
        let mut db = AccountsDB::new(Vec::new(), &ClusterType::Development);
        db.caching_enabled = true;
        let key = Pubkey::default();
        let account0 = Account::new(1, 0, &key);
        let ancestors: HashMap<_, _> = vec![(unrooted_slot, 1)].into_iter().collect();
        db.store_cached(unrooted_slot, &[(&key, &account0)]);
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
        db.remove_unrooted_slot(unrooted_slot);
        assert!(db.load_slow(&ancestors, &key).is_none());
        assert!(db.bank_hashes.read().unwrap().get(&unrooted_slot).is_none());
        assert!(db.storage.0.get(&unrooted_slot).is_none());
        assert!(db
            .accounts_index
            .get_account_read_entry(&key)
            .map(|locked_entry| locked_entry.slot_list().is_empty())
            .unwrap_or(true));
        assert!(db
            .accounts_index
            .get(&key, Some(&ancestors), None)
            .is_none());

        // Test we can store for the same slot again and get the right information
        let account0 = Account::new(2, 0, &key);
        db.store_uncached(unrooted_slot, &[(&key, &account0)]);
        assert_load_account(&db, unrooted_slot, key, 2);
    }

    #[test]
    fn test_remove_unrooted_slot_snapshot() {
        solana_logger::setup();
        let unrooted_slot = 9;
        let db = AccountsDB::new(Vec::new(), &ClusterType::Development);
        let key = solana_sdk::pubkey::new_rand();
        let account0 = Account::new(1, 0, &key);
        db.store_uncached(unrooted_slot, &[(&key, &account0)]);

        // Purge the slot
        db.remove_unrooted_slot(unrooted_slot);

        // Add a new root
        let key2 = solana_sdk::pubkey::new_rand();
        let new_root = unrooted_slot + 1;
        db.store_uncached(new_root, &[(&key2, &account0)]);
        db.add_root(new_root);

        // Simulate reconstruction from snapshot
        let db = reconstruct_accounts_db_via_serialization(&db, new_root);

        // Check root account exists
        assert_load_account(&db, new_root, key2, 1);

        // Check purged account stays gone
        let unrooted_slot_ancestors: HashMap<_, _> = vec![(unrooted_slot, 1)].into_iter().collect();
        assert!(db.load_slow(&unrooted_slot_ancestors, &key).is_none());
    }

    fn create_account(
        accounts: &AccountsDB,
        pubkeys: &mut Vec<Pubkey>,
        slot: Slot,
        num: usize,
        space: usize,
        num_vote: usize,
    ) {
        let ancestors = vec![(slot, 0)].into_iter().collect();
        for t in 0..num {
            let pubkey = solana_sdk::pubkey::new_rand();
            let account = Account::new((t + 1) as u64, space, &Account::default().owner);
            pubkeys.push(pubkey);
            assert!(accounts.load_slow(&ancestors, &pubkey).is_none());
            accounts.store_uncached(slot, &[(&pubkey, &account)]);
        }
        for t in 0..num_vote {
            let pubkey = solana_sdk::pubkey::new_rand();
            let account = Account::new((num + t + 1) as u64, space, &solana_vote_program::id());
            pubkeys.push(pubkey);
            let ancestors = vec![(slot, 0)].into_iter().collect();
            assert!(accounts.load_slow(&ancestors, &pubkey).is_none());
            accounts.store_uncached(slot, &[(&pubkey, &account)]);
        }
    }

    fn update_accounts(accounts: &AccountsDB, pubkeys: &[Pubkey], slot: Slot, range: usize) {
        for _ in 1..1000 {
            let idx = thread_rng().gen_range(0, range);
            let ancestors = vec![(slot, 0)].into_iter().collect();
            if let Some((mut account, _)) = accounts.load_slow(&ancestors, &pubkeys[idx]) {
                account.lamports += 1;
                accounts.store_uncached(slot, &[(&pubkeys[idx], &account)]);
                if account.lamports == 0 {
                    let ancestors = vec![(slot, 0)].into_iter().collect();
                    assert!(accounts.load_slow(&ancestors, &pubkeys[idx]).is_none());
                } else {
                    let default_account = Account {
                        lamports: account.lamports,
                        ..Account::default()
                    };
                    assert_eq!(default_account, account);
                }
            }
        }
    }

    fn check_storage(accounts: &AccountsDB, slot: Slot, count: usize) -> bool {
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
        accounts: &AccountsDB,
        pubkeys: &[Pubkey],
        slot: Slot,
        num: usize,
        count: usize,
    ) {
        let ancestors = vec![(slot, 0)].into_iter().collect();
        for _ in 0..num {
            let idx = thread_rng().gen_range(0, num);
            let account = accounts.load_slow(&ancestors, &pubkeys[idx]);
            let account1 = Some((
                Account::new((idx + count) as u64, 0, &Account::default().owner),
                slot,
            ));
            assert_eq!(account, account1);
        }
    }

    #[allow(clippy::needless_range_loop)]
    fn modify_accounts(
        accounts: &AccountsDB,
        pubkeys: &[Pubkey],
        slot: Slot,
        num: usize,
        count: usize,
    ) {
        for idx in 0..num {
            let account = Account::new((idx + count) as u64, 0, &Account::default().owner);
            accounts.store_uncached(slot, &[(&pubkeys[idx], &account)]);
        }
    }

    #[test]
    fn test_account_one() {
        let (_accounts_dirs, paths) = get_temp_accounts_paths(1).unwrap();
        let db = AccountsDB::new(paths, &ClusterType::Development);
        let mut pubkeys: Vec<Pubkey> = vec![];
        create_account(&db, &mut pubkeys, 0, 1, 0, 0);
        let ancestors = vec![(0, 0)].into_iter().collect();
        let account = db.load_slow(&ancestors, &pubkeys[0]).unwrap();
        let default_account = Account {
            lamports: 1,
            ..Account::default()
        };
        assert_eq!((default_account, 0), account);
    }

    #[test]
    fn test_account_many() {
        let (_accounts_dirs, paths) = get_temp_accounts_paths(2).unwrap();
        let db = AccountsDB::new(paths, &ClusterType::Development);
        let mut pubkeys: Vec<Pubkey> = vec![];
        create_account(&db, &mut pubkeys, 0, 100, 0, 0);
        check_accounts(&db, &pubkeys, 0, 100, 1);
    }

    #[test]
    fn test_account_update() {
        let accounts = AccountsDB::new_single();
        let mut pubkeys: Vec<Pubkey> = vec![];
        create_account(&accounts, &mut pubkeys, 0, 100, 0, 0);
        update_accounts(&accounts, &pubkeys, 0, 99);
        assert_eq!(check_storage(&accounts, 0, 100), true);
    }

    #[test]
    fn test_account_grow_many() {
        let (_accounts_dir, paths) = get_temp_accounts_paths(2).unwrap();
        let size = 4096;
        let accounts = AccountsDB::new_sized(paths, size);
        let mut keys = vec![];
        for i in 0..9 {
            let key = solana_sdk::pubkey::new_rand();
            let account = Account::new(i + 1, size as usize / 4, &key);
            accounts.store_uncached(0, &[(&key, &account)]);
            keys.push(key);
        }
        let ancestors = vec![(0, 0)].into_iter().collect();
        for (i, key) in keys.iter().enumerate() {
            assert_eq!(
                accounts.load_slow(&ancestors, &key).unwrap().0.lamports,
                (i as u64) + 1
            );
        }

        let mut append_vec_histogram = HashMap::new();
        let mut all_storages = vec![];
        for slot_storage in accounts.storage.0.iter() {
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
        let accounts = AccountsDB::new_single();

        let status = [AccountStorageStatus::Available, AccountStorageStatus::Full];
        let pubkey1 = solana_sdk::pubkey::new_rand();
        let account1 = Account::new(1, DEFAULT_FILE_SIZE as usize / 2, &pubkey1);
        accounts.store_uncached(0, &[(&pubkey1, &account1)]);
        {
            let stores = &accounts.storage.get_slot_stores(0).unwrap();
            let r_stores = stores.read().unwrap();
            assert_eq!(r_stores.len(), 1);
            assert_eq!(r_stores[&0].count(), 1);
            assert_eq!(r_stores[&0].status(), AccountStorageStatus::Available);
        }

        let pubkey2 = solana_sdk::pubkey::new_rand();
        let account2 = Account::new(1, DEFAULT_FILE_SIZE as usize / 2, &pubkey2);
        accounts.store_uncached(0, &[(&pubkey2, &account2)]);
        {
            assert_eq!(accounts.storage.0.len(), 1);
            let stores = &accounts.storage.get_slot_stores(0).unwrap();
            let r_stores = stores.read().unwrap();
            assert_eq!(r_stores.len(), 2);
            assert_eq!(r_stores[&0].count(), 1);
            assert_eq!(r_stores[&0].status(), AccountStorageStatus::Full);
            assert_eq!(r_stores[&1].count(), 1);
            assert_eq!(r_stores[&1].status(), AccountStorageStatus::Available);
        }
        let ancestors = vec![(0, 0)].into_iter().collect();
        assert_eq!(
            accounts.load_slow(&ancestors, &pubkey1).unwrap().0,
            account1
        );
        assert_eq!(
            accounts.load_slow(&ancestors, &pubkey2).unwrap().0,
            account2
        );

        // lots of stores, but 3 storages should be enough for everything
        for _ in 0..25 {
            accounts.store_uncached(0, &[(&pubkey1, &account1)]);
            {
                assert_eq!(accounts.storage.0.len(), 1);
                let stores = &accounts.storage.get_slot_stores(0).unwrap();
                let r_stores = stores.read().unwrap();
                assert!(r_stores.len() <= 5);
                assert_eq!(r_stores[&0].status(), status[0]);
            }
            let ancestors = vec![(0, 0)].into_iter().collect();
            assert_eq!(
                accounts.load_slow(&ancestors, &pubkey1).unwrap().0,
                account1
            );
            assert_eq!(
                accounts.load_slow(&ancestors, &pubkey2).unwrap().0,
                account2
            );
        }
    }

    #[test]
    fn test_lazy_gc_slot() {
        solana_logger::setup();
        //This test is pedantic
        //A slot is purged when a non root bank is cleaned up.  If a slot is behind root but it is
        //not root, it means we are retaining dead banks.
        let accounts = AccountsDB::new(Vec::new(), &ClusterType::Development);
        let pubkey = solana_sdk::pubkey::new_rand();
        let account = Account::new(1, 0, &Account::default().owner);
        //store an account
        accounts.store_uncached(0, &[(&pubkey, &account)]);
        let ancestors = vec![(0, 0)].into_iter().collect();
        let id = {
            let (lock, idx) = accounts
                .accounts_index
                .get(&pubkey, Some(&ancestors), None)
                .unwrap();
            lock.slot_list()[idx].1.store_id
        };
        accounts.get_accounts_delta_hash(0);
        accounts.add_root(1);

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
        accounts.store_uncached(1, &[(&pubkey, &account)]);

        // generate delta state for slot 1, so clean operates on it.
        accounts.get_accounts_delta_hash(1);

        //slot is gone
        accounts.print_accounts_stats("pre-clean");
        accounts.clean_accounts(None);
        assert!(accounts.storage.0.get(&0).is_none());

        //new value is there
        let ancestors = vec![(1, 1)].into_iter().collect();
        assert_eq!(accounts.load_slow(&ancestors, &pubkey), Some((account, 1)));
    }

    impl AccountsDB {
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

        fn ref_count_for_pubkey(&self, pubkey: &Pubkey) -> RefCount {
            self.accounts_index.ref_count_from_storage(&pubkey)
        }
    }

    #[test]
    fn test_clean_zero_lamport_and_dead_slot() {
        solana_logger::setup();

        let accounts = AccountsDB::new(Vec::new(), &ClusterType::Development);
        let pubkey1 = solana_sdk::pubkey::new_rand();
        let pubkey2 = solana_sdk::pubkey::new_rand();
        let account = Account::new(1, 1, &Account::default().owner);
        let zero_lamport_account = Account::new(0, 0, &Account::default().owner);

        // Store two accounts
        accounts.store_uncached(0, &[(&pubkey1, &account)]);
        accounts.store_uncached(0, &[(&pubkey2, &account)]);

        // Make sure both accounts are in the same AppendVec in slot 0, which
        // will prevent pubkey1 from being cleaned up later even when it's a
        // zero-lamport account
        let ancestors: HashMap<Slot, usize> = vec![(0, 1)].into_iter().collect();
        let (slot1, account_info1) = accounts
            .accounts_index
            .get(&pubkey1, Some(&ancestors), None)
            .map(|(account_list1, index1)| account_list1.slot_list()[index1].clone())
            .unwrap();
        let (slot2, account_info2) = accounts
            .accounts_index
            .get(&pubkey2, Some(&ancestors), None)
            .map(|(account_list2, index2)| account_list2.slot_list()[index2].clone())
            .unwrap();
        assert_eq!(slot1, 0);
        assert_eq!(slot1, slot2);
        assert_eq!(account_info1.store_id, account_info2.store_id);

        // Update account 1 in slot 1
        accounts.store_uncached(1, &[(&pubkey1, &account)]);

        // Update account 1 as  zero lamports account
        accounts.store_uncached(2, &[(&pubkey1, &zero_lamport_account)]);

        // Pubkey 1 was the only account in slot 1, and it was updated in slot 2, so
        // slot 1 should be purged
        accounts.add_root(0);
        accounts.add_root(1);
        accounts.add_root(2);

        // Slot 1 should be removed, slot 0 cannot be removed because it still has
        // the latest update for pubkey 2
        accounts.clean_accounts(None);
        assert!(accounts.storage.get_slot_stores(0).is_some());
        assert!(accounts.storage.get_slot_stores(1).is_none());

        // Slot 1 should be cleaned because all it's accounts are
        // zero lamports, and are not present in any other slot's
        // storage entries
        assert_eq!(accounts.alive_account_count_in_slot(1), 0);
    }

    #[test]
    fn test_clean_zero_lamport_and_old_roots() {
        solana_logger::setup();

        let accounts = AccountsDB::new(Vec::new(), &ClusterType::Development);
        let pubkey = solana_sdk::pubkey::new_rand();
        let account = Account::new(1, 0, &Account::default().owner);
        let zero_lamport_account = Account::new(0, 0, &Account::default().owner);

        // Store a zero-lamport account
        accounts.store_uncached(0, &[(&pubkey, &account)]);
        accounts.store_uncached(1, &[(&pubkey, &zero_lamport_account)]);

        // Simulate rooting the zero-lamport account, should be a
        // candidate for cleaning
        accounts.add_root(0);
        accounts.add_root(1);

        // Slot 0 should be removed, and
        // zero-lamport account should be cleaned
        accounts.clean_accounts(None);

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
        assert!(accounts.accounts_index.get(&pubkey, None, None).is_none());
    }

    #[test]
    fn test_clean_old_with_normal_account() {
        solana_logger::setup();

        let accounts = AccountsDB::new(Vec::new(), &ClusterType::Development);
        let pubkey = solana_sdk::pubkey::new_rand();
        let account = Account::new(1, 0, &Account::default().owner);
        //store an account
        accounts.store_uncached(0, &[(&pubkey, &account)]);
        accounts.store_uncached(1, &[(&pubkey, &account)]);

        // simulate slots are rooted after while
        accounts.get_accounts_delta_hash(0);
        accounts.add_root(0);
        accounts.get_accounts_delta_hash(1);
        accounts.add_root(1);

        //even if rooted, old state isn't cleaned up
        assert_eq!(accounts.alive_account_count_in_slot(0), 1);
        assert_eq!(accounts.alive_account_count_in_slot(1), 1);

        accounts.clean_accounts(None);

        //now old state is cleaned up
        assert_eq!(accounts.alive_account_count_in_slot(0), 0);
        assert_eq!(accounts.alive_account_count_in_slot(1), 1);
    }

    #[test]
    fn test_clean_old_with_zero_lamport_account() {
        solana_logger::setup();

        let accounts = AccountsDB::new(Vec::new(), &ClusterType::Development);
        let pubkey1 = solana_sdk::pubkey::new_rand();
        let pubkey2 = solana_sdk::pubkey::new_rand();
        let normal_account = Account::new(1, 0, &Account::default().owner);
        let zero_account = Account::new(0, 0, &Account::default().owner);
        //store an account
        accounts.store_uncached(0, &[(&pubkey1, &normal_account)]);
        accounts.store_uncached(1, &[(&pubkey1, &zero_account)]);
        accounts.store_uncached(0, &[(&pubkey2, &normal_account)]);
        accounts.store_uncached(1, &[(&pubkey2, &normal_account)]);

        //simulate slots are rooted after while
        accounts.get_accounts_delta_hash(0);
        accounts.add_root(0);
        accounts.get_accounts_delta_hash(1);
        accounts.add_root(1);

        //even if rooted, old state isn't cleaned up
        assert_eq!(accounts.alive_account_count_in_slot(0), 2);
        assert_eq!(accounts.alive_account_count_in_slot(1), 2);

        accounts.print_accounts_stats("");

        accounts.clean_accounts(None);

        //Old state behind zero-lamport account is cleaned up
        assert_eq!(accounts.alive_account_count_in_slot(0), 0);
        assert_eq!(accounts.alive_account_count_in_slot(1), 2);
    }

    #[test]
    fn test_clean_old_with_both_normal_and_zero_lamport_accounts() {
        solana_logger::setup();

        let accounts = AccountsDB::new_with_config(
            Vec::new(),
            &ClusterType::Development,
            spl_token_mint_index_enabled(),
            false,
        );
        let pubkey1 = solana_sdk::pubkey::new_rand();
        let pubkey2 = solana_sdk::pubkey::new_rand();

        // Set up account to be added to secondary index
        let mint_key = Pubkey::new_unique();
        let mut account_data_with_mint =
            vec![0; inline_spl_token_v2_0::state::Account::get_packed_len()];
        account_data_with_mint[..PUBKEY_BYTES].clone_from_slice(&(mint_key.clone().to_bytes()));

        let mut normal_account = Account::new(1, 0, &Account::default().owner);
        normal_account.owner = inline_spl_token_v2_0::id();
        normal_account.data = account_data_with_mint.clone();
        let mut zero_account = Account::new(0, 0, &Account::default().owner);
        zero_account.owner = inline_spl_token_v2_0::id();
        zero_account.data = account_data_with_mint;

        //store an account
        accounts.store_uncached(0, &[(&pubkey1, &normal_account)]);
        accounts.store_uncached(0, &[(&pubkey1, &normal_account)]);
        accounts.store_uncached(1, &[(&pubkey1, &zero_account)]);
        accounts.store_uncached(0, &[(&pubkey2, &normal_account)]);
        accounts.store_uncached(2, &[(&pubkey2, &normal_account)]);

        //simulate slots are rooted after while
        accounts.get_accounts_delta_hash(0);
        accounts.add_root(0);
        accounts.get_accounts_delta_hash(1);
        accounts.add_root(1);
        accounts.get_accounts_delta_hash(2);
        accounts.add_root(2);

        //even if rooted, old state isn't cleaned up
        assert_eq!(accounts.alive_account_count_in_slot(0), 2);
        assert_eq!(accounts.alive_account_count_in_slot(1), 1);
        assert_eq!(accounts.alive_account_count_in_slot(2), 1);

        // Secondary index should still find both pubkeys
        let mut found_accounts = HashSet::new();
        accounts.accounts_index.index_scan_accounts(
            &HashMap::new(),
            IndexKey::SplTokenMint(mint_key),
            |key, _| {
                found_accounts.insert(*key);
            },
        );
        assert_eq!(found_accounts.len(), 2);
        assert!(found_accounts.contains(&pubkey1));
        assert!(found_accounts.contains(&pubkey2));

        accounts.clean_accounts(None);

        //both zero lamport and normal accounts are cleaned up
        assert_eq!(accounts.alive_account_count_in_slot(0), 0);
        // The only store to slot 1 was a zero lamport account, should
        // be purged by zero-lamport cleaning logic because slot 1 is
        // rooted
        assert_eq!(accounts.alive_account_count_in_slot(1), 0);
        assert_eq!(accounts.alive_account_count_in_slot(2), 1);

        // `pubkey1`, a zero lamport account, should no longer exist in accounts index
        // because it has been removed by the clean
        assert!(accounts.accounts_index.get(&pubkey1, None, None).is_none());

        // Secondary index should have purged `pubkey1` as well
        let mut found_accounts = vec![];
        accounts.accounts_index.index_scan_accounts(
            &HashMap::new(),
            IndexKey::SplTokenMint(mint_key),
            |key, _| found_accounts.push(*key),
        );
        assert_eq!(found_accounts, vec![pubkey2]);
    }

    #[test]
    fn test_clean_max_slot_zero_lamport_account() {
        solana_logger::setup();

        let accounts = AccountsDB::new(Vec::new(), &ClusterType::Development);
        let pubkey = solana_sdk::pubkey::new_rand();
        let account = Account::new(1, 0, &Account::default().owner);
        let zero_account = Account::new(0, 0, &Account::default().owner);

        // store an account, make it a zero lamport account
        // in slot 1
        accounts.store_uncached(0, &[(&pubkey, &account)]);
        accounts.store_uncached(1, &[(&pubkey, &zero_account)]);

        // simulate slots are rooted after while
        accounts.add_root(0);
        accounts.add_root(1);

        // Only clean up to account 0, should not purge slot 0 based on
        // updates in later slots in slot 1
        assert_eq!(accounts.alive_account_count_in_slot(0), 1);
        assert_eq!(accounts.alive_account_count_in_slot(1), 1);
        accounts.clean_accounts(Some(0));
        assert_eq!(accounts.alive_account_count_in_slot(0), 1);
        assert_eq!(accounts.alive_account_count_in_slot(1), 1);
        assert!(accounts.accounts_index.get(&pubkey, None, None).is_some());

        // Now the account can be cleaned up
        accounts.clean_accounts(Some(1));
        assert_eq!(accounts.alive_account_count_in_slot(0), 0);
        assert_eq!(accounts.alive_account_count_in_slot(1), 0);

        // The zero lamport account, should no longer exist in accounts index
        // because it has been removed
        assert!(accounts.accounts_index.get(&pubkey, None, None).is_none());
    }

    #[test]
    fn test_uncleaned_roots_with_account() {
        solana_logger::setup();

        let accounts = AccountsDB::new(Vec::new(), &ClusterType::Development);
        let pubkey = solana_sdk::pubkey::new_rand();
        let account = Account::new(1, 0, &Account::default().owner);
        //store an account
        accounts.store_uncached(0, &[(&pubkey, &account)]);
        assert_eq!(accounts.accounts_index.uncleaned_roots_len(), 0);

        // simulate slots are rooted after while
        accounts.add_root(0);
        assert_eq!(accounts.accounts_index.uncleaned_roots_len(), 1);

        //now uncleaned roots are cleaned up
        accounts.clean_accounts(None);
        assert_eq!(accounts.accounts_index.uncleaned_roots_len(), 0);
    }

    #[test]
    fn test_uncleaned_roots_with_no_account() {
        solana_logger::setup();

        let accounts = AccountsDB::new(Vec::new(), &ClusterType::Development);

        assert_eq!(accounts.accounts_index.uncleaned_roots_len(), 0);

        // simulate slots are rooted after while
        accounts.add_root(0);
        assert_eq!(accounts.accounts_index.uncleaned_roots_len(), 1);

        //now uncleaned roots are cleaned up
        accounts.clean_accounts(None);
        assert_eq!(accounts.accounts_index.uncleaned_roots_len(), 0);
    }

    #[test]
    fn test_accounts_db_serialize1() {
        solana_logger::setup();
        let accounts = AccountsDB::new_single();
        let mut pubkeys: Vec<Pubkey> = vec![];

        // Create 100 accounts in slot 0
        create_account(&accounts, &mut pubkeys, 0, 100, 0, 0);
        accounts.clean_accounts(None);
        check_accounts(&accounts, &pubkeys, 0, 100, 1);

        // do some updates to those accounts and re-check
        modify_accounts(&accounts, &pubkeys, 0, 100, 2);
        assert_eq!(check_storage(&accounts, 0, 100), true);
        check_accounts(&accounts, &pubkeys, 0, 100, 2);
        accounts.get_accounts_delta_hash(0);
        accounts.add_root(0);

        let mut pubkeys1: Vec<Pubkey> = vec![];

        // CREATE SLOT 1
        let latest_slot = 1;

        // Modify the first 10 of the accounts from slot 0 in slot 1
        modify_accounts(&accounts, &pubkeys, latest_slot, 10, 3);
        // Overwrite account 30 from slot 0 with lamports=0 into slot 1.
        // Slot 1 should now have 10 + 1 = 11 accounts
        let account = Account::new(0, 0, &Account::default().owner);
        accounts.store_uncached(latest_slot, &[(&pubkeys[30], &account)]);

        // Create 10 new accounts in slot 1, should now have 11 + 10 = 21
        // accounts
        create_account(&accounts, &mut pubkeys1, latest_slot, 10, 0, 0);

        accounts.get_accounts_delta_hash(latest_slot);
        accounts.add_root(latest_slot);
        assert!(check_storage(&accounts, 1, 21));

        // CREATE SLOT 2
        let latest_slot = 2;
        let mut pubkeys2: Vec<Pubkey> = vec![];

        // Modify first 20 of the accounts from slot 0 in slot 2
        modify_accounts(&accounts, &pubkeys, latest_slot, 20, 4);
        accounts.clean_accounts(None);
        // Overwrite account 31 from slot 0 with lamports=0 into slot 2.
        // Slot 2 should now have 20 + 1 = 21 accounts
        let account = Account::new(0, 0, &Account::default().owner);
        accounts.store_uncached(latest_slot, &[(&pubkeys[31], &account)]);

        // Create 10 new accounts in slot 2. Slot 2 should now have
        // 21 + 10 = 31 accounts
        create_account(&accounts, &mut pubkeys2, latest_slot, 10, 0, 0);

        accounts.get_accounts_delta_hash(latest_slot);
        accounts.add_root(latest_slot);
        assert!(check_storage(&accounts, 2, 31));

        accounts.clean_accounts(None);
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
            daccounts.write_version.load(Ordering::Relaxed),
            accounts.write_version.load(Ordering::Relaxed)
        );

        assert_eq!(
            daccounts.next_id.load(Ordering::Relaxed),
            accounts.next_id.load(Ordering::Relaxed)
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
            daccounts.update_accounts_hash(latest_slot, &ancestors, true),
            accounts.update_accounts_hash(latest_slot, &ancestors, true)
        );
    }

    fn assert_load_account(
        accounts: &AccountsDB,
        slot: Slot,
        pubkey: Pubkey,
        expected_lamports: u64,
    ) {
        let ancestors = vec![(slot, 0)].into_iter().collect();
        let (account, slot) = accounts.load_slow(&ancestors, &pubkey).unwrap();
        assert_eq!((account.lamports, slot), (expected_lamports, slot));
    }

    fn assert_not_load_account(accounts: &AccountsDB, slot: Slot, pubkey: Pubkey) {
        let ancestors = vec![(slot, 0)].into_iter().collect();
        assert!(accounts.load_slow(&ancestors, &pubkey).is_none());
    }

    fn reconstruct_accounts_db_via_serialization(accounts: &AccountsDB, slot: Slot) -> AccountsDB {
        let daccounts =
            crate::serde_snapshot::reconstruct_accounts_db_via_serialization(accounts, slot);
        daccounts.print_count_and_status("daccounts");
        daccounts
    }

    fn assert_no_stores(accounts: &AccountsDB, slot: Slot) {
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
        let owner = Account::default().owner;

        let account = Account::new(some_lamport, no_data, &owner);
        let pubkey = solana_sdk::pubkey::new_rand();

        let account2 = Account::new(some_lamport, no_data, &owner);
        let pubkey2 = solana_sdk::pubkey::new_rand();

        let zero_lamport_account = Account::new(zero_lamport, no_data, &owner);

        let accounts = AccountsDB::new_single();
        accounts.add_root(0);

        // Step A
        let mut current_slot = 1;
        accounts.store_uncached(current_slot, &[(&pubkey, &account)]);
        // Store another live account to slot 1 which will prevent any purge
        // since the store count will not be zero
        accounts.store_uncached(current_slot, &[(&pubkey2, &account2)]);
        accounts.add_root(current_slot);
        let (slot1, account_info1) = accounts
            .accounts_index
            .get(&pubkey, None, None)
            .map(|(account_list1, index1)| account_list1.slot_list()[index1].clone())
            .unwrap();
        let (slot2, account_info2) = accounts
            .accounts_index
            .get(&pubkey2, None, None)
            .map(|(account_list2, index2)| account_list2.slot_list()[index2].clone())
            .unwrap();
        assert_eq!(slot1, current_slot);
        assert_eq!(slot1, slot2);
        assert_eq!(account_info1.store_id, account_info2.store_id);

        // Step B
        current_slot += 1;
        let zero_lamport_slot = current_slot;
        accounts.store_uncached(current_slot, &[(&pubkey, &zero_lamport_account)]);
        accounts.add_root(current_slot);

        assert_load_account(&accounts, current_slot, pubkey, zero_lamport);

        current_slot += 1;
        accounts.add_root(current_slot);

        accounts.print_accounts_stats("pre_purge");

        accounts.clean_accounts(None);

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
        let owner = Account::default().owner;

        let account = Account::new(some_lamport, no_data, &owner);
        let pubkey = solana_sdk::pubkey::new_rand();

        let zero_lamport_account = Account::new(zero_lamport, no_data, &owner);

        let accounts = AccountsDB::new_single();
        accounts.add_root(0);

        let mut current_slot = 1;
        accounts.set_hash(current_slot, current_slot - 1);
        accounts.store_uncached(current_slot, &[(&pubkey, &account)]);
        accounts.add_root(current_slot);

        current_slot += 1;
        accounts.set_hash(current_slot, current_slot - 1);
        accounts.store_uncached(current_slot, &[(&pubkey, &zero_lamport_account)]);
        accounts.add_root(current_slot);

        assert_load_account(&accounts, current_slot, pubkey, zero_lamport);

        // Otherwise slot 2 will not be removed
        current_slot += 1;
        accounts.set_hash(current_slot, current_slot - 1);
        accounts.add_root(current_slot);

        accounts.print_accounts_stats("pre_purge");

        let ancestors = linear_ancestors(current_slot);
        info!("ancestors: {:?}", ancestors);
        let hash = accounts.update_accounts_hash_test(current_slot, &ancestors, true);

        accounts.clean_accounts(None);

        assert_eq!(
            accounts.update_accounts_hash_test(current_slot, &ancestors, true),
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
        let owner = Account::default().owner;

        let account = Account::new(some_lamport, no_data, &owner);
        let pubkey = solana_sdk::pubkey::new_rand();
        let zero_lamport_account = Account::new(zero_lamport, no_data, &owner);

        let account2 = Account::new(some_lamport + 1, no_data, &owner);
        let pubkey2 = solana_sdk::pubkey::new_rand();

        let filler_account = Account::new(some_lamport, no_data, &owner);
        let filler_account_pubkey = solana_sdk::pubkey::new_rand();

        let accounts = AccountsDB::new_single();

        let mut current_slot = 1;
        accounts.store_uncached(current_slot, &[(&pubkey, &account)]);
        accounts.add_root(current_slot);

        current_slot += 1;
        accounts.store_uncached(current_slot, &[(&pubkey, &zero_lamport_account)]);
        accounts.store_uncached(current_slot, &[(&pubkey2, &account2)]);

        // Store enough accounts such that an additional store for slot 2 is created.
        while accounts
            .storage
            .get_slot_stores(current_slot)
            .unwrap()
            .read()
            .unwrap()
            .len()
            < 2
        {
            accounts.store_uncached(current_slot, &[(&filler_account_pubkey, &filler_account)]);
        }
        accounts.add_root(current_slot);

        assert_load_account(&accounts, current_slot, pubkey, zero_lamport);

        accounts.print_accounts_stats("accounts");

        accounts.clean_accounts(None);

        accounts.print_accounts_stats("accounts_post_purge");
        let accounts = reconstruct_accounts_db_via_serialization(&accounts, current_slot);

        accounts.print_accounts_stats("reconstructed");

        assert_load_account(&accounts, current_slot, pubkey, zero_lamport);
    }

    fn with_chained_zero_lamport_accounts<F>(f: F)
    where
        F: Fn(AccountsDB, Slot) -> AccountsDB,
    {
        let some_lamport = 223;
        let zero_lamport = 0;
        let dummy_lamport = 999;
        let no_data = 0;
        let owner = Account::default().owner;

        let account = Account::new(some_lamport, no_data, &owner);
        let account2 = Account::new(some_lamport + 100_001, no_data, &owner);
        let account3 = Account::new(some_lamport + 100_002, no_data, &owner);
        let zero_lamport_account = Account::new(zero_lamport, no_data, &owner);

        let pubkey = solana_sdk::pubkey::new_rand();
        let purged_pubkey1 = solana_sdk::pubkey::new_rand();
        let purged_pubkey2 = solana_sdk::pubkey::new_rand();

        let dummy_account = Account::new(dummy_lamport, no_data, &owner);
        let dummy_pubkey = Pubkey::default();

        let accounts = AccountsDB::new_single();

        let mut current_slot = 1;
        accounts.store_uncached(current_slot, &[(&pubkey, &account)]);
        accounts.store_uncached(current_slot, &[(&purged_pubkey1, &account2)]);
        accounts.add_root(current_slot);

        current_slot += 1;
        accounts.store_uncached(current_slot, &[(&purged_pubkey1, &zero_lamport_account)]);
        accounts.store_uncached(current_slot, &[(&purged_pubkey2, &account3)]);
        accounts.add_root(current_slot);

        current_slot += 1;
        accounts.store_uncached(current_slot, &[(&purged_pubkey2, &zero_lamport_account)]);
        accounts.add_root(current_slot);

        current_slot += 1;
        accounts.store_uncached(current_slot, &[(&dummy_pubkey, &dummy_account)]);
        accounts.add_root(current_slot);

        accounts.print_accounts_stats("pre_f");
        accounts.update_accounts_hash(4, &HashMap::default(), true);

        let accounts = f(accounts, current_slot);

        accounts.print_accounts_stats("post_f");

        assert_load_account(&accounts, current_slot, pubkey, some_lamport);
        assert_load_account(&accounts, current_slot, purged_pubkey1, 0);
        assert_load_account(&accounts, current_slot, purged_pubkey2, 0);
        assert_load_account(&accounts, current_slot, dummy_pubkey, dummy_lamport);

        accounts
            .verify_bank_hash_and_lamports(4, &HashMap::default(), 1222, true)
            .unwrap();
    }

    #[test]
    fn test_accounts_purge_chained_purge_before_snapshot_restore() {
        solana_logger::setup();
        with_chained_zero_lamport_accounts(|accounts, current_slot| {
            accounts.clean_accounts(None);
            reconstruct_accounts_db_via_serialization(&accounts, current_slot)
        });
    }

    #[test]
    fn test_accounts_purge_chained_purge_after_snapshot_restore() {
        solana_logger::setup();
        with_chained_zero_lamport_accounts(|accounts, current_slot| {
            let accounts = reconstruct_accounts_db_via_serialization(&accounts, current_slot);
            accounts.print_accounts_stats("after_reconstruct");
            accounts.clean_accounts(None);
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

        let db = Arc::new(AccountsDB::new_sized(Vec::new(), min_file_bytes as u64));

        db.add_root(slot);
        let thread_hdls: Vec<_> = (0..num_threads)
            .map(|_| {
                let db = db.clone();
                std::thread::Builder::new()
                    .name("account-writers".to_string())
                    .spawn(move || {
                        let pubkey = solana_sdk::pubkey::new_rand();
                        let mut account = Account::new(1, 0, &pubkey);
                        let mut i = 0;
                        loop {
                            let account_bal = thread_rng().gen_range(1, 99);
                            account.lamports = account_bal;
                            db.store_uncached(slot, &[(&pubkey, &account)]);

                            let (account, slot) =
                                db.load_slow(&HashMap::new(), &pubkey).unwrap_or_else(|| {
                                    panic!("Could not fetch stored account {}, iter {}", pubkey, i)
                                });
                            assert_eq!(slot, slot);
                            assert_eq!(account.lamports, account_bal);
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
        let db = AccountsDB::new(Vec::new(), &ClusterType::Development);
        let key = Pubkey::default();
        let key0 = solana_sdk::pubkey::new_rand();
        let account0 = Account::new(1, 0, &key);

        db.store_uncached(0, &[(&key0, &account0)]);

        let key1 = solana_sdk::pubkey::new_rand();
        let account1 = Account::new(2, 0, &key);
        db.store_uncached(1, &[(&key1, &account1)]);

        let ancestors = vec![(0, 0)].into_iter().collect();
        let accounts: Vec<Account> =
            db.unchecked_scan_accounts("", &ancestors, |accounts: &mut Vec<Account>, option| {
                accounts.push(option.1.account());
            });
        assert_eq!(accounts, vec![account0]);

        let ancestors = vec![(1, 1), (0, 0)].into_iter().collect();
        let accounts: Vec<Account> =
            db.unchecked_scan_accounts("", &ancestors, |accounts: &mut Vec<Account>, option| {
                accounts.push(option.1.account());
            });
        assert_eq!(accounts.len(), 2);
    }

    #[test]
    fn test_cleanup_key_not_removed() {
        solana_logger::setup();
        let db = AccountsDB::new_single();

        let key = Pubkey::default();
        let key0 = solana_sdk::pubkey::new_rand();
        let account0 = Account::new(1, 0, &key);

        db.store_uncached(0, &[(&key0, &account0)]);

        let key1 = solana_sdk::pubkey::new_rand();
        let account1 = Account::new(2, 0, &key);
        db.store_uncached(1, &[(&key1, &account1)]);

        db.print_accounts_stats("pre");

        let slots: HashSet<Slot> = vec![1].into_iter().collect();
        let purge_keys = vec![(key1, slots)];
        db.purge_keys_exact(&purge_keys);

        let account2 = Account::new(3, 0, &key);
        db.store_uncached(2, &[(&key1, &account2)]);

        db.print_accounts_stats("post");
        let ancestors = vec![(2, 0)].into_iter().collect();
        assert_eq!(db.load_slow(&ancestors, &key1).unwrap().0.lamports, 3);
    }

    #[test]
    fn test_store_large_account() {
        solana_logger::setup();
        let db = AccountsDB::new(Vec::new(), &ClusterType::Development);

        let key = Pubkey::default();
        let data_len = DEFAULT_FILE_SIZE as usize + 7;
        let account = Account::new(1, data_len, &key);

        db.store_uncached(0, &[(&key, &account)]);

        let ancestors = vec![(0, 0)].into_iter().collect();
        let ret = db.load_slow(&ancestors, &key).unwrap();
        assert_eq!(ret.0.data.len(), data_len);
    }

    #[test]
    fn test_hash_frozen_account_data() {
        let account = Account::new(1, 42, &Pubkey::default());

        let hash = AccountsDB::hash_frozen_account_data(&account);
        assert_ne!(hash, Hash::default()); // Better not be the default Hash

        // Lamports changes to not affect the hash
        let mut account_modified = account.clone();
        account_modified.lamports -= 1;
        assert_eq!(
            hash,
            AccountsDB::hash_frozen_account_data(&account_modified)
        );

        // Rent epoch may changes to not affect the hash
        let mut account_modified = account.clone();
        account_modified.rent_epoch += 1;
        assert_eq!(
            hash,
            AccountsDB::hash_frozen_account_data(&account_modified)
        );

        // Account data may not be modified
        let mut account_modified = account.clone();
        account_modified.data[0] = 42;
        assert_ne!(
            hash,
            AccountsDB::hash_frozen_account_data(&account_modified)
        );

        // Owner may not be modified
        let mut account_modified = account.clone();
        account_modified.owner =
            Pubkey::from_str("My11111111111111111111111111111111111111111").unwrap();
        assert_ne!(
            hash,
            AccountsDB::hash_frozen_account_data(&account_modified)
        );

        // Executable may not be modified
        let mut account_modified = account;
        account_modified.executable = true;
        assert_ne!(
            hash,
            AccountsDB::hash_frozen_account_data(&account_modified)
        );
    }

    #[test]
    fn test_frozen_account_lamport_increase() {
        let frozen_pubkey =
            Pubkey::from_str("My11111111111111111111111111111111111111111").unwrap();
        let mut db = AccountsDB::new(Vec::new(), &ClusterType::Development);

        let mut account = Account::new(1, 42, &frozen_pubkey);
        db.store_uncached(0, &[(&frozen_pubkey, &account)]);

        let ancestors = vec![(0, 0)].into_iter().collect();
        db.freeze_accounts(&ancestors, &[frozen_pubkey]);

        // Store with no account changes is ok
        db.store_uncached(0, &[(&frozen_pubkey, &account)]);

        // Store with an increase in lamports is ok
        account.lamports = 2;
        db.store_uncached(0, &[(&frozen_pubkey, &account)]);

        // Store with an decrease that does not go below the frozen amount of lamports is tolerated
        account.lamports = 1;
        db.store_uncached(0, &[(&frozen_pubkey, &account)]);

        // A store of any value over the frozen value of '1' across different slots is also ok
        account.lamports = 3;
        db.store_uncached(1, &[(&frozen_pubkey, &account)]);
        account.lamports = 2;
        db.store_uncached(2, &[(&frozen_pubkey, &account)]);
        account.lamports = 1;
        db.store_uncached(3, &[(&frozen_pubkey, &account)]);
    }

    #[test]
    #[should_panic(
        expected = "Frozen account My11111111111111111111111111111111111111111 modified.  Lamports decreased from 1 to 0"
    )]
    fn test_frozen_account_lamport_decrease() {
        let frozen_pubkey =
            Pubkey::from_str("My11111111111111111111111111111111111111111").unwrap();
        let mut db = AccountsDB::new(Vec::new(), &ClusterType::Development);

        let mut account = Account::new(1, 42, &frozen_pubkey);
        db.store_uncached(0, &[(&frozen_pubkey, &account)]);

        let ancestors = vec![(0, 0)].into_iter().collect();
        db.freeze_accounts(&ancestors, &[frozen_pubkey]);

        // Store with a decrease below the frozen amount of lamports is not ok
        account.lamports -= 1;
        db.store_uncached(0, &[(&frozen_pubkey, &account)]);
    }

    #[test]
    #[should_panic(
        expected = "Unable to freeze an account that does not exist: My11111111111111111111111111111111111111111"
    )]
    fn test_frozen_account_nonexistent() {
        let frozen_pubkey =
            Pubkey::from_str("My11111111111111111111111111111111111111111").unwrap();
        let mut db = AccountsDB::new(Vec::new(), &ClusterType::Development);

        let ancestors = vec![(0, 0)].into_iter().collect();
        db.freeze_accounts(&ancestors, &[frozen_pubkey]);
    }

    #[test]
    #[should_panic(
        expected = "Frozen account My11111111111111111111111111111111111111111 modified.  Hash changed from 8wHcxDkjiwdrkPAsDnmNrF1UDGJFAtZzPQBSVweY3yRA to JdscGYB1uczVssmYuJusDD1Bfe6wpNeeho8XjcH8inN"
    )]
    fn test_frozen_account_data_modified() {
        let frozen_pubkey =
            Pubkey::from_str("My11111111111111111111111111111111111111111").unwrap();
        let mut db = AccountsDB::new(Vec::new(), &ClusterType::Development);

        let mut account = Account::new(1, 42, &frozen_pubkey);
        db.store_uncached(0, &[(&frozen_pubkey, &account)]);

        let ancestors = vec![(0, 0)].into_iter().collect();
        db.freeze_accounts(&ancestors, &[frozen_pubkey]);

        account.data[0] = 42;
        db.store_uncached(0, &[(&frozen_pubkey, &account)]);
    }

    #[test]
    fn test_hash_stored_account() {
        // This test uses some UNSAFE trick to detect most of account's field
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
            stored_size: CACHE_VIRTUAL_STORED_SIZE,
            hash: &hash,
        };
        let account = stored_account.clone_account();
        let expected_account_hash =
            Hash::from_str("4StuvYHFd7xuShVXB94uHHvpqGMCaacdZnYB74QQkPA1").unwrap();

        assert_eq!(
            AccountsDB::hash_stored_account(slot, &stored_account, &ClusterType::Development),
            expected_account_hash,
            "StoredAccountMeta's data layout might be changed; update hashing if needed."
        );
        assert_eq!(
            AccountsDB::hash_account(
                slot,
                &account,
                &stored_account.meta.pubkey,
                &ClusterType::Development
            ),
            expected_account_hash,
            "Account-based hashing must be consistent with StoredAccountMeta-based one."
        );
    }

    #[test]
    fn test_bank_hash_stats() {
        solana_logger::setup();
        let db = AccountsDB::new(Vec::new(), &ClusterType::Development);

        let key = Pubkey::default();
        let some_data_len = 5;
        let some_slot: Slot = 0;
        let account = Account::new(1, some_data_len, &key);
        let ancestors = vec![(some_slot, 0)].into_iter().collect();

        db.store_uncached(some_slot, &[(&key, &account)]);
        let mut account = db.load_slow(&ancestors, &key).unwrap().0;
        account.lamports -= 1;
        account.executable = true;
        db.store_uncached(some_slot, &[(&key, &account)]);
        db.add_root(some_slot);

        let bank_hashes = db.bank_hashes.read().unwrap();
        let bank_hash = bank_hashes.get(&some_slot).unwrap();
        assert_eq!(bank_hash.stats.num_updated_accounts, 1);
        assert_eq!(bank_hash.stats.num_removed_accounts, 1);
        assert_eq!(bank_hash.stats.num_lamports_stored, 1);
        assert_eq!(bank_hash.stats.total_data_len, 2 * some_data_len as u64);
        assert_eq!(bank_hash.stats.num_executable_accounts, 1);
    }

    #[test]
    fn test_verify_bank_hash() {
        use BankHashVerificationError::*;
        solana_logger::setup();
        let db = AccountsDB::new(Vec::new(), &ClusterType::Development);

        let key = solana_sdk::pubkey::new_rand();
        let some_data_len = 0;
        let some_slot: Slot = 0;
        let account = Account::new(1, some_data_len, &key);
        let ancestors = vec![(some_slot, 0)].into_iter().collect();

        db.store_uncached(some_slot, &[(&key, &account)]);
        db.add_root(some_slot);
        db.update_accounts_hash_test(some_slot, &ancestors, true);
        assert_matches!(
            db.verify_bank_hash_and_lamports(some_slot, &ancestors, 1, true),
            Ok(_)
        );

        db.bank_hashes.write().unwrap().remove(&some_slot).unwrap();
        assert_matches!(
            db.verify_bank_hash_and_lamports(some_slot, &ancestors, 1, true),
            Err(MissingBankHash)
        );

        let some_bank_hash = Hash::new(&[0xca; HASH_BYTES]);
        let bank_hash_info = BankHashInfo {
            hash: some_bank_hash,
            snapshot_hash: Hash::new(&[0xca; HASH_BYTES]),
            stats: BankHashStats::default(),
        };
        db.bank_hashes
            .write()
            .unwrap()
            .insert(some_slot, bank_hash_info);
        assert_matches!(
            db.verify_bank_hash_and_lamports(some_slot, &ancestors, 1, true),
            Err(MismatchedBankHash)
        );
    }

    #[test]
    fn test_verify_bank_capitalization() {
        use BankHashVerificationError::*;
        solana_logger::setup();
        let db = AccountsDB::new(Vec::new(), &ClusterType::Development);

        let key = solana_sdk::pubkey::new_rand();
        let some_data_len = 0;
        let some_slot: Slot = 0;
        let account = Account::new(1, some_data_len, &key);
        let ancestors = vec![(some_slot, 0)].into_iter().collect();

        db.store_uncached(some_slot, &[(&key, &account)]);
        db.add_root(some_slot);
        db.update_accounts_hash_test(some_slot, &ancestors, true);
        assert_matches!(
            db.verify_bank_hash_and_lamports(some_slot, &ancestors, 1, true),
            Ok(_)
        );

        let native_account_pubkey = solana_sdk::pubkey::new_rand();
        db.store_uncached(
            some_slot,
            &[(
                &native_account_pubkey,
                &solana_sdk::native_loader::create_loadable_account("foo", 1),
            )],
        );
        db.update_accounts_hash_test(some_slot, &ancestors, true);
        assert_matches!(
            db.verify_bank_hash_and_lamports(some_slot, &ancestors, 1, false),
            Ok(_)
        );
        assert_matches!(
            db.verify_bank_hash_and_lamports(some_slot, &ancestors, 2, true),
            Ok(_)
        );

        assert_matches!(
            db.verify_bank_hash_and_lamports(some_slot, &ancestors, 10, true),
            Err(MismatchedTotalLamports(expected, actual)) if expected == 2 && actual == 10
        );
    }

    #[test]
    fn test_verify_bank_hash_no_account() {
        solana_logger::setup();
        let db = AccountsDB::new(Vec::new(), &ClusterType::Development);

        let some_slot: Slot = 0;
        let ancestors = vec![(some_slot, 0)].into_iter().collect();

        db.bank_hashes
            .write()
            .unwrap()
            .insert(some_slot, BankHashInfo::default());
        db.add_root(some_slot);
        db.update_accounts_hash_test(some_slot, &ancestors, true);
        assert_matches!(
            db.verify_bank_hash_and_lamports(some_slot, &ancestors, 0, true),
            Ok(_)
        );
    }

    #[test]
    fn test_verify_bank_hash_bad_account_hash() {
        use BankHashVerificationError::*;
        solana_logger::setup();
        let db = AccountsDB::new(Vec::new(), &ClusterType::Development);

        let key = Pubkey::default();
        let some_data_len = 0;
        let some_slot: Slot = 0;
        let account = Account::new(1, some_data_len, &key);
        let ancestors = vec![(some_slot, 0)].into_iter().collect();

        let accounts = &[(&key, &account)];
        // update AccountsDB's bank hash but discard real account hashes
        db.hash_accounts(some_slot, accounts, &ClusterType::Development);
        // provide bogus account hashes
        let some_hash = Hash::new(&[0xca; HASH_BYTES]);
        db.store_accounts_unfrozen(some_slot, accounts, &[some_hash], false);
        db.add_root(some_slot);
        assert_matches!(
            db.verify_bank_hash_and_lamports(some_slot, &ancestors, 1, true),
            Err(MismatchedAccountHash)
        );
    }

    #[test]
    fn test_storage_finder() {
        solana_logger::setup();
        let db = AccountsDB::new_sized(Vec::new(), 16 * 1024);
        let key = solana_sdk::pubkey::new_rand();
        let lamports = 100;
        let data_len = 8190;
        let account = Account::new(lamports, data_len, &solana_sdk::pubkey::new_rand());
        // pre-populate with a smaller empty store
        db.create_and_insert_store(1, 8192, "test_storage_finder");
        db.store_uncached(1, &[(&key, &account)]);
    }

    #[test]
    fn test_get_snapshot_storages_empty() {
        let db = AccountsDB::new(Vec::new(), &ClusterType::Development);
        assert!(db.get_snapshot_storages(0).is_empty());
    }

    #[test]
    fn test_get_snapshot_storages_only_older_than_or_equal_to_snapshot_slot() {
        let db = AccountsDB::new(Vec::new(), &ClusterType::Development);

        let key = Pubkey::default();
        let account = Account::new(1, 0, &key);
        let before_slot = 0;
        let base_slot = before_slot + 1;
        let after_slot = base_slot + 1;

        db.add_root(base_slot);
        db.store_uncached(base_slot, &[(&key, &account)]);
        assert!(db.get_snapshot_storages(before_slot).is_empty());

        assert_eq!(1, db.get_snapshot_storages(base_slot).len());
        assert_eq!(1, db.get_snapshot_storages(after_slot).len());
    }

    #[test]
    fn test_get_snapshot_storages_only_non_empty() {
        let db = AccountsDB::new(Vec::new(), &ClusterType::Development);

        let key = Pubkey::default();
        let account = Account::new(1, 0, &key);
        let base_slot = 0;
        let after_slot = base_slot + 1;

        db.store_uncached(base_slot, &[(&key, &account)]);
        db.storage
            .get_slot_stores(base_slot)
            .unwrap()
            .write()
            .unwrap()
            .clear();
        db.add_root(base_slot);
        assert!(db.get_snapshot_storages(after_slot).is_empty());

        db.store_uncached(base_slot, &[(&key, &account)]);
        assert_eq!(1, db.get_snapshot_storages(after_slot).len());
    }

    #[test]
    fn test_get_snapshot_storages_only_roots() {
        let db = AccountsDB::new(Vec::new(), &ClusterType::Development);

        let key = Pubkey::default();
        let account = Account::new(1, 0, &key);
        let base_slot = 0;
        let after_slot = base_slot + 1;

        db.store_uncached(base_slot, &[(&key, &account)]);
        assert!(db.get_snapshot_storages(after_slot).is_empty());

        db.add_root(base_slot);
        assert_eq!(1, db.get_snapshot_storages(after_slot).len());
    }

    #[test]
    fn test_get_snapshot_storages_exclude_empty() {
        let db = AccountsDB::new(Vec::new(), &ClusterType::Development);

        let key = Pubkey::default();
        let account = Account::new(1, 0, &key);
        let base_slot = 0;
        let after_slot = base_slot + 1;

        db.store_uncached(base_slot, &[(&key, &account)]);
        db.add_root(base_slot);
        assert_eq!(1, db.get_snapshot_storages(after_slot).len());

        db.storage
            .get_slot_stores(0)
            .unwrap()
            .read()
            .unwrap()
            .values()
            .next()
            .unwrap()
            .remove_account(0, true);
        assert!(db.get_snapshot_storages(after_slot).is_empty());
    }

    #[test]
    #[should_panic(expected = "double remove of account in slot: 0/store: 0!!")]
    fn test_storage_remove_account_double_remove() {
        let accounts = AccountsDB::new(Vec::new(), &ClusterType::Development);
        let pubkey = solana_sdk::pubkey::new_rand();
        let account = Account::new(1, 0, &Account::default().owner);
        accounts.store_uncached(0, &[(&pubkey, &account)]);
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
        let owner = Account::default().owner;

        let account = Account::new(old_lamport, no_data, &owner);
        let account2 = Account::new(old_lamport + 100_001, no_data, &owner);
        let account3 = Account::new(old_lamport + 100_002, no_data, &owner);
        let dummy_account = Account::new(99_999_999, no_data, &owner);
        let zero_lamport_account = Account::new(zero_lamport, no_data, &owner);

        let pubkey = solana_sdk::pubkey::new_rand();
        let dummy_pubkey = solana_sdk::pubkey::new_rand();
        let purged_pubkey1 = solana_sdk::pubkey::new_rand();
        let purged_pubkey2 = solana_sdk::pubkey::new_rand();

        let mut current_slot = 0;
        let accounts = AccountsDB::new_single();

        // create intermediate updates to purged_pubkey1 so that
        // generate_index must add slots as root last at once
        current_slot += 1;
        accounts.store_uncached(current_slot, &[(&pubkey, &account)]);
        accounts.store_uncached(current_slot, &[(&purged_pubkey1, &account2)]);
        accounts.add_root(current_slot);

        current_slot += 1;
        accounts.store_uncached(current_slot, &[(&purged_pubkey1, &account2)]);
        accounts.add_root(current_slot);

        current_slot += 1;
        accounts.store_uncached(current_slot, &[(&purged_pubkey1, &account2)]);
        accounts.add_root(current_slot);

        current_slot += 1;
        accounts.store_uncached(current_slot, &[(&purged_pubkey1, &zero_lamport_account)]);
        accounts.store_uncached(current_slot, &[(&purged_pubkey2, &account3)]);
        accounts.add_root(current_slot);

        current_slot += 1;
        accounts.store_uncached(current_slot, &[(&purged_pubkey2, &zero_lamport_account)]);
        accounts.add_root(current_slot);

        current_slot += 1;
        accounts.store_uncached(current_slot, &[(&dummy_pubkey, &dummy_account)]);
        accounts.add_root(current_slot);

        accounts.print_count_and_status("before reconstruct");
        let accounts = reconstruct_accounts_db_via_serialization(&accounts, current_slot);
        accounts.print_count_and_status("before purge zero");
        accounts.clean_accounts(None);
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

        let owner = Account::default().owner;

        let account = Account::new(old_lamport, data_size, &owner);
        let account2 = Account::new(old_lamport + 100_001, data_size, &owner);
        let account3 = Account::new(old_lamport + 100_002, data_size, &owner);
        let account4 = Account::new(dummy_lamport, data_size, &owner);
        let zero_lamport_account = Account::new(zero_lamport, data_size, &owner);

        let mut current_slot = 0;
        let accounts = AccountsDB::new_sized_no_extra_stores(Vec::new(), store_size);

        // A: Initialize AccountsDB with pubkey1 and pubkey2
        current_slot += 1;
        if store1_first {
            accounts.store_uncached(current_slot, &[(&pubkey1, &account)]);
            accounts.store_uncached(current_slot, &[(&pubkey2, &account)]);
        } else {
            accounts.store_uncached(current_slot, &[(&pubkey2, &account)]);
            accounts.store_uncached(current_slot, &[(&pubkey1, &account)]);
        }
        accounts.get_accounts_delta_hash(current_slot);
        accounts.add_root(current_slot);

        info!("post A");
        accounts.print_accounts_stats("Post-A");

        // B: Test multiple updates to pubkey1 in a single slot/storage
        current_slot += 1;
        assert_eq!(0, accounts.alive_account_count_in_slot(current_slot));
        assert_eq!(1, accounts.ref_count_for_pubkey(&pubkey1));
        accounts.store_uncached(current_slot, &[(&pubkey1, &account2)]);
        accounts.store_uncached(current_slot, &[(&pubkey1, &account2)]);
        assert_eq!(1, accounts.alive_account_count_in_slot(current_slot));
        // Stores to same pubkey, same slot only count once towards the
        // ref count
        assert_eq!(2, accounts.ref_count_for_pubkey(&pubkey1));
        accounts.get_accounts_delta_hash(current_slot);
        accounts.add_root(current_slot);

        accounts.print_accounts_stats("Post-B pre-clean");

        accounts.clean_accounts(None);

        info!("post B");
        accounts.print_accounts_stats("Post-B");

        // C: more updates to trigger clean of previous updates
        current_slot += 1;
        assert_eq!(2, accounts.ref_count_for_pubkey(&pubkey1));
        accounts.store_uncached(current_slot, &[(&pubkey1, &account3)]);
        accounts.store_uncached(current_slot, &[(&pubkey2, &account3)]);
        accounts.store_uncached(current_slot, &[(&pubkey3, &account4)]);
        assert_eq!(3, accounts.ref_count_for_pubkey(&pubkey1));
        accounts.get_accounts_delta_hash(current_slot);
        accounts.add_root(current_slot);

        info!("post C");

        accounts.print_accounts_stats("Post-C");

        // D: Make all keys 0-lamport, cleans all keys
        current_slot += 1;
        assert_eq!(3, accounts.ref_count_for_pubkey(&pubkey1));
        accounts.store_uncached(current_slot, &[(&pubkey1, &zero_lamport_account)]);
        accounts.store_uncached(current_slot, &[(&pubkey2, &zero_lamport_account)]);
        accounts.store_uncached(current_slot, &[(&pubkey3, &zero_lamport_account)]);

        let snapshot_stores = accounts.get_snapshot_storages(current_slot);
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
        accounts.add_root(current_slot);
        accounts.clean_accounts(None);

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
        // when pubkey1 is cleaned and therefor cause the ref count to be incorrect
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
        let owner = Account::default().owner;

        let account = Account::new(old_lamport, no_data, &owner);
        let account2 = Account::new(old_lamport + 100_001, no_data, &owner);
        let account3 = Account::new(old_lamport + 100_002, no_data, &owner);
        let dummy_account = Account::new(dummy_lamport, no_data, &owner);
        let zero_lamport_account = Account::new(zero_lamport, no_data, &owner);

        let pubkey1 = solana_sdk::pubkey::new_rand();
        let pubkey2 = solana_sdk::pubkey::new_rand();
        let dummy_pubkey = solana_sdk::pubkey::new_rand();

        let mut current_slot = 0;
        let accounts = AccountsDB::new_single();

        // A: Initialize AccountsDB with pubkey1 and pubkey2
        current_slot += 1;
        accounts.store_uncached(current_slot, &[(&pubkey1, &account)]);
        accounts.store_uncached(current_slot, &[(&pubkey2, &account)]);
        accounts.get_accounts_delta_hash(current_slot);
        accounts.add_root(current_slot);

        // B: Test multiple updates to pubkey1 in a single slot/storage
        current_slot += 1;
        assert_eq!(0, accounts.alive_account_count_in_slot(current_slot));
        assert_eq!(1, accounts.ref_count_for_pubkey(&pubkey1));
        accounts.store_uncached(current_slot, &[(&pubkey1, &account2)]);
        accounts.store_uncached(current_slot, &[(&pubkey1, &account2)]);
        assert_eq!(1, accounts.alive_account_count_in_slot(current_slot));
        // Stores to same pubkey, same slot only count once towards the
        // ref count
        assert_eq!(2, accounts.ref_count_for_pubkey(&pubkey1));
        accounts.get_accounts_delta_hash(current_slot);
        accounts.add_root(current_slot);

        // C: Yet more update to trigger lazy clean of step A
        current_slot += 1;
        assert_eq!(2, accounts.ref_count_for_pubkey(&pubkey1));
        accounts.store_uncached(current_slot, &[(&pubkey1, &account3)]);
        assert_eq!(3, accounts.ref_count_for_pubkey(&pubkey1));
        accounts.get_accounts_delta_hash(current_slot);
        accounts.add_root(current_slot);

        // D: Make pubkey1 0-lamport; also triggers clean of step B
        current_slot += 1;
        assert_eq!(3, accounts.ref_count_for_pubkey(&pubkey1));
        accounts.store_uncached(current_slot, &[(&pubkey1, &zero_lamport_account)]);
        accounts.clean_accounts(None);

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
        accounts.store_uncached(current_slot, &[(&dummy_pubkey, &dummy_account)]);
        accounts.get_accounts_delta_hash(current_slot);
        accounts.add_root(current_slot);

        assert_load_account(&accounts, current_slot, pubkey1, zero_lamport);
        assert_load_account(&accounts, current_slot, pubkey2, old_lamport);
        assert_load_account(&accounts, current_slot, dummy_pubkey, dummy_lamport);

        // At this point, there is no index entries for A and B
        // If step C and step D should be purged, snapshot restore would cause
        // pubkey1 to be revived as the state of step A.
        // So, prevent that from happening by introducing refcount
        accounts.clean_accounts(None);
        let accounts = reconstruct_accounts_db_via_serialization(&accounts, current_slot);
        accounts.clean_accounts(None);

        info!("pubkey: {}", pubkey1);
        accounts.print_accounts_stats("pre_clean");
        assert_load_account(&accounts, current_slot, pubkey1, zero_lamport);
        assert_load_account(&accounts, current_slot, pubkey2, old_lamport);
        assert_load_account(&accounts, current_slot, dummy_pubkey, dummy_lamport);

        // F: Finally, make Step A cleanable
        current_slot += 1;
        accounts.store_uncached(current_slot, &[(&pubkey2, &account)]);
        accounts.get_accounts_delta_hash(current_slot);
        accounts.add_root(current_slot);

        // Do clean
        accounts.clean_accounts(None);

        // Ensure pubkey2 is cleaned from the index finally
        assert_not_load_account(&accounts, current_slot, pubkey1);
        assert_load_account(&accounts, current_slot, pubkey2, old_lamport);
        assert_load_account(&accounts, current_slot, dummy_pubkey, dummy_lamport);
    }

    #[test]
    fn test_clean_stored_dead_slots_empty() {
        let accounts = AccountsDB::new_single();
        let mut dead_slots = HashSet::new();
        dead_slots.insert(10);
        accounts.clean_stored_dead_slots(&dead_slots, None);
    }

    #[test]
    fn test_shrink_all_slots_none() {
        let accounts = AccountsDB::new_single();

        for _ in 0..10 {
            accounts.shrink_candidate_slots();
        }

        accounts.shrink_all_slots();
    }

    #[test]
    fn test_shrink_next_slots() {
        let mut accounts = AccountsDB::new_single();
        accounts.caching_enabled = false;

        let mut current_slot = 7;

        assert_eq!(
            vec![None, None, None],
            (0..3)
                .map(|_| accounts.next_shrink_slot_v1())
                .collect::<Vec<_>>()
        );

        accounts.get_accounts_delta_hash(current_slot);
        accounts.add_root(current_slot);

        assert_eq!(
            vec![Some(7), Some(7), Some(7)],
            (0..3)
                .map(|_| accounts.next_shrink_slot_v1())
                .collect::<Vec<_>>()
        );

        current_slot += 1;
        accounts.get_accounts_delta_hash(current_slot);
        accounts.add_root(current_slot);

        let slots = (0..6)
            .map(|_| accounts.next_shrink_slot_v1())
            .collect::<Vec<_>>();

        // Because the origin of this data is HashMap (not BTreeMap), key order is arbitrary per cycle.
        assert!(
            vec![Some(7), Some(8), Some(7), Some(8), Some(7), Some(8)] == slots
                || vec![Some(8), Some(7), Some(8), Some(7), Some(8), Some(7)] == slots
        );
    }

    #[test]
    fn test_shrink_reset_uncleaned_roots() {
        let mut accounts = AccountsDB::new_single();
        accounts.caching_enabled = false;

        accounts.reset_uncleaned_roots_v1();
        assert_eq!(
            *accounts.shrink_candidate_slots_v1.lock().unwrap(),
            vec![] as Vec<Slot>
        );

        accounts.get_accounts_delta_hash(0);
        accounts.add_root(0);
        accounts.get_accounts_delta_hash(1);
        accounts.add_root(1);
        accounts.get_accounts_delta_hash(2);
        accounts.add_root(2);

        accounts.reset_uncleaned_roots_v1();
        let actual_slots = accounts.shrink_candidate_slots_v1.lock().unwrap().clone();
        assert_eq!(actual_slots, vec![] as Vec<Slot>);

        accounts.reset_uncleaned_roots_v1();
        let mut actual_slots = accounts.shrink_candidate_slots_v1.lock().unwrap().clone();
        actual_slots.sort_unstable();
        assert_eq!(actual_slots, vec![0, 1, 2]);

        accounts.accounts_index.clear_roots();
        let mut actual_slots = (0..5)
            .map(|_| accounts.next_shrink_slot_v1())
            .collect::<Vec<_>>();
        actual_slots.sort();
        assert_eq!(actual_slots, vec![None, None, Some(0), Some(1), Some(2)],);
    }

    #[test]
    fn test_shrink_stale_slots_processed() {
        solana_logger::setup();

        let accounts = AccountsDB::new_single();

        let pubkey_count = 100;
        let pubkeys: Vec<_> = (0..pubkey_count)
            .map(|_| solana_sdk::pubkey::new_rand())
            .collect();

        let some_lamport = 223;
        let no_data = 0;
        let owner = Account::default().owner;

        let account = Account::new(some_lamport, no_data, &owner);

        let mut current_slot = 0;

        current_slot += 1;
        for pubkey in &pubkeys {
            accounts.store_uncached(current_slot, &[(&pubkey, &account)]);
        }
        let shrink_slot = current_slot;
        accounts.get_accounts_delta_hash(current_slot);
        accounts.add_root(current_slot);

        current_slot += 1;
        let pubkey_count_after_shrink = 10;
        let updated_pubkeys = &pubkeys[0..pubkey_count - pubkey_count_after_shrink];

        for pubkey in updated_pubkeys {
            accounts.store_uncached(current_slot, &[(&pubkey, &account)]);
        }
        accounts.get_accounts_delta_hash(current_slot);
        accounts.add_root(current_slot);

        accounts.clean_accounts(None);

        assert_eq!(
            pubkey_count,
            accounts.all_account_count_in_append_vec(shrink_slot)
        );
        accounts.shrink_all_slots();
        assert_eq!(
            pubkey_count_after_shrink,
            accounts.all_account_count_in_append_vec(shrink_slot)
        );

        let no_ancestors = HashMap::default();
        accounts.update_accounts_hash(current_slot, &no_ancestors, true);
        accounts
            .verify_bank_hash_and_lamports(current_slot, &no_ancestors, 22300, true)
            .unwrap();

        let accounts = reconstruct_accounts_db_via_serialization(&accounts, current_slot);
        accounts
            .verify_bank_hash_and_lamports(current_slot, &no_ancestors, 22300, true)
            .unwrap();

        // repeating should be no-op
        accounts.shrink_all_slots();
        assert_eq!(
            pubkey_count_after_shrink,
            accounts.all_account_count_in_append_vec(shrink_slot)
        );
    }

    #[test]
    fn test_shrink_candidate_slots() {
        solana_logger::setup();

        let accounts = AccountsDB::new_single();

        let pubkey_count = 30000;
        let pubkeys: Vec<_> = (0..pubkey_count)
            .map(|_| solana_sdk::pubkey::new_rand())
            .collect();

        let some_lamport = 223;
        let no_data = 0;
        let owner = Account::default().owner;

        let account = Account::new(some_lamport, no_data, &owner);

        let mut current_slot = 0;

        current_slot += 1;
        for pubkey in &pubkeys {
            accounts.store_uncached(current_slot, &[(&pubkey, &account)]);
        }
        let shrink_slot = current_slot;
        accounts.get_accounts_delta_hash(current_slot);
        accounts.add_root(current_slot);

        current_slot += 1;
        let pubkey_count_after_shrink = 25000;
        let updated_pubkeys = &pubkeys[0..pubkey_count - pubkey_count_after_shrink];

        for pubkey in updated_pubkeys {
            accounts.store_uncached(current_slot, &[(&pubkey, &account)]);
        }
        accounts.get_accounts_delta_hash(current_slot);
        accounts.add_root(current_slot);
        accounts.clean_accounts(None);

        assert_eq!(
            pubkey_count,
            accounts.all_account_count_in_append_vec(shrink_slot)
        );

        // Only, try to shrink stale slots, nothing happens because 90/100
        // is not small enough to do a shrink
        accounts.shrink_candidate_slots();
        assert_eq!(
            pubkey_count,
            accounts.all_account_count_in_append_vec(shrink_slot)
        );

        // Now, do full-shrink.
        accounts.shrink_all_slots();
        assert_eq!(
            pubkey_count_after_shrink,
            accounts.all_account_count_in_append_vec(shrink_slot)
        );
    }

    #[test]
    fn test_shrink_stale_slots_skipped() {
        solana_logger::setup();

        let mut accounts = AccountsDB::new_single();
        accounts.caching_enabled = false;

        let pubkey_count = 30000;
        let pubkeys: Vec<_> = (0..pubkey_count)
            .map(|_| solana_sdk::pubkey::new_rand())
            .collect();

        let some_lamport = 223;
        let no_data = 0;
        let owner = Account::default().owner;

        let account = Account::new(some_lamport, no_data, &owner);

        let mut current_slot = 0;

        current_slot += 1;
        for pubkey in &pubkeys {
            accounts.store_uncached(current_slot, &[(&pubkey, &account)]);
        }
        let shrink_slot = current_slot;
        accounts.get_accounts_delta_hash(current_slot);
        accounts.add_root(current_slot);

        current_slot += 1;
        let pubkey_count_after_shrink = 25000;
        let updated_pubkeys = &pubkeys[0..pubkey_count - pubkey_count_after_shrink];

        for pubkey in updated_pubkeys {
            accounts.store_uncached(current_slot, &[(&pubkey, &account)]);
        }
        accounts.get_accounts_delta_hash(current_slot);
        accounts.add_root(current_slot);

        accounts.clean_accounts(None);

        assert_eq!(
            pubkey_count,
            accounts.all_account_count_in_append_vec(shrink_slot)
        );

        // Only, try to shrink stale slots.
        accounts.shrink_all_stale_slots_v1();
        assert_eq!(
            pubkey_count,
            accounts.all_account_count_in_append_vec(shrink_slot)
        );

        // Now, do full-shrink.
        accounts.shrink_all_slots();
        assert_eq!(
            pubkey_count_after_shrink,
            accounts.all_account_count_in_append_vec(shrink_slot)
        );
    }

    #[test]
    fn test_delete_dependencies() {
        solana_logger::setup();
        let accounts_index = AccountsIndex::default();
        let key0 = Pubkey::new_from_array([0u8; 32]);
        let key1 = Pubkey::new_from_array([1u8; 32]);
        let key2 = Pubkey::new_from_array([2u8; 32]);
        let info0 = AccountInfo {
            store_id: 0,
            offset: 0,
            stored_size: 0,
            lamports: 0,
        };
        let info1 = AccountInfo {
            store_id: 1,
            offset: 0,
            stored_size: 0,
            lamports: 0,
        };
        let info2 = AccountInfo {
            store_id: 2,
            offset: 0,
            stored_size: 0,
            lamports: 0,
        };
        let info3 = AccountInfo {
            store_id: 3,
            offset: 0,
            stored_size: 0,
            lamports: 0,
        };
        let mut reclaims = vec![];
        accounts_index.upsert(
            0,
            &key0,
            &Pubkey::default(),
            &[],
            &HashSet::new(),
            info0,
            &mut reclaims,
        );
        accounts_index.upsert(
            1,
            &key0,
            &Pubkey::default(),
            &[],
            &HashSet::new(),
            info1.clone(),
            &mut reclaims,
        );
        accounts_index.upsert(
            1,
            &key1,
            &Pubkey::default(),
            &[],
            &HashSet::new(),
            info1,
            &mut reclaims,
        );
        accounts_index.upsert(
            2,
            &key1,
            &Pubkey::default(),
            &[],
            &HashSet::new(),
            info2.clone(),
            &mut reclaims,
        );
        accounts_index.upsert(
            2,
            &key2,
            &Pubkey::default(),
            &[],
            &HashSet::new(),
            info2,
            &mut reclaims,
        );
        accounts_index.upsert(
            3,
            &key2,
            &Pubkey::default(),
            &[],
            &HashSet::new(),
            info3,
            &mut reclaims,
        );
        accounts_index.add_root(0, false);
        accounts_index.add_root(1, false);
        accounts_index.add_root(2, false);
        accounts_index.add_root(3, false);
        let mut purges = HashMap::new();
        let (key0_entry, _) = accounts_index.get(&key0, None, None).unwrap();
        purges.insert(key0, accounts_index.roots_and_ref_count(&key0_entry, None));
        let (key1_entry, _) = accounts_index.get(&key1, None, None).unwrap();
        purges.insert(key1, accounts_index.roots_and_ref_count(&key1_entry, None));
        let (key2_entry, _) = accounts_index.get(&key2, None, None).unwrap();
        purges.insert(key2, accounts_index.roots_and_ref_count(&key2_entry, None));
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
        AccountsDB::calc_delete_dependencies(&purges, &mut store_counts);
        let mut stores: Vec<_> = store_counts.keys().cloned().collect();
        stores.sort_unstable();
        for store in &stores {
            info!(
                "store: {:?} : {:?}",
                store,
                store_counts.get(&store).unwrap()
            );
        }
        for x in 0..3 {
            assert!(store_counts[&x].0 >= 1);
        }
    }

    #[test]
    fn test_account_balance_for_capitalization_normal() {
        // system accounts
        assert_eq!(
            AccountsDB::account_balance_for_capitalization(10, &Pubkey::default(), false, true),
            10
        );
        // any random program data accounts
        assert_eq!(
            AccountsDB::account_balance_for_capitalization(
                10,
                &solana_sdk::pubkey::new_rand(),
                false,
                true,
            ),
            10
        );
        assert_eq!(
            AccountsDB::account_balance_for_capitalization(
                10,
                &solana_sdk::pubkey::new_rand(),
                false,
                false,
            ),
            10
        );
    }

    #[test]
    fn test_account_balance_for_capitalization_sysvar() {
        let normal_sysvar = solana_sdk::account::create_account(
            &solana_sdk::slot_history::SlotHistory::default(),
            1,
        );
        assert_eq!(
            AccountsDB::account_balance_for_capitalization(
                normal_sysvar.lamports,
                &normal_sysvar.owner,
                normal_sysvar.executable,
                false,
            ),
            0
        );
        assert_eq!(
            AccountsDB::account_balance_for_capitalization(
                normal_sysvar.lamports,
                &normal_sysvar.owner,
                normal_sysvar.executable,
                true,
            ),
            1
        );

        // currently transactions can send any lamports to sysvars although this is not sensible.
        assert_eq!(
            AccountsDB::account_balance_for_capitalization(
                10,
                &solana_sdk::sysvar::id(),
                false,
                false
            ),
            9
        );
        assert_eq!(
            AccountsDB::account_balance_for_capitalization(
                10,
                &solana_sdk::sysvar::id(),
                false,
                true
            ),
            10
        );
    }

    #[test]
    fn test_account_balance_for_capitalization_native_program() {
        let normal_native_program = solana_sdk::native_loader::create_loadable_account("foo", 1);
        assert_eq!(
            AccountsDB::account_balance_for_capitalization(
                normal_native_program.lamports,
                &normal_native_program.owner,
                normal_native_program.executable,
                false,
            ),
            0
        );
        assert_eq!(
            AccountsDB::account_balance_for_capitalization(
                normal_native_program.lamports,
                &normal_native_program.owner,
                normal_native_program.executable,
                true,
            ),
            1
        );

        // test maliciously assigned bogus native loader account
        assert_eq!(
            AccountsDB::account_balance_for_capitalization(
                1,
                &solana_sdk::native_loader::id(),
                false,
                false,
            ),
            1
        );
        assert_eq!(
            AccountsDB::account_balance_for_capitalization(
                1,
                &solana_sdk::native_loader::id(),
                false,
                true,
            ),
            1
        );
    }

    #[test]
    fn test_checked_sum_for_capitalization_normal() {
        assert_eq!(
            AccountsDB::checked_sum_for_capitalization(vec![1, 2].into_iter()),
            3
        );
    }

    #[test]
    #[should_panic(expected = "overflow is detected while summing capitalization")]
    fn test_checked_sum_for_capitalization_overflow() {
        assert_eq!(
            AccountsDB::checked_sum_for_capitalization(vec![1, u64::max_value()].into_iter()),
            3
        );
    }

    #[test]
    fn test_store_overhead() {
        solana_logger::setup();
        let accounts = AccountsDB::new_single();
        let account = Account::default();
        let pubkey = solana_sdk::pubkey::new_rand();
        accounts.store_uncached(0, &[(&pubkey, &account)]);
        let slot_stores = accounts.storage.get_slot_stores(0).unwrap();
        let mut total_len = 0;
        for (_id, store) in slot_stores.read().unwrap().iter() {
            total_len += store.accounts.len();
        }
        info!("total: {}", total_len);
        assert!(total_len < STORE_META_OVERHEAD);
    }

    #[test]
    fn test_store_reuse() {
        solana_logger::setup();
        let accounts = AccountsDB::new_sized(vec![], 4096);

        let size = 100;
        let num_accounts: usize = 100;
        let mut keys = Vec::new();
        for i in 0..num_accounts {
            let account = Account::new((i + 1) as u64, size, &Pubkey::default());
            let pubkey = solana_sdk::pubkey::new_rand();
            accounts.store_uncached(0, &[(&pubkey, &account)]);
            keys.push(pubkey);
        }
        accounts.add_root(0);

        for (i, key) in keys[1..].iter().enumerate() {
            let account = Account::new((1 + i + num_accounts) as u64, size, &Pubkey::default());
            accounts.store_uncached(1, &[(key, &account)]);
        }
        accounts.add_root(1);
        accounts.clean_accounts(None);
        accounts.shrink_all_slots();
        accounts.print_accounts_stats("post-shrink");
        let num_stores = accounts.recycle_stores.read().unwrap().len();
        assert!(num_stores > 0);

        let mut account_refs = Vec::new();
        let num_to_store = 20;
        for (i, key) in keys[..num_to_store].iter().enumerate() {
            let account = Account::new(
                (1 + i + 2 * num_accounts) as u64,
                i + 20,
                &Pubkey::default(),
            );
            accounts.store_uncached(2, &[(key, &account)]);
            account_refs.push(account);
        }
        assert!(accounts.recycle_stores.read().unwrap().len() < num_stores);

        accounts.print_accounts_stats("post-store");

        let mut ancestors = HashMap::new();
        ancestors.insert(1, 0);
        ancestors.insert(2, 1);
        for (key, account_ref) in keys[..num_to_store].iter().zip(account_refs) {
            assert_eq!(accounts.load_slow(&ancestors, key).unwrap().0, account_ref);
        }
    }

    #[test]
    fn test_zero_lamport_new_root_not_cleaned() {
        let db = AccountsDB::new(Vec::new(), &ClusterType::Development);
        let account_key = Pubkey::new_unique();
        let zero_lamport_account = Account::new(0, 0, &Account::default().owner);

        // Store zero lamport account into slots 0 and 1, root both slots
        db.store_uncached(0, &[(&account_key, &zero_lamport_account)]);
        db.store_uncached(1, &[(&account_key, &zero_lamport_account)]);
        db.get_accounts_delta_hash(0);
        db.add_root(0);
        db.get_accounts_delta_hash(1);
        db.add_root(1);

        // Only clean zero lamport accounts up to slot 0
        db.clean_accounts(Some(0));

        // Should still be able to find zero lamport account in slot 1
        assert_eq!(
            db.load_slow(&HashMap::new(), &account_key),
            Some((zero_lamport_account, 1))
        );
    }

    #[test]
    fn test_store_load_cached() {
        let mut db = AccountsDB::new(Vec::new(), &ClusterType::Development);
        db.caching_enabled = true;
        let key = Pubkey::default();
        let account0 = Account::new(1, 0, &key);
        let slot = 0;
        db.store_cached(slot, &[(&key, &account0)]);

        // Load with no ancestors and no root will return nothing
        assert!(db.load_slow(&HashMap::new(), &key).is_none());

        // Load with ancestors not equal to `slot` will return nothing
        let ancestors = vec![(slot + 1, 1)].into_iter().collect();
        assert!(db.load_slow(&ancestors, &key).is_none());

        // Load with ancestors equal to `slot` will return the account
        let ancestors = vec![(slot, 1)].into_iter().collect();
        assert_eq!(
            db.load_slow(&ancestors, &key),
            Some((account0.clone(), slot))
        );

        // Adding root will return the account even without ancestors
        db.add_root(slot);
        assert_eq!(db.load_slow(&HashMap::new(), &key), Some((account0, slot)));
    }

    #[test]
    fn test_store_flush_load_cached() {
        let mut db = AccountsDB::new(Vec::new(), &ClusterType::Development);
        db.caching_enabled = true;
        let key = Pubkey::default();
        let account0 = Account::new(1, 0, &key);
        let slot = 0;
        db.store_cached(slot, &[(&key, &account0)]);
        db.mark_slot_frozen(slot);

        // No root was added yet, requires an ancestor to find
        // the account
        db.flush_accounts_cache(true, None);
        let ancestors = vec![(slot, 1)].into_iter().collect();
        assert_eq!(
            db.load_slow(&ancestors, &key),
            Some((account0.clone(), slot))
        );

        // Add root then flush
        db.add_root(slot);
        db.flush_accounts_cache(true, None);
        assert_eq!(db.load_slow(&HashMap::new(), &key), Some((account0, slot)));
    }

    #[test]
    fn test_flush_accounts_cache() {
        let mut db = AccountsDB::new(Vec::new(), &ClusterType::Development);
        db.caching_enabled = true;
        let account0 = Account::new(1, 0, &Pubkey::default());

        let unrooted_slot = 4;
        let root5 = 5;
        let root6 = 6;
        let unrooted_key = solana_sdk::pubkey::new_rand();
        let key5 = solana_sdk::pubkey::new_rand();
        let key6 = solana_sdk::pubkey::new_rand();
        db.store_cached(unrooted_slot, &[(&unrooted_key, &account0)]);
        db.store_cached(root5, &[(&key5, &account0)]);
        db.store_cached(root6, &[(&key6, &account0)]);
        for slot in &[unrooted_slot, root5, root6] {
            db.mark_slot_frozen(*slot);
        }
        db.add_root(root5);
        db.add_root(root6);

        // Unrooted slot should be able to be fetched before the flush
        let ancestors = vec![(unrooted_slot, 1)].into_iter().collect();
        assert_eq!(
            db.load_slow(&ancestors, &unrooted_key),
            Some((account0.clone(), unrooted_slot))
        );
        db.flush_accounts_cache(true, None);

        // After the flush, the unrooted slot is still in the cache
        assert!(db.load_slow(&ancestors, &unrooted_key).is_some());
        assert!(db
            .accounts_index
            .get_account_read_entry(&unrooted_key)
            .is_some());
        assert_eq!(db.accounts_cache.num_slots(), 1);
        assert!(db.accounts_cache.slot_cache(unrooted_slot).is_some());
        assert_eq!(
            db.load_slow(&HashMap::new(), &key5),
            Some((account0.clone(), root5))
        );
        assert_eq!(
            db.load_slow(&HashMap::new(), &key6),
            Some((account0, root6))
        );
    }

    #[test]
    fn test_flush_accounts_cache_if_needed() {
        run_test_flush_accounts_cache_if_needed(0, 2 * MAX_CACHE_SLOTS);
        run_test_flush_accounts_cache_if_needed(2 * MAX_CACHE_SLOTS, 0);
        run_test_flush_accounts_cache_if_needed(MAX_CACHE_SLOTS - 1, 0);
        run_test_flush_accounts_cache_if_needed(0, MAX_CACHE_SLOTS - 1);
        run_test_flush_accounts_cache_if_needed(MAX_CACHE_SLOTS, 0);
        run_test_flush_accounts_cache_if_needed(0, MAX_CACHE_SLOTS);
        run_test_flush_accounts_cache_if_needed(2 * MAX_CACHE_SLOTS, 2 * MAX_CACHE_SLOTS);
        run_test_flush_accounts_cache_if_needed(MAX_CACHE_SLOTS - 1, MAX_CACHE_SLOTS - 1);
        run_test_flush_accounts_cache_if_needed(MAX_CACHE_SLOTS, MAX_CACHE_SLOTS);
    }

    fn run_test_flush_accounts_cache_if_needed(num_roots: usize, num_unrooted: usize) {
        let mut db = AccountsDB::new(Vec::new(), &ClusterType::Development);
        db.caching_enabled = true;
        let account0 = Account::new(1, 0, &Pubkey::default());
        let mut keys = vec![];
        let num_slots = 2 * MAX_CACHE_SLOTS;
        for i in 0..num_roots + num_unrooted {
            let key = Pubkey::new_unique();
            db.store_cached(i as Slot, &[(&key, &account0)]);
            keys.push(key);
            db.mark_slot_frozen(i as Slot);
            if i < num_roots {
                db.add_root(i as Slot);
            }
        }

        db.flush_accounts_cache(false, None);

        let total_slots = num_roots + num_unrooted;
        // If there's <= the max size, then nothing will be flushed from the slot
        if total_slots <= MAX_CACHE_SLOTS {
            assert_eq!(db.accounts_cache.num_slots(), total_slots);
        } else {
            // Otherwise, all the roots are flushed, and only at most MAX_CACHE_SLOTS
            // of the unrooted slots are kept in the cache
            let expected_size = std::cmp::min(num_unrooted, MAX_CACHE_SLOTS);
            if expected_size > 0 {
                for unrooted_slot in total_slots - expected_size..total_slots {
                    assert!(db
                        .accounts_cache
                        .slot_cache(unrooted_slot as Slot)
                        .is_some());
                }
            }
        }

        // Should still be able to fetch all the accounts after flush
        for (slot, key) in (0..num_slots as Slot).zip(keys) {
            let ancestors = if slot < num_roots as Slot {
                HashMap::new()
            } else {
                vec![(slot, 1)].into_iter().collect()
            };
            assert_eq!(
                db.load_slow(&ancestors, &key),
                Some((account0.clone(), slot))
            );
        }
    }

    fn slot_stores(db: &AccountsDB, slot: Slot) -> Vec<Arc<AccountStorageEntry>> {
        if let Some(x) = db.storage.get_slot_stores(slot) {
            x.read().unwrap().values().cloned().collect()
        } else {
            vec![]
        }
    }

    #[test]
    fn test_flush_cache_clean() {
        let caching_enabled = true;
        let db = Arc::new(AccountsDB::new_with_config(
            Vec::new(),
            &ClusterType::Development,
            HashSet::new(),
            caching_enabled,
        ));

        let account_key = Pubkey::new_unique();
        let zero_lamport_account = Account::new(0, 0, &Account::default().owner);
        let slot1_account = Account::new(1, 1, &Account::default().owner);
        db.store_cached(0, &[(&account_key, &zero_lamport_account)]);
        db.store_cached(1, &[(&account_key, &slot1_account)]);

        db.add_root(0);
        db.add_root(1);

        // Clean should not remove anything yet as nothing has been flushed
        db.clean_accounts(None);
        let account = db
            .do_load(&Ancestors::default(), &account_key, Some(0))
            .unwrap();
        assert_eq!(account.0.lamports, 0);

        // Flush, then clean again. Should not need another root to initiate the cleaning
        // because `accounts_index.uncleaned_roots` should be correct
        db.flush_accounts_cache(true, None);
        db.clean_accounts(None);
        assert!(db
            .do_load(&Ancestors::default(), &account_key, Some(0))
            .is_none());
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
        db: Arc<AccountsDB>,
        scan_ancestors: Arc<Ancestors>,
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
                    |_collector: &mut Vec<(Pubkey, Account)>, maybe_account| {
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
                );
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
        let caching_enabled = true;
        let db = Arc::new(AccountsDB::new_with_config(
            Vec::new(),
            &ClusterType::Development,
            HashSet::new(),
            caching_enabled,
        ));
        let account_key = Pubkey::new_unique();
        let account_key2 = Pubkey::new_unique();
        let zero_lamport_account = Account::new(0, 0, &Account::default().owner);
        let slot1_account = Account::new(1, 1, &Account::default().owner);
        let slot2_account = Account::new(2, 1, &Account::default().owner);

        /*
            Store zero lamport account into slots 0, 1, 2 where
            root slots are 0, 2, and slot 1 is unrooted.
                                    0 (root)
                                /        \
                              1            2 (root)
        */
        db.store_cached(0, &[(&account_key, &zero_lamport_account)]);
        db.store_cached(1, &[(&account_key, &slot1_account)]);
        // Fodder for the scan so that the lock on `account_key` is not held
        db.store_cached(1, &[(&account_key2, &slot1_account)]);
        db.store_cached(2, &[(&account_key, &slot2_account)]);
        db.get_accounts_delta_hash(0);

        let max_scan_root = 0;
        db.add_root(max_scan_root);
        let scan_ancestors: Arc<Ancestors> = Arc::new(vec![(0, 1), (1, 1)].into_iter().collect());
        let scan_tracker = setup_scan(db.clone(), scan_ancestors.clone(), account_key2);

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
            .do_load(&Ancestors::default(), &account_key, Some(0))
            .unwrap();
        assert_eq!(account.0.lamports, zero_lamport_account.lamports);

        // Run clean, unrooted slot 1 should not be purged, and still readable from the cache,
        // because we're still doing a scan on it.
        db.clean_accounts(None);
        let account = db
            .do_load(&scan_ancestors, &account_key, Some(max_scan_root))
            .unwrap();
        assert_eq!(account.0.lamports, slot1_account.lamports);

        // When the scan is over, clean should not panic and should not purge something
        // still in the cache.
        scan_tracker.exit().unwrap();
        db.clean_accounts(None);
        let account = db
            .do_load(&scan_ancestors, &account_key, Some(max_scan_root))
            .unwrap();
        assert_eq!(account.0.lamports, slot1_account.lamports);

        // Simulate dropping the bank, which finally removes the slot from the cache
        db.purge_slot(1);
        assert!(db
            .do_load(&scan_ancestors, &account_key, Some(max_scan_root))
            .is_none());
    }

    #[test]
    fn test_alive_bytes() {
        let caching_enabled = true;
        let accounts_db = AccountsDB::new_with_config(
            Vec::new(),
            &ClusterType::Development,
            HashSet::new(),
            caching_enabled,
        );
        let slot: Slot = 0;
        let num_keys = 10;

        for data_size in 0..num_keys {
            let account = Account::new(1, data_size, &Pubkey::default());
            accounts_db.store_cached(slot, &[(&Pubkey::new_unique(), &account)]);
        }

        accounts_db.add_root(slot);
        accounts_db.flush_accounts_cache(true, None);

        let mut storage_maps: Vec<Arc<AccountStorageEntry>> = accounts_db
            .storage
            .get_slot_stores(slot)
            .map(|res| res.read().unwrap().values().cloned().collect())
            .unwrap_or_default();

        // Flushing cache should only create one storage entry
        assert_eq!(storage_maps.len(), 1);
        let storage0 = storage_maps.pop().unwrap();
        let accounts = storage0.all_accounts();

        for account in accounts {
            let before_size = storage0.alive_bytes.load(Ordering::Relaxed);
            let account_info = accounts_db
                .accounts_index
                .get_account_read_entry(&account.meta.pubkey)
                .map(|locked_entry| {
                    // Should only be one entry per key, since every key was only stored to slot 0
                    locked_entry.slot_list()[0].clone()
                })
                .unwrap();
            let removed_data_size = account_info.1.stored_size;
            // Fetching the account from storage should return the same
            // stored size as in the index.
            assert_eq!(removed_data_size, account.stored_size);
            assert_eq!(account_info.0, slot);
            let reclaims = vec![account_info];
            accounts_db.remove_dead_accounts(&reclaims, None, None, true);
            let after_size = storage0.alive_bytes.load(Ordering::Relaxed);
            assert_eq!(before_size, after_size + account.stored_size);
        }
    }

    fn setup_accounts_db_cache_clean(
        num_slots: usize,
        scan_slot: Option<Slot>,
    ) -> (Arc<AccountsDB>, Vec<Pubkey>, Vec<Slot>, Option<ScanTracker>) {
        let caching_enabled = true;
        let accounts_db = Arc::new(AccountsDB::new_with_config(
            Vec::new(),
            &ClusterType::Development,
            HashSet::new(),
            caching_enabled,
        ));
        let slots: Vec<_> = (0..num_slots as Slot).into_iter().collect();
        let stall_slot = num_slots as Slot;
        let scan_stall_key = Pubkey::new_unique();
        let keys: Vec<Pubkey> = std::iter::repeat_with(Pubkey::new_unique)
            .take(num_slots)
            .collect();
        if scan_slot.is_some() {
            accounts_db.store_cached(
                // Store it in a slot that isn't returned in `slots`
                stall_slot,
                &[(&scan_stall_key, &Account::new(1, 0, &Pubkey::default()))],
            );
        }

        // Store some subset of the keys in slots 0..num_slots
        let mut scan_tracker = None;
        for slot in &slots {
            for key in &keys[*slot as usize..] {
                accounts_db.store_cached(*slot, &[(key, &Account::new(1, 0, &Pubkey::default()))]);
            }
            accounts_db.add_root(*slot as Slot);
            if Some(*slot) == scan_slot {
                let ancestors = Arc::new(vec![(stall_slot, 1), (*slot, 1)].into_iter().collect());
                scan_tracker = Some(setup_scan(accounts_db.clone(), ancestors, scan_stall_key));
                assert_eq!(
                    accounts_db.accounts_index.min_ongoing_scan_root().unwrap(),
                    *slot
                );
            }
        }

        accounts_db.accounts_cache.remove_slot(stall_slot);

        // If there's <= MAX_CACHE_SLOTS, no slots should be flushed
        if accounts_db.accounts_cache.num_slots() <= MAX_CACHE_SLOTS {
            accounts_db.flush_accounts_cache(false, None);
            assert_eq!(accounts_db.accounts_cache.num_slots(), num_slots);
        }

        (accounts_db, keys, slots, scan_tracker)
    }

    #[test]
    fn test_accounts_db_cache_clean_dead_slots() {
        let num_slots = 10;
        let (accounts_db, keys, mut slots, _) = setup_accounts_db_cache_clean(num_slots, None);
        let last_dead_slot = (num_slots - 1) as Slot;
        assert_eq!(*slots.last().unwrap(), last_dead_slot);
        let alive_slot = last_dead_slot as Slot + 1;
        slots.push(alive_slot);
        for key in &keys {
            // Store a slot that overwrites all previous keys, rendering all previous keys dead
            accounts_db.store_cached(
                alive_slot,
                &[(key, &Account::new(1, 0, &Pubkey::default()))],
            );
            accounts_db.add_root(alive_slot);
        }

        // Before the flush, we can find entries in the database for slots < alive_slot if we specify
        // a smaller max root
        for key in &keys {
            assert!(accounts_db
                .do_load(&Ancestors::default(), key, Some(last_dead_slot))
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
                .do_load(&Ancestors::default(), key, Some(last_dead_slot))
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
        let (accounts_db, keys, slots, _) = setup_accounts_db_cache_clean(10, None);

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
            setup_accounts_db_cache_clean(num_slots, scan_root);
        let is_cache_at_limit = num_slots - requested_flush_root as usize - 1 > MAX_CACHE_SLOTS;

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
                    if is_cache_at_limit {
                        panic!(
                            "When cache is at limit, all roots should have been flushed to storage"
                        );
                    }
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
                    std::iter::once(keys[*slot as usize])
                        .into_iter()
                        .collect::<HashSet<Pubkey>>()
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
        // Test that if there are > MAX_CACHE_SLOTS in the cache after flush, then more roots
        // will be flushed
        run_test_accounts_db_cache_clean_max_root(
            MAX_CACHE_SLOTS + requested_flush_root as usize + 2,
            requested_flush_root,
            None,
        );
    }

    #[test]
    fn test_accounts_db_cache_clean_max_root_with_cache_limit_hit_and_scan() {
        let requested_flush_root = 5;
        // Test that if there are > MAX_CACHE_SLOTS in the cache after flush, then more roots
        // will be flushed
        run_test_accounts_db_cache_clean_max_root(
            MAX_CACHE_SLOTS + requested_flush_root as usize + 2,
            requested_flush_root,
            Some(requested_flush_root - 1),
        );
        run_test_accounts_db_cache_clean_max_root(
            MAX_CACHE_SLOTS + requested_flush_root as usize + 2,
            requested_flush_root,
            Some(requested_flush_root + 1),
        );
    }

    fn run_flush_rooted_accounts_cache(should_clean: bool) {
        let num_slots = 10;
        let (accounts_db, keys, slots, _) = setup_accounts_db_cache_clean(num_slots, None);
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
            if !should_clean || slot == slots.last().unwrap() {
                // The slot was not cleaned before being flushed to storage,
                // so it also contains all the original updates.
                assert_eq!(
                    slot_accounts,
                    keys[*slot as usize..]
                        .iter()
                        .cloned()
                        .collect::<HashSet<Pubkey>>()
                );
            } else {
                // If clean was specified, only the latest slot should have all the updates.
                // All these other slots have been cleaned before flush
                assert_eq!(
                    slot_accounts,
                    std::iter::once(keys[*slot as usize])
                        .into_iter()
                        .collect::<HashSet<Pubkey>>()
                );
            }
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

    #[test]
    fn test_partial_clean() {
        solana_logger::setup();
        let db = AccountsDB::new(Vec::new(), &ClusterType::Development);
        let account_key1 = Pubkey::new_unique();
        let account_key2 = Pubkey::new_unique();
        let account1 = Account::new(1, 0, &Account::default().owner);
        let account2 = Account::new(2, 0, &Account::default().owner);
        let account3 = Account::new(3, 0, &Account::default().owner);
        let account4 = Account::new(4, 0, &Account::default().owner);

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
        db.clean_accounts(None);

        db.print_accounts_stats("post-clean1");
        // Check stores > 0
        assert!(!slot_stores(&db, 0).is_empty());
        assert!(!slot_stores(&db, 1).is_empty());

        // root slot 0
        db.add_root(0);

        // store into slot 2
        db.store_uncached(2, &[(&account_key2, &account3)]);
        db.store_uncached(2, &[(&account_key1, &account3)]);
        db.get_accounts_delta_hash(2);

        db.clean_accounts(None);
        db.print_accounts_stats("post-clean2");

        // root slots 1
        db.add_root(1);
        db.clean_accounts(None);

        db.print_accounts_stats("post-clean3");

        db.store_uncached(3, &[(&account_key2, &account4)]);
        db.get_accounts_delta_hash(3);
        db.add_root(3);

        // Check that we can clean where max_root=3 and slot=2 is not rooted
        db.clean_accounts(None);

        assert!(db.uncleaned_pubkeys.is_empty());

        db.print_accounts_stats("post-clean4");

        assert!(slot_stores(&db, 0).is_empty());
        assert!(!slot_stores(&db, 1).is_empty());
    }
}

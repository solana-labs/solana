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
    accounts_index::AccountsIndex,
    append_vec::{AppendVec, StoredAccount, StoredMeta},
    bank::deserialize_from_snapshot,
};
use bincode::{deserialize_from, serialize_into};
use byteorder::{ByteOrder, LittleEndian};
use fs_extra::dir::CopyOptions;
use lazy_static::lazy_static;
use log::*;
use rand::{thread_rng, Rng};
use rayon::{prelude::*, ThreadPool};
use serde::{
    de::{MapAccess, Visitor},
    ser::{SerializeMap, Serializer},
    Deserialize, Serialize,
};
use solana_measure::measure::Measure;
use solana_rayon_threadlimit::get_thread_count;
use solana_sdk::{
    account::Account,
    clock::{Epoch, Slot},
    hash::{Hash, Hasher},
    pubkey::Pubkey,
};
use std::{
    collections::{HashMap, HashSet},
    fmt,
    io::{BufReader, Cursor, Error as IOError, ErrorKind, Read, Result as IOResult},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    sync::{Arc, RwLock},
};
use tempfile::TempDir;

pub const DEFAULT_FILE_SIZE: u64 = 4 * 1024 * 1024;
pub const DEFAULT_NUM_THREADS: u32 = 8;
pub const DEFAULT_NUM_DIRS: u32 = 4;

lazy_static! {
    // FROZEN_ACCOUNT_PANIC is used to signal local_cluster that an AccountsDB panic has occurred,
    // as |cargo test| cannot observe panics in other threads
    pub static ref FROZEN_ACCOUNT_PANIC: Arc<AtomicBool> = { Arc::new(AtomicBool::new(false)) };
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
}

#[derive(Default, Debug, PartialEq, Clone)]
pub struct AccountInfo {
    /// index identifying the append storage
    store_id: AppendVecId,

    /// offset into the storage
    offset: usize,

    /// lamports in the account used when squashing kept for optimization
    /// purposes to remove accounts with zero balance.
    lamports: u64,
}
/// An offset into the AccountsDB::storage vector
pub type AppendVecId = usize;
pub type SnapshotStorage = Vec<Arc<AccountStorageEntry>>;
pub type SnapshotStorages = Vec<SnapshotStorage>;

// Each slot has a set of storage entries.
type SlotStores = HashMap<usize, Arc<AccountStorageEntry>>;

struct AccountStorageVisitor;

impl<'de> Visitor<'de> for AccountStorageVisitor {
    type Value = AccountStorage;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("Expecting AccountStorage")
    }

    #[allow(clippy::mutex_atomic)]
    fn visit_map<M>(self, mut access: M) -> Result<Self::Value, M::Error>
    where
        M: MapAccess<'de>,
    {
        let mut map = HashMap::new();
        while let Some((slot_id, storage_entries)) = access.next_entry()? {
            let storage_entries: Vec<AccountStorageEntry> = storage_entries;
            let storage_slot_map = map.entry(slot_id).or_insert_with(HashMap::new);
            for mut storage in storage_entries {
                storage.slot_id = slot_id;
                storage_slot_map.insert(storage.id, Arc::new(storage));
            }
        }

        Ok(AccountStorage(map))
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

impl Versioned for (u64, AccountInfo) {
    fn version(&self) -> u64 {
        self.0
    }
}

struct AccountStorageSerialize<'a> {
    account_storage_entries: &'a [SnapshotStorage],
}

impl<'a> Serialize for AccountStorageSerialize<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.account_storage_entries.len()))?;
        let mut count = 0;
        let mut serialize_account_storage_timer = Measure::start("serialize_account_storage_ms");
        for storage_entries in self.account_storage_entries {
            map.serialize_entry(&storage_entries.first().unwrap().slot_id, storage_entries)?;
            count += storage_entries.len();
        }
        serialize_account_storage_timer.stop();
        datapoint_info!(
            "serialize_account_storage_ms",
            ("duration", serialize_account_storage_timer.as_ms(), i64),
            ("num_entries", count, i64),
        );
        map.end()
    }
}

#[derive(Clone, Default, Debug)]
pub struct AccountStorage(pub HashMap<Slot, SlotStores>);
impl<'de> Deserialize<'de> for AccountStorage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_map(AccountStorageVisitor)
    }
}

#[derive(Debug, PartialEq, Copy, Clone, Deserialize, Serialize)]
pub enum AccountStorageStatus {
    Available = 0,
    Full = 1,
    Candidate = 2,
}

#[derive(Debug)]
pub enum BankHashVerificationError {
    MismatchedAccountHash,
    MismatchedBankHash,
    MissingBankHash,
}

/// Persistent storage structure holding the accounts
#[derive(Debug, Serialize, Deserialize)]
pub struct AccountStorageEntry {
    id: AppendVecId,

    #[serde(skip)]
    slot_id: Slot,

    /// storage holding the accounts
    accounts: AppendVec,

    /// Keeps track of the number of accounts stored in a specific AppendVec.
    ///  This is periodically checked to reuse the stores that do not have
    ///  any accounts in it
    /// status corresponding to the storage, lets us know that
    ///  the append_vec, once maxed out, then emptied, can be reclaimed
    count_and_status: RwLock<(usize, AccountStorageStatus)>,
}

impl AccountStorageEntry {
    pub fn new(path: &Path, slot_id: Slot, id: usize, file_size: u64) -> Self {
        let tail = AppendVec::new_relative_path(slot_id, id);
        let path = Path::new(path).join(&tail);
        let accounts = AppendVec::new(&path, true, file_size as usize);

        AccountStorageEntry {
            id,
            slot_id,
            accounts,
            count_and_status: RwLock::new((0, AccountStorageStatus::Available)),
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

    pub fn status(&self) -> AccountStorageStatus {
        self.count_and_status.read().unwrap().1
    }

    pub fn count(&self) -> usize {
        self.count_and_status.read().unwrap().0
    }

    pub fn has_accounts(&self) -> bool {
        self.count() > 0
    }

    pub fn slot_id(&self) -> Slot {
        self.slot_id
    }

    pub fn append_vec_id(&self) -> AppendVecId {
        self.id
    }

    pub fn flush(&self) -> Result<(), IOError> {
        self.accounts.flush()
    }

    fn add_account(&self) {
        let mut count_and_status = self.count_and_status.write().unwrap();
        *count_and_status = (count_and_status.0 + 1, count_and_status.1);
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

    fn remove_account(&self) -> usize {
        let mut count_and_status = self.count_and_status.write().unwrap();
        let (mut count, mut status) = *count_and_status;

        if count == 1 && status == AccountStorageStatus::Full {
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
            self.slot_id,
            self.id
        );

        count -= 1;
        *count_and_status = (count, status);
        count
    }

    pub fn set_file<P: AsRef<Path>>(&mut self, path: P) -> IOResult<()> {
        self.accounts.set_file(path)
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

pub struct AccountsDBSerialize<'a, 'b> {
    accounts_db: &'a AccountsDB,
    slot: Slot,
    account_storage_entries: &'b [SnapshotStorage],
}

impl<'a, 'b> AccountsDBSerialize<'a, 'b> {
    pub fn new(
        accounts_db: &'a AccountsDB,
        slot: Slot,
        account_storage_entries: &'b [SnapshotStorage],
    ) -> Self {
        Self {
            accounts_db,
            slot,
            account_storage_entries,
        }
    }
}

impl<'a, 'b> Serialize for AccountsDBSerialize<'a, 'b> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        use serde::ser::Error;
        let mut wr = Cursor::new(vec![]);
        let version: u64 = self.accounts_db.write_version.load(Ordering::Relaxed) as u64;
        let account_storage_serialize = AccountStorageSerialize {
            account_storage_entries: self.account_storage_entries,
        };
        serialize_into(&mut wr, &account_storage_serialize).map_err(Error::custom)?;
        serialize_into(&mut wr, &version).map_err(Error::custom)?;
        let bank_hashes = self.accounts_db.bank_hashes.read().unwrap();
        serialize_into(
            &mut wr,
            &(
                self.slot,
                &*bank_hashes
                    .get(&self.slot)
                    .unwrap_or_else(|| panic!("No bank_hashes entry for slot {}", self.slot)),
            ),
        )
        .map_err(Error::custom)?;
        let len = wr.position() as usize;
        serializer.serialize_bytes(&wr.into_inner()[..len])
    }
}

#[derive(Clone, Default, Debug, Serialize, Deserialize, PartialEq)]
pub struct BankHashStats {
    pub num_removed_accounts: u64,
    pub num_added_accounts: u64,
    pub num_lamports_stored: u64,
    pub total_data_len: u64,
    pub num_executable_accounts: u64,
}

impl BankHashStats {
    pub fn update(&mut self, account: &Account) {
        if Hash::default() == account.hash {
            self.num_added_accounts += 1;
        } else {
            self.num_removed_accounts += 1;
        }
        self.total_data_len = self.total_data_len.wrapping_add(account.data.len() as u64);
        if account.executable {
            self.num_executable_accounts += 1;
        }
        self.num_lamports_stored = self.num_lamports_stored.wrapping_add(account.lamports);
    }

    pub fn merge(&mut self, other: &BankHashStats) {
        self.num_removed_accounts += other.num_removed_accounts;
        self.num_added_accounts += other.num_added_accounts;
        self.total_data_len = self.total_data_len.wrapping_add(other.total_data_len);
        self.num_lamports_stored = self
            .num_lamports_stored
            .wrapping_add(other.num_lamports_stored);
        self.num_executable_accounts += other.num_executable_accounts;
    }
}

#[derive(Clone, Default, Debug, Serialize, Deserialize, PartialEq)]
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

// This structure handles the load/store of the accounts
#[derive(Debug)]
pub struct AccountsDB {
    /// Keeps tracks of index into AppendVec on a per slot basis
    pub accounts_index: RwLock<AccountsIndex<AccountInfo>>,

    pub storage: RwLock<AccountStorage>,

    /// distribute the accounts across storage lists
    pub next_id: AtomicUsize,

    write_version: AtomicUsize,

    /// Set of storage paths to pick from
    paths: Vec<PathBuf>,

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

    pub dead_slots: RwLock<HashSet<Slot>>,
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

impl Default for AccountsDB {
    fn default() -> Self {
        let num_threads = get_thread_count();

        let mut bank_hashes = HashMap::new();
        bank_hashes.insert(0, BankHashInfo::default());
        AccountsDB {
            accounts_index: RwLock::new(AccountsIndex::default()),
            storage: RwLock::new(AccountStorage(HashMap::new())),
            next_id: AtomicUsize::new(0),
            write_version: AtomicUsize::new(0),
            paths: vec![],
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
            dead_slots: RwLock::new(HashSet::new()),
        }
    }
}

impl AccountsDB {
    pub fn new(paths: Vec<PathBuf>) -> Self {
        let new = if !paths.is_empty() {
            Self {
                paths,
                temp_paths: None,
                ..Self::default()
            }
        } else {
            // Create a temporary set of accounts directories, used primarily
            // for testing
            let (temp_dirs, paths) = get_temp_accounts_paths(DEFAULT_NUM_DIRS).unwrap();
            Self {
                paths,
                temp_paths: Some(temp_dirs),
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

    #[cfg(test)]
    pub fn new_single() -> Self {
        AccountsDB {
            min_num_stores: 0,
            ..AccountsDB::new(Vec::new())
        }
    }
    #[cfg(test)]
    pub fn new_sized(paths: Vec<PathBuf>, file_size: u64) -> Self {
        AccountsDB {
            file_size,
            ..AccountsDB::new(paths)
        }
    }

    pub fn accounts_from_stream<R: Read, P: AsRef<Path>>(
        &self,
        mut stream: &mut BufReader<R>,
        stream_append_vecs_path: P,
    ) -> Result<(), IOError> {
        let _len: usize =
            deserialize_from(&mut stream).map_err(|e| AccountsDB::get_io_error(&e.to_string()))?;
        let storage: AccountStorage = deserialize_from_snapshot(&mut stream)
            .map_err(|e| AccountsDB::get_io_error(&e.to_string()))?;

        // Remap the deserialized AppendVec paths to point to correct local paths
        let new_storage_map: Result<HashMap<Slot, SlotStores>, IOError> = storage
            .0
            .into_iter()
            .map(|(slot_id, mut slot_storage)| {
                let mut new_slot_storage = HashMap::new();
                for (id, storage_entry) in slot_storage.drain() {
                    let path_index = thread_rng().gen_range(0, self.paths.len());
                    let local_dir = &self.paths[path_index];

                    std::fs::create_dir_all(local_dir).expect("Create directory failed");

                    // Move the corresponding AppendVec from the snapshot into the directory pointed
                    // at by `local_dir`
                    let append_vec_relative_path =
                        AppendVec::new_relative_path(slot_id, storage_entry.id);
                    let append_vec_abs_path = stream_append_vecs_path
                        .as_ref()
                        .join(&append_vec_relative_path);
                    let target = local_dir.join(append_vec_abs_path.file_name().unwrap());
                    if std::fs::rename(append_vec_abs_path.clone(), target).is_err() {
                        let mut copy_options = CopyOptions::new();
                        copy_options.overwrite = true;
                        let e = fs_extra::move_items(
                            &vec![&append_vec_abs_path],
                            &local_dir,
                            &copy_options,
                        )
                        .map_err(|e| {
                            AccountsDB::get_io_error(&format!(
                                "Unable to move {:?} to {:?}: {}",
                                append_vec_abs_path, local_dir, e
                            ))
                        });
                        if e.is_err() {
                            info!("{:?}", e);
                            continue;
                        }
                    };

                    // Notify the AppendVec of the new file location
                    let local_path = local_dir.join(append_vec_relative_path);
                    let mut u_storage_entry = Arc::try_unwrap(storage_entry).unwrap();
                    u_storage_entry
                        .set_file(local_path)
                        .map_err(|e| AccountsDB::get_io_error(&e.to_string()))?;
                    new_slot_storage.insert(id, Arc::new(u_storage_entry));
                }
                Ok((slot_id, new_slot_storage))
            })
            .collect();

        let new_storage_map = new_storage_map?;
        let mut storage = AccountStorage(new_storage_map);

        // discard any slots with no storage entries
        // this can happen if a non-root slot was serialized
        // but non-root stores should not be included in the snapshot
        storage.0.retain(|_slot_id, stores| !stores.is_empty());

        let version: u64 = deserialize_from(&mut stream)
            .map_err(|_| AccountsDB::get_io_error("write version deserialize error"))?;

        let (slot, bank_hash): (Slot, BankHashInfo) = deserialize_from(&mut stream)
            .map_err(|_| AccountsDB::get_io_error("bank hashes deserialize error"))?;
        self.bank_hashes.write().unwrap().insert(slot, bank_hash);

        // Process deserialized data, set necessary fields in self
        let max_id: usize = *storage
            .0
            .values()
            .flat_map(HashMap::keys)
            .max()
            .expect("At least one storage entry must exist from deserializing stream");

        {
            let mut stores = self.storage.write().unwrap();
            stores.0.extend(storage.0);
        }

        self.next_id.store(max_id + 1, Ordering::Relaxed);
        self.write_version
            .fetch_add(version as usize, Ordering::Relaxed);
        self.generate_index();
        Ok(())
    }

    fn new_storage_entry(&self, slot_id: Slot, path: &Path, size: u64) -> AccountStorageEntry {
        AccountStorageEntry::new(
            path,
            slot_id,
            self.next_id.fetch_add(1, Ordering::Relaxed),
            size,
        )
    }

    // Reclaim older states of rooted non-zero lamport accounts as a general
    // AccountsDB bloat mitigation and preprocess for better zero-lamport purging.
    fn clean_old_rooted_accounts(&self, purges_in_root: Vec<Pubkey>) {
        // This number isn't carefully chosen; just guessed randomly such that
        // the hot loop will be the order of ~Xms.
        const INDEX_CLEAN_BULK_COUNT: usize = 4096;

        let mut clean_rooted = Measure::start("clean_old_root-ms");
        let reclaim_vecs =
            purges_in_root
                .par_chunks(INDEX_CLEAN_BULK_COUNT)
                .map(|pubkeys: &[Pubkey]| {
                    let mut reclaims = Vec::new();
                    let accounts_index = self.accounts_index.read().unwrap();
                    for pubkey in pubkeys {
                        accounts_index.clean_rooted_entries(&pubkey, &mut reclaims);
                    }
                    reclaims
                });
        let reclaims: Vec<_> = reclaim_vecs.flatten().collect();
        clean_rooted.stop();
        inc_new_counter_info!("clean-old-root-par-clean-ms", clean_rooted.as_ms() as usize);

        let mut measure = Measure::start("clean_old_root_reclaims");
        self.handle_reclaims(&reclaims);
        measure.stop();
        debug!("{} {}", clean_rooted, measure);
        inc_new_counter_info!("clean-old-root-reclaim-ms", measure.as_ms() as usize);
    }

    fn clear_uncleaned_roots(&self) {
        let mut accounts_index = self.accounts_index.write().unwrap();
        accounts_index.uncleaned_roots.clear();
    }

    // Purge zero lamport accounts and older rooted account states as garbage
    // collection
    // Only remove those accounts where the entire rooted history of the account
    // can be purged because there are no live append vecs in the ancestors
    pub fn clean_accounts(&self) {
        self.report_store_stats();

        let no_ancestors = HashMap::new();
        let mut accounts_scan = Measure::start("accounts_scan");
        let accounts_index = self.accounts_index.read().unwrap();
        let pubkeys: Vec<Pubkey> = accounts_index.account_maps.keys().cloned().collect();
        // parallel scan the index.
        let (mut purges, purges_in_root) = pubkeys
            .par_chunks(4096)
            .map(|pubkeys: &[Pubkey]| {
                let mut purges_in_root = Vec::new();
                let mut purges = HashMap::new();
                for pubkey in pubkeys {
                    if let Some((list, index)) = accounts_index.get(pubkey, &no_ancestors) {
                        let (slot, account_info) = &list[index];
                        if account_info.lamports == 0 {
                            purges.insert(*pubkey, accounts_index.would_purge(pubkey));
                        } else if accounts_index.uncleaned_roots.contains(slot) {
                            purges_in_root.push(*pubkey);
                        }
                    }
                }
                (purges, purges_in_root)
            })
            .reduce(
                || (HashMap::new(), Vec::new()),
                |m1, m2| {
                    // Collapse down the hashmaps/vecs into one.
                    let x = m2.0.iter().fold(m1.0, |mut acc, (k, vs)| {
                        acc.insert(k.clone(), vs.clone());
                        acc
                    });
                    let mut y = vec![];
                    y.extend(m1.1);
                    y.extend(m2.1);
                    (x, y)
                },
            );

        drop(accounts_index);
        accounts_scan.stop();

        let mut clean_old_rooted = Measure::start("clean_old_roots");
        if !purges_in_root.is_empty() {
            self.clean_old_rooted_accounts(purges_in_root);
        }
        self.clear_uncleaned_roots();
        clean_old_rooted.stop();

        let mut store_counts_time = Measure::start("store_counts");
        let accounts_index = self.accounts_index.read().unwrap();

        // Calculate store counts as if everything was purged
        // Then purge if we can
        let mut store_counts: HashMap<AppendVecId, usize> = HashMap::new();
        let storage = self.storage.read().unwrap();
        for account_infos in purges.values() {
            for (slot_id, account_info) in account_infos {
                let slot_storage = storage.0.get(&slot_id).unwrap();
                let store = slot_storage.get(&account_info.store_id).unwrap();
                if let Some(store_count) = store_counts.get_mut(&account_info.store_id) {
                    *store_count -= 1;
                } else {
                    store_counts.insert(
                        account_info.store_id,
                        store.count_and_status.read().unwrap().0 - 1,
                    );
                }
            }
        }

        for account_infos in purges.values() {
            let mut no_delete = false;
            for (_slot_id, account_info) in account_infos {
                if *store_counts.get(&account_info.store_id).unwrap() != 0 {
                    no_delete = true;
                    break;
                }
            }
            if no_delete {
                for (_slot_id, account_info) in account_infos {
                    *store_counts.get_mut(&account_info.store_id).unwrap() += 1;
                }
            }
        }
        store_counts_time.stop();

        // Only keep purges where the entire history of the account in the root set
        // can be purged. All AppendVecs for those updates are dead.
        let mut purge_filter = Measure::start("purge_filter");
        purges.retain(|pubkey, account_infos| {
            let mut would_unref_count = 0;
            for (_slot_id, account_info) in account_infos {
                if *store_counts.get(&account_info.store_id).unwrap() == 0 {
                    would_unref_count += 1;
                } else {
                    return false;
                }
            }

            would_unref_count == accounts_index.ref_count_from_storage(&pubkey)
        });
        purge_filter.stop();

        let mut reclaims_time = Measure::start("reclaims");
        // Recalculate reclaims with new purge set
        let mut reclaims = Vec::new();
        let mut dead_keys = Vec::new();
        for pubkey in purges.keys() {
            let (new_reclaims, is_empty) = accounts_index.purge(&pubkey);
            if is_empty {
                dead_keys.push(*pubkey);
            }
            reclaims.extend(new_reclaims);
        }

        drop(storage);
        drop(accounts_index);

        if !dead_keys.is_empty() {
            let mut accounts_index = self.accounts_index.write().unwrap();
            for key in &dead_keys {
                accounts_index.account_maps.remove(key);
            }
        }

        self.handle_reclaims(&reclaims);
        reclaims_time.stop();
        debug!(
            "clean_accounts: {} {} {} {}",
            accounts_scan, store_counts_time, purge_filter, reclaims_time
        );
    }

    fn handle_reclaims(&self, reclaims: &[(Slot, AccountInfo)]) {
        let mut dead_accounts = Measure::start("reclaims::remove_dead_accounts");
        let dead_slots = self.remove_dead_accounts(reclaims);
        dead_accounts.stop();
        let dead_slots_len = {
            let mut dead_slots_w = self.dead_slots.write().unwrap();
            dead_slots_w.extend(dead_slots);
            dead_slots_w.len()
        };
        if dead_slots_len > 5000 {
            self.process_dead_slots();
        }
    }

    pub fn process_dead_slots(&self) {
        let empty = HashSet::new();
        let mut dead_slots_w = self.dead_slots.write().unwrap();
        let dead_slots = std::mem::replace(&mut *dead_slots_w, empty);
        drop(dead_slots_w);

        let mut clean_dead_slots = Measure::start("reclaims::purge_slots");
        self.clean_dead_slots(&dead_slots);
        clean_dead_slots.stop();

        let mut purge_slots = Measure::start("reclaims::purge_slots");
        self.purge_slots(&dead_slots);
        purge_slots.stop();

        debug!(
            "process_dead_slots({}): {} {}",
            dead_slots.len(),
            clean_dead_slots,
            purge_slots
        );
    }

    pub fn scan_accounts<F, A>(&self, ancestors: &HashMap<Slot, usize>, scan_func: F) -> A
    where
        F: Fn(&mut A, Option<(&Pubkey, Account, Slot)>) -> (),
        A: Default,
    {
        let mut collector = A::default();
        let accounts_index = self.accounts_index.read().unwrap();
        let storage = self.storage.read().unwrap();
        accounts_index.scan_accounts(ancestors, |pubkey, (account_info, slot)| {
            scan_func(
                &mut collector,
                storage
                    .0
                    .get(&slot)
                    .and_then(|storage_map| storage_map.get(&account_info.store_id))
                    .and_then(|store| {
                        Some(
                            store
                                .accounts
                                .get_account(account_info.offset)?
                                .0
                                .clone_account(),
                        )
                    })
                    .map(|account| (pubkey, account, slot)),
            )
        });
        collector
    }

    /// Scan a specific slot through all the account storage in parallel with sequential read
    // PERF: Sequentially read each storage entry in parallel
    pub fn scan_account_storage<F, B>(&self, slot_id: Slot, scan_func: F) -> Vec<B>
    where
        F: Fn(&StoredAccount, AppendVecId, &mut B) -> () + Send + Sync,
        B: Send + Default,
    {
        let storage_maps: Vec<Arc<AccountStorageEntry>> = self
            .storage
            .read()
            .unwrap()
            .0
            .get(&slot_id)
            .unwrap_or(&HashMap::new())
            .values()
            .cloned()
            .collect();
        self.thread_pool.install(|| {
            storage_maps
                .into_par_iter()
                .map(|storage| {
                    let accounts = storage.accounts.accounts(0);
                    let mut retval = B::default();
                    accounts.iter().for_each(|stored_account| {
                        scan_func(stored_account, storage.id, &mut retval)
                    });
                    retval
                })
                .collect()
        })
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

    pub fn load(
        storage: &AccountStorage,
        ancestors: &HashMap<Slot, usize>,
        accounts_index: &AccountsIndex<AccountInfo>,
        pubkey: &Pubkey,
    ) -> Option<(Account, Slot)> {
        let (lock, index) = accounts_index.get(pubkey, ancestors)?;
        let slot = lock[index].0;
        //TODO: thread this as a ref
        if let Some(slot_storage) = storage.0.get(&slot) {
            let info = &lock[index].1;
            slot_storage
                .get(&info.store_id)
                .and_then(|store| Some(store.accounts.get_account(info.offset)?.0.clone_account()))
                .map(|account| (account, slot))
        } else {
            None
        }
    }

    pub fn load_slow(
        &self,
        ancestors: &HashMap<Slot, usize>,
        pubkey: &Pubkey,
    ) -> Option<(Account, Slot)> {
        let accounts_index = self.accounts_index.read().unwrap();
        let storage = self.storage.read().unwrap();
        Self::load(&storage, ancestors, &accounts_index, pubkey)
    }

    fn find_storage_candidate(&self, slot_id: Slot) -> Arc<AccountStorageEntry> {
        let mut create_extra = false;
        let stores = self.storage.read().unwrap();

        if let Some(slot_stores) = stores.0.get(&slot_id) {
            if !slot_stores.is_empty() {
                if slot_stores.len() <= self.min_num_stores {
                    let mut total_accounts = 0;
                    for store in slot_stores.values() {
                        total_accounts += store.count_and_status.read().unwrap().0;
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
                        drop(stores);
                        if create_extra {
                            self.create_and_insert_store(slot_id, self.file_size);
                        }
                        return ret;
                    }
                    // looked at every store, bail...
                    if i == slot_stores.len() {
                        break;
                    }
                }
            }
        }

        drop(stores);

        let store = self.create_and_insert_store(slot_id, self.file_size);
        store.try_available();
        store
    }

    fn create_and_insert_store(&self, slot_id: Slot, size: u64) -> Arc<AccountStorageEntry> {
        let path_index = thread_rng().gen_range(0, self.paths.len());
        let store =
            Arc::new(self.new_storage_entry(slot_id, &Path::new(&self.paths[path_index]), size));
        let store_for_index = store.clone();

        let mut stores = self.storage.write().unwrap();
        let slot_storage = stores.0.entry(slot_id).or_insert_with(HashMap::new);
        slot_storage.insert(store.id, store_for_index);
        store
    }

    pub fn purge_slot(&self, slot: Slot) {
        let mut slots = HashSet::new();
        slots.insert(slot);
        self.purge_slots(&slots);
    }

    pub fn purge_slots(&self, slots: &HashSet<Slot>) {
        //add_root should be called first
        let accounts_index = self.accounts_index.read().unwrap();
        let non_roots: Vec<_> = slots
            .iter()
            .filter(|slot| !accounts_index.is_root(**slot))
            .collect();
        drop(accounts_index);
        let mut storage = self.storage.write().unwrap();
        for slot in non_roots {
            storage.0.remove(&slot);
        }
    }

    pub fn hash_stored_account(slot: Slot, account: &StoredAccount) -> Hash {
        Self::hash_account_data(
            slot,
            account.account_meta.lamports,
            account.account_meta.executable,
            account.account_meta.rent_epoch,
            account.data,
            &account.meta.pubkey,
        )
    }

    pub fn hash_account(slot: Slot, account: &Account, pubkey: &Pubkey) -> Hash {
        Self::hash_account_data(
            slot,
            account.lamports,
            account.executable,
            account.rent_epoch,
            &account.data,
            pubkey,
        )
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
        executable: bool,
        rent_epoch: Epoch,
        data: &[u8],
        pubkey: &Pubkey,
    ) -> Hash {
        if lamports == 0 {
            return Hash::default();
        }

        let mut hasher = Hasher::default();
        let mut buf = [0u8; 8];

        LittleEndian::write_u64(&mut buf[..], lamports);
        hasher.hash(&buf);

        LittleEndian::write_u64(&mut buf[..], slot);
        hasher.hash(&buf);

        LittleEndian::write_u64(&mut buf[..], rent_epoch);
        hasher.hash(&buf);

        hasher.hash(&data);

        if executable {
            hasher.hash(&[1u8; 1]);
        } else {
            hasher.hash(&[0u8; 1]);
        }

        hasher.hash(&pubkey.as_ref());

        hasher.result()
    }

    fn store_accounts(
        &self,
        slot_id: Slot,
        accounts: &[(&Pubkey, &Account)],
        hashes: &[Hash],
    ) -> Vec<AccountInfo> {
        let default_account = Account::default();

        let with_meta: Vec<(StoredMeta, &Account)> = accounts
            .iter()
            .map(|(pubkey, account)| {
                let write_version = self.write_version.fetch_add(1, Ordering::Relaxed) as u64;
                let account = if account.lamports == 0 {
                    &default_account
                } else {
                    *account
                };
                let data_len = account.data.len() as u64;

                let meta = StoredMeta {
                    write_version,
                    pubkey: **pubkey,
                    data_len,
                };
                (meta, account)
            })
            .collect();
        let mut infos: Vec<AccountInfo> = Vec::with_capacity(with_meta.len());
        while infos.len() < with_meta.len() {
            let storage = self.find_storage_candidate(slot_id);
            let rvs = storage
                .accounts
                .append_accounts(&with_meta[infos.len()..], &hashes[infos.len()..]);
            if rvs.is_empty() {
                storage.set_status(AccountStorageStatus::Full);

                // See if an account overflows the default append vec size.
                let data_len = (with_meta[infos.len()].1.data.len() + 4096) as u64;
                if data_len > self.file_size {
                    self.create_and_insert_store(slot_id, data_len * 2);
                }
                continue;
            }
            for (offset, (_, account)) in rvs.iter().zip(&with_meta[infos.len()..]) {
                storage.add_account();
                infos.push(AccountInfo {
                    store_id: storage.id,
                    offset: *offset,
                    lamports: account.lamports,
                });
            }
            // restore the state to available
            storage.set_status(AccountStorageStatus::Available);
        }
        infos
    }

    fn report_store_stats(&self) {
        let mut total_count = 0;
        let mut min = std::usize::MAX;
        let mut min_slot = 0;
        let mut max = 0;
        let mut max_slot = 0;
        let mut newest_slot = 0;
        let mut oldest_slot = std::u64::MAX;
        let stores = self.storage.read().unwrap();
        for (slot, slot_stores) in &stores.0 {
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
        datapoint_info!("accounts_db-stores", ("total_count", total_count, i64));
    }

    pub fn compute_merkle_root(hashes: Vec<(Pubkey, Hash)>, fanout: usize) -> Hash {
        let hashes: Vec<_> = hashes.into_iter().map(|(_pubkey, hash)| hash).collect();
        let mut hashes: Vec<_> = hashes.chunks(fanout).map(|x| x.to_vec()).collect();
        while hashes.len() > 1 {
            let mut time = Measure::start("time");
            let new_hashes: Vec<Hash> = hashes
                .par_iter()
                .map(|h| {
                    let mut hasher = Hasher::default();
                    for v in h.iter() {
                        hasher.hash(v.as_ref());
                    }
                    hasher.result()
                })
                .collect();
            time.stop();
            debug!("hashing {} {}", hashes.len(), time);
            hashes = new_hashes.chunks(fanout).map(|x| x.to_vec()).collect();
        }
        let mut hasher = Hasher::default();
        hashes
            .into_iter()
            .flatten()
            .map(|hash| hash)
            .for_each(|hash| {
                hasher.hash(hash.as_ref());
            });
        hasher.result()
    }

    fn accumulate_account_hashes(mut hashes: Vec<(Pubkey, Hash)>) -> Hash {
        let mut sort = Measure::start("sort");
        hashes.par_sort_by(|a, b| a.0.cmp(&b.0));
        sort.stop();
        let mut hash_time = Measure::start("hash");

        let fanout = 16;

        let res = Self::compute_merkle_root(hashes, fanout);

        hash_time.stop();
        debug!("{} {}", sort, hash_time);

        res
    }

    fn calculate_accounts_hash(
        &self,
        ancestors: &HashMap<Slot, usize>,
        check_hash: bool,
    ) -> Result<Hash, BankHashVerificationError> {
        use BankHashVerificationError::*;
        let mut scan = Measure::start("scan");
        let accounts_index = self.accounts_index.read().unwrap();
        let storage = self.storage.read().unwrap();
        let keys: Vec<_> = accounts_index.account_maps.keys().collect();
        let mismatch_found = AtomicBool::new(false);
        let hashes: Vec<_> = keys
            .par_iter()
            .filter_map(|pubkey| {
                if let Some((list, index)) = accounts_index.get(pubkey, ancestors) {
                    let (slot, account_info) = &list[index];
                    if account_info.lamports != 0 {
                        storage
                            .0
                            .get(&slot)
                            .and_then(|storage_map| storage_map.get(&account_info.store_id))
                            .and_then(|store| {
                                let account = store.accounts.get_account(account_info.offset)?.0;

                                if check_hash {
                                    let hash = Self::hash_stored_account(*slot, &account);
                                    if hash != *account.hash {
                                        mismatch_found.store(true, Ordering::Relaxed);
                                    }
                                    if mismatch_found.load(Ordering::Relaxed) {
                                        return None;
                                    }
                                }

                                Some((**pubkey, *account.hash))
                            })
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();
        if mismatch_found.load(Ordering::Relaxed) {
            return Err(MismatchedAccountHash);
        }

        scan.stop();
        debug!("{}", scan);

        Ok(Self::accumulate_account_hashes(hashes))
    }

    pub fn get_accounts_hash(&self, slot: Slot) -> Hash {
        let bank_hashes = self.bank_hashes.read().unwrap();
        let bank_hash_info = bank_hashes.get(&slot).unwrap();
        bank_hash_info.snapshot_hash
    }

    pub fn update_accounts_hash(&self, slot: Slot, ancestors: &HashMap<Slot, usize>) -> Hash {
        let hash = self.calculate_accounts_hash(ancestors, false).unwrap();
        let mut bank_hashes = self.bank_hashes.write().unwrap();
        let mut bank_hash_info = bank_hashes.get_mut(&slot).unwrap();
        bank_hash_info.snapshot_hash = hash;
        hash
    }

    pub fn verify_bank_hash(
        &self,
        slot: Slot,
        ancestors: &HashMap<Slot, usize>,
    ) -> Result<(), BankHashVerificationError> {
        use BankHashVerificationError::*;

        let calculated_hash = self.calculate_accounts_hash(ancestors, true)?;

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

    pub fn get_accounts_delta_hash(&self, slot_id: Slot) -> Hash {
        let mut scan = Measure::start("scan");
        let mut accumulator: Vec<HashMap<Pubkey, (u64, Hash)>> = self.scan_account_storage(
            slot_id,
            |stored_account: &StoredAccount,
             _store_id: AppendVecId,
             accum: &mut HashMap<Pubkey, (u64, Hash)>| {
                accum.insert(
                    stored_account.meta.pubkey,
                    (stored_account.meta.write_version, *stored_account.hash),
                );
            },
        );
        scan.stop();
        let mut merge = Measure::start("merge");
        let mut account_maps = accumulator.pop().unwrap();
        while let Some(maps) = accumulator.pop() {
            AccountsDB::merge(&mut account_maps, &maps);
        }
        merge.stop();
        let mut accumulate = Measure::start("accumulate");
        let hashes: Vec<_> = account_maps
            .into_iter()
            .map(|(pubkey, (_, hash))| (pubkey, hash))
            .collect();
        let ret = Self::accumulate_account_hashes(hashes);
        accumulate.stop();
        info!("{} {} {}", scan, merge, accumulate);
        ret
    }

    fn update_index(
        &self,
        slot_id: Slot,
        infos: Vec<AccountInfo>,
        accounts: &[(&Pubkey, &Account)],
    ) -> Vec<(Slot, AccountInfo)> {
        let mut reclaims: Vec<(Slot, AccountInfo)> = Vec::with_capacity(infos.len() * 2);
        let index = self.accounts_index.read().unwrap();
        let mut update_index_work = Measure::start("update_index_work");
        let inserts: Vec<_> = infos
            .into_iter()
            .zip(accounts.iter())
            .filter_map(|(info, pubkey_account)| {
                let pubkey = pubkey_account.0;
                index
                    .update(slot_id, pubkey, info, &mut reclaims)
                    .map(|info| (pubkey, info))
            })
            .collect();

        drop(index);
        if !inserts.is_empty() {
            let mut index = self.accounts_index.write().unwrap();
            for (pubkey, info) in inserts {
                index.insert(slot_id, pubkey, info, &mut reclaims);
            }
        }
        update_index_work.stop();
        reclaims
    }

    fn remove_dead_accounts(&self, reclaims: &[(Slot, AccountInfo)]) -> HashSet<Slot> {
        let storage = self.storage.read().unwrap();
        let mut dead_slots = HashSet::new();
        for (slot_id, account_info) in reclaims {
            if let Some(slot_storage) = storage.0.get(slot_id) {
                if let Some(store) = slot_storage.get(&account_info.store_id) {
                    assert_eq!(
                        *slot_id, store.slot_id,
                        "AccountDB::accounts_index corrupted. Storage should only point to one slot"
                    );
                    let count = store.remove_account();
                    if count == 0 {
                        dead_slots.insert(*slot_id);
                    }
                }
            }
        }

        dead_slots.retain(|slot| {
            if let Some(slot_storage) = storage.0.get(&slot) {
                for x in slot_storage.values() {
                    if x.count() != 0 {
                        return false;
                    }
                }
            }
            true
        });

        dead_slots
    }

    pub fn clean_dead_slots(&self, dead_slots: &HashSet<Slot>) {
        if !dead_slots.is_empty() {
            {
                let mut measure = Measure::start("clean_dead_slots-ms");
                let storage = self.storage.read().unwrap();
                let mut stores: Vec<Arc<AccountStorageEntry>> = vec![];
                for slot in dead_slots.iter() {
                    if let Some(slot_storage) = storage.0.get(slot) {
                        for store in slot_storage.values() {
                            stores.push(store.clone());
                        }
                    }
                }
                drop(storage);
                datapoint_info!("clean_dead_slots", ("stores", stores.len(), i64));
                let pubkeys: Vec<Vec<Pubkey>> = {
                    self.thread_pool_clean.install(|| {
                        stores
                            .into_par_iter()
                            .map(|store| {
                                let accounts = store.accounts.accounts(0);
                                accounts
                                    .into_iter()
                                    .map(|account| account.meta.pubkey)
                                    .collect::<Vec<Pubkey>>()
                            })
                            .collect()
                    })
                };
                let index = self.accounts_index.read().unwrap();
                for pubkey_v in pubkeys {
                    for pubkey in pubkey_v {
                        index.unref_from_storage(&pubkey);
                    }
                }
                drop(index);
                measure.stop();
                inc_new_counter_info!("clean_dead_slots-unref-ms", measure.as_ms() as usize);

                let mut index = self.accounts_index.write().unwrap();
                for slot in dead_slots.iter() {
                    index.clean_dead_slot(*slot);
                }
            }
            {
                let mut bank_hashes = self.bank_hashes.write().unwrap();
                for slot in dead_slots.iter() {
                    bank_hashes.remove(slot);
                }
            }
        }
    }

    fn hash_accounts(&self, slot_id: Slot, accounts: &[(&Pubkey, &Account)]) -> Vec<Hash> {
        let mut stats = BankHashStats::default();
        let hashes: Vec<_> = accounts
            .iter()
            .map(|(pubkey, account)| {
                stats.update(account);
                Self::hash_account(slot_id, account, pubkey)
            })
            .collect();

        let mut bank_hashes = self.bank_hashes.write().unwrap();
        let slot_info = bank_hashes
            .entry(slot_id)
            .or_insert_with(BankHashInfo::default);
        slot_info.stats.merge(&stats);

        hashes
    }

    pub fn freeze_accounts(
        &mut self,
        ancestors: &HashMap<Slot, usize>,
        account_pubkeys: &[Pubkey],
    ) {
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

    /// Store the account update.
    pub fn store(&self, slot_id: Slot, accounts: &[(&Pubkey, &Account)]) {
        self.assert_frozen_accounts(accounts);
        let hashes = self.hash_accounts(slot_id, accounts);
        self.store_with_hashes(slot_id, accounts, &hashes);
    }

    fn store_with_hashes(&self, slot_id: Slot, accounts: &[(&Pubkey, &Account)], hashes: &[Hash]) {
        let mut store_accounts = Measure::start("store::store_accounts");
        let infos = self.store_accounts(slot_id, accounts, hashes);
        store_accounts.stop();

        let mut update_index = Measure::start("store::update_index");
        let reclaims = self.update_index(slot_id, infos, accounts);
        update_index.stop();
        trace!("reclaim: {}", reclaims.len());

        self.handle_reclaims(&reclaims);
    }

    pub fn add_root(&self, slot: Slot) {
        self.accounts_index.write().unwrap().add_root(slot)
    }

    pub fn get_snapshot_storages(&self, snapshot_slot: Slot) -> SnapshotStorages {
        let accounts_index = self.accounts_index.read().unwrap();
        let r_storage = self.storage.read().unwrap();
        r_storage
            .0
            .iter()
            .filter(|(slot, _slot_stores)| {
                **slot <= snapshot_slot && accounts_index.is_root(**slot)
            })
            .map(|(_slot, slot_stores)| {
                slot_stores
                    .values()
                    .filter(|x| x.has_accounts())
                    .cloned()
                    .collect()
            })
            .filter(|snapshot_storage: &SnapshotStorage| !snapshot_storage.is_empty())
            .collect()
    }

    fn merge<X>(dest: &mut HashMap<Pubkey, X>, source: &HashMap<Pubkey, X>)
    where
        X: Versioned + Clone,
    {
        for (key, source_item) in source.iter() {
            if let Some(dest_item) = dest.get(key) {
                if dest_item.version() > source_item.version() {
                    continue;
                }
            }
            dest.insert(*key, source_item.clone());
        }
    }

    fn get_io_error(error: &str) -> IOError {
        warn!("AccountsDB error: {:?}", error);
        IOError::new(ErrorKind::Other, error)
    }

    fn generate_index(&self) {
        let storage = self.storage.read().unwrap();
        let mut slots: Vec<Slot> = storage.0.keys().cloned().collect();
        slots.sort();
        let mut accounts_index = self.accounts_index.write().unwrap();
        for slot_id in slots.iter() {
            let accumulator: Vec<HashMap<Pubkey, Vec<(u64, AccountInfo)>>> = self
                .scan_account_storage(
                    *slot_id,
                    |stored_account: &StoredAccount,
                     store_id: AppendVecId,
                     accum: &mut HashMap<Pubkey, Vec<(u64, AccountInfo)>>| {
                        let account_info = AccountInfo {
                            store_id,
                            offset: stored_account.offset,
                            lamports: stored_account.account_meta.lamports,
                        };
                        let entry = accum
                            .entry(stored_account.meta.pubkey)
                            .or_insert_with(|| vec![]);
                        entry.push((stored_account.meta.write_version, account_info));
                    },
                );

            let mut accounts_map: HashMap<Pubkey, Vec<(u64, AccountInfo)>> = HashMap::new();
            for accumulator_entry in accumulator.iter() {
                for (pubkey, storage_entry) in accumulator_entry {
                    let entry = accounts_map.entry(*pubkey).or_insert_with(|| vec![]);
                    entry.extend(storage_entry.iter().cloned());
                }
            }

            // Need to restore indexes even with older write versions which may
            // be shielding other accounts. When they are then purged, the
            // original non-shielded account value will be visible when the account
            // is restored from the append-vec
            if !accumulator.is_empty() {
                let mut _reclaims: Vec<(u64, AccountInfo)> = vec![];
                for (pubkey, account_infos) in accounts_map.iter_mut() {
                    account_infos.sort_by(|a, b| a.0.cmp(&b.0));
                    for (_, account_info) in account_infos {
                        accounts_index.insert(
                            *slot_id,
                            pubkey,
                            account_info.clone(),
                            &mut _reclaims,
                        );
                    }
                }
            }
        }
        // Need to add these last, otherwise older updates will be cleaned
        for slot_id in slots {
            accounts_index.add_root(slot_id);
        }

        let mut counts = HashMap::new();
        for slot_list in accounts_index.account_maps.values() {
            for (_slot, account_entry) in slot_list.1.read().unwrap().iter() {
                *counts.entry(account_entry.store_id).or_insert(0) += 1;
            }
        }
        for slot_stores in storage.0.values() {
            for (id, store) in slot_stores {
                if let Some(count) = counts.get(&id) {
                    trace!(
                        "id: {} setting count: {} cur: {}",
                        id,
                        count,
                        store.count_and_status.read().unwrap().0
                    );
                    store.count_and_status.write().unwrap().0 = *count;
                } else {
                    trace!("id: {} clearing count", id);
                    store.count_and_status.write().unwrap().0 = 0;
                }
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    // TODO: all the bank tests are bank specific, issue: 2194
    use super::*;
    use crate::accounts_index::RefCount;
    use crate::append_vec::AccountMeta;
    use assert_matches::assert_matches;
    use bincode::serialize_into;
    use rand::{thread_rng, Rng};
    use solana_sdk::{account::Account, hash::HASH_BYTES};
    use std::{fs, str::FromStr};
    use tempfile::TempDir;

    fn linear_ancestors(end_slot: u64) -> HashMap<Slot, usize> {
        let mut ancestors: HashMap<Slot, usize> = vec![(0, 0)].into_iter().collect();
        for i in 1..end_slot {
            ancestors.insert(i, (i - 1) as usize);
        }
        ancestors
    }

    #[test]
    fn test_accountsdb_add_root() {
        solana_logger::setup();
        let db = AccountsDB::new(Vec::new());
        let key = Pubkey::default();
        let account0 = Account::new(1, 0, &key);

        db.store(0, &[(&key, &account0)]);
        db.add_root(0);
        let ancestors = vec![(1, 1)].into_iter().collect();
        assert_eq!(db.load_slow(&ancestors, &key), Some((account0, 0)));
    }

    #[test]
    fn test_accountsdb_latest_ancestor() {
        solana_logger::setup();
        let db = AccountsDB::new(Vec::new());
        let key = Pubkey::default();
        let account0 = Account::new(1, 0, &key);

        db.store(0, &[(&key, &account0)]);

        let account1 = Account::new(0, 0, &key);
        db.store(1, &[(&key, &account1)]);

        let ancestors = vec![(1, 1)].into_iter().collect();
        assert_eq!(&db.load_slow(&ancestors, &key).unwrap().0, &account1);

        let ancestors = vec![(1, 1), (0, 0)].into_iter().collect();
        assert_eq!(&db.load_slow(&ancestors, &key).unwrap().0, &account1);

        let accounts: Vec<Account> =
            db.scan_accounts(&ancestors, |accounts: &mut Vec<Account>, option| {
                if let Some(data) = option {
                    accounts.push(data.1);
                }
            });
        assert_eq!(accounts, vec![account1]);
    }

    #[test]
    fn test_accountsdb_latest_ancestor_with_root() {
        solana_logger::setup();
        let db = AccountsDB::new(Vec::new());
        let key = Pubkey::default();
        let account0 = Account::new(1, 0, &key);

        db.store(0, &[(&key, &account0)]);

        let account1 = Account::new(0, 0, &key);
        db.store(1, &[(&key, &account1)]);
        db.add_root(0);

        let ancestors = vec![(1, 1)].into_iter().collect();
        assert_eq!(&db.load_slow(&ancestors, &key).unwrap().0, &account1);

        let ancestors = vec![(1, 1), (0, 0)].into_iter().collect();
        assert_eq!(&db.load_slow(&ancestors, &key).unwrap().0, &account1);
    }

    #[test]
    fn test_accountsdb_root_one_slot() {
        solana_logger::setup();
        let db = AccountsDB::new(Vec::new());

        let key = Pubkey::default();
        let account0 = Account::new(1, 0, &key);

        // store value 1 in the "root", i.e. db zero
        db.store(0, &[(&key, &account0)]);

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
        db.store(1, &[(&key, &account1)]);

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
        let db = AccountsDB::new(Vec::new());

        let mut pubkeys: Vec<Pubkey> = vec![];
        create_account(&db, &mut pubkeys, 0, 100, 0, 0);
        for _ in 1..100 {
            let idx = thread_rng().gen_range(0, 99);
            let ancestors = vec![(0, 0)].into_iter().collect();
            let account = db.load_slow(&ancestors, &pubkeys[idx]).unwrap();
            let mut default_account = Account::default();
            default_account.lamports = (idx + 1) as u64;
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
            let mut default_account = Account::default();
            default_account.lamports = (idx + 1) as u64;
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

        let pubkey = Pubkey::new_rand();
        let account = Account::new(1, DEFAULT_FILE_SIZE as usize / 3, &pubkey);
        db.store(1, &[(&pubkey, &account)]);
        db.store(1, &[(&pubkeys[0], &account)]);
        {
            let stores = db.storage.read().unwrap();
            let slot_0_stores = &stores.0.get(&0).unwrap();
            let slot_1_stores = &stores.0.get(&1).unwrap();
            assert_eq!(slot_0_stores.len(), 1);
            assert_eq!(slot_1_stores.len(), 1);
            assert_eq!(slot_0_stores[&0].count(), 2);
            assert_eq!(slot_1_stores[&1].count(), 2);
        }
        db.add_root(1);
        {
            let stores = db.storage.read().unwrap();
            let slot_0_stores = &stores.0.get(&0).unwrap();
            let slot_1_stores = &stores.0.get(&1).unwrap();
            assert_eq!(slot_0_stores.len(), 1);
            assert_eq!(slot_1_stores.len(), 1);
            assert_eq!(slot_0_stores[&0].count(), 2);
            assert_eq!(slot_1_stores[&1].count(), 2);
        }
    }

    #[test]
    fn test_accounts_unsquashed() {
        let key = Pubkey::default();

        // 1 token in the "root", i.e. db zero
        let db0 = AccountsDB::new(Vec::new());
        let account0 = Account::new(1, 0, &key);
        db0.store(0, &[(&key, &account0)]);

        // 0 lamports in the child
        let account1 = Account::new(0, 0, &key);
        db0.store(1, &[(&key, &account1)]);

        // masking accounts is done at the Accounts level, at accountsDB we see
        // original account
        let ancestors = vec![(0, 0), (1, 1)].into_iter().collect();
        assert_eq!(db0.load_slow(&ancestors, &key), Some((account1, 1)));
        let ancestors = vec![(0, 0)].into_iter().collect();
        assert_eq!(db0.load_slow(&ancestors, &key), Some((account0, 0)));
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
            let pubkey = Pubkey::new_rand();
            let account = Account::new((t + 1) as u64, space, &Account::default().owner);
            pubkeys.push(pubkey.clone());
            assert!(accounts.load_slow(&ancestors, &pubkey).is_none());
            accounts.store(slot, &[(&pubkey, &account)]);
        }
        for t in 0..num_vote {
            let pubkey = Pubkey::new_rand();
            let account = Account::new((num + t + 1) as u64, space, &solana_vote_program::id());
            pubkeys.push(pubkey.clone());
            let ancestors = vec![(slot, 0)].into_iter().collect();
            assert!(accounts.load_slow(&ancestors, &pubkey).is_none());
            accounts.store(slot, &[(&pubkey, &account)]);
        }
    }

    fn update_accounts(accounts: &AccountsDB, pubkeys: &Vec<Pubkey>, slot: Slot, range: usize) {
        for _ in 1..1000 {
            let idx = thread_rng().gen_range(0, range);
            let ancestors = vec![(slot, 0)].into_iter().collect();
            if let Some((mut account, _)) = accounts.load_slow(&ancestors, &pubkeys[idx]) {
                account.lamports = account.lamports + 1;
                accounts.store(slot, &[(&pubkeys[idx], &account)]);
                if account.lamports == 0 {
                    let ancestors = vec![(slot, 0)].into_iter().collect();
                    assert!(accounts.load_slow(&ancestors, &pubkeys[idx]).is_none());
                } else {
                    let mut default_account = Account::default();
                    default_account.lamports = account.lamports;
                    assert_eq!(default_account, account);
                }
            }
        }
    }

    fn check_storage(accounts: &AccountsDB, slot: Slot, count: usize) -> bool {
        let storage = accounts.storage.read().unwrap();
        assert_eq!(storage.0[&slot].len(), 1);
        let slot_storage = storage.0.get(&slot).unwrap();
        let mut total_count: usize = 0;
        for store in slot_storage.values() {
            assert_eq!(store.status(), AccountStorageStatus::Available);
            total_count += store.count();
        }
        assert_eq!(total_count, count);
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

    fn modify_accounts(
        accounts: &AccountsDB,
        pubkeys: &Vec<Pubkey>,
        slot: Slot,
        num: usize,
        count: usize,
    ) {
        for idx in 0..num {
            let account = Account::new((idx + count) as u64, 0, &Account::default().owner);
            accounts.store(slot, &[(&pubkeys[idx], &account)]);
        }
    }

    #[test]
    fn test_account_one() {
        let (_accounts_dirs, paths) = get_temp_accounts_paths(1).unwrap();
        let db = AccountsDB::new(paths);
        let mut pubkeys: Vec<Pubkey> = vec![];
        create_account(&db, &mut pubkeys, 0, 1, 0, 0);
        let ancestors = vec![(0, 0)].into_iter().collect();
        let account = db.load_slow(&ancestors, &pubkeys[0]).unwrap();
        let mut default_account = Account::default();
        default_account.lamports = 1;
        assert_eq!((default_account, 0), account);
    }

    #[test]
    fn test_account_many() {
        let (_accounts_dirs, paths) = get_temp_accounts_paths(2).unwrap();
        let db = AccountsDB::new(paths);
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
            let key = Pubkey::new_rand();
            let account = Account::new(i + 1, size as usize / 4, &key);
            accounts.store(0, &[(&key, &account)]);
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
        for storage in accounts
            .storage
            .read()
            .unwrap()
            .0
            .values()
            .flat_map(|x| x.values())
        {
            *append_vec_histogram.entry(storage.slot_id).or_insert(0) += 1;
        }
        for count in append_vec_histogram.values() {
            assert!(*count >= 2);
        }
    }

    #[test]
    fn test_account_grow() {
        let accounts = AccountsDB::new_single();

        let count = [0, 1];
        let status = [AccountStorageStatus::Available, AccountStorageStatus::Full];
        let pubkey1 = Pubkey::new_rand();
        let account1 = Account::new(1, DEFAULT_FILE_SIZE as usize / 2, &pubkey1);
        accounts.store(0, &[(&pubkey1, &account1)]);
        {
            let stores = accounts.storage.read().unwrap();
            assert_eq!(stores.0.len(), 1);
            assert_eq!(stores.0[&0][&0].count(), 1);
            assert_eq!(stores.0[&0][&0].status(), AccountStorageStatus::Available);
        }

        let pubkey2 = Pubkey::new_rand();
        let account2 = Account::new(1, DEFAULT_FILE_SIZE as usize / 2, &pubkey2);
        accounts.store(0, &[(&pubkey2, &account2)]);
        {
            let stores = accounts.storage.read().unwrap();
            assert_eq!(stores.0.len(), 1);
            assert_eq!(stores.0[&0].len(), 2);
            assert_eq!(stores.0[&0][&0].count(), 1);
            assert_eq!(stores.0[&0][&0].status(), AccountStorageStatus::Full);
            assert_eq!(stores.0[&0][&1].count(), 1);
            assert_eq!(stores.0[&0][&1].status(), AccountStorageStatus::Available);
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
        for i in 0..25 {
            let index = i % 2;
            accounts.store(0, &[(&pubkey1, &account1)]);
            {
                let stores = accounts.storage.read().unwrap();
                assert_eq!(stores.0.len(), 1);
                assert_eq!(stores.0[&0].len(), 3);
                assert_eq!(stores.0[&0][&0].count(), count[index]);
                assert_eq!(stores.0[&0][&0].status(), status[0]);
                assert_eq!(stores.0[&0][&1].count(), 1);
                assert_eq!(stores.0[&0][&1].status(), status[1]);
                assert_eq!(stores.0[&0][&2].count(), count[index ^ 1]);
                assert_eq!(stores.0[&0][&2].status(), status[0]);
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
    fn test_purge_slot_not_root() {
        let accounts = AccountsDB::new(Vec::new());
        let mut pubkeys: Vec<Pubkey> = vec![];
        create_account(&accounts, &mut pubkeys, 0, 1, 0, 0);
        let ancestors = vec![(0, 0)].into_iter().collect();
        assert!(accounts.load_slow(&ancestors, &pubkeys[0]).is_some());
        accounts.purge_slot(0);
        assert!(accounts.load_slow(&ancestors, &pubkeys[0]).is_none());
    }

    #[test]
    fn test_purge_slot_after_root() {
        let accounts = AccountsDB::new(Vec::new());
        let mut pubkeys: Vec<Pubkey> = vec![];
        create_account(&accounts, &mut pubkeys, 0, 1, 0, 0);
        let ancestors = vec![(0, 0)].into_iter().collect();
        accounts.add_root(0);
        accounts.purge_slot(0);
        assert!(accounts.load_slow(&ancestors, &pubkeys[0]).is_some());
    }

    #[test]
    fn test_lazy_gc_slot() {
        solana_logger::setup();
        //This test is pedantic
        //A slot is purged when a non root bank is cleaned up.  If a slot is behind root but it is
        //not root, it means we are retaining dead banks.
        let accounts = AccountsDB::new(Vec::new());
        let pubkey = Pubkey::new_rand();
        let account = Account::new(1, 0, &Account::default().owner);
        //store an account
        accounts.store(0, &[(&pubkey, &account)]);
        let ancestors = vec![(0, 0)].into_iter().collect();
        let id = {
            let index = accounts.accounts_index.read().unwrap();
            let (list, idx) = index.get(&pubkey, &ancestors).unwrap();
            list[idx].1.store_id
        };
        accounts.add_root(1);

        //slot is still there, since gc is lazy
        assert!(accounts.storage.read().unwrap().0[&0].get(&id).is_some());

        //store causes clean
        accounts.store(1, &[(&pubkey, &account)]);

        //slot is gone
        print_accounts("pre-clean", &accounts);
        accounts.clean_accounts();
        accounts.process_dead_slots();
        assert!(accounts.storage.read().unwrap().0.get(&0).is_none());

        //new value is there
        let ancestors = vec![(1, 1)].into_iter().collect();
        assert_eq!(accounts.load_slow(&ancestors, &pubkey), Some((account, 1)));
    }

    impl AccountsDB {
        fn store_count_for_slot(&self, slot: Slot) -> usize {
            let storage = self.storage.read().unwrap();

            let slot_storage = storage.0.get(&slot);
            if let Some(slot_storage) = slot_storage {
                slot_storage.values().nth(0).unwrap().count()
            } else {
                0
            }
        }

        fn ref_count_for_pubkey(&self, pubkey: &Pubkey) -> RefCount {
            self.accounts_index
                .read()
                .unwrap()
                .ref_count_from_storage(&pubkey)
        }

        fn uncleaned_root_count(&self) -> usize {
            self.accounts_index.read().unwrap().uncleaned_roots.len()
        }
    }

    #[test]
    fn test_clean_old_with_normal_account() {
        solana_logger::setup();

        let accounts = AccountsDB::new(Vec::new());
        let pubkey = Pubkey::new_rand();
        let account = Account::new(1, 0, &Account::default().owner);
        //store an account
        accounts.store(0, &[(&pubkey, &account)]);
        accounts.store(1, &[(&pubkey, &account)]);

        // simulate slots are rooted after while
        accounts.add_root(0);
        accounts.add_root(1);

        //even if rooted, old state isn't cleaned up
        assert_eq!(accounts.store_count_for_slot(0), 1);
        assert_eq!(accounts.store_count_for_slot(1), 1);

        accounts.clean_accounts();

        //now old state is cleaned up
        assert_eq!(accounts.store_count_for_slot(0), 0);
        assert_eq!(accounts.store_count_for_slot(1), 1);
    }

    #[test]
    fn test_clean_old_with_zero_lamport_account() {
        solana_logger::setup();

        let accounts = AccountsDB::new(Vec::new());
        let pubkey1 = Pubkey::new_rand();
        let pubkey2 = Pubkey::new_rand();
        let normal_account = Account::new(1, 0, &Account::default().owner);
        let zero_account = Account::new(0, 0, &Account::default().owner);
        //store an account
        accounts.store(0, &[(&pubkey1, &normal_account)]);
        accounts.store(1, &[(&pubkey1, &zero_account)]);
        accounts.store(0, &[(&pubkey2, &normal_account)]);
        accounts.store(1, &[(&pubkey2, &normal_account)]);

        //simulate slots are rooted after while
        accounts.add_root(0);
        accounts.add_root(1);

        //even if rooted, old state isn't cleaned up
        assert_eq!(accounts.store_count_for_slot(0), 2);
        assert_eq!(accounts.store_count_for_slot(1), 2);

        accounts.clean_accounts();

        //still old state behind zero-lamport account isn't cleaned up
        assert_eq!(accounts.store_count_for_slot(0), 1);
        assert_eq!(accounts.store_count_for_slot(1), 2);
    }

    #[test]
    fn test_clean_old_with_both_normal_and_zero_lamport_accounts() {
        solana_logger::setup();

        let accounts = AccountsDB::new(Vec::new());
        let pubkey1 = Pubkey::new_rand();
        let pubkey2 = Pubkey::new_rand();
        let normal_account = Account::new(1, 0, &Account::default().owner);
        let zero_account = Account::new(0, 0, &Account::default().owner);
        //store an account
        accounts.store(0, &[(&pubkey1, &normal_account)]);
        accounts.store(1, &[(&pubkey1, &zero_account)]);
        accounts.store(0, &[(&pubkey2, &normal_account)]);
        accounts.store(2, &[(&pubkey2, &normal_account)]);

        //simulate slots are rooted after while
        accounts.add_root(0);
        accounts.add_root(1);
        accounts.add_root(2);

        //even if rooted, old state isn't cleaned up
        assert_eq!(accounts.store_count_for_slot(0), 2);
        assert_eq!(accounts.store_count_for_slot(1), 1);
        assert_eq!(accounts.store_count_for_slot(2), 1);

        accounts.clean_accounts();

        //both zero lamport and normal accounts are cleaned up
        assert_eq!(accounts.store_count_for_slot(0), 0);
        assert_eq!(accounts.store_count_for_slot(1), 0);
        assert_eq!(accounts.store_count_for_slot(2), 1);
    }

    #[test]
    fn test_uncleaned_roots_with_account() {
        solana_logger::setup();

        let accounts = AccountsDB::new(Vec::new());
        let pubkey = Pubkey::new_rand();
        let account = Account::new(1, 0, &Account::default().owner);
        //store an account
        accounts.store(0, &[(&pubkey, &account)]);
        assert_eq!(accounts.uncleaned_root_count(), 0);

        // simulate slots are rooted after while
        accounts.add_root(0);
        assert_eq!(accounts.uncleaned_root_count(), 1);

        //now uncleaned roots are cleaned up
        accounts.clean_accounts();
        assert_eq!(accounts.uncleaned_root_count(), 0);
    }

    #[test]
    fn test_uncleaned_roots_with_no_account() {
        solana_logger::setup();

        let accounts = AccountsDB::new(Vec::new());

        assert_eq!(accounts.uncleaned_root_count(), 0);

        // simulate slots are rooted after while
        accounts.add_root(0);
        assert_eq!(accounts.uncleaned_root_count(), 1);

        //now uncleaned roots are cleaned up
        accounts.clean_accounts();
        assert_eq!(accounts.uncleaned_root_count(), 0);
    }

    fn print_accounts(label: &'static str, accounts: &AccountsDB) {
        print_index(label, accounts);
        print_count_and_status(label, accounts);
    }

    fn print_index(label: &'static str, accounts: &AccountsDB) {
        let mut roots: Vec<_> = accounts
            .accounts_index
            .read()
            .unwrap()
            .roots
            .iter()
            .cloned()
            .collect();
        roots.sort();
        info!("{}: accounts.accounts_index roots: {:?}", label, roots,);
        for (pubkey, list) in &accounts.accounts_index.read().unwrap().account_maps {
            info!("  key: {}", pubkey);
            info!("      slots: {:?}", *list.1.read().unwrap());
        }
    }

    fn print_count_and_status(label: &'static str, accounts: &AccountsDB) {
        let storage = accounts.storage.read().unwrap();
        let mut slots: Vec<_> = storage.0.keys().cloned().collect();
        slots.sort();
        info!("{}: count_and status for {} slots:", label, slots.len());
        for slot in &slots {
            let slot_stores = storage.0.get(slot).unwrap();

            let mut ids: Vec<_> = slot_stores.keys().cloned().collect();
            ids.sort();
            for id in &ids {
                let entry = slot_stores.get(id).unwrap();
                info!(
                    "  slot: {} id: {} count_and_status: {:?}",
                    slot,
                    id,
                    *entry.count_and_status.read().unwrap()
                );
            }
        }
    }

    #[test]
    fn test_accounts_db_serialize1() {
        solana_logger::setup();
        let accounts = AccountsDB::new_single();
        let mut pubkeys: Vec<Pubkey> = vec![];

        // Create 100 accounts in slot 0
        create_account(&accounts, &mut pubkeys, 0, 100, 0, 0);
        assert_eq!(check_storage(&accounts, 0, 100), true);
        check_accounts(&accounts, &pubkeys, 0, 100, 1);

        // do some updates to those accounts and re-check
        modify_accounts(&accounts, &pubkeys, 0, 100, 2);
        check_accounts(&accounts, &pubkeys, 0, 100, 2);
        accounts.add_root(0);

        let mut pubkeys1: Vec<Pubkey> = vec![];
        let latest_slot = 1;

        // Modify the first 10 of the slot 0 accounts as updates in slot 1
        modify_accounts(&accounts, &pubkeys, latest_slot, 10, 3);

        // Create 10 new accounts in slot 1
        create_account(&accounts, &mut pubkeys1, latest_slot, 10, 0, 0);

        // Store a lamports=0 account in slot 1
        let account = Account::new(0, 0, &Account::default().owner);
        accounts.store(latest_slot, &[(&pubkeys[30], &account)]);
        accounts.add_root(latest_slot);
        info!("added root 1");

        let latest_slot = 2;
        let mut pubkeys2: Vec<Pubkey> = vec![];
        // Modify original slot 0 accounts in slot 2
        modify_accounts(&accounts, &pubkeys, latest_slot, 20, 4);

        // Create 10 new accounts in slot 2
        create_account(&accounts, &mut pubkeys2, latest_slot, 10, 0, 0);

        // Store a lamports=0 account in slot 2
        let account = Account::new(0, 0, &Account::default().owner);
        accounts.store(latest_slot, &[(&pubkeys[31], &account)]);
        accounts.add_root(latest_slot);

        assert!(check_storage(&accounts, 0, 90));
        assert!(check_storage(&accounts, 1, 21));
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

        print_count_and_status("daccounts", &daccounts);

        // Don't check the first 35 accounts which have not been modified on slot 0
        check_accounts(&daccounts, &pubkeys[35..], 0, 65, 37);
        check_accounts(&daccounts, &pubkeys1, 1, 10, 1);
        assert!(check_storage(&daccounts, 0, 100));
        assert!(check_storage(&daccounts, 1, 21));
        assert!(check_storage(&daccounts, 2, 31));

        let ancestors = linear_ancestors(latest_slot);
        assert_eq!(
            daccounts.update_accounts_hash(latest_slot, &ancestors),
            accounts.update_accounts_hash(latest_slot, &ancestors)
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
        let mut writer = Cursor::new(vec![]);
        let snapshot_storages = accounts.get_snapshot_storages(slot);
        serialize_into(
            &mut writer,
            &AccountsDBSerialize::new(&accounts, slot, &snapshot_storages),
        )
        .unwrap();

        let buf = writer.into_inner();
        let mut reader = BufReader::new(&buf[..]);
        let daccounts = AccountsDB::new(Vec::new());
        let copied_accounts = TempDir::new().unwrap();
        // Simulate obtaining a copy of the AppendVecs from a tarball
        copy_append_vecs(&accounts, copied_accounts.path()).unwrap();
        daccounts
            .accounts_from_stream(&mut reader, copied_accounts.path())
            .unwrap();

        print_count_and_status("daccounts", &daccounts);

        daccounts
    }

    fn assert_no_stores(accounts: &AccountsDB, slot: Slot) {
        let stores = accounts.storage.read().unwrap();
        info!("{:?}", stores.0.get(&slot));
        assert!(stores.0.get(&slot).is_none() || stores.0.get(&slot).unwrap().len() == 0);
    }

    #[test]
    fn test_accounts_db_purge_keep_live() {
        solana_logger::setup();
        let some_lamport = 223;
        let zero_lamport = 0;
        let no_data = 0;
        let owner = Account::default().owner;

        let account = Account::new(some_lamport, no_data, &owner);
        let pubkey = Pubkey::new_rand();

        let account2 = Account::new(some_lamport, no_data, &owner);
        let pubkey2 = Pubkey::new_rand();

        let zero_lamport_account = Account::new(zero_lamport, no_data, &owner);

        let accounts = AccountsDB::new_single();
        accounts.add_root(0);

        let mut current_slot = 1;
        accounts.store(current_slot, &[(&pubkey, &account)]);

        // Store another live account to slot 1 which will prevent any purge
        // since the store count will not be zero
        accounts.store(current_slot, &[(&pubkey2, &account2)]);
        accounts.add_root(current_slot);

        current_slot += 1;
        accounts.store(current_slot, &[(&pubkey, &zero_lamport_account)]);
        accounts.add_root(current_slot);

        assert_load_account(&accounts, current_slot, pubkey, zero_lamport);

        current_slot += 1;
        accounts.add_root(current_slot);

        print_accounts("pre_purge", &accounts);

        accounts.clean_accounts();

        print_accounts("post_purge", &accounts);

        // Make sure the index is not touched
        assert_eq!(
            accounts
                .accounts_index
                .read()
                .unwrap()
                .account_maps
                .get(&pubkey)
                .unwrap()
                .1
                .read()
                .unwrap()
                .len(),
            2
        );

        // slot 1 & 2 should have stores
        check_storage(&accounts, 1, 2);
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
        let pubkey = Pubkey::new_rand();

        let zero_lamport_account = Account::new(zero_lamport, no_data, &owner);

        let accounts = AccountsDB::new_single();
        accounts.add_root(0);

        let mut current_slot = 1;
        accounts.set_hash(current_slot, current_slot - 1);
        accounts.store(current_slot, &[(&pubkey, &account)]);
        accounts.add_root(current_slot);

        current_slot += 1;
        accounts.set_hash(current_slot, current_slot - 1);
        accounts.store(current_slot, &[(&pubkey, &zero_lamport_account)]);
        accounts.add_root(current_slot);

        assert_load_account(&accounts, current_slot, pubkey, zero_lamport);

        // Otherwise slot 2 will not be removed
        current_slot += 1;
        accounts.set_hash(current_slot, current_slot - 1);
        accounts.add_root(current_slot);

        print_accounts("pre_purge", &accounts);

        let ancestors = linear_ancestors(current_slot);
        info!("ancestors: {:?}", ancestors);
        let hash = accounts.update_accounts_hash(current_slot, &ancestors);

        accounts.clean_accounts();
        accounts.process_dead_slots();

        assert_eq!(
            accounts.update_accounts_hash(current_slot, &ancestors),
            hash
        );

        print_accounts("post_purge", &accounts);

        // Make sure the index is for pubkey cleared
        assert!(accounts
            .accounts_index
            .read()
            .unwrap()
            .account_maps
            .get(&pubkey)
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
        let pubkey = Pubkey::new_rand();
        let zero_lamport_account = Account::new(zero_lamport, no_data, &owner);

        let account2 = Account::new(some_lamport + 1, no_data, &owner);
        let pubkey2 = Pubkey::new_rand();

        let filler_account = Account::new(some_lamport, no_data, &owner);
        let filler_account_pubkey = Pubkey::new_rand();

        let accounts = AccountsDB::new_single();

        let mut current_slot = 1;
        accounts.store(current_slot, &[(&pubkey, &account)]);
        accounts.add_root(current_slot);

        current_slot += 1;
        accounts.store(current_slot, &[(&pubkey, &zero_lamport_account)]);
        accounts.store(current_slot, &[(&pubkey2, &account2)]);

        // Store enough accounts such that an additional store for slot 2 is created.
        while accounts
            .storage
            .read()
            .unwrap()
            .0
            .get(&current_slot)
            .unwrap()
            .len()
            < 2
        {
            accounts.store(current_slot, &[(&filler_account_pubkey, &filler_account)]);
        }
        accounts.add_root(current_slot);

        assert_load_account(&accounts, current_slot, pubkey, zero_lamport);

        print_accounts("accounts", &accounts);

        accounts.clean_accounts();

        print_accounts("accounts_post_purge", &accounts);
        let accounts = reconstruct_accounts_db_via_serialization(&accounts, current_slot);

        print_accounts("reconstructed", &accounts);

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

        let pubkey = Pubkey::new_rand();
        let purged_pubkey1 = Pubkey::new_rand();
        let purged_pubkey2 = Pubkey::new_rand();

        let dummy_account = Account::new(dummy_lamport, no_data, &owner);
        let dummy_pubkey = Pubkey::default();

        let accounts = AccountsDB::new_single();

        let mut current_slot = 1;
        accounts.store(current_slot, &[(&pubkey, &account)]);
        accounts.store(current_slot, &[(&purged_pubkey1, &account2)]);
        accounts.add_root(current_slot);

        current_slot += 1;
        accounts.store(current_slot, &[(&purged_pubkey1, &zero_lamport_account)]);
        accounts.store(current_slot, &[(&purged_pubkey2, &account3)]);
        accounts.add_root(current_slot);

        current_slot += 1;
        accounts.store(current_slot, &[(&purged_pubkey2, &zero_lamport_account)]);
        accounts.add_root(current_slot);

        current_slot += 1;
        accounts.store(current_slot, &[(&dummy_pubkey, &dummy_account)]);
        accounts.add_root(current_slot);

        print_accounts("pre_f", &accounts);
        accounts.update_accounts_hash(4, &HashMap::default());

        let accounts = f(accounts, current_slot);

        print_accounts("post_f", &accounts);

        assert_load_account(&accounts, current_slot, pubkey, some_lamport);
        assert_load_account(&accounts, current_slot, purged_pubkey1, 0);
        assert_load_account(&accounts, current_slot, purged_pubkey2, 0);
        assert_load_account(&accounts, current_slot, dummy_pubkey, dummy_lamport);

        accounts.verify_bank_hash(4, &HashMap::default()).unwrap();
    }

    #[test]
    fn test_accounts_purge_chained_purge_before_snapshot_restore() {
        solana_logger::setup();
        with_chained_zero_lamport_accounts(|accounts, current_slot| {
            accounts.clean_accounts();
            let accounts = reconstruct_accounts_db_via_serialization(&accounts, current_slot);
            accounts
        });
    }

    #[test]
    fn test_accounts_purge_chained_purge_after_snapshot_restore() {
        solana_logger::setup();
        with_chained_zero_lamport_accounts(|accounts, current_slot| {
            let accounts = reconstruct_accounts_db_via_serialization(&accounts, current_slot);
            print_accounts("after_reconstruct", &accounts);
            accounts.clean_accounts();
            let accounts = reconstruct_accounts_db_via_serialization(&accounts, current_slot);
            accounts
        });
    }

    #[test]
    #[ignore]
    fn test_store_account_stress() {
        let slot_id = 42;
        let num_threads = 2;

        let min_file_bytes = std::mem::size_of::<StoredMeta>()
            + std::mem::size_of::<crate::append_vec::AccountMeta>();

        let db = Arc::new(AccountsDB::new_sized(Vec::new(), min_file_bytes as u64));

        db.add_root(slot_id);
        let thread_hdls: Vec<_> = (0..num_threads)
            .into_iter()
            .map(|_| {
                let db = db.clone();
                std::thread::Builder::new()
                    .name("account-writers".to_string())
                    .spawn(move || {
                        let pubkey = Pubkey::new_rand();
                        let mut account = Account::new(1, 0, &pubkey);
                        let mut i = 0;
                        loop {
                            let account_bal = thread_rng().gen_range(1, 99);
                            account.lamports = account_bal;
                            db.store(slot_id, &[(&pubkey, &account)]);
                            let (account, slot) = db.load_slow(&HashMap::new(), &pubkey).expect(
                                &format!("Could not fetch stored account {}, iter {}", pubkey, i),
                            );
                            assert_eq!(slot, slot_id);
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
        let db = AccountsDB::new(Vec::new());
        let key = Pubkey::default();
        let key0 = Pubkey::new_rand();
        let account0 = Account::new(1, 0, &key);

        db.store(0, &[(&key0, &account0)]);

        let key1 = Pubkey::new_rand();
        let account1 = Account::new(2, 0, &key);
        db.store(1, &[(&key1, &account1)]);

        let ancestors = vec![(0, 0)].into_iter().collect();
        let accounts: Vec<Account> =
            db.scan_accounts(&ancestors, |accounts: &mut Vec<Account>, option| {
                if let Some(data) = option {
                    accounts.push(data.1);
                }
            });
        assert_eq!(accounts, vec![account0]);

        let ancestors = vec![(1, 1), (0, 0)].into_iter().collect();
        let accounts: Vec<Account> =
            db.scan_accounts(&ancestors, |accounts: &mut Vec<Account>, option| {
                if let Some(data) = option {
                    accounts.push(data.1);
                }
            });
        assert_eq!(accounts.len(), 2);
    }

    #[test]
    fn test_store_large_account() {
        solana_logger::setup();
        let db = AccountsDB::new(Vec::new());

        let key = Pubkey::default();
        let data_len = DEFAULT_FILE_SIZE as usize + 7;
        let account = Account::new(1, data_len, &key);

        db.store(0, &[(&key, &account)]);

        let ancestors = vec![(0, 0)].into_iter().collect();
        let ret = db.load_slow(&ancestors, &key).unwrap();
        assert_eq!(ret.0.data.len(), data_len);
    }

    pub fn copy_append_vecs<P: AsRef<Path>>(
        accounts_db: &AccountsDB,
        output_dir: P,
    ) -> IOResult<()> {
        let storage_entries = accounts_db.get_snapshot_storages(Slot::max_value());
        for storage in storage_entries.iter().flatten() {
            let storage_path = storage.get_path();
            let output_path = output_dir.as_ref().join(
                storage_path
                    .file_name()
                    .expect("Invalid AppendVec file path"),
            );

            fs::copy(storage_path, output_path)?;
        }

        Ok(())
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
        let mut account_modified = account.clone();
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
        let mut db = AccountsDB::new(Vec::new());

        let mut account = Account::new(1, 42, &frozen_pubkey);
        db.store(0, &[(&frozen_pubkey, &account)]);

        let ancestors = vec![(0, 0)].into_iter().collect();
        db.freeze_accounts(&ancestors, &[frozen_pubkey]);

        // Store with no account changes is ok
        db.store(0, &[(&frozen_pubkey, &account)]);

        // Store with an increase in lamports is ok
        account.lamports = 2;
        db.store(0, &[(&frozen_pubkey, &account)]);

        // Store with an decrease that does not go below the frozen amount of lamports is tolerated
        account.lamports = 1;
        db.store(0, &[(&frozen_pubkey, &account)]);

        // A store of any value over the frozen value of '1' across different slots is also ok
        account.lamports = 3;
        db.store(1, &[(&frozen_pubkey, &account)]);
        account.lamports = 2;
        db.store(2, &[(&frozen_pubkey, &account)]);
        account.lamports = 1;
        db.store(3, &[(&frozen_pubkey, &account)]);
    }

    #[test]
    #[should_panic(
        expected = "Frozen account My11111111111111111111111111111111111111111 modified.  Lamports decreased from 1 to 0"
    )]
    fn test_frozen_account_lamport_decrease() {
        let frozen_pubkey =
            Pubkey::from_str("My11111111111111111111111111111111111111111").unwrap();
        let mut db = AccountsDB::new(Vec::new());

        let mut account = Account::new(1, 42, &frozen_pubkey);
        db.store(0, &[(&frozen_pubkey, &account)]);

        let ancestors = vec![(0, 0)].into_iter().collect();
        db.freeze_accounts(&ancestors, &[frozen_pubkey]);

        // Store with a decrease below the frozen amount of lamports is not ok
        account.lamports -= 1;
        db.store(0, &[(&frozen_pubkey, &account)]);
    }

    #[test]
    #[should_panic(
        expected = "Unable to freeze an account that does not exist: My11111111111111111111111111111111111111111"
    )]
    fn test_frozen_account_nonexistent() {
        let frozen_pubkey =
            Pubkey::from_str("My11111111111111111111111111111111111111111").unwrap();
        let mut db = AccountsDB::new(Vec::new());

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
        let mut db = AccountsDB::new(Vec::new());

        let mut account = Account::new(1, 42, &frozen_pubkey);
        db.store(0, &[(&frozen_pubkey, &account)]);

        let ancestors = vec![(0, 0)].into_iter().collect();
        db.freeze_accounts(&ancestors, &[frozen_pubkey]);

        account.data[0] = 42;
        db.store(0, &[(&frozen_pubkey, &account)]);
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
            usize, // for StoredAccount::offset
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

        let stored_account = StoredAccount {
            meta: &meta,
            account_meta: &account_meta,
            data: &data,
            offset,
            hash: &hash,
        };
        let account = stored_account.clone_account();
        let expected_account_hash =
            Hash::from_str("GGTsxvxwnMsNfN6yYbBVQaRgvbVLfxeWnGXNyB8iXDyE").unwrap();

        assert_eq!(
            AccountsDB::hash_stored_account(slot, &stored_account),
            expected_account_hash,
            "StoredAccount's data layout might be changed; update hashing if needed."
        );
        assert_eq!(
            AccountsDB::hash_account(slot, &account, &stored_account.meta.pubkey),
            expected_account_hash,
            "Account-based hashing must be consistent with StoredAccount-based one."
        );
    }

    #[test]
    fn test_bank_hash_stats() {
        solana_logger::setup();
        let db = AccountsDB::new(Vec::new());

        let key = Pubkey::default();
        let some_data_len = 5;
        let some_slot: Slot = 0;
        let account = Account::new(1, some_data_len, &key);
        let ancestors = vec![(some_slot, 0)].into_iter().collect();

        db.store(some_slot, &[(&key, &account)]);
        let mut account = db.load_slow(&ancestors, &key).unwrap().0;
        account.lamports += 1;
        account.executable = true;
        db.store(some_slot, &[(&key, &account)]);
        db.add_root(some_slot);

        let bank_hashes = db.bank_hashes.read().unwrap();
        let bank_hash = bank_hashes.get(&some_slot).unwrap();
        assert_eq!(bank_hash.stats.num_removed_accounts, 1);
        assert_eq!(bank_hash.stats.num_added_accounts, 1);
        assert_eq!(bank_hash.stats.num_lamports_stored, 3);
        assert_eq!(bank_hash.stats.total_data_len, 2 * some_data_len as u64);
        assert_eq!(bank_hash.stats.num_executable_accounts, 1);
    }

    #[test]
    fn test_verify_bank_hash() {
        use BankHashVerificationError::*;
        solana_logger::setup();
        let db = AccountsDB::new(Vec::new());

        let key = Pubkey::default();
        let some_data_len = 0;
        let some_slot: Slot = 0;
        let account = Account::new(1, some_data_len, &key);
        let ancestors = vec![(some_slot, 0)].into_iter().collect();

        db.store(some_slot, &[(&key, &account)]);
        db.add_root(some_slot);
        db.update_accounts_hash(some_slot, &ancestors);
        assert_matches!(db.verify_bank_hash(some_slot, &ancestors), Ok(_));

        db.bank_hashes.write().unwrap().remove(&some_slot).unwrap();
        assert_matches!(
            db.verify_bank_hash(some_slot, &ancestors),
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
            db.verify_bank_hash(some_slot, &ancestors),
            Err(MismatchedBankHash)
        );
    }

    #[test]
    fn test_verify_bank_hash_no_account() {
        solana_logger::setup();
        let db = AccountsDB::new(Vec::new());

        let some_slot: Slot = 0;
        let ancestors = vec![(some_slot, 0)].into_iter().collect();

        db.bank_hashes
            .write()
            .unwrap()
            .insert(some_slot, BankHashInfo::default());
        db.add_root(some_slot);
        db.update_accounts_hash(some_slot, &ancestors);
        assert_matches!(db.verify_bank_hash(some_slot, &ancestors), Ok(_));
    }

    #[test]
    fn test_verify_bank_hash_bad_account_hash() {
        use BankHashVerificationError::*;
        solana_logger::setup();
        let db = AccountsDB::new(Vec::new());

        let key = Pubkey::default();
        let some_data_len = 0;
        let some_slot: Slot = 0;
        let account = Account::new(1, some_data_len, &key);
        let ancestors = vec![(some_slot, 0)].into_iter().collect();

        let accounts = &[(&key, &account)];
        // update AccountsDB's bank hash but discard real account hashes
        db.hash_accounts(some_slot, accounts);
        // provide bogus account hashes
        let some_hash = Hash::new(&[0xca; HASH_BYTES]);
        db.store_with_hashes(some_slot, accounts, &vec![some_hash]);
        db.add_root(some_slot);
        assert_matches!(
            db.verify_bank_hash(some_slot, &ancestors),
            Err(MismatchedAccountHash)
        );
    }

    #[test]
    fn test_bad_bank_hash() {
        use solana_sdk::signature::{Keypair, Signer};
        let db = AccountsDB::new(Vec::new());

        let some_slot: Slot = 0;
        let ancestors = vec![(some_slot, 0)].into_iter().collect();

        for _ in 0..10_000 {
            let num_accounts = thread_rng().gen_range(0, 100);
            let accounts_keys: Vec<_> = (0..num_accounts)
                .into_iter()
                .map(|_| {
                    let key = Keypair::new().pubkey();
                    let lamports = thread_rng().gen_range(0, 100);
                    let some_data_len = thread_rng().gen_range(0, 1000);
                    let account = Account::new(lamports, some_data_len, &key);
                    (key, account)
                })
                .collect();
            let account_refs: Vec<_> = accounts_keys
                .iter()
                .map(|(key, account)| (key, account))
                .collect();
            db.store(some_slot, &account_refs);

            for (key, account) in &accounts_keys {
                let loaded_account = db.load_slow(&ancestors, key).unwrap().0;
                assert_eq!(
                    loaded_account.hash,
                    AccountsDB::hash_account(some_slot, &account, &key)
                );
            }
        }
    }

    #[test]
    fn test_get_snapshot_storages_empty() {
        let db = AccountsDB::new(Vec::new());
        assert!(db.get_snapshot_storages(0).is_empty());
    }

    #[test]
    fn test_get_snapshot_storages_only_older_than_or_equal_to_snapshot_slot() {
        let db = AccountsDB::new(Vec::new());

        let key = Pubkey::default();
        let account = Account::new(1, 0, &key);
        let before_slot = 0;
        let base_slot = before_slot + 1;
        let after_slot = base_slot + 1;

        db.add_root(base_slot);
        db.store(base_slot, &[(&key, &account)]);
        assert!(db.get_snapshot_storages(before_slot).is_empty());

        assert_eq!(1, db.get_snapshot_storages(base_slot).len());
        assert_eq!(1, db.get_snapshot_storages(after_slot).len());
    }

    #[test]
    fn test_get_snapshot_storages_only_non_empty() {
        let db = AccountsDB::new(Vec::new());

        let key = Pubkey::default();
        let account = Account::new(1, 0, &key);
        let base_slot = 0;
        let after_slot = base_slot + 1;

        db.store(base_slot, &[(&key, &account)]);
        db.storage
            .write()
            .unwrap()
            .0
            .get_mut(&base_slot)
            .unwrap()
            .clear();
        db.add_root(base_slot);
        assert!(db.get_snapshot_storages(after_slot).is_empty());

        db.store(base_slot, &[(&key, &account)]);
        assert_eq!(1, db.get_snapshot_storages(after_slot).len());
    }

    #[test]
    fn test_get_snapshot_storages_only_roots() {
        let db = AccountsDB::new(Vec::new());

        let key = Pubkey::default();
        let account = Account::new(1, 0, &key);
        let base_slot = 0;
        let after_slot = base_slot + 1;

        db.store(base_slot, &[(&key, &account)]);
        assert!(db.get_snapshot_storages(after_slot).is_empty());

        db.add_root(base_slot);
        assert_eq!(1, db.get_snapshot_storages(after_slot).len());
    }

    #[test]
    fn test_get_snapshot_storages_exclude_empty() {
        let db = AccountsDB::new(Vec::new());

        let key = Pubkey::default();
        let account = Account::new(1, 0, &key);
        let base_slot = 0;
        let after_slot = base_slot + 1;

        db.store(base_slot, &[(&key, &account)]);
        db.add_root(base_slot);
        assert_eq!(1, db.get_snapshot_storages(after_slot).len());

        let storage = db.storage.read().unwrap();
        storage.0[&0].values().next().unwrap().remove_account();
        assert!(db.get_snapshot_storages(after_slot).is_empty());
    }

    #[test]
    #[should_panic(expected = "double remove of account in slot: 0/store: 0!!")]
    fn test_storage_remove_account_double_remove() {
        let accounts = AccountsDB::new(Vec::new());
        let pubkey = Pubkey::new_rand();
        let account = Account::new(1, 0, &Account::default().owner);
        accounts.store(0, &[(&pubkey, &account)]);
        let storage = accounts.storage.read().unwrap();
        let storage_entry = storage.0[&0].values().next().unwrap();
        storage_entry.remove_account();
        storage_entry.remove_account();
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
        let dummy_account = Account::new(99999999, no_data, &owner);
        let zero_lamport_account = Account::new(zero_lamport, no_data, &owner);

        let pubkey = Pubkey::new_rand();
        let dummy_pubkey = Pubkey::new_rand();
        let purged_pubkey1 = Pubkey::new_rand();
        let purged_pubkey2 = Pubkey::new_rand();

        let mut current_slot = 0;
        let accounts = AccountsDB::new_single();

        // create intermidiate updates to purged_pubkey1 so that
        // generate_index must add slots as root last at once
        current_slot += 1;
        accounts.store(current_slot, &[(&pubkey, &account)]);
        accounts.store(current_slot, &[(&purged_pubkey1, &account2)]);
        accounts.add_root(current_slot);

        current_slot += 1;
        accounts.store(current_slot, &[(&purged_pubkey1, &account2)]);
        accounts.add_root(current_slot);

        current_slot += 1;
        accounts.store(current_slot, &[(&purged_pubkey1, &account2)]);
        accounts.add_root(current_slot);

        current_slot += 1;
        accounts.store(current_slot, &[(&purged_pubkey1, &zero_lamport_account)]);
        accounts.store(current_slot, &[(&purged_pubkey2, &account3)]);
        accounts.add_root(current_slot);

        current_slot += 1;
        accounts.store(current_slot, &[(&purged_pubkey2, &zero_lamport_account)]);
        accounts.add_root(current_slot);

        current_slot += 1;
        accounts.store(current_slot, &[(&dummy_pubkey, &dummy_account)]);
        accounts.add_root(current_slot);

        print_count_and_status("before reconstruct", &accounts);
        let accounts = reconstruct_accounts_db_via_serialization(&accounts, current_slot);
        print_count_and_status("before purge zero", &accounts);
        accounts.clean_accounts();
        print_count_and_status("after purge zero", &accounts);

        assert_load_account(&accounts, current_slot, pubkey, old_lamport);
        assert_load_account(&accounts, current_slot, purged_pubkey1, 0);
        assert_load_account(&accounts, current_slot, purged_pubkey2, 0);
    }

    #[test]
    fn test_accounts_clean_after_snapshot_restore_then_old_revives() {
        solana_logger::setup();
        let old_lamport = 223;
        let zero_lamport = 0;
        let no_data = 0;
        let dummy_lamport = 999999;
        let owner = Account::default().owner;

        let account = Account::new(old_lamport, no_data, &owner);
        let account2 = Account::new(old_lamport + 100_001, no_data, &owner);
        let account3 = Account::new(old_lamport + 100_002, no_data, &owner);
        let dummy_account = Account::new(dummy_lamport, no_data, &owner);
        let zero_lamport_account = Account::new(zero_lamport, no_data, &owner);

        let pubkey1 = Pubkey::new_rand();
        let pubkey2 = Pubkey::new_rand();
        let dummy_pubkey = Pubkey::new_rand();

        let mut current_slot = 0;
        let accounts = AccountsDB::new_single();

        // A: Initialize AccountsDB with pubkey1 and pubkey2
        current_slot += 1;
        accounts.store(current_slot, &[(&pubkey1, &account)]);
        accounts.store(current_slot, &[(&pubkey2, &account)]);
        accounts.add_root(current_slot);

        // B: Test multiple updates to pubkey1 in a single slot/storage
        current_slot += 1;
        assert_eq!(0, accounts.store_count_for_slot(current_slot));
        assert_eq!(1, accounts.ref_count_for_pubkey(&pubkey1));
        accounts.store(current_slot, &[(&pubkey1, &account2)]);
        accounts.store(current_slot, &[(&pubkey1, &account2)]);
        assert_eq!(1, accounts.store_count_for_slot(current_slot));
        assert_eq!(3, accounts.ref_count_for_pubkey(&pubkey1));
        accounts.add_root(current_slot);

        // C: Yet more update to trigger lazy clean of step A
        current_slot += 1;
        assert_eq!(3, accounts.ref_count_for_pubkey(&pubkey1));
        accounts.store(current_slot, &[(&pubkey1, &account3)]);
        assert_eq!(4, accounts.ref_count_for_pubkey(&pubkey1));
        accounts.add_root(current_slot);

        // D: Make pubkey1 0-lamport; also triggers clean of step B
        current_slot += 1;
        assert_eq!(4, accounts.ref_count_for_pubkey(&pubkey1));
        accounts.store(current_slot, &[(&pubkey1, &zero_lamport_account)]);
        accounts.process_dead_slots();
        assert_eq!(
            3, /* == 4 - 2 + 1 */
            accounts.ref_count_for_pubkey(&pubkey1)
        );
        accounts.add_root(current_slot);

        // E: Avoid missing bank hash error
        current_slot += 1;
        accounts.store(current_slot, &[(&dummy_pubkey, &dummy_account)]);
        accounts.add_root(current_slot);

        assert_load_account(&accounts, current_slot, pubkey1, zero_lamport);
        assert_load_account(&accounts, current_slot, pubkey2, old_lamport);
        assert_load_account(&accounts, current_slot, dummy_pubkey, dummy_lamport);

        // At this point, there is no index entries for A and B
        // If step C and step D should be purged, snapshot restore would cause
        // pubkey1 to be revived as the state of step A.
        // So, prevent that from happening by introducing refcount
        accounts.clean_accounts();
        let accounts = reconstruct_accounts_db_via_serialization(&accounts, current_slot);
        accounts.clean_accounts();

        assert_load_account(&accounts, current_slot, pubkey1, zero_lamport);
        assert_load_account(&accounts, current_slot, pubkey2, old_lamport);
        assert_load_account(&accounts, current_slot, dummy_pubkey, dummy_lamport);

        // F: Finally, make Step A cleanable
        current_slot += 1;
        accounts.store(current_slot, &[(&pubkey2, &account)]);
        accounts.add_root(current_slot);

        // Do clean
        accounts.clean_accounts();

        // Ensure pubkey2 is cleaned from the index finally
        assert_not_load_account(&accounts, current_slot, pubkey1);
        assert_load_account(&accounts, current_slot, pubkey2, old_lamport);
        assert_load_account(&accounts, current_slot, dummy_pubkey, dummy_lamport);
    }

    #[test]
    fn clean_dead_slots_empty() {
        let accounts = AccountsDB::new_single();
        let mut dead_slots = HashSet::new();
        dead_slots.insert(10);
        accounts.clean_dead_slots(&dead_slots);
    }
}

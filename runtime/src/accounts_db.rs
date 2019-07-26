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
//! AppendVec's only store accounts for single forks.  To bootstrap the
//! index from a persistent store of AppendVec's, the entries include
//! a "write_version".  A single global atomic `AccountsDB::write_version`
//! tracks the number of commits to the entire data store. So the latest
//! commit for each fork entry would be indexed.

use crate::accounts_index::{AccountsIndex, Fork};
use crate::append_vec::{AppendVec, StorageMeta, StoredAccount};
use bincode::{deserialize_from, serialize_into, serialized_size};
use log::*;
use rand::{thread_rng, Rng};
use rayon::prelude::*;
use rayon::ThreadPool;
use serde::de::{MapAccess, Visitor};
use serde::ser::{SerializeMap, Serializer};
use serde::{Deserialize, Serialize};
use solana_measure::measure::Measure;
use solana_sdk::account::{Account, LamportCredit};
use solana_sdk::pubkey::Pubkey;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs::remove_dir_all;
use std::io::{BufReader, Cursor, Error, ErrorKind, Read};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use sys_info;

pub const DEFAULT_FILE_SIZE: u64 = 4 * 1024 * 1024;
pub const DEFAULT_NUM_THREADS: u32 = 8;
pub const DEFAULT_DIRS: &str = "0,1,2,3";

#[derive(Debug, Default)]
pub struct ErrorCounters {
    pub account_not_found: usize,
    pub account_in_use: usize,
    pub account_loaded_twice: usize,
    pub blockhash_not_found: usize,
    pub blockhash_too_old: usize,
    pub reserve_blockhash: usize,
    pub invalid_account_for_fee: usize,
    pub insufficient_funds: usize,
    pub invalid_account_index: usize,
    pub duplicate_signature: usize,
    pub call_chain_too_deep: usize,
    pub missing_signature_for_fee: usize,
}

#[derive(Deserialize, Serialize, Default, Debug, PartialEq, Clone)]
pub struct AccountInfo {
    /// index identifying the append storage
    id: AppendVecId,

    /// offset into the storage
    offset: usize,

    /// lamports in the account used when squashing kept for optimization
    /// purposes to remove accounts with zero balance.
    lamports: u64,
}
/// An offset into the AccountsDB::storage vector
pub type AppendVecId = usize;
pub type InstructionAccounts = Vec<Account>;
pub type InstructionCredits = Vec<LamportCredit>;
pub type InstructionLoaders = Vec<Vec<(Pubkey, Account)>>;

// Each fork has a set of storage entries.
type ForkStores = HashMap<usize, Arc<AccountStorageEntry>>;

#[derive(Default, Debug)]
pub struct AccountStorage(HashMap<Fork, ForkStores>);

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

        while let Some((storage_id, storage_entry)) = access.next_entry()? {
            let storage_entry: AccountStorageEntry = storage_entry;
            let storage_fork_map = map
                .entry(storage_entry.fork_id)
                .or_insert_with(HashMap::new);
            storage_fork_map.insert(storage_id, Arc::new(storage_entry));
        }

        Ok(AccountStorage(map))
    }
}

impl Serialize for AccountStorage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut len: usize = 0;
        for storage in self.0.values() {
            len += storage.len();
        }
        let mut map = serializer.serialize_map(Some(len))?;
        for fork_storage in self.0.values() {
            for (storage_id, account_storage_entry) in fork_storage {
                map.serialize_entry(storage_id, &**account_storage_entry)?;
            }
        }
        map.end()
    }
}

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

/// Persistent storage structure holding the accounts
#[derive(Debug, Deserialize, Serialize)]
pub struct AccountStorageEntry {
    id: AppendVecId,

    fork_id: Fork,

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
    pub fn new(path: &str, fork_id: Fork, id: usize, file_size: u64) -> Self {
        let tail = format!("{}.{}", fork_id, id);
        let path = Path::new(path).join(&tail);
        let accounts = AppendVec::new(&path, true, file_size as usize);

        AccountStorageEntry {
            id,
            fork_id,
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
        let (count, mut status) = *count_and_status;

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

        if count > 0 {
            *count_and_status = (count - 1, status);
        } else {
            warn!("count value 0 for fork {}", self.fork_id);
        }
        count_and_status.0
    }
}

pub fn get_paths_vec(paths: &str) -> Vec<String> {
    paths.split(',').map(ToString::to_string).collect()
}

#[derive(Debug)]
struct TempPaths {
    pub paths: String,
}

impl Drop for TempPaths {
    fn drop(&mut self) {
        let paths = get_paths_vec(&self.paths);
        paths.iter().for_each(|p| {
            let _ignored = remove_dir_all(p);
        });
    }
}

fn get_temp_accounts_path(paths: &str) -> TempPaths {
    let paths = get_paths_vec(paths);
    let out_dir = std::env::var("FARF_DIR").unwrap_or_else(|_| "farf".to_string());
    let rand = Pubkey::new_rand();
    let paths: Vec<_> = paths
        .iter()
        .map(|path| format!("{}/accounts_db/{}/{}", out_dir, rand, path))
        .collect();
    TempPaths {
        paths: paths.join(","),
    }
}

// This structure handles the load/store of the accounts
#[derive(Debug)]
pub struct AccountsDB {
    /// Keeps tracks of index into AppendVec on a per fork basis
    pub accounts_index: RwLock<AccountsIndex<AccountInfo>>,

    /// Account storage
    pub storage: RwLock<AccountStorage>,

    /// distribute the accounts across storage lists
    pub next_id: AtomicUsize,

    /// write version
    write_version: AtomicUsize,

    /// Set of storage paths to pick from
    paths: RwLock<Vec<String>>,

    /// Set of paths this accounts_db needs to hold/remove
    temp_paths: Option<TempPaths>,

    /// Starting file size of appendvecs
    file_size: u64,

    /// Thread pool used for par_iter
    pub thread_pool: ThreadPool,

    min_num_stores: usize,
}

impl Default for AccountsDB {
    fn default() -> Self {
        let num_threads = sys_info::cpu_num().unwrap_or(DEFAULT_NUM_THREADS) as usize;

        let temp_paths = get_temp_accounts_path(DEFAULT_DIRS); // make 4 directories by default

        AccountsDB {
            accounts_index: RwLock::new(AccountsIndex::default()),
            storage: RwLock::new(AccountStorage(HashMap::new())),
            next_id: AtomicUsize::new(0),
            write_version: AtomicUsize::new(0),
            paths: RwLock::new(get_paths_vec(&temp_paths.paths)),
            temp_paths: Some(temp_paths),
            file_size: DEFAULT_FILE_SIZE,
            thread_pool: rayon::ThreadPoolBuilder::new()
                .num_threads(num_threads)
                .build()
                .unwrap(),
            min_num_stores: num_threads,
        }
    }
}

impl AccountsDB {
    pub fn new(paths: Option<String>) -> Self {
        if let Some(paths) = paths {
            AccountsDB {
                paths: RwLock::new(get_paths_vec(&paths)),
                temp_paths: None,
                ..AccountsDB::default()
            }
        } else {
            AccountsDB::default()
        }
    }

    #[cfg(test)]
    pub fn new_single() -> Self {
        AccountsDB {
            min_num_stores: 0,
            ..AccountsDB::new(None)
        }
    }
    #[cfg(test)]
    pub fn new_sized(paths: Option<String>, file_size: u64) -> Self {
        AccountsDB {
            file_size,
            ..AccountsDB::new(paths)
        }
    }

    pub fn paths(&self) -> String {
        self.paths.read().unwrap().join(",")
    }

    pub fn update_from_stream<R: Read>(
        &self,
        mut stream: &mut BufReader<R>,
    ) -> Result<(), std::io::Error> {
        let _len: usize = deserialize_from(&mut stream)
            .map_err(|_| AccountsDB::get_io_error("len deserialize error"))?;
        *self.paths.write().unwrap() = deserialize_from(&mut stream)
            .map_err(|_| AccountsDB::get_io_error("paths deserialize error"))?;
        let mut storage: AccountStorage = deserialize_from(&mut stream)
            .map_err(|_| AccountsDB::get_io_error("storage deserialize error"))?;
        let version: u64 = deserialize_from(&mut stream)
            .map_err(|_| AccountsDB::get_io_error("write version deserialize error"))?;

        let mut ids: Vec<usize> = storage
            .0
            .values()
            .flat_map(HashMap::keys)
            .cloned()
            .collect();
        ids.sort();

        {
            let mut stores = self.storage.write().unwrap();
            if let Some((_, store0)) = storage.0.remove_entry(&0) {
                let fork_storage0 = stores.0.entry(0).or_insert_with(HashMap::new);
                for (id, store) in store0.iter() {
                    fork_storage0.insert(*id, store.clone());
                }
            }
            stores.0.extend(storage.0);
        }
        self.next_id
            .store(ids[ids.len() - 1] + 1, Ordering::Relaxed);
        self.write_version
            .fetch_add(version as usize, Ordering::Relaxed);
        self.generate_index();
        Ok(())
    }

    fn new_storage_entry(&self, fork_id: Fork, path: &str, size: u64) -> AccountStorageEntry {
        AccountStorageEntry::new(
            path,
            fork_id,
            self.next_id.fetch_add(1, Ordering::Relaxed),
            size,
        )
    }

    pub fn has_accounts(&self, fork: Fork) -> bool {
        if let Some(storage_forks) = self.storage.read().unwrap().0.get(&fork) {
            for x in storage_forks.values() {
                if x.count() > 0 {
                    return true;
                }
            }
        }
        false
    }

    pub fn scan_accounts<F, A>(&self, ancestors: &HashMap<Fork, usize>, scan_func: F) -> A
    where
        F: Fn(&mut A, Option<(&Pubkey, Account, Fork)>) -> (),
        A: Default,
    {
        let mut collector = A::default();
        let accounts_index = self.accounts_index.read().unwrap();
        let storage = self.storage.read().unwrap();
        accounts_index.scan_accounts(ancestors, |pubkey, (account_info, fork)| {
            scan_func(
                &mut collector,
                storage
                    .0
                    .get(&fork)
                    .and_then(|storage_map| storage_map.get(&account_info.id))
                    .and_then(|store| {
                        Some(
                            store
                                .accounts
                                .get_account(account_info.offset)?
                                .0
                                .clone_account(),
                        )
                    })
                    .map(|account| (pubkey, account, fork)),
            )
        });
        collector
    }

    /// Scan a specific fork through all the account storage in parallel with sequential read
    // PERF: Sequentially read each storage entry in parallel
    pub fn scan_account_storage<F, B>(&self, fork_id: Fork, scan_func: F) -> Vec<B>
    where
        F: Fn(&StoredAccount, AppendVecId, &mut B) -> (),
        F: Send + Sync,
        B: Send + Default,
    {
        let storage_maps: Vec<Arc<AccountStorageEntry>> = self
            .storage
            .read()
            .unwrap()
            .0
            .get(&fork_id)
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

    pub fn load(
        storage: &AccountStorage,
        ancestors: &HashMap<Fork, usize>,
        accounts_index: &AccountsIndex<AccountInfo>,
        pubkey: &Pubkey,
    ) -> Option<(Account, Fork)> {
        let (lock, index) = accounts_index.get(pubkey, ancestors)?;
        let fork = lock[index].0;
        //TODO: thread this as a ref
        if let Some(fork_storage) = storage.0.get(&fork) {
            let info = &lock[index].1;
            fork_storage
                .get(&info.id)
                .and_then(|store| Some(store.accounts.get_account(info.offset)?.0.clone_account()))
                .map(|account| (account, fork))
        } else {
            None
        }
    }

    pub fn load_slow(
        &self,
        ancestors: &HashMap<Fork, usize>,
        pubkey: &Pubkey,
    ) -> Option<(Account, Fork)> {
        let accounts_index = self.accounts_index.read().unwrap();
        let storage = self.storage.read().unwrap();
        Self::load(&storage, ancestors, &accounts_index, pubkey)
    }

    fn find_storage_candidate(&self, fork_id: Fork) -> Arc<AccountStorageEntry> {
        let stores = self.storage.read().unwrap();

        if let Some(fork_stores) = stores.0.get(&fork_id) {
            if !fork_stores.is_empty() {
                // pick an available store at random by iterating from a random point
                let to_skip = thread_rng().gen_range(0, fork_stores.len());

                for (i, store) in fork_stores.values().cycle().skip(to_skip).enumerate() {
                    if store.try_available() {
                        return store.clone();
                    }
                    // looked at every store, bail...
                    if i == fork_stores.len() {
                        break;
                    }
                }
            }
        }
        drop(stores);

        let store = self.create_and_insert_store(fork_id, self.file_size);
        store.try_available();
        store
    }

    fn create_and_insert_store(&self, fork_id: Fork, size: u64) -> Arc<AccountStorageEntry> {
        let mut stores = self.storage.write().unwrap();
        let fork_storage = stores.0.entry(fork_id).or_insert_with(HashMap::new);

        // Create more stores so that when scanning the storage all CPUs have work
        while fork_storage.len() + 1 < self.min_num_stores {
            self.create_store(fork_id, fork_storage, self.file_size);
        }

        self.create_store(fork_id, fork_storage, size)
    }

    fn create_store(
        &self,
        fork_id: Fork,
        fork_storage: &mut ForkStores,
        size: u64,
    ) -> Arc<AccountStorageEntry> {
        let paths = self.paths.read().unwrap();
        let path_index = thread_rng().gen_range(0, paths.len());
        let store = Arc::new(self.new_storage_entry(fork_id, &paths[path_index], size));
        fork_storage.insert(store.id, store.clone());
        store
    }

    pub fn purge_fork(&self, fork: Fork) {
        //add_root should be called first
        let is_root = self.accounts_index.read().unwrap().is_root(fork);
        if !is_root {
            self.storage.write().unwrap().0.remove(&fork);
        }
    }

    fn store_accounts(
        &self,
        fork_id: Fork,
        accounts: &HashMap<&Pubkey, &Account>,
    ) -> Vec<AccountInfo> {
        let with_meta: Vec<(StorageMeta, &Account)> = accounts
            .iter()
            .map(|(pubkey, account)| {
                let write_version = self.write_version.fetch_add(1, Ordering::Relaxed) as u64;
                let data_len = if account.lamports == 0 {
                    0
                } else {
                    account.data.len() as u64
                };
                let meta = StorageMeta {
                    write_version,
                    pubkey: **pubkey,
                    data_len,
                };

                (meta, *account)
            })
            .collect();
        let mut infos: Vec<AccountInfo> = vec![];
        while infos.len() < with_meta.len() {
            let storage = self.find_storage_candidate(fork_id);
            let rvs = storage.accounts.append_accounts(&with_meta[infos.len()..]);
            if rvs.is_empty() {
                storage.set_status(AccountStorageStatus::Full);

                // See if an account overflows the default append vec size.
                let data_len = (with_meta[infos.len()].1.data.len() + 4096) as u64;
                if data_len > self.file_size {
                    self.create_and_insert_store(fork_id, data_len * 2);
                }
                continue;
            }
            for (offset, (_, account)) in rvs.iter().zip(&with_meta[infos.len()..]) {
                storage.add_account();
                infos.push(AccountInfo {
                    id: storage.id,
                    offset: *offset,
                    lamports: account.lamports,
                });
            }
            // restore the state to available
            storage.set_status(AccountStorageStatus::Available);
        }
        infos
    }

    fn update_index(
        &self,
        fork_id: Fork,
        infos: Vec<AccountInfo>,
        accounts: &HashMap<&Pubkey, &Account>,
    ) -> (Vec<(Fork, AccountInfo)>, u64) {
        let mut reclaims: Vec<(Fork, AccountInfo)> = Vec::with_capacity(infos.len() * 2);
        let mut inserts = vec![];
        let index = self.accounts_index.read().unwrap();
        let mut update_index_work = Measure::start("update_index_work");
        for (info, account) in infos.into_iter().zip(accounts.iter()) {
            let key = &account.0;
            if let Some(info) = index.update(fork_id, key, info, &mut reclaims) {
                inserts.push((account, info));
            }
        }
        let last_root = index.last_root;
        drop(index);
        if !inserts.is_empty() {
            let mut index = self.accounts_index.write().unwrap();
            for ((pubkey, _account), info) in inserts {
                index.insert(fork_id, pubkey, info, &mut reclaims);
            }
        }
        update_index_work.stop();
        (reclaims, last_root)
    }

    fn remove_dead_accounts(&self, reclaims: Vec<(Fork, AccountInfo)>) -> HashSet<Fork> {
        let storage = self.storage.read().unwrap();
        let mut dead_forks = HashSet::new();
        for (fork_id, account_info) in reclaims {
            if let Some(fork_storage) = storage.0.get(&fork_id) {
                if let Some(store) = fork_storage.get(&account_info.id) {
                    assert_eq!(
                        fork_id, store.fork_id,
                        "AccountDB::accounts_index corrupted. Storage should only point to one fork"
                    );
                    let count = store.remove_account();
                    if count == 0 {
                        dead_forks.insert(fork_id);
                    }
                }
            }
        }

        dead_forks.retain(|fork| {
            if let Some(fork_storage) = storage.0.get(&fork) {
                for x in fork_storage.values() {
                    if x.count() != 0 {
                        return false;
                    }
                }
            }
            true
        });

        dead_forks
    }

    fn cleanup_dead_forks(&self, dead_forks: &mut HashSet<Fork>, last_root: u64) {
        // a fork is not totally dead until it is older than the root
        dead_forks.retain(|fork| *fork < last_root);
        if !dead_forks.is_empty() {
            let mut index = self.accounts_index.write().unwrap();
            for fork in dead_forks.iter() {
                index.cleanup_dead_fork(*fork);
            }
        }
    }

    /// Store the account update.
    pub fn store(&self, fork_id: Fork, accounts: &HashMap<&Pubkey, &Account>) {
        let mut store_accounts = Measure::start("store::store_accounts");
        let infos = self.store_accounts(fork_id, accounts);
        store_accounts.stop();

        let mut update_index = Measure::start("store::update_index");
        let (reclaims, last_root) = self.update_index(fork_id, infos, accounts);
        update_index.stop();
        trace!("reclaim: {}", reclaims.len());

        let mut remove_dead_accounts = Measure::start("store::remove_dead");
        let mut dead_forks = self.remove_dead_accounts(reclaims);
        remove_dead_accounts.stop();
        trace!("dead_forks: {}", dead_forks.len());

        let mut cleanup_dead_forks = Measure::start("store::cleanup_dead_forks");
        self.cleanup_dead_forks(&mut dead_forks, last_root);
        cleanup_dead_forks.stop();
        trace!("purge_forks: {}", dead_forks.len());

        let mut purge_forks = Measure::start("store::purge_forks");
        for fork in dead_forks {
            self.purge_fork(fork);
        }
        purge_forks.stop();
    }

    pub fn add_root(&self, fork: Fork) {
        self.accounts_index.write().unwrap().add_root(fork)
    }

    fn merge(
        dest: &mut HashMap<Pubkey, (u64, AccountInfo)>,
        source: &HashMap<Pubkey, (u64, AccountInfo)>,
    ) {
        for (key, (source_version, source_info)) in source.iter() {
            if let Some((dest_version, _)) = dest.get(key) {
                if dest_version > source_version {
                    continue;
                }
            }
            dest.insert(*key, (*source_version, source_info.clone()));
        }
    }

    fn get_io_error(error: &str) -> Error {
        warn!("AccountsDB error: {:?}", error);
        Error::new(ErrorKind::Other, error)
    }

    fn generate_index(&self) {
        let storage = self.storage.read().unwrap();
        let mut forks: Vec<Fork> = storage.0.keys().cloned().collect();
        forks.sort();
        let mut accounts_index = self.accounts_index.write().unwrap();
        accounts_index.roots.insert(0);
        for fork_id in forks.iter() {
            let mut accumulator: Vec<HashMap<Pubkey, (u64, AccountInfo)>> = self
                .scan_account_storage(
                    *fork_id,
                    |stored_account: &StoredAccount,
                     id: AppendVecId,
                     accum: &mut HashMap<Pubkey, (u64, AccountInfo)>| {
                        let account_info = AccountInfo {
                            id,
                            offset: stored_account.offset,
                            lamports: stored_account.balance.lamports,
                        };
                        accum.insert(
                            stored_account.meta.pubkey,
                            (stored_account.meta.write_version, account_info),
                        );
                    },
                );

            let mut account_maps = accumulator.pop().unwrap();
            while let Some(maps) = accumulator.pop() {
                AccountsDB::merge(&mut account_maps, &maps);
            }
            if !account_maps.is_empty() {
                accounts_index.roots.insert(*fork_id);
                let mut _reclaims: Vec<(u64, AccountInfo)> = vec![];
                for (pubkey, (_, account_info)) in account_maps.iter() {
                    accounts_index.insert(*fork_id, pubkey, account_info.clone(), &mut _reclaims);
                }
            }
        }
    }
}

impl Serialize for AccountsDB {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        use serde::ser::Error;
        let storage = self.storage.read().unwrap();
        let len = serialized_size(&self.paths).unwrap()
            + serialized_size(&*storage).unwrap()
            + std::mem::size_of::<u64>() as u64;
        let mut buf = vec![0u8; len as usize];
        let mut wr = Cursor::new(&mut buf[..]);
        let version: u64 = self.write_version.load(Ordering::Relaxed) as u64;
        serialize_into(&mut wr, &self.paths).map_err(Error::custom)?;
        serialize_into(&mut wr, &*storage).map_err(Error::custom)?;
        serialize_into(&mut wr, &version).map_err(Error::custom)?;
        let len = wr.position() as usize;
        serializer.serialize_bytes(&wr.into_inner()[..len])
    }
}

#[cfg(test)]
mod tests {
    // TODO: all the bank tests are bank specific, issue: 2194
    use super::*;
    use bincode::{serialize_into, serialized_size};
    use maplit::hashmap;
    use rand::{thread_rng, Rng};
    use solana_sdk::account::Account;

    #[test]
    fn test_accountsdb_add_root() {
        solana_logger::setup();
        let db = AccountsDB::new(None);
        let key = Pubkey::default();
        let account0 = Account::new(1, 0, &key);

        db.store(0, &hashmap!(&key => &account0));
        db.add_root(0);
        let ancestors = vec![(1, 1)].into_iter().collect();
        assert_eq!(db.load_slow(&ancestors, &key), Some((account0, 0)));
    }

    #[test]
    fn test_accountsdb_latest_ancestor() {
        solana_logger::setup();
        let db = AccountsDB::new(None);
        let key = Pubkey::default();
        let account0 = Account::new(1, 0, &key);

        db.store(0, &hashmap!(&key => &account0));

        let account1 = Account::new(0, 0, &key);
        db.store(1, &hashmap!(&key => &account1));

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
        let db = AccountsDB::new(None);
        let key = Pubkey::default();
        let account0 = Account::new(1, 0, &key);

        db.store(0, &hashmap!(&key => &account0));

        let account1 = Account::new(0, 0, &key);
        db.store(1, &hashmap!(&key => &account1));
        db.add_root(0);

        let ancestors = vec![(1, 1)].into_iter().collect();
        assert_eq!(&db.load_slow(&ancestors, &key).unwrap().0, &account1);

        let ancestors = vec![(1, 1), (0, 0)].into_iter().collect();
        assert_eq!(&db.load_slow(&ancestors, &key).unwrap().0, &account1);
    }

    #[test]
    fn test_accountsdb_root_one_fork() {
        solana_logger::setup();
        let db = AccountsDB::new(None);

        let key = Pubkey::default();
        let account0 = Account::new(1, 0, &key);

        // store value 1 in the "root", i.e. db zero
        db.store(0, &hashmap!(&key => &account0));

        // now we have:
        //
        //                       root0 -> key.lamports==1
        //                        / \
        //                       /   \
        //  key.lamports==0 <- fork1    \
        //                             fork2 -> key.lamports==1
        //                                       (via root0)

        // store value 0 in one child
        let account1 = Account::new(0, 0, &key);
        db.store(1, &hashmap!(&key => &account1));

        // masking accounts is done at the Accounts level, at accountsDB we see
        // original account (but could also accept "None", which is implemented
        // at the Accounts level)
        let ancestors = vec![(0, 0), (1, 1)].into_iter().collect();
        assert_eq!(&db.load_slow(&ancestors, &key).unwrap().0, &account1);

        // we should see 1 token in fork 2
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
        let db = AccountsDB::new(None);

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
        db.store(1, &hashmap!(&pubkey => &account));
        db.store(1, &hashmap!(&pubkeys[0] => &account));
        {
            let stores = db.storage.read().unwrap();
            let fork_0_stores = &stores.0.get(&0).unwrap();
            let fork_1_stores = &stores.0.get(&1).unwrap();
            assert_eq!(fork_0_stores.len(), 1);
            assert_eq!(fork_1_stores.len(), 1);
            assert_eq!(fork_0_stores[&0].count(), 2);
            assert_eq!(fork_1_stores[&1].count(), 2);
        }
        db.add_root(1);
        {
            let stores = db.storage.read().unwrap();
            let fork_0_stores = &stores.0.get(&0).unwrap();
            let fork_1_stores = &stores.0.get(&1).unwrap();
            assert_eq!(fork_0_stores.len(), 1);
            assert_eq!(fork_1_stores.len(), 1);
            assert_eq!(fork_0_stores[&0].count(), 2);
            assert_eq!(fork_1_stores[&1].count(), 2);
        }
    }

    #[test]
    fn test_accounts_unsquashed() {
        let key = Pubkey::default();

        // 1 token in the "root", i.e. db zero
        let db0 = AccountsDB::new(None);
        let account0 = Account::new(1, 0, &key);
        db0.store(0, &hashmap!(&key => &account0));

        // 0 lamports in the child
        let account1 = Account::new(0, 0, &key);
        db0.store(1, &hashmap!(&key => &account1));

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
        fork: Fork,
        num: usize,
        space: usize,
        num_vote: usize,
    ) {
        let ancestors = vec![(fork, 0)].into_iter().collect();
        for t in 0..num {
            let pubkey = Pubkey::new_rand();
            let account = Account::new((t + 1) as u64, space, &Account::default().owner);
            pubkeys.push(pubkey.clone());
            assert!(accounts.load_slow(&ancestors, &pubkey).is_none());
            accounts.store(fork, &hashmap!(&pubkey => &account));
        }
        for t in 0..num_vote {
            let pubkey = Pubkey::new_rand();
            let account = Account::new((num + t + 1) as u64, space, &solana_vote_api::id());
            pubkeys.push(pubkey.clone());
            let ancestors = vec![(fork, 0)].into_iter().collect();
            assert!(accounts.load_slow(&ancestors, &pubkey).is_none());
            accounts.store(fork, &hashmap!(&pubkey => &account));
        }
    }

    fn update_accounts(accounts: &AccountsDB, pubkeys: &Vec<Pubkey>, fork: Fork, range: usize) {
        for _ in 1..1000 {
            let idx = thread_rng().gen_range(0, range);
            let ancestors = vec![(fork, 0)].into_iter().collect();
            if let Some((mut account, _)) = accounts.load_slow(&ancestors, &pubkeys[idx]) {
                account.lamports = account.lamports + 1;
                accounts.store(fork, &hashmap!(&pubkeys[idx] => &account));
                if account.lamports == 0 {
                    let ancestors = vec![(fork, 0)].into_iter().collect();
                    assert!(accounts.load_slow(&ancestors, &pubkeys[idx]).is_none());
                } else {
                    let mut default_account = Account::default();
                    default_account.lamports = account.lamports;
                    assert_eq!(default_account, account);
                }
            }
        }
    }

    fn check_storage(accounts: &AccountsDB, fork: Fork, count: usize) -> bool {
        let storage = accounts.storage.read().unwrap();
        assert_eq!(storage.0[&fork].len(), 1);
        let fork_storage = storage.0.get(&fork).unwrap();
        let mut total_count: usize = 0;
        for store in fork_storage.values() {
            assert_eq!(store.status(), AccountStorageStatus::Available);
            total_count += store.count();
        }
        assert_eq!(total_count, count);
        total_count == count
    }

    fn check_accounts(
        accounts: &AccountsDB,
        pubkeys: &Vec<Pubkey>,
        fork: Fork,
        num: usize,
        count: usize,
    ) {
        let ancestors = vec![(fork, 0)].into_iter().collect();
        for _ in 0..num {
            let idx = thread_rng().gen_range(0, num);
            let account = accounts.load_slow(&ancestors, &pubkeys[idx]);
            let account1 = Some((
                Account::new((idx + count) as u64, 0, &Account::default().owner),
                fork,
            ));
            assert_eq!(account, account1);
        }
    }

    fn modify_accounts(
        accounts: &AccountsDB,
        pubkeys: &Vec<Pubkey>,
        fork: Fork,
        num: usize,
        count: usize,
    ) {
        for idx in 0..num {
            let account = Account::new((idx + count) as u64, 0, &Account::default().owner);
            accounts.store(fork, &hashmap!(&pubkeys[idx] => &account));
        }
    }

    #[test]
    fn test_account_one() {
        let paths = get_temp_accounts_path("one");
        let db = AccountsDB::new(Some(paths.paths.clone()));
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
        let paths = get_temp_accounts_path("many0,many1");
        let db = AccountsDB::new(Some(paths.paths.clone()));
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
        let paths = get_temp_accounts_path("many2,many3");
        let size = 4096;
        let accounts = AccountsDB::new_sized(Some(paths.paths.clone()), size);
        let mut keys = vec![];
        for i in 0..9 {
            let key = Pubkey::new_rand();
            let account = Account::new(i + 1, size as usize / 4, &key);
            accounts.store(0, &hashmap!(&key => &account));
            keys.push(key);
        }
        for (i, key) in keys.iter().enumerate() {
            let ancestors = vec![(0, 0)].into_iter().collect();
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
            *append_vec_histogram.entry(storage.fork_id).or_insert(0) += 1;
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
        accounts.store(0, &hashmap!(&pubkey1 => &account1));
        {
            let stores = accounts.storage.read().unwrap();
            assert_eq!(stores.0.len(), 1);
            assert_eq!(stores.0[&0][&0].count(), 1);
            assert_eq!(stores.0[&0][&0].status(), AccountStorageStatus::Available);
        }

        let pubkey2 = Pubkey::new_rand();
        let account2 = Account::new(1, DEFAULT_FILE_SIZE as usize / 2, &pubkey2);
        accounts.store(0, &hashmap!(&pubkey2 => &account2));
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
            accounts.store(0, &hashmap!(&pubkey1 => &account1));
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
    fn test_purge_fork_not_root() {
        let accounts = AccountsDB::new(None);
        let mut pubkeys: Vec<Pubkey> = vec![];
        create_account(&accounts, &mut pubkeys, 0, 1, 0, 0);
        let ancestors = vec![(0, 0)].into_iter().collect();
        assert!(accounts.load_slow(&ancestors, &pubkeys[0]).is_some());;
        accounts.purge_fork(0);
        assert!(accounts.load_slow(&ancestors, &pubkeys[0]).is_none());;
    }

    #[test]
    fn test_purge_fork_after_root() {
        let accounts = AccountsDB::new(None);
        let mut pubkeys: Vec<Pubkey> = vec![];
        create_account(&accounts, &mut pubkeys, 0, 1, 0, 0);
        let ancestors = vec![(0, 0)].into_iter().collect();
        accounts.add_root(0);
        accounts.purge_fork(0);
        assert!(accounts.load_slow(&ancestors, &pubkeys[0]).is_some());
    }

    #[test]
    fn test_lazy_gc_fork() {
        //This test is pedantic
        //A fork is purged when a non root bank is cleaned up.  If a fork is behind root but it is
        //not root, it means we are retaining dead banks.
        let accounts = AccountsDB::new(None);
        let pubkey = Pubkey::new_rand();
        let account = Account::new(1, 0, &Account::default().owner);
        //store an account
        accounts.store(0, &hashmap!(&pubkey => &account));
        let ancestors = vec![(0, 0)].into_iter().collect();
        let id = {
            let index = accounts.accounts_index.read().unwrap();
            let (list, idx) = index.get(&pubkey, &ancestors).unwrap();
            list[idx].1.id
        };
        //fork 0 is behind root, but it is not root, therefore it is purged
        accounts.add_root(1);
        assert!(accounts.accounts_index.read().unwrap().is_purged(0));

        //fork is still there, since gc is lazy
        assert!(accounts.storage.read().unwrap().0[&0].get(&id).is_some());

        //store causes cleanup
        accounts.store(1, &hashmap!(&pubkey => &account));

        //fork is gone
        assert!(accounts.storage.read().unwrap().0.get(&0).is_none());

        //new value is there
        let ancestors = vec![(1, 1)].into_iter().collect();
        assert_eq!(accounts.load_slow(&ancestors, &pubkey), Some((account, 1)));
    }

    #[test]
    fn test_accounts_db_serialize() {
        solana_logger::setup();
        let accounts = AccountsDB::new_single();
        let mut pubkeys: Vec<Pubkey> = vec![];
        create_account(&accounts, &mut pubkeys, 0, 100, 0, 0);
        assert_eq!(check_storage(&accounts, 0, 100), true);
        check_accounts(&accounts, &pubkeys, 0, 100, 1);
        modify_accounts(&accounts, &pubkeys, 0, 100, 2);
        check_accounts(&accounts, &pubkeys, 0, 100, 2);
        accounts.add_root(0);

        let mut pubkeys1: Vec<Pubkey> = vec![];
        create_account(&accounts, &mut pubkeys1, 1, 10, 0, 0);

        let mut buf = vec![0u8; serialized_size(&accounts).unwrap() as usize];
        let mut writer = Cursor::new(&mut buf[..]);
        serialize_into(&mut writer, &accounts).unwrap();
        assert!(check_storage(&accounts, 0, 100));
        assert!(check_storage(&accounts, 1, 10));

        let mut reader = BufReader::new(&buf[..]);
        let daccounts = AccountsDB::new(None);
        assert!(daccounts.update_from_stream(&mut reader).is_ok());
        assert_eq!(
            daccounts.write_version.load(Ordering::Relaxed),
            accounts.write_version.load(Ordering::Relaxed)
        );
        assert_eq!(daccounts.paths(), accounts.paths());

        check_accounts(&daccounts, &pubkeys, 0, 100, 2);
        check_accounts(&daccounts, &pubkeys1, 1, 10, 1);
        assert!(check_storage(&daccounts, 0, 100));
        assert!(check_storage(&daccounts, 1, 10));
    }

    #[test]
    #[ignore]
    fn test_store_account_stress() {
        let fork_id = 42;
        let num_threads = 2;

        let min_file_bytes = std::mem::size_of::<StorageMeta>()
            + std::mem::size_of::<crate::append_vec::AccountBalance>();

        let db = Arc::new(AccountsDB::new_sized(None, min_file_bytes as u64));

        db.add_root(fork_id);
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
                            db.store(fork_id, &hashmap!(&pubkey => &account));
                            let (account, fork) = db.load_slow(&HashMap::new(), &pubkey).expect(
                                &format!("Could not fetch stored account {}, iter {}", pubkey, i),
                            );
                            assert_eq!(fork, fork_id);
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
        let db = AccountsDB::new(None);
        let key = Pubkey::default();
        let key0 = Pubkey::new_rand();
        let account0 = Account::new(1, 0, &key);

        db.store(0, &hashmap!(&key0 => &account0));

        let key1 = Pubkey::new_rand();
        let account1 = Account::new(2, 0, &key);
        db.store(1, &hashmap!(&key1 => &account1));

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
        let db = AccountsDB::new(None);

        let key = Pubkey::default();
        let data_len = DEFAULT_FILE_SIZE as usize + 7;
        let account = Account::new(1, data_len, &key);

        db.store(0, &hashmap!(&key => &account));

        let ancestors = vec![(0, 0)].into_iter().collect();
        let ret = db.load_slow(&ancestors, &key).unwrap();
        assert_eq!(ret.0.data.len(), data_len);
    }
}

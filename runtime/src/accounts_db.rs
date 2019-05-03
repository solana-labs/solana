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
use hashbrown::{HashMap, HashSet};
use log::*;
use rand::{thread_rng, Rng};
use rayon::prelude::*;
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use std::fs::{create_dir_all, remove_dir_all};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

const ACCOUNT_DATA_FILE_SIZE: u64 = 64 * 1024 * 1024;
const ACCOUNT_DATA_FILE: &str = "data";

#[derive(Debug, Default)]
pub struct ErrorCounters {
    pub account_not_found: usize,
    pub account_in_use: usize,
    pub account_loaded_twice: usize,
    pub blockhash_not_found: usize,
    pub blockhash_too_old: usize,
    pub reserve_blockhash: usize,
    pub insufficient_funds: usize,
    pub invalid_account_index: usize,
    pub duplicate_signature: usize,
    pub call_chain_too_deep: usize,
    pub missing_signature_for_fee: usize,
}

#[derive(Default, Clone)]
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
type AppendVecId = usize;
pub type AccountStorage = HashMap<usize, Arc<AccountStorageEntry>>;
pub type InstructionAccounts = Vec<Account>;
pub type InstructionLoaders = Vec<Vec<(Pubkey, Account)>>;

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum AccountStorageStatus {
    StorageAvailable = 0,
    StorageFull = 1,
}

/// Persistent storage structure holding the accounts
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
        let p = format!("{}/{}", path, id);
        let path = Path::new(&p);
        let _ignored = remove_dir_all(path);
        create_dir_all(path).expect("Create directory failed");
        let accounts = AppendVec::new(&path.join(ACCOUNT_DATA_FILE), true, file_size as usize);

        AccountStorageEntry {
            id,
            fork_id,
            accounts,
            count_and_status: RwLock::new((0, AccountStorageStatus::StorageAvailable)),
        }
    }

    pub fn set_status(&self, mut status: AccountStorageStatus) {
        let mut count_and_status = self.count_and_status.write().unwrap();

        let count = count_and_status.0;

        if status == AccountStorageStatus::StorageFull && count == 0 {
            // this case arises when the append_vec is full (store_ptrs fails),
            //  but all accounts have already been removed from the storage
            //
            // the only time it's safe to call reset() on an append_vec is when
            //  every account has been removed
            //          **and**
            //  the append_vec has previously been completely full
            //
            self.accounts.reset();
            status = AccountStorageStatus::StorageAvailable;
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

    fn remove_account(&self) {
        let mut count_and_status = self.count_and_status.write().unwrap();
        let (count, mut status) = *count_and_status;

        if count == 1 && status == AccountStorageStatus::StorageFull {
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
            status = AccountStorageStatus::StorageAvailable;
        }

        *count_and_status = (count - 1, status);
    }
}

// This structure handles the load/store of the accounts
#[derive(Default)]
pub struct AccountsDB {
    /// Keeps tracks of index into AppendVec on a per fork basis
    pub accounts_index: RwLock<AccountsIndex<AccountInfo>>,

    /// Account storage
    pub storage: RwLock<AccountStorage>,

    /// distribute the accounts across storage lists
    next_id: AtomicUsize,

    /// write version
    write_version: AtomicUsize,

    /// Set of storage paths to pick from
    paths: Vec<String>,

    /// Starting file size of appendvecs
    file_size: u64,
}

pub fn get_paths_vec(paths: &str) -> Vec<String> {
    paths.split(',').map(ToString::to_string).collect()
}

impl AccountsDB {
    pub fn new_with_file_size(paths: &str, file_size: u64) -> Self {
        let paths = get_paths_vec(&paths);
        AccountsDB {
            accounts_index: RwLock::new(AccountsIndex::default()),
            storage: RwLock::new(HashMap::new()),
            next_id: AtomicUsize::new(0),
            write_version: AtomicUsize::new(0),
            paths,
            file_size,
        }
    }

    pub fn new(paths: &str) -> Self {
        Self::new_with_file_size(paths, ACCOUNT_DATA_FILE_SIZE)
    }

    fn new_storage_entry(&self, fork_id: Fork, path: &str) -> AccountStorageEntry {
        AccountStorageEntry::new(
            path,
            fork_id,
            self.next_id.fetch_add(1, Ordering::Relaxed),
            self.file_size,
        )
    }

    pub fn has_accounts(&self, fork: Fork) -> bool {
        for x in self.storage.read().unwrap().values() {
            if x.fork_id == fork && x.count() > 0 {
                return true;
            }
        }
        false
    }

    /// Scan a specific fork through all the account storage in parallel with sequential read
    // PERF: Sequentially read each storage entry in parallel
    pub fn scan_account_storage<F, B>(&self, fork_id: Fork, scan_func: F) -> Vec<B>
    where
        F: Fn(&StoredAccount, &mut B) -> (),
        F: Send + Sync,
        B: Send + Default,
    {
        let storage_maps: Vec<Arc<AccountStorageEntry>> = self
            .storage
            .read()
            .unwrap()
            .values()
            .filter(|store| store.fork_id == fork_id)
            .cloned()
            .collect();
        storage_maps
            .into_par_iter()
            .map(|storage| {
                let accounts = storage.accounts.accounts(0);
                let mut retval = B::default();
                accounts
                    .iter()
                    .for_each(|stored_account| scan_func(stored_account, &mut retval));
                retval
            })
            .collect()
    }

    pub fn load(
        storage: &AccountStorage,
        ancestors: &HashMap<Fork, usize>,
        accounts_index: &AccountsIndex<AccountInfo>,
        pubkey: &Pubkey,
    ) -> Option<(Account, Fork)> {
        let (info, fork) = accounts_index.get(pubkey, ancestors)?;
        //TODO: thread this as a ref
        storage
            .get(&info.id)
            .and_then(|store| Some(store.accounts.get_account(info.offset)?.0.clone_account()))
            .map(|account| (account, fork))
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

    fn fork_storage(&self, fork_id: Fork) -> Arc<AccountStorageEntry> {
        let mut candidates: Vec<Arc<AccountStorageEntry>> = {
            let stores = self.storage.read().unwrap();
            stores
                .values()
                .filter_map(|x| {
                    if x.status() == AccountStorageStatus::StorageAvailable && x.fork_id == fork_id
                    {
                        Some(x.clone())
                    } else {
                        None
                    }
                })
                .collect()
        };
        if candidates.is_empty() {
            let mut stores = self.storage.write().unwrap();
            let path_idx = thread_rng().gen_range(0, self.paths.len());
            let storage = Arc::new(self.new_storage_entry(fork_id, &self.paths[path_idx]));
            stores.insert(storage.id, storage.clone());
            candidates.push(storage);
        }
        let rv = thread_rng().gen_range(0, candidates.len());
        candidates[rv].clone()
    }

    pub fn purge_fork(&self, fork: Fork) {
        //add_root should be called first
        let is_root = self.accounts_index.read().unwrap().is_root(fork);
        trace!("PURGING {} {}", fork, is_root);
        if !is_root {
            self.storage.write().unwrap().retain(|_, v| {
                trace!("PURGING {} {}", v.fork_id, fork);
                v.fork_id != fork
            });
        }
    }

    fn store_accounts(&self, fork_id: Fork, accounts: &[(&Pubkey, &Account)]) -> Vec<AccountInfo> {
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
            let storage = self.fork_storage(fork_id);
            let rvs = storage.accounts.append_accounts(&with_meta[infos.len()..]);
            if rvs.is_empty() {
                storage.set_status(AccountStorageStatus::StorageFull);
            }
            for (offset, (_, account)) in rvs.iter().zip(&with_meta[infos.len()..]) {
                storage.add_account();
                infos.push(AccountInfo {
                    id: storage.id,
                    offset: *offset,
                    lamports: account.lamports,
                });
            }
        }
        infos
    }

    fn update_index(
        &self,
        fork_id: Fork,
        infos: Vec<AccountInfo>,
        accounts: &[(&Pubkey, &Account)],
    ) -> Vec<(Fork, AccountInfo)> {
        let mut index = self.accounts_index.write().unwrap();
        let mut reclaims = vec![];
        for (i, info) in infos.into_iter().enumerate() {
            let key = &accounts[i].0;
            reclaims.extend(index.insert(fork_id, key, info).into_iter())
        }
        reclaims
    }

    fn remove_dead_accounts(&self, reclaims: Vec<(Fork, AccountInfo)>) -> HashSet<Fork> {
        let storage = self.storage.read().unwrap();
        for (fork_id, account_info) in reclaims {
            if let Some(store) = storage.get(&account_info.id) {
                assert_eq!(
                    fork_id, store.fork_id,
                    "AccountDB::accounts_index corrupted. Storage should only point to one fork"
                );
                store.remove_account();
            }
        }
        //TODO: performance here could be improved if AccountsDB::storage was organized by fork
        let dead_forks: HashSet<Fork> = storage
            .values()
            .filter_map(|x| {
                if x.count() == 0 {
                    Some(x.fork_id)
                } else {
                    None
                }
            })
            .collect();
        let live_forks: HashSet<Fork> = storage
            .values()
            .filter_map(|x| if x.count() > 0 { Some(x.fork_id) } else { None })
            .collect();
        dead_forks.difference(&live_forks).cloned().collect()
    }
    fn cleanup_dead_forks(&self, dead_forks: &mut HashSet<Fork>) {
        let mut index = self.accounts_index.write().unwrap();
        // a fork is not totally dead until it is older than the root
        dead_forks.retain(|fork| *fork < index.last_root);
        for fork in dead_forks.iter() {
            index.cleanup_dead_fork(*fork);
        }
    }

    /// Store the account update.
    pub fn store(&self, fork_id: Fork, accounts: &[(&Pubkey, &Account)]) {
        let infos = self.store_accounts(fork_id, accounts);
        let reclaims = self.update_index(fork_id, infos, accounts);
        trace!("reclaim: {}", reclaims.len());
        let mut dead_forks = self.remove_dead_accounts(reclaims);
        trace!("dead_forks: {}", dead_forks.len());
        self.cleanup_dead_forks(&mut dead_forks);
        trace!("purge_forks: {}", dead_forks.len());
        for fork in dead_forks {
            self.purge_fork(fork);
        }
    }

    pub fn add_root(&self, fork: Fork) {
        self.accounts_index.write().unwrap().add_root(fork)
    }
}

#[cfg(test)]
mod tests {
    // TODO: all the bank tests are bank specific, issue: 2194
    use super::*;
    use rand::{thread_rng, Rng};
    use solana_sdk::account::Account;

    fn cleanup_paths(paths: &str) {
        let paths = get_paths_vec(&paths);
        paths.iter().for_each(|p| {
            let _ignored = remove_dir_all(p);
        });
    }

    struct TempPaths {
        pub paths: String,
    }

    impl Drop for TempPaths {
        fn drop(&mut self) {
            cleanup_paths(&self.paths);
        }
    }

    fn get_tmp_accounts_path(paths: &str) -> TempPaths {
        let vpaths = get_paths_vec(paths);
        let out_dir = std::env::var("OUT_DIR").unwrap_or_else(|_| "target".to_string());
        let vpaths: Vec<_> = vpaths
            .iter()
            .map(|path| format!("{}/{}", out_dir, path))
            .collect();
        TempPaths {
            paths: vpaths.join(","),
        }
    }

    #[macro_export]
    macro_rules! tmp_accounts_name {
        () => {
            &format!("{}-{}", file!(), line!())
        };
    }

    #[macro_export]
    macro_rules! get_tmp_accounts_path {
        () => {
            get_tmp_accounts_path(tmp_accounts_name!())
        };
    }

    #[test]
    fn test_accountsdb_add_root() {
        solana_logger::setup();
        let paths = get_tmp_accounts_path!();
        let db = AccountsDB::new(&paths.paths);
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
        let paths = get_tmp_accounts_path!();
        let db = AccountsDB::new(&paths.paths);
        let key = Pubkey::default();
        let account0 = Account::new(1, 0, &key);

        db.store(0, &[(&key, &account0)]);

        let account1 = Account::new(0, 0, &key);
        db.store(1, &[(&key, &account1)]);

        let ancestors = vec![(1, 1)].into_iter().collect();
        assert_eq!(&db.load_slow(&ancestors, &key).unwrap().0, &account1);

        let ancestors = vec![(1, 1), (0, 0)].into_iter().collect();
        assert_eq!(&db.load_slow(&ancestors, &key).unwrap().0, &account1);
    }

    #[test]
    fn test_accountsdb_latest_ancestor_with_root() {
        solana_logger::setup();
        let paths = get_tmp_accounts_path!();
        let db = AccountsDB::new(&paths.paths);
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
    fn test_accountsdb_root_one_fork() {
        solana_logger::setup();
        let paths = get_tmp_accounts_path!();
        let db = AccountsDB::new(&paths.paths);
        let key = Pubkey::default();
        let account0 = Account::new(1, 0, &key);

        // store value 1 in the "root", i.e. db zero
        db.store(0, &[(&key, &account0)]);

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
        db.store(1, &[(&key, &account1)]);

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
        let paths = get_tmp_accounts_path!();
        let db = AccountsDB::new(&paths.paths);

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
        let paths = get_tmp_accounts_path!();
        let db = AccountsDB::new(&paths.paths);

        let mut pubkeys: Vec<Pubkey> = vec![];
        create_account(
            &db,
            &mut pubkeys,
            0,
            2,
            ACCOUNT_DATA_FILE_SIZE as usize / 3,
            0,
        );
        assert!(check_storage(&db, 2));

        let pubkey = Pubkey::new_rand();
        let account = Account::new(1, ACCOUNT_DATA_FILE_SIZE as usize / 3, &pubkey);
        db.store(1, &[(&pubkey, &account)]);
        db.store(1, &[(&pubkeys[0], &account)]);
        {
            let stores = db.storage.read().unwrap();
            assert_eq!(stores.len(), 2);
            assert_eq!(stores[&0].count(), 2);
            assert_eq!(stores[&1].count(), 2);
        }
        db.add_root(1);
        {
            let stores = db.storage.read().unwrap();
            assert_eq!(stores.len(), 2);
            assert_eq!(stores[&0].count(), 2);
            assert_eq!(stores[&1].count(), 2);
        }
    }

    #[test]
    fn test_accounts_unsquashed() {
        let key = Pubkey::default();

        // 1 token in the "root", i.e. db zero
        let paths = get_tmp_accounts_path!();
        let db0 = AccountsDB::new(&paths.paths);
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
        fork: Fork,
        num: usize,
        space: usize,
        num_vote: usize,
    ) {
        for t in 0..num {
            let pubkey = Pubkey::new_rand();
            let account = Account::new((t + 1) as u64, space, &Account::default().owner);
            pubkeys.push(pubkey.clone());
            let ancestors = vec![(fork, 0)].into_iter().collect();
            assert!(accounts.load_slow(&ancestors, &pubkey).is_none());
            accounts.store(fork, &[(&pubkey, &account)]);
        }
        for t in 0..num_vote {
            let pubkey = Pubkey::new_rand();
            let account = Account::new((num + t + 1) as u64, space, &solana_vote_api::id());
            pubkeys.push(pubkey.clone());
            let ancestors = vec![(fork, 0)].into_iter().collect();
            assert!(accounts.load_slow(&ancestors, &pubkey).is_none());
            accounts.store(fork, &[(&pubkey, &account)]);
        }
    }

    fn update_accounts(accounts: &AccountsDB, pubkeys: &Vec<Pubkey>, fork: Fork, range: usize) {
        for _ in 1..1000 {
            let idx = thread_rng().gen_range(0, range);
            let ancestors = vec![(fork, 0)].into_iter().collect();
            if let Some((mut account, _)) = accounts.load_slow(&ancestors, &pubkeys[idx]) {
                account.lamports = account.lamports + 1;
                accounts.store(fork, &[(&pubkeys[idx], &account)]);
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

    fn check_storage(accounts: &AccountsDB, count: usize) -> bool {
        let stores = accounts.storage.read().unwrap();
        assert_eq!(stores.len(), 1);
        assert_eq!(stores[&0].status(), AccountStorageStatus::StorageAvailable);
        stores[&0].count() == count
    }

    fn check_accounts(accounts: &AccountsDB, pubkeys: &Vec<Pubkey>, fork: Fork) {
        for _ in 1..100 {
            let idx = thread_rng().gen_range(0, 99);
            let ancestors = vec![(fork, 0)].into_iter().collect();
            let account = accounts.load_slow(&ancestors, &pubkeys[idx]).unwrap();
            let mut default_account = Account::default();
            default_account.lamports = (idx + 1) as u64;
            assert_eq!((default_account, 0), account);
        }
    }

    #[test]
    fn test_account_one() {
        let paths = get_tmp_accounts_path!();
        let accounts = AccountsDB::new(&paths.paths);
        let mut pubkeys: Vec<Pubkey> = vec![];
        create_account(&accounts, &mut pubkeys, 0, 1, 0, 0);
        let ancestors = vec![(0, 0)].into_iter().collect();
        let account = accounts.load_slow(&ancestors, &pubkeys[0]).unwrap();
        let mut default_account = Account::default();
        default_account.lamports = 1;
        assert_eq!((default_account, 0), account);
    }

    #[test]
    fn test_account_many() {
        let paths = get_tmp_accounts_path("many0,many1");
        let accounts = AccountsDB::new(&paths.paths);
        let mut pubkeys: Vec<Pubkey> = vec![];
        create_account(&accounts, &mut pubkeys, 0, 100, 0, 0);
        check_accounts(&accounts, &pubkeys, 0);
    }

    #[test]
    fn test_account_update() {
        let paths = get_tmp_accounts_path!();
        let accounts = AccountsDB::new(&paths.paths);
        let mut pubkeys: Vec<Pubkey> = vec![];
        create_account(&accounts, &mut pubkeys, 0, 100, 0, 0);
        update_accounts(&accounts, &pubkeys, 0, 99);
        assert_eq!(check_storage(&accounts, 100), true);
    }

    #[test]
    fn test_account_grow_many() {
        let paths = get_tmp_accounts_path("many2,many3");
        let size = 4096;
        let accounts = AccountsDB::new_with_file_size(&paths.paths, size);
        let mut keys = vec![];
        for i in 0..9 {
            let key = Pubkey::new_rand();
            let account = Account::new(i + 1, size as usize / 4, &key);
            accounts.store(0, &[(&key, &account)]);
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
        for storage in accounts.storage.read().unwrap().values() {
            *append_vec_histogram.entry(storage.fork_id).or_insert(0) += 1;
        }
        for count in append_vec_histogram.values() {
            assert!(*count >= 2);
        }
    }

    #[test]
    fn test_account_grow() {
        let paths = get_tmp_accounts_path!();
        let accounts = AccountsDB::new(&paths.paths);
        let count = [0, 1];
        let status = [
            AccountStorageStatus::StorageAvailable,
            AccountStorageStatus::StorageFull,
        ];
        let pubkey1 = Pubkey::new_rand();
        let account1 = Account::new(1, ACCOUNT_DATA_FILE_SIZE as usize / 2, &pubkey1);
        accounts.store(0, &[(&pubkey1, &account1)]);
        {
            let stores = accounts.storage.read().unwrap();
            assert_eq!(stores.len(), 1);
            assert_eq!(stores[&0].count(), 1);
            assert_eq!(stores[&0].status(), AccountStorageStatus::StorageAvailable);
        }

        let pubkey2 = Pubkey::new_rand();
        let account2 = Account::new(1, ACCOUNT_DATA_FILE_SIZE as usize / 2, &pubkey2);
        accounts.store(0, &[(&pubkey2, &account2)]);
        {
            let stores = accounts.storage.read().unwrap();
            assert_eq!(stores.len(), 2);
            assert_eq!(stores[&0].count(), 1);
            assert_eq!(stores[&0].status(), AccountStorageStatus::StorageFull);
            assert_eq!(stores[&1].count(), 1);
            assert_eq!(stores[&1].status(), AccountStorageStatus::StorageAvailable);
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
                assert_eq!(stores.len(), 3);
                assert_eq!(stores[&0].count(), count[index]);
                assert_eq!(stores[&0].status(), status[0]);
                assert_eq!(stores[&1].count(), 1);
                assert_eq!(stores[&1].status(), status[1]);
                assert_eq!(stores[&2].count(), count[index ^ 1]);
                assert_eq!(stores[&2].status(), status[0]);
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
        let paths = get_tmp_accounts_path!();
        let accounts = AccountsDB::new(&paths.paths);
        let mut pubkeys: Vec<Pubkey> = vec![];
        create_account(&accounts, &mut pubkeys, 0, 1, 0, 0);
        let ancestors = vec![(0, 0)].into_iter().collect();
        assert!(accounts.load_slow(&ancestors, &pubkeys[0]).is_some());;
        accounts.purge_fork(0);
        assert!(accounts.load_slow(&ancestors, &pubkeys[0]).is_none());;
    }

    #[test]
    fn test_purge_fork_after_root() {
        let paths = get_tmp_accounts_path!();
        let accounts = AccountsDB::new(&paths.paths);
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
        let paths = get_tmp_accounts_path!();
        let accounts = AccountsDB::new(&paths.paths);
        let pubkey = Pubkey::new_rand();
        let account = Account::new(1, 0, &Account::default().owner);
        //store an account
        accounts.store(0, &[(&pubkey, &account)]);
        let ancestors = vec![(0, 0)].into_iter().collect();
        let info = accounts
            .accounts_index
            .read()
            .unwrap()
            .get(&pubkey, &ancestors)
            .unwrap()
            .0
            .clone();
        //fork 0 is behind root, but it is not root, therefore it is purged
        accounts.add_root(1);
        assert!(accounts.accounts_index.read().unwrap().is_purged(0));

        //fork is still there, since gc is lazy
        assert!(accounts.storage.read().unwrap().get(&info.id).is_some());

        //store causes cleanup
        accounts.store(1, &[(&pubkey, &account)]);

        //fork is gone
        assert!(accounts.storage.read().unwrap().get(&info.id).is_none());

        //new value is there
        let ancestors = vec![(1, 1)].into_iter().collect();
        assert_eq!(accounts.load_slow(&ancestors, &pubkey), Some((account, 1)));
    }

}

use crate::appendvec::AppendVec;
use crate::bank::{BankError, Result};
use crate::runtime::has_duplicates;
use bincode::serialize;
use hashbrown::{HashMap, HashSet};
use log::*;
use solana_metrics::counter::Counter;
use solana_sdk::account::Account;
use solana_sdk::hash::{hash, Hash};
use solana_sdk::native_loader;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::transaction::Transaction;
use solana_sdk::vote_program;
use std::collections::BTreeMap;
use std::env;
use std::fs::{create_dir_all, remove_dir_all};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};

pub type InstructionAccounts = Vec<Account>;
pub type InstructionLoaders = Vec<Vec<(Pubkey, Account)>>;

#[derive(Debug, Default)]
pub struct ErrorCounters {
    pub account_not_found: usize,
    pub account_in_use: usize,
    pub account_loaded_twice: usize,
    pub last_id_not_found: usize,
    pub last_id_too_old: usize,
    pub reserve_last_id: usize,
    pub insufficient_funds: usize,
    pub duplicate_signature: usize,
    pub call_chain_too_deep: usize,
    pub missing_signature_for_fee: usize,
}

//
// Persistent accounts are stored in below path location:
//  <path>/<pid>/data/
//
// Each account is stored in below format:
//  <length><account>
//
// The persistent store would allow for this mode of operation:
//  - Concurrent single thread append with many concurrent readers.
//  - Exclusive resize or truncate from the start.
//
// The underlying memory is memory mapped to a file. The accounts would be
// stored across multiple files and the mappings of file and offset of a
// particular account would be stored in a shared index. This will allow for
// concurrent commits without blocking reads, which will sequentially write
// to memory, ssd or disk, and should be as fast as the hardware allow for.
// The only required in memory data structure with a write lock is the index,
// which should be fast to update.
//
// To garbage collect, data can be re-appended to defragmnted and truncated from
// the start. The AccountsDB data structure would allow for
// - multiple readers
// - multiple writers
// - persistent backed memory
//
// To bootstrap the index from a persistent store of AppendVec's, the entries should
// also include a "commit counter".  A single global atomic that tracks the number
// of commits to the entire data store. So the latest commit for each fork entry
// would be indexed. (TODO)

const ACCOUNT_DATA_FILE_SIZE: u64 = 64 * 1024 * 1024;
const ACCOUNT_DATA_FILE: &str = "data";
const ACCOUNTSDB_DIR: &str = "accountsdb";
const NUM_ACCOUNT_DIRS: usize = 4;

/// An offset into the AccountsDB::storage vector
type AppendVecId = usize;

type Fork = u64;

#[derive(Debug)]
struct AccountMap(RwLock<HashMap<Fork, (AppendVecId, u64)>>);

#[derive(Debug, PartialEq)]
enum AccountStorageStatus {
    StorageAvailable = 0,
    StorageFull = 1,
}

impl From<usize> for AccountStorageStatus {
    fn from(status: usize) -> Self {
        use self::AccountStorageStatus::*;
        match status {
            0 => StorageAvailable,
            1 => StorageFull,
            _ => unreachable!(),
        }
    }
}

struct AccountIndexInfo {
    /// For each Pubkey, the account for a specific fork is in a specific
    /// AppendVec at a specific index
    index: RwLock<HashMap<Pubkey, AccountMap>>,

    /// Cached index to vote accounts for performance reasons to avoid having
    /// to iterate through the entire accounts each time
    vote_index: RwLock<HashSet<Pubkey>>,
}

/// Persistent storage structure holding the accounts
struct AccountStorage {
    /// storage holding the accounts
    appendvec: Arc<RwLock<AppendVec<Account>>>,

    /// Keeps track of the number of accounts stored in a specific AppendVec.
    /// This is periodically checked to reuse the stores that do not have
    /// any accounts in it.
    count: AtomicUsize,

    /// status corresponding to the storage
    status: AtomicUsize,

    /// Path to the persistent store
    path: String,
}

impl AccountStorage {
    pub fn set_status(&self, status: AccountStorageStatus) {
        self.status.store(status as usize, Ordering::Relaxed);
    }

    pub fn get_status(&self) -> AccountStorageStatus {
        self.status.load(Ordering::Relaxed).into()
    }
}

#[derive(Default)]
struct AccountsForkInfo {
    /// The number of transactions the bank has processed without error since the
    /// start of the ledger.
    transaction_count: u64,

    /// List of all parents corresponding to this fork
    parents: Vec<Fork>,
}

// This structure handles the load/store of the accounts
pub struct AccountsDB {
    /// Keeps tracks of index into AppendVec on a per fork basis
    index_info: AccountIndexInfo,

    /// Account storage
    storage: RwLock<Vec<AccountStorage>>,

    /// distribute the accounts across storage lists
    next_id: AtomicUsize,

    /// Information related to the fork
    fork_info: RwLock<HashMap<Fork, AccountsForkInfo>>,
}

/// This structure handles synchronization for db
pub struct Accounts {
    pub accounts_db: AccountsDB,

    /// set of accounts which are currently in the pipeline
    account_locks: Mutex<HashMap<Fork, HashSet<Pubkey>>>,

    /// List of persistent stores
    paths: String,
}

fn get_paths_vec(paths: &str) -> Vec<String> {
    paths.split(',').map(|s| s.to_string()).collect()
}

fn cleanup_dirs(paths: &str) {
    let paths = get_paths_vec(&paths);
    paths.iter().for_each(|p| {
        let _ignored = remove_dir_all(p);
        let path = Path::new(p);
        let _ignored = remove_dir_all(path.parent().unwrap());
    });
}

impl Drop for Accounts {
    fn drop(&mut self) {
        cleanup_dirs(&self.paths);
    }
}

impl AccountsDB {
    pub fn new(fork: Fork, paths: &str) -> Self {
        let index_info = AccountIndexInfo {
            index: RwLock::new(HashMap::new()),
            vote_index: RwLock::new(HashSet::new()),
        };
        let accounts_db = AccountsDB {
            index_info,
            storage: RwLock::new(vec![]),
            next_id: AtomicUsize::new(0),
            fork_info: RwLock::new(HashMap::new()),
        };
        accounts_db.add_storage(paths);
        accounts_db.add_fork(fork, None);
        accounts_db
    }

    pub fn add_fork(&self, fork: Fork, parent: Option<Fork>) {
        let mut info = self.fork_info.write().unwrap();
        let mut fork_info = AccountsForkInfo::default();
        if parent.is_some() {
            fork_info.parents.push(parent.unwrap());
            if let Some(list) = info.get(&parent.unwrap()) {
                fork_info.parents.extend_from_slice(&list.parents);
            }
        }
        info.insert(fork, fork_info);
    }

    fn add_storage(&self, paths: &str) {
        let paths = get_paths_vec(&paths);
        let mut stores: Vec<AccountStorage> = vec![];
        paths.iter().for_each(|p| {
            let storage = AccountStorage {
                appendvec: self.new_account_storage(&p),
                status: AtomicUsize::new(AccountStorageStatus::StorageAvailable as usize),
                count: AtomicUsize::new(0),
                path: p.to_string(),
            };
            stores.push(storage);
        });
        let mut storage = self.storage.write().unwrap();
        storage.append(&mut stores);
    }

    fn new_account_storage(&self, p: &str) -> Arc<RwLock<AppendVec<Account>>> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let p = format!("{}/{}", p, id);
        let path = Path::new(&p);
        let _ignored = remove_dir_all(path);
        create_dir_all(path).expect("Create directory failed");
        Arc::new(RwLock::new(AppendVec::<Account>::new(
            &path.join(ACCOUNT_DATA_FILE),
            true,
            ACCOUNT_DATA_FILE_SIZE,
            0,
        )))
    }

    fn get_vote_accounts(&self, fork: Fork) -> HashMap<Pubkey, Account> {
        self.index_info
            .vote_index
            .read()
            .unwrap()
            .iter()
            .filter_map(|pubkey| {
                if let Some(account) = self.load(fork, pubkey, true) {
                    Some((*pubkey, account))
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn has_accounts(&self, fork: Fork) -> bool {
        let index = self.index_info.index.read().unwrap();

        for entry in index.values() {
            let account_map = entry.0.read().unwrap();
            if account_map.contains_key(&fork) {
                return true;
            }
        }
        false
    }

    pub fn hash_internal_state(&self, fork: Fork) -> Option<Hash> {
        let mut ordered_accounts = BTreeMap::new();
        let rindex = self.index_info.index.read().unwrap();
        rindex.iter().for_each(|(p, entry)| {
            let forks = entry.0.read().unwrap();
            if let Some((id, index)) = forks.get(&fork) {
                let account = self.storage.read().unwrap()[*id]
                    .appendvec
                    .read()
                    .unwrap()
                    .get_account(*index)
                    .unwrap();
                ordered_accounts.insert(*p, account);
            }
        });

        if ordered_accounts.is_empty() {
            return None;
        }
        Some(hash(&serialize(&ordered_accounts).unwrap()))
    }

    fn get_account(&self, id: AppendVecId, offset: u64) -> Account {
        let appendvec = &self.storage.read().unwrap()[id].appendvec;
        let av = appendvec.read().unwrap();
        av.get_account(offset).unwrap()
    }

    fn load(&self, fork: Fork, pubkey: &Pubkey, walk_back: bool) -> Option<Account> {
        let index = self.index_info.index.read().unwrap();
        if let Some(map) = index.get(pubkey) {
            let forks = map.0.read().unwrap();
            // find most recent fork that is an ancestor of current_fork
            if let Some((id, offset)) = forks.get(&fork) {
                return Some(self.get_account(*id, *offset));
            } else {
                if !walk_back {
                    return None;
                }
                let fork_info = self.fork_info.read().unwrap();
                if let Some(info) = fork_info.get(&fork) {
                    for parent_fork in info.parents.iter() {
                        if let Some((id, offset)) = forks.get(&parent_fork) {
                            return Some(self.get_account(*id, *offset));
                        }
                    }
                }
            }
        }
        None
    }

    fn get_storage_id(&self, start: usize, current: usize) -> usize {
        let mut id = current;
        let len: usize;
        {
            let stores = self.storage.read().unwrap();
            len = stores.len();
            if id == std::usize::MAX {
                id = start % len;
                if stores[id].get_status() == AccountStorageStatus::StorageAvailable {
                    return id;
                }
            } else {
                stores[id].set_status(AccountStorageStatus::StorageFull);
            }

            loop {
                id = (id + 1) % len;
                if stores[id].get_status() == AccountStorageStatus::StorageAvailable {
                    break;
                }
                if id == start % len {
                    break;
                }
            }
        }
        if id == start % len {
            let mut stores = self.storage.write().unwrap();
            // check if new store was already created
            if stores.len() == len {
                let storage = AccountStorage {
                    appendvec: self.new_account_storage(&stores[id].path),
                    count: AtomicUsize::new(0),
                    status: AtomicUsize::new(AccountStorageStatus::StorageAvailable as usize),
                    path: stores[id].path.clone(),
                };
                stores.push(storage);
            }
            id = stores.len() - 1;
        }
        id
    }

    fn append_account(&self, account: &Account) -> (usize, u64) {
        let offset: u64;
        let start = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut id = self.get_storage_id(start, std::usize::MAX);

        // Even if no tokens, need to preserve the account owner so
        // we can update the vote_index correctly if this account is purged
        // when squashing.
        let acc = &mut account.clone();
        if account.tokens == 0 {
            acc.userdata.resize(0, 0);
        }

        loop {
            let result: Option<u64>;
            {
                let av = &self.storage.read().unwrap()[id].appendvec;
                result = av.read().unwrap().append_account(acc);
            }
            if let Some(val) = result {
                offset = val;
                break;
            } else {
                id = self.get_storage_id(start, id);
            }
        }
        (id, offset)
    }

    fn remove_account_entries(&self, entries: &[Fork], map: &AccountMap) -> bool {
        let mut forks = map.0.write().unwrap();
        for fork in entries.iter() {
            if let Some((id, _)) = forks.remove(&fork) {
                let stores = self.storage.read().unwrap();
                if stores[id].count.fetch_sub(1, Ordering::Relaxed) == 1 {
                    stores[id].appendvec.write().unwrap().reset();
                    stores[id].set_status(AccountStorageStatus::StorageAvailable);
                }
            }
        }
        forks.is_empty()
    }

    fn account_map_is_empty(pubkey: &Pubkey, index: &HashMap<Pubkey, AccountMap>) -> bool {
        if let Some(account_map) = index.get(pubkey) {
            if account_map.0.read().unwrap().len() == 0 {
                return true;
            }
        }
        false
    }

    fn update_vote_cache(
        &self,
        account: &Account,
        index: &HashMap<Pubkey, AccountMap>,
        pubkey: &Pubkey,
    ) {
        if vote_program::check_id(&account.owner) {
            if Self::account_map_is_empty(pubkey, index) {
                self.index_info.vote_index.write().unwrap().remove(pubkey);
            } else {
                self.index_info.vote_index.write().unwrap().insert(*pubkey);
            }
        }
    }

    fn insert_account_entry(&self, fork: Fork, id: AppendVecId, offset: u64, map: &AccountMap) {
        let mut forks = map.0.write().unwrap();
        let stores = self.storage.read().unwrap();
        stores[id].count.fetch_add(1, Ordering::Relaxed);
        if let Some((old_id, _)) = forks.insert(fork, (id, offset)) {
            if stores[old_id].count.fetch_sub(1, Ordering::Relaxed) == 1 {
                stores[old_id].appendvec.write().unwrap().reset();
                stores[old_id].set_status(AccountStorageStatus::StorageAvailable);
            }
        }
    }

    /// Store the account update.
    fn store_account(&self, fork: Fork, pubkey: &Pubkey, account: &Account) {
        if account.tokens == 0 && self.is_squashed(fork) {
            // purge if balance is 0 and no checkpoints
            let index = self.index_info.index.read().unwrap();
            let map = index.get(&pubkey).unwrap();
            self.remove_account_entries(&[fork], &map);
            self.update_vote_cache(account, &index, pubkey);
        } else {
            let (id, offset) = self.append_account(account);

            let index = self.index_info.index.read().unwrap();

            let map = index.get(&pubkey).unwrap();
            self.insert_account_entry(fork, id, offset, &map);
            self.update_vote_cache(account, &index, pubkey);
        }
    }

    pub fn store(&self, fork: Fork, pubkey: &Pubkey, account: &Account) {
        {
            if !self.index_info.index.read().unwrap().contains_key(&pubkey) {
                let mut windex = self.index_info.index.write().unwrap();
                windex.insert(*pubkey, AccountMap(RwLock::new(HashMap::new())));
            }
        }
        self.store_account(fork, pubkey, account);
    }

    pub fn store_accounts(
        &self,
        fork: Fork,
        txs: &[Transaction],
        res: &[Result<()>],
        loaded: &[Result<(InstructionAccounts, InstructionLoaders)>],
    ) {
        let mut keys = vec![];
        {
            let index = self.index_info.index.read().unwrap();
            for (i, raccs) in loaded.iter().enumerate() {
                if res[i].is_err() || raccs.is_err() {
                    continue;
                }
                let tx = &txs[i];
                for key in tx.account_keys.iter() {
                    if !index.contains_key(&key) {
                        keys.push(*key);
                    }
                }
            }
        }
        if !keys.is_empty() {
            let mut index = self.index_info.index.write().unwrap();
            for key in keys.iter() {
                index.insert(*key, AccountMap(RwLock::new(HashMap::new())));
            }
        }
        for (i, raccs) in loaded.iter().enumerate() {
            if res[i].is_err() || raccs.is_err() {
                continue;
            }

            let tx = &txs[i];
            let acc = raccs.as_ref().unwrap();
            for (key, account) in tx.account_keys.iter().zip(acc.0.iter()) {
                self.store(fork, key, account);
            }
        }
    }

    fn load_tx_accounts(
        &self,
        fork: Fork,
        tx: &Transaction,
        error_counters: &mut ErrorCounters,
    ) -> Result<Vec<Account>> {
        // Copy all the accounts
        if tx.signatures.is_empty() && tx.fee != 0 {
            Err(BankError::MissingSignatureForFee)
        } else {
            // Check for unique account keys
            if has_duplicates(&tx.account_keys) {
                error_counters.account_loaded_twice += 1;
                return Err(BankError::AccountLoadedTwice);
            }

            // There is no way to predict what program will execute without an error
            // If a fee can pay for execution then the program will be scheduled
            let mut called_accounts: Vec<Account> = vec![];
            for key in &tx.account_keys {
                called_accounts.push(self.load(fork, key, true).unwrap_or_default());
            }
            if called_accounts.is_empty() || called_accounts[0].tokens == 0 {
                error_counters.account_not_found += 1;
                Err(BankError::AccountNotFound)
            } else if called_accounts[0].tokens < tx.fee {
                error_counters.insufficient_funds += 1;
                Err(BankError::InsufficientFundsForFee)
            } else {
                called_accounts[0].tokens -= tx.fee;
                Ok(called_accounts)
            }
        }
    }

    fn load_executable_accounts(
        &self,
        fork: Fork,
        mut program_id: Pubkey,
        error_counters: &mut ErrorCounters,
    ) -> Result<Vec<(Pubkey, Account)>> {
        let mut accounts = Vec::new();
        let mut depth = 0;
        loop {
            if native_loader::check_id(&program_id) {
                // at the root of the chain, ready to dispatch
                break;
            }

            if depth >= 5 {
                error_counters.call_chain_too_deep += 1;
                return Err(BankError::CallChainTooDeep);
            }
            depth += 1;

            let program = match self.load(fork, &program_id, true) {
                Some(program) => program,
                None => {
                    error_counters.account_not_found += 1;
                    return Err(BankError::AccountNotFound);
                }
            };
            if !program.executable || program.owner == Pubkey::default() {
                error_counters.account_not_found += 1;
                return Err(BankError::AccountNotFound);
            }

            // add loader to chain
            accounts.insert(0, (program_id, program.clone()));

            program_id = program.owner;
        }
        Ok(accounts)
    }

    /// For each program_id in the transaction, load its loaders.
    fn load_loaders(
        &self,
        fork: Fork,
        tx: &Transaction,
        error_counters: &mut ErrorCounters,
    ) -> Result<Vec<Vec<(Pubkey, Account)>>> {
        tx.instructions
            .iter()
            .map(|ix| {
                if tx.program_ids.len() <= ix.program_ids_index as usize {
                    error_counters.account_not_found += 1;
                    return Err(BankError::AccountNotFound);
                }
                let program_id = tx.program_ids[ix.program_ids_index as usize];
                self.load_executable_accounts(fork, program_id, error_counters)
            })
            .collect()
    }

    fn load_accounts(
        &self,
        fork: Fork,
        txs: &[Transaction],
        lock_results: Vec<Result<()>>,
        error_counters: &mut ErrorCounters,
    ) -> Vec<Result<(InstructionAccounts, InstructionLoaders)>> {
        txs.iter()
            .zip(lock_results.into_iter())
            .map(|etx| match etx {
                (tx, Ok(())) => {
                    let accounts = self.load_tx_accounts(fork, tx, error_counters)?;
                    let loaders = self.load_loaders(fork, tx, error_counters)?;
                    Ok((accounts, loaders))
                }
                (_, Err(e)) => Err(e),
            })
            .collect()
    }

    pub fn increment_transaction_count(&self, fork: Fork, tx_count: usize) {
        let mut info = self.fork_info.write().unwrap();
        let entry = info.entry(fork).or_insert(AccountsForkInfo::default());
        entry.transaction_count += tx_count as u64;
    }

    pub fn transaction_count(&self, fork: Fork) -> u64 {
        let info = self.fork_info.read().unwrap();
        if let Some(entry) = info.get(&fork) {
            entry.transaction_count
        } else {
            0
        }
    }

    fn remove_parents(&self, fork: Fork) -> Vec<Fork> {
        let mut info = self.fork_info.write().unwrap();
        let fork_info = info.get_mut(&fork).unwrap();
        fork_info.parents.split_off(0)
    }

    fn is_squashed(&self, fork: Fork) -> bool {
        self.fork_info
            .read()
            .unwrap()
            .get(&fork)
            .unwrap()
            .parents
            .is_empty()
    }

    fn get_merged_index(
        &self,
        fork: Fork,
        parents: &[Fork],
        map: &AccountMap,
    ) -> Option<(Fork, AppendVecId, u64)> {
        let forks = map.0.read().unwrap();
        if let Some((id, offset)) = forks.get(&fork) {
            return Some((fork, *id, *offset));
        } else {
            for parent_fork in parents.iter() {
                if let Some((id, offset)) = forks.get(parent_fork) {
                    return Some((*parent_fork, *id, *offset));
                }
            }
        }
        None
    }

    /// make fork a root, i.e. forget its heritage
    fn squash(&self, fork: Fork) {
        let parents = self.remove_parents(fork);
        let tx_count = parents
            .iter()
            .fold(0, |sum, parent| sum + self.transaction_count(*parent));
        self.increment_transaction_count(fork, tx_count as usize);

        // for every account in all the parents, load latest and update self if
        // absent
        let mut keys = vec![];
        {
            let index = self.index_info.index.read().unwrap();
            index.iter().for_each(|(pubkey, map)| {
                if let Some((parent_fork, id, offset)) = self.get_merged_index(fork, &parents, &map)
                {
                    if parent_fork != fork {
                        self.insert_account_entry(fork, id, offset, &map);
                    } else {
                        let account = self.get_account(id, offset);
                        if account.tokens == 0 {
                            if self.remove_account_entries(&[fork], &map) {
                                keys.push(pubkey.clone());
                            }
                            self.update_vote_cache(&account, &index, pubkey);
                        }
                    }
                }
            });
        }
        if !keys.is_empty() {
            let mut index = self.index_info.index.write().unwrap();
            for key in keys.iter() {
                index.remove(&key);
            }
        }
    }
}

impl Accounts {
    fn make_new_dir() -> String {
        static ACCOUNT_DIR: AtomicUsize = AtomicUsize::new(0);
        let dir = ACCOUNT_DIR.fetch_add(1, Ordering::Relaxed);
        let out_dir = env::var("OUT_DIR").unwrap_or_else(|_| "target".to_string());
        let keypair = Keypair::new();
        format!(
            "{}/{}/{}/{}",
            out_dir,
            ACCOUNTSDB_DIR,
            keypair.pubkey(),
            dir.to_string()
        )
    }

    fn make_default_paths() -> String {
        let mut paths = "".to_string();
        for index in 0..NUM_ACCOUNT_DIRS {
            if index > 0 {
                paths.push_str(",");
            }
            paths.push_str(&Self::make_new_dir());
        }
        paths
    }

    pub fn new(fork: Fork, in_paths: Option<String>) -> Self {
        let paths = if in_paths.is_none() {
            Self::make_default_paths()
        } else {
            in_paths.unwrap()
        };
        let accounts_db = AccountsDB::new(fork, &paths);
        accounts_db.add_fork(fork, None);
        Accounts {
            accounts_db,
            account_locks: Mutex::new(HashMap::new()),
            paths,
        }
    }

    pub fn new_from_parent(&self, fork: Fork, parent: Fork) {
        self.accounts_db.add_fork(fork, Some(parent));
    }

    /// Slow because lock is held for 1 operation insted of many
    pub fn load_slow(&self, fork: Fork, pubkey: &Pubkey) -> Option<Account> {
        self.accounts_db
            .load(fork, pubkey, true)
            .filter(|acc| acc.tokens != 0)
    }

    /// Slow because lock is held for 1 operation insted of many
    pub fn load_slow_no_parent(&self, fork: Fork, pubkey: &Pubkey) -> Option<Account> {
        self.accounts_db
            .load(fork, pubkey, false)
            .filter(|acc| acc.tokens != 0)
    }

    /// Slow because lock is held for 1 operation insted of many
    pub fn store_slow(&self, fork: Fork, pubkey: &Pubkey, account: &Account) {
        self.accounts_db.store(fork, pubkey, account);
    }

    fn lock_account(
        fork: Fork,
        account_locks: &mut HashMap<Fork, HashSet<Pubkey>>,
        keys: &[Pubkey],
        error_counters: &mut ErrorCounters,
    ) -> Result<()> {
        // Copy all the accounts
        let locks = account_locks.entry(fork).or_insert(HashSet::new());
        for k in keys {
            if locks.contains(k) {
                error_counters.account_in_use += 1;
                return Err(BankError::AccountInUse);
            }
        }
        for k in keys {
            locks.insert(*k);
        }
        Ok(())
    }

    fn unlock_account(
        fork: Fork,
        tx: &Transaction,
        result: &Result<()>,
        account_locks: &mut HashMap<Fork, HashSet<Pubkey>>,
    ) {
        match result {
            Err(BankError::AccountInUse) => (),
            _ => {
                if let Some(locks) = account_locks.get_mut(&fork) {
                    for k in &tx.account_keys {
                        locks.remove(k);
                    }
                    if locks.is_empty() {
                        account_locks.remove(&fork);
                    }
                }
            }
        }
    }

    pub fn hash_internal_state(&self, fork: Fork) -> Option<Hash> {
        self.accounts_db.hash_internal_state(fork)
    }

    /// This function will prevent multiple threads from modifying the same account state at the
    /// same time
    #[must_use]
    pub fn lock_accounts(&self, fork: Fork, txs: &[Transaction]) -> Vec<Result<()>> {
        let mut account_locks = self.account_locks.lock().unwrap();
        let mut error_counters = ErrorCounters::default();
        let rv = txs
            .iter()
            .map(|tx| {
                Self::lock_account(
                    fork,
                    &mut account_locks,
                    &tx.account_keys,
                    &mut error_counters,
                )
            })
            .collect();
        if error_counters.account_in_use != 0 {
            inc_new_counter_info!(
                "bank-process_transactions-account_in_use",
                error_counters.account_in_use
            );
        }
        rv
    }

    /// Once accounts are unlocked, new transactions that modify that state can enter the pipeline
    pub fn unlock_accounts(&self, fork: Fork, txs: &[Transaction], results: &[Result<()>]) {
        let mut account_locks = self.account_locks.lock().unwrap();
        debug!("bank unlock accounts");
        txs.iter()
            .zip(results.iter())
            .for_each(|(tx, result)| Self::unlock_account(fork, tx, result, &mut account_locks));
    }

    pub fn has_accounts(&self, fork: Fork) -> bool {
        self.accounts_db.has_accounts(fork)
    }

    pub fn load_accounts(
        &self,
        fork: Fork,
        txs: &[Transaction],
        results: Vec<Result<()>>,
        error_counters: &mut ErrorCounters,
    ) -> Vec<Result<(InstructionAccounts, InstructionLoaders)>> {
        self.accounts_db
            .load_accounts(fork, txs, results, error_counters)
    }

    /// Store the accounts into the DB
    pub fn store_accounts(
        &self,
        fork: Fork,
        txs: &[Transaction],
        res: &[Result<()>],
        loaded: &[Result<(InstructionAccounts, InstructionLoaders)>],
    ) {
        self.accounts_db.store_accounts(fork, txs, res, loaded)
    }

    pub fn increment_transaction_count(&self, fork: Fork, tx_count: usize) {
        self.accounts_db.increment_transaction_count(fork, tx_count)
    }

    pub fn transaction_count(&self, fork: Fork) -> u64 {
        self.accounts_db.transaction_count(fork)
    }

    /// accounts starts with an empty data structure for every child/fork
    ///   this function squashes all the parents into this instance
    pub fn squash(&self, fork: Fork) {
        assert!(!self.account_locks.lock().unwrap().contains_key(&fork));
        self.accounts_db.squash(fork);
    }

    pub fn get_vote_accounts(&self, fork: Fork) -> HashMap<Pubkey, Account> {
        self.accounts_db
            .get_vote_accounts(fork)
            .into_iter()
            .filter(|(_, acc)| acc.tokens != 0)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    // TODO: all the bank tests are bank specific, issue: 2194

    use super::*;
    use rand::{thread_rng, Rng};
    use solana_sdk::account::Account;
    use solana_sdk::hash::Hash;
    use solana_sdk::signature::Keypair;
    use solana_sdk::signature::KeypairUtil;
    use solana_sdk::transaction::Instruction;
    use solana_sdk::transaction::Transaction;

    fn load_accounts(
        tx: Transaction,
        ka: &Vec<(Pubkey, Account)>,
        error_counters: &mut ErrorCounters,
    ) -> Vec<Result<(InstructionAccounts, InstructionLoaders)>> {
        let accounts = Accounts::new(0, None);
        for ka in ka.iter() {
            accounts.store_slow(0, &ka.0, &ka.1);
        }

        let res = accounts.load_accounts(0, &[tx], vec![Ok(())], error_counters);
        res
    }

    #[test]
    fn test_load_accounts_no_key() {
        let accounts: Vec<(Pubkey, Account)> = Vec::new();
        let mut error_counters = ErrorCounters::default();

        let instructions = vec![Instruction::new(1, &(), vec![0])];
        let tx = Transaction::new_with_instructions::<Keypair>(
            &[],
            &[],
            Hash::default(),
            0,
            vec![native_loader::id()],
            instructions,
        );

        let loaded_accounts = load_accounts(tx, &accounts, &mut error_counters);

        assert_eq!(error_counters.account_not_found, 1);
        assert_eq!(loaded_accounts.len(), 1);
        assert_eq!(loaded_accounts[0], Err(BankError::AccountNotFound));
    }

    #[test]
    fn test_load_accounts_no_account_0_exists() {
        let accounts: Vec<(Pubkey, Account)> = Vec::new();
        let mut error_counters = ErrorCounters::default();

        let keypair = Keypair::new();

        let instructions = vec![Instruction::new(1, &(), vec![0])];
        let tx = Transaction::new_with_instructions(
            &[&keypair],
            &[],
            Hash::default(),
            0,
            vec![native_loader::id()],
            instructions,
        );

        let loaded_accounts = load_accounts(tx, &accounts, &mut error_counters);

        assert_eq!(error_counters.account_not_found, 1);
        assert_eq!(loaded_accounts.len(), 1);
        assert_eq!(loaded_accounts[0], Err(BankError::AccountNotFound));
    }

    #[test]
    fn test_load_accounts_unknown_program_id() {
        let mut accounts: Vec<(Pubkey, Account)> = Vec::new();
        let mut error_counters = ErrorCounters::default();

        let keypair = Keypair::new();
        let key0 = keypair.pubkey();
        let key1 = Pubkey::new(&[5u8; 32]);

        let account = Account::new(1, 1, Pubkey::default());
        accounts.push((key0, account));

        let account = Account::new(2, 1, Pubkey::default());
        accounts.push((key1, account));

        let instructions = vec![Instruction::new(1, &(), vec![0])];
        let tx = Transaction::new_with_instructions(
            &[&keypair],
            &[],
            Hash::default(),
            0,
            vec![Pubkey::default()],
            instructions,
        );

        let loaded_accounts = load_accounts(tx, &accounts, &mut error_counters);

        assert_eq!(error_counters.account_not_found, 1);
        assert_eq!(loaded_accounts.len(), 1);
        assert_eq!(loaded_accounts[0], Err(BankError::AccountNotFound));
    }

    #[test]
    fn test_load_accounts_insufficient_funds() {
        let mut accounts: Vec<(Pubkey, Account)> = Vec::new();
        let mut error_counters = ErrorCounters::default();

        let keypair = Keypair::new();
        let key0 = keypair.pubkey();

        let account = Account::new(1, 1, Pubkey::default());
        accounts.push((key0, account));

        let instructions = vec![Instruction::new(1, &(), vec![0])];
        let tx = Transaction::new_with_instructions(
            &[&keypair],
            &[],
            Hash::default(),
            10,
            vec![native_loader::id()],
            instructions,
        );

        let loaded_accounts = load_accounts(tx, &accounts, &mut error_counters);

        assert_eq!(error_counters.insufficient_funds, 1);
        assert_eq!(loaded_accounts.len(), 1);
        assert_eq!(loaded_accounts[0], Err(BankError::InsufficientFundsForFee));
    }

    #[test]
    fn test_load_accounts_no_loaders() {
        let mut accounts: Vec<(Pubkey, Account)> = Vec::new();
        let mut error_counters = ErrorCounters::default();

        let keypair = Keypair::new();
        let key0 = keypair.pubkey();
        let key1 = Pubkey::new(&[5u8; 32]);

        let account = Account::new(1, 1, Pubkey::default());
        accounts.push((key0, account));

        let account = Account::new(2, 1, Pubkey::default());
        accounts.push((key1, account));

        let instructions = vec![Instruction::new(0, &(), vec![0, 1])];
        let tx = Transaction::new_with_instructions(
            &[&keypair],
            &[key1],
            Hash::default(),
            0,
            vec![native_loader::id()],
            instructions,
        );

        let loaded_accounts = load_accounts(tx, &accounts, &mut error_counters);

        assert_eq!(error_counters.account_not_found, 0);
        assert_eq!(loaded_accounts.len(), 1);
        match &loaded_accounts[0] {
            Ok((a, l)) => {
                assert_eq!(a.len(), 2);
                assert_eq!(a[0], accounts[0].1);
                assert_eq!(l.len(), 1);
                assert_eq!(l[0].len(), 0);
            }
            Err(e) => Err(e).unwrap(),
        }
    }

    #[test]
    fn test_load_accounts_max_call_depth() {
        let mut accounts: Vec<(Pubkey, Account)> = Vec::new();
        let mut error_counters = ErrorCounters::default();

        let keypair = Keypair::new();
        let key0 = keypair.pubkey();
        let key1 = Pubkey::new(&[5u8; 32]);
        let key2 = Pubkey::new(&[6u8; 32]);
        let key3 = Pubkey::new(&[7u8; 32]);
        let key4 = Pubkey::new(&[8u8; 32]);
        let key5 = Pubkey::new(&[9u8; 32]);
        let key6 = Pubkey::new(&[10u8; 32]);

        let account = Account::new(1, 1, Pubkey::default());
        accounts.push((key0, account));

        let mut account = Account::new(40, 1, Pubkey::default());
        account.executable = true;
        account.owner = native_loader::id();
        accounts.push((key1, account));

        let mut account = Account::new(41, 1, Pubkey::default());
        account.executable = true;
        account.owner = key1;
        accounts.push((key2, account));

        let mut account = Account::new(42, 1, Pubkey::default());
        account.executable = true;
        account.owner = key2;
        accounts.push((key3, account));

        let mut account = Account::new(43, 1, Pubkey::default());
        account.executable = true;
        account.owner = key3;
        accounts.push((key4, account));

        let mut account = Account::new(44, 1, Pubkey::default());
        account.executable = true;
        account.owner = key4;
        accounts.push((key5, account));

        let mut account = Account::new(45, 1, Pubkey::default());
        account.executable = true;
        account.owner = key5;
        accounts.push((key6, account));

        let instructions = vec![Instruction::new(0, &(), vec![0])];
        let tx = Transaction::new_with_instructions(
            &[&keypair],
            &[],
            Hash::default(),
            0,
            vec![key6],
            instructions,
        );

        let loaded_accounts = load_accounts(tx, &accounts, &mut error_counters);

        assert_eq!(error_counters.call_chain_too_deep, 1);
        assert_eq!(loaded_accounts.len(), 1);
        assert_eq!(loaded_accounts[0], Err(BankError::CallChainTooDeep));
    }

    #[test]
    fn test_load_accounts_bad_program_id() {
        let mut accounts: Vec<(Pubkey, Account)> = Vec::new();
        let mut error_counters = ErrorCounters::default();

        let keypair = Keypair::new();
        let key0 = keypair.pubkey();
        let key1 = Pubkey::new(&[5u8; 32]);

        let account = Account::new(1, 1, Pubkey::default());
        accounts.push((key0, account));

        let mut account = Account::new(40, 1, Pubkey::default());
        account.executable = true;
        account.owner = Pubkey::default();
        accounts.push((key1, account));

        let instructions = vec![Instruction::new(0, &(), vec![0])];
        let tx = Transaction::new_with_instructions(
            &[&keypair],
            &[],
            Hash::default(),
            0,
            vec![key1],
            instructions,
        );

        let loaded_accounts = load_accounts(tx, &accounts, &mut error_counters);

        assert_eq!(error_counters.account_not_found, 1);
        assert_eq!(loaded_accounts.len(), 1);
        assert_eq!(loaded_accounts[0], Err(BankError::AccountNotFound));
    }

    #[test]
    fn test_load_accounts_not_executable() {
        let mut accounts: Vec<(Pubkey, Account)> = Vec::new();
        let mut error_counters = ErrorCounters::default();

        let keypair = Keypair::new();
        let key0 = keypair.pubkey();
        let key1 = Pubkey::new(&[5u8; 32]);

        let account = Account::new(1, 1, Pubkey::default());
        accounts.push((key0, account));

        let mut account = Account::new(40, 1, Pubkey::default());
        account.owner = native_loader::id();
        accounts.push((key1, account));

        let instructions = vec![Instruction::new(0, &(), vec![0])];
        let tx = Transaction::new_with_instructions(
            &[&keypair],
            &[],
            Hash::default(),
            0,
            vec![key1],
            instructions,
        );

        let loaded_accounts = load_accounts(tx, &accounts, &mut error_counters);

        assert_eq!(error_counters.account_not_found, 1);
        assert_eq!(loaded_accounts.len(), 1);
        assert_eq!(loaded_accounts[0], Err(BankError::AccountNotFound));
    }

    #[test]
    fn test_load_accounts_multiple_loaders() {
        let mut accounts: Vec<(Pubkey, Account)> = Vec::new();
        let mut error_counters = ErrorCounters::default();

        let keypair = Keypair::new();
        let key0 = keypair.pubkey();
        let key1 = Pubkey::new(&[5u8; 32]);
        let key2 = Pubkey::new(&[6u8; 32]);
        let key3 = Pubkey::new(&[7u8; 32]);

        let account = Account::new(1, 1, Pubkey::default());
        accounts.push((key0, account));

        let mut account = Account::new(40, 1, Pubkey::default());
        account.executable = true;
        account.owner = native_loader::id();
        accounts.push((key1, account));

        let mut account = Account::new(41, 1, Pubkey::default());
        account.executable = true;
        account.owner = key1;
        accounts.push((key2, account));

        let mut account = Account::new(42, 1, Pubkey::default());
        account.executable = true;
        account.owner = key2;
        accounts.push((key3, account));

        let instructions = vec![
            Instruction::new(0, &(), vec![0]),
            Instruction::new(1, &(), vec![0]),
        ];
        let tx = Transaction::new_with_instructions(
            &[&keypair],
            &[],
            Hash::default(),
            0,
            vec![key1, key2],
            instructions,
        );

        let loaded_accounts = load_accounts(tx, &accounts, &mut error_counters);

        assert_eq!(error_counters.account_not_found, 0);
        assert_eq!(loaded_accounts.len(), 1);
        match &loaded_accounts[0] {
            Ok((a, l)) => {
                assert_eq!(a.len(), 1);
                assert_eq!(a[0], accounts[0].1);
                assert_eq!(l.len(), 2);
                assert_eq!(l[0].len(), 1);
                assert_eq!(l[1].len(), 2);
                for instruction_loaders in l.iter() {
                    for (i, a) in instruction_loaders.iter().enumerate() {
                        // +1 to skip first not loader account
                        assert_eq![a.1, accounts[i + 1].1];
                    }
                }
            }
            Err(e) => Err(e).unwrap(),
        }
    }

    #[test]
    fn test_load_account_pay_to_self() {
        let mut accounts: Vec<(Pubkey, Account)> = Vec::new();
        let mut error_counters = ErrorCounters::default();

        let keypair = Keypair::new();
        let pubkey = keypair.pubkey();

        let account = Account::new(10, 1, Pubkey::default());
        accounts.push((pubkey, account));

        let instructions = vec![Instruction::new(0, &(), vec![0, 1])];
        // Simulate pay-to-self transaction, which loads the same account twice
        let tx = Transaction::new_with_instructions(
            &[&keypair],
            &[pubkey],
            Hash::default(),
            0,
            vec![native_loader::id()],
            instructions,
        );
        let loaded_accounts = load_accounts(tx, &accounts, &mut error_counters);

        assert_eq!(error_counters.account_loaded_twice, 1);
        assert_eq!(loaded_accounts.len(), 1);
        loaded_accounts[0].clone().unwrap_err();
        assert_eq!(loaded_accounts[0], Err(BankError::AccountLoadedTwice));
    }

    #[test]
    fn test_accountsdb_squash_one_fork() {
        let paths = "squash_one_fork".to_string();
        let db = AccountsDB::new(0, &paths);
        let key = Pubkey::default();
        let account0 = Account::new(1, 0, key);

        // store value 1 in the "root", i.e. db zero
        db.store(0, &key, &account0);

        db.add_fork(1, Some(0));
        db.add_fork(2, Some(0));

        // now we have:
        //
        //                       root0 -> key.tokens==1
        //                        / \
        //                       /   \
        //  key.tokens==0 <- fork1    \
        //                             fork2 -> key.tokens==1
        //                                       (via root0)

        // store value 0 in one child
        let account1 = Account::new(0, 0, key);
        db.store(1, &key, &account1);

        // masking accounts is done at the Accounts level, at accountsDB we see
        // original account (but could also accept "None", which is implemented
        // at the Accounts level)
        assert_eq!(&db.load(1, &key, true).unwrap(), &account1);

        // we should see 1 token in fork 2
        assert_eq!(&db.load(2, &key, true).unwrap(), &account0);

        // squash, which should whack key's account
        db.squash(1);

        // now we should have:
        //
        //                          root0 -> key.tokens==1
        //                             \
        //                              \
        //  key.tokens==ANF <- root1     \
        //                              fork2 -> key.tokens==1 (from root0)
        //
        assert_eq!(db.load(1, &key, true), None); // purged
        assert_eq!(&db.load(2, &key, true).unwrap(), &account0); // original value

        cleanup_dirs(&paths);
    }

    #[test]
    fn test_accountsdb_squash() {
        let paths = "squash".to_string();
        let db = AccountsDB::new(0, &paths);

        let mut pubkeys: Vec<Pubkey> = vec![];
        create_account(&db, &mut pubkeys, 100, 0);
        for _ in 1..100 {
            let idx = thread_rng().gen_range(0, 99);
            let account = db.load(0, &pubkeys[idx], true).unwrap();
            let mut default_account = Account::default();
            default_account.tokens = (idx + 1) as u64;
            assert_eq!(compare_account(&default_account, &account), true);
        }
        db.add_fork(1, Some(0));

        // now we have:
        //
        //                        root0 -> key[X].tokens==X
        //                         /
        //                        /
        //  key[X].tokens==X <- fork1
        //    (via root0)
        //

        // merge, which should whack key's account
        db.squash(1);

        // now we should have:
        //                root0 -> purged ??
        //
        //
        //  key[X].tokens==X <- root1
        //

        // check that all the accounts appear in parent after a squash ???
        for _ in 1..100 {
            let idx = thread_rng().gen_range(0, 99);
            let account0 = db.load(0, &pubkeys[idx], true).unwrap();
            let account1 = db.load(1, &pubkeys[idx], true).unwrap();
            let mut default_account = Account::default();
            default_account.tokens = (idx + 1) as u64;
            assert_eq!(&default_account, &account0);
            assert_eq!(&default_account, &account1);
        }
        cleanup_dirs(&paths);
    }

    #[test]
    fn test_accounts_unsquashed() {
        let key = Pubkey::default();

        // 1 token in the "root", i.e. db zero
        let paths = "unsquash".to_string();
        let db0 = AccountsDB::new(0, &paths);
        let account0 = Account::new(1, 0, key);
        db0.store(0, &key, &account0);

        db0.add_fork(1, Some(0));
        // 0 tokens in the child
        let account1 = Account::new(0, 0, key);
        db0.store(1, &key, &account1);

        // masking accounts is done at the Accounts level, at accountsDB we see
        // original account
        assert_eq!(db0.load(1, &key, true), Some(account1));

        let mut accounts1 = Accounts::new(3, None);
        accounts1.accounts_db = db0;
        assert_eq!(accounts1.load_slow(1, &key), None);
        assert_eq!(accounts1.load_slow(0, &key), Some(account0));
        cleanup_dirs(&paths);
    }

    fn create_account(
        accounts: &AccountsDB,
        pubkeys: &mut Vec<Pubkey>,
        num: usize,
        num_vote: usize,
    ) {
        for t in 0..num {
            let pubkey = Keypair::new().pubkey();
            let mut default_account = Account::default();
            pubkeys.push(pubkey.clone());
            default_account.tokens = (t + 1) as u64;
            assert!(accounts.load(0, &pubkey, true).is_none());
            accounts.store(0, &pubkey, &default_account);
        }
        for t in 0..num_vote {
            let pubkey = Keypair::new().pubkey();
            let mut default_account = Account::default();
            pubkeys.push(pubkey.clone());
            default_account.owner = vote_program::id();
            default_account.tokens = (num + t + 1) as u64;
            assert!(accounts.load(0, &pubkey, true).is_none());
            accounts.store(0, &pubkey, &default_account);
        }
    }

    fn update_accounts(accounts: &AccountsDB, pubkeys: Vec<Pubkey>, range: usize) {
        for _ in 1..1000 {
            let idx = thread_rng().gen_range(0, range);
            if let Some(mut account) = accounts.load(0, &pubkeys[idx], true) {
                account.tokens = account.tokens + 1;
                accounts.store(0, &pubkeys[idx], &account);
                if account.tokens == 0 {
                    assert!(accounts.load(0, &pubkeys[idx], true).is_none());
                } else {
                    let mut default_account = Account::default();
                    default_account.tokens = account.tokens;
                    assert_eq!(compare_account(&default_account, &account), true);
                }
            }
        }
    }

    fn compare_account(account1: &Account, account2: &Account) -> bool {
        if account1.userdata != account2.userdata
            || account1.owner != account2.owner
            || account1.executable != account2.executable
            || account1.tokens != account2.tokens
        {
            return false;
        }
        true
    }

    #[test]
    fn test_account_one() {
        let paths = "one".to_string();
        let accounts = AccountsDB::new(0, &paths);
        let mut pubkeys: Vec<Pubkey> = vec![];
        create_account(&accounts, &mut pubkeys, 1, 0);
        let account = accounts.load(0, &pubkeys[0], true).unwrap();
        let mut default_account = Account::default();
        default_account.tokens = 1;
        assert_eq!(compare_account(&default_account, &account), true);
        cleanup_dirs(&paths);
    }

    #[test]
    fn test_account_many() {
        let paths = "many0,many1".to_string();
        let accounts = AccountsDB::new(0, &paths);
        let mut pubkeys: Vec<Pubkey> = vec![];
        create_account(&accounts, &mut pubkeys, 100, 0);
        for _ in 1..100 {
            let idx = thread_rng().gen_range(0, 99);
            let account = accounts.load(0, &pubkeys[idx], true).unwrap();
            let mut default_account = Account::default();
            default_account.tokens = (idx + 1) as u64;
            assert_eq!(compare_account(&default_account, &account), true);
        }
        cleanup_dirs(&paths);
    }

    #[test]
    fn test_account_update() {
        let paths = "update0".to_string();
        let accounts = AccountsDB::new(0, &paths);
        let mut pubkeys: Vec<Pubkey> = vec![];
        create_account(&accounts, &mut pubkeys, 100, 0);
        update_accounts(&accounts, pubkeys, 99);
        {
            let stores = accounts.storage.read().unwrap();
            assert_eq!(stores.len(), 1);
            assert_eq!(stores[0].count.load(Ordering::Relaxed), 100);
            assert_eq!(
                stores[0].get_status(),
                AccountStorageStatus::StorageAvailable
            );
        }
        cleanup_dirs(&paths);
    }

    #[test]
    fn test_account_grow() {
        let paths = "grow0".to_string();
        let accounts = AccountsDB::new(0, &paths);
        let count = [0, 1];
        let status = [
            AccountStorageStatus::StorageAvailable,
            AccountStorageStatus::StorageFull,
        ];
        let pubkey1 = Keypair::new().pubkey();
        let account1 = Account::new(1, ACCOUNT_DATA_FILE_SIZE as usize / 2, pubkey1);
        accounts.store(0, &pubkey1, &account1);
        {
            let stores = accounts.storage.read().unwrap();
            assert_eq!(stores.len(), 1);
            assert_eq!(stores[0].count.load(Ordering::Relaxed), 1);
            assert_eq!(stores[0].get_status(), status[0]);
        }

        let pubkey2 = Keypair::new().pubkey();
        let account2 = Account::new(1, ACCOUNT_DATA_FILE_SIZE as usize / 2, pubkey2);
        accounts.store(0, &pubkey2, &account2);
        {
            let stores = accounts.storage.read().unwrap();
            assert_eq!(stores.len(), 2);
            assert_eq!(stores[0].count.load(Ordering::Relaxed), 1);
            assert_eq!(stores[0].get_status(), status[1]);
            assert_eq!(stores[1].count.load(Ordering::Relaxed), 1);
            assert_eq!(stores[1].get_status(), status[0]);
        }
        assert_eq!(accounts.load(0, &pubkey1, true).unwrap(), account1);
        assert_eq!(accounts.load(0, &pubkey2, true).unwrap(), account2);

        for i in 0..25 {
            let index = i % 2;
            accounts.store(0, &pubkey1, &account1);
            {
                let stores = accounts.storage.read().unwrap();
                assert_eq!(stores.len(), 3);
                assert_eq!(stores[0].count.load(Ordering::Relaxed), count[index]);
                assert_eq!(stores[0].get_status(), status[0]);
                assert_eq!(stores[1].count.load(Ordering::Relaxed), 1);
                assert_eq!(stores[1].get_status(), status[1]);
                assert_eq!(stores[2].count.load(Ordering::Relaxed), count[index ^ 1]);
                assert_eq!(stores[2].get_status(), status[0]);
            }
            assert_eq!(accounts.load(0, &pubkey1, true).unwrap(), account1);
            assert_eq!(accounts.load(0, &pubkey2, true).unwrap(), account2);
        }
        cleanup_dirs(&paths);
    }

    #[test]
    fn test_accounts_vote_filter() {
        solana_logger::setup();
        let accounts = Accounts::new(0, None);
        let mut vote_account = Account::new(1, 0, vote_program::id());
        let key = Keypair::new().pubkey();
        accounts.store_slow(0, &key, &vote_account);

        accounts.new_from_parent(1, 0);

        assert_eq!(accounts.get_vote_accounts(1).len(), 1);

        vote_account.tokens = 0;
        accounts.store_slow(1, &key, &vote_account);

        assert_eq!(accounts.get_vote_accounts(1).len(), 0);

        let mut vote_account1 = Account::new(2, 0, vote_program::id());
        let key1 = Keypair::new().pubkey();
        accounts.store_slow(1, &key1, &vote_account1);

        accounts.squash(1);
        assert_eq!(accounts.get_vote_accounts(0).len(), 1);
        assert_eq!(accounts.get_vote_accounts(1).len(), 1);

        vote_account1.tokens = 0;
        accounts.store_slow(1, &key1, &vote_account1);
        accounts.store_slow(0, &key, &vote_account);

        assert_eq!(accounts.get_vote_accounts(1).len(), 0);
    }

    #[test]
    fn test_account_vote() {
        solana_logger::setup();
        let paths = "vote0".to_string();
        let accounts_db = AccountsDB::new(0, &paths);
        let mut pubkeys: Vec<Pubkey> = vec![];
        create_account(&accounts_db, &mut pubkeys, 0, 1);
        let accounts = accounts_db.get_vote_accounts(0);
        assert_eq!(accounts.len(), 1);
        accounts.iter().for_each(|(_, account)| {
            assert_eq!(account.owner, vote_program::id());
        });
        let lastkey = Keypair::new().pubkey();
        let mut lastaccount = Account::new(1, 0, vote_program::id());
        accounts_db.store(0, &lastkey, &lastaccount);
        assert_eq!(accounts_db.get_vote_accounts(0).len(), 2);

        accounts_db.add_fork(1, Some(0));

        assert_eq!(
            accounts_db.get_vote_accounts(1),
            accounts_db.get_vote_accounts(0)
        );

        info!("storing tokens=0 to fork 1");
        // should store a tokens=0 account in 1
        lastaccount.tokens = 0;
        accounts_db.store(1, &lastkey, &lastaccount);
        // len == 2 because tokens=0 accounts are filtered at the Accounts interface.
        assert_eq!(accounts_db.get_vote_accounts(1).len(), 2);

        accounts_db.squash(1);

        // should still be in 0
        assert_eq!(accounts_db.get_vote_accounts(0).len(), 2);

        // delete it from 0
        accounts_db.store(0, &lastkey, &lastaccount);
        assert_eq!(accounts_db.get_vote_accounts(0).len(), 1);
        assert_eq!(accounts_db.get_vote_accounts(1).len(), 1);

        // Add fork 2 and squash
        accounts_db.add_fork(2, Some(1));
        accounts_db.store(2, &lastkey, &lastaccount);

        accounts_db.squash(1);
        accounts_db.squash(2);

        assert_eq!(accounts_db.index_info.vote_index.read().unwrap().len(), 1);
        assert_eq!(accounts_db.get_vote_accounts(1).len(), 1);
        assert_eq!(accounts_db.get_vote_accounts(2).len(), 1);

        cleanup_dirs(&paths);
    }

    #[test]
    fn test_accounts_empty_hash_internal_state() {
        let paths = "empty_hash".to_string();
        let accounts = AccountsDB::new(0, &paths);
        assert_eq!(accounts.hash_internal_state(0), None);
        cleanup_dirs(&paths);
    }

}

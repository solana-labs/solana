use crate::append_vec::AppendVec;
use crate::bank::{Result, TransactionError};
use crate::runtime::has_duplicates;
use bincode::serialize;
use hashbrown::{HashMap, HashSet};
use log::*;
use rand::{thread_rng, Rng};
use solana_metrics::counter::Counter;
use solana_sdk::account::Account;
use solana_sdk::hash::{hash, Hash};
use solana_sdk::native_loader;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::transaction::Transaction;
use solana_vote_api;
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
    pub blockhash_not_found: usize,
    pub blockhash_too_old: usize,
    pub reserve_blockhash: usize,
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
// To garbage collect, data can be re-appended to defragmented and truncated from
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

// in a given a Fork, which AppendVecId and offset
type AccountMap = RwLock<HashMap<Fork, (AppendVecId, u64)>>;

/// information about where Accounts are stored and which vote accounts are present
/// keying hierarchy is:
///
///    pubkey->fork->append_vec->offset
///
#[derive(Default)]
struct AccountIndex {
    /// For each Pubkey, the Account for a specific Fork is in a specific
    ///  AppendVec at a specific index.  There may be an Account for Pubkey
    ///  in any number of Forks.
    account_maps: RwLock<HashMap<Pubkey, AccountMap>>,

    /// Cached index to vote accounts for performance reasons to avoid having
    ///  to iterate through the entire accounts each time
    vote_accounts: RwLock<HashSet<Pubkey>>,
}

/// Persistent storage structure holding the accounts
struct AccountStorageEntry {
    /// storage holding the accounts
    accounts: Arc<RwLock<AppendVec<Account>>>,

    /// Keeps track of the number of accounts stored in a specific AppendVec.
    /// This is periodically checked to reuse the stores that do not have
    /// any accounts in it.
    count: AtomicUsize,

    /// status corresponding to the storage
    status: AtomicUsize,
}

impl AccountStorageEntry {
    pub fn new(path: &str, id: usize, file_size: u64, inc_size: u64) -> Self {
        let p = format!("{}/{}", path, id);
        let path = Path::new(&p);
        let _ignored = remove_dir_all(path);
        create_dir_all(path).expect("Create directory failed");
        let accounts = Arc::new(RwLock::new(AppendVec::<Account>::new(
            &path.join(ACCOUNT_DATA_FILE),
            true,
            file_size,
            inc_size,
        )));

        AccountStorageEntry {
            accounts,
            count: AtomicUsize::new(0),
            status: AtomicUsize::new(AccountStorageStatus::StorageAvailable as usize),
        }
    }

    pub fn set_status(&self, status: AccountStorageStatus) {
        self.status.store(status as usize, Ordering::Relaxed);
    }

    pub fn get_status(&self) -> AccountStorageStatus {
        self.status.load(Ordering::Relaxed).into()
    }

    fn add_account(&self) {
        self.count.fetch_add(1, Ordering::Relaxed);
    }

    fn remove_account(&self) {
        if self.count.fetch_sub(1, Ordering::Relaxed) == 1 {
            self.accounts.write().unwrap().reset();
            self.set_status(AccountStorageStatus::StorageAvailable);
        }
    }
}

type AccountStorage = Vec<AccountStorageEntry>;

#[derive(Default, Debug)]
struct ForkInfo {
    /// The number of transactions processed without error
    transaction_count: u64,

    /// List of all parents of this fork
    parents: Vec<Fork>,
}

// This structure handles the load/store of the accounts
#[derive(Default)]
pub struct AccountsDB {
    /// Keeps tracks of index into AppendVec on a per fork basis
    account_index: AccountIndex,

    /// Account storage
    storage: RwLock<AccountStorage>,

    /// distribute the accounts across storage lists
    next_id: AtomicUsize,

    /// Information related to the fork
    fork_infos: RwLock<HashMap<Fork, ForkInfo>>,

    /// Set of storage paths to pick from
    paths: Vec<String>,

    /// Starting file size of appendvecs
    file_size: u64,

    /// Increment size of appendvecs
    inc_size: u64,
}

/// This structure handles synchronization for db
#[derive(Default)]
pub struct Accounts {
    pub accounts_db: AccountsDB,

    /// set of accounts which are currently in the pipeline
    account_locks: Mutex<HashMap<Fork, HashSet<Pubkey>>>,

    /// List of persistent stores
    paths: String,

    /// set to true if object created the directories in paths
    /// when true, delete parents of 'paths' on drop
    own_paths: bool,
}

fn get_paths_vec(paths: &str) -> Vec<String> {
    paths.split(',').map(|s| s.to_string()).collect()
}

impl Drop for Accounts {
    fn drop(&mut self) {
        let paths = get_paths_vec(&self.paths);
        paths.iter().for_each(|p| {
            let _ignored = remove_dir_all(p);

            // it is safe to delete the parent
            if self.own_paths {
                let path = Path::new(p);
                let _ignored = remove_dir_all(path.parent().unwrap());
            }
        });
    }
}

impl AccountsDB {
    pub fn new_with_file_size(fork: Fork, paths: &str, file_size: u64, inc_size: u64) -> Self {
        let account_index = AccountIndex {
            account_maps: RwLock::new(HashMap::new()),
            vote_accounts: RwLock::new(HashSet::new()),
        };
        let paths = get_paths_vec(&paths);
        let accounts_db = AccountsDB {
            account_index,
            storage: RwLock::new(vec![]),
            next_id: AtomicUsize::new(0),
            fork_infos: RwLock::new(HashMap::new()),
            paths,
            file_size,
            inc_size,
        };
        accounts_db.add_storage(&accounts_db.paths);
        accounts_db.add_fork(fork, None);
        accounts_db
    }

    pub fn new(fork: Fork, paths: &str) -> Self {
        Self::new_with_file_size(fork, paths, ACCOUNT_DATA_FILE_SIZE, 0)
    }

    pub fn add_fork(&self, fork: Fork, parent: Option<Fork>) {
        let mut fork_infos = self.fork_infos.write().unwrap();
        let mut fork_info = ForkInfo::default();
        if let Some(parent) = parent {
            fork_info.parents.push(parent);
            if let Some(parent_fork_info) = fork_infos.get(&parent) {
                fork_info.transaction_count = parent_fork_info.transaction_count;
                fork_info
                    .parents
                    .extend_from_slice(&parent_fork_info.parents);
            }
        }
        if let Some(old_fork_info) = fork_infos.insert(fork, fork_info) {
            panic!("duplicate forks! {} {:?}", fork, old_fork_info);
        }
    }

    fn new_storage_entry(&self, path: &str) -> AccountStorageEntry {
        AccountStorageEntry::new(
            path,
            self.next_id.fetch_add(1, Ordering::Relaxed),
            self.file_size,
            self.inc_size,
        )
    }

    fn add_storage(&self, paths: &[String]) {
        let mut stores = paths.iter().map(|p| self.new_storage_entry(&p)).collect();
        let mut storage = self.storage.write().unwrap();
        storage.append(&mut stores);
    }

    fn get_vote_accounts(&self, fork: Fork) -> HashMap<Pubkey, Account> {
        self.account_index
            .vote_accounts
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
        let account_maps = self.account_index.account_maps.read().unwrap();

        for account_map in account_maps.values() {
            if account_map.read().unwrap().contains_key(&fork) {
                return true;
            }
        }
        false
    }

    pub fn hash_internal_state(&self, fork: Fork) -> Option<Hash> {
        let ordered_accounts: BTreeMap<_, _> = self
            .account_index
            .account_maps
            .read()
            .unwrap()
            .iter()
            .filter_map(|(pubkey, account_map)| {
                let account_map = account_map.read().unwrap();
                if let Some((vec_id, offset)) = account_map.get(&fork) {
                    Some((
                        *pubkey,
                        self.storage.read().unwrap()[*vec_id]
                            .accounts
                            .read()
                            .unwrap()
                            .get_account(*offset)
                            .unwrap(),
                    ))
                } else {
                    None
                }
            })
            .collect();

        if ordered_accounts.is_empty() {
            return None;
        }
        Some(hash(&serialize(&ordered_accounts).unwrap()))
    }

    fn get_account(&self, id: AppendVecId, offset: u64) -> Account {
        let accounts = &self.storage.read().unwrap()[id].accounts;
        let av = accounts.read().unwrap();
        av.get_account(offset).unwrap()
    }

    fn load(&self, fork: Fork, pubkey: &Pubkey, walk_back: bool) -> Option<Account> {
        let account_maps = self.account_index.account_maps.read().unwrap();
        if let Some(account_map) = account_maps.get(pubkey) {
            let account_map = account_map.read().unwrap();
            // find most recent fork that is an ancestor of current_fork
            if let Some((id, offset)) = account_map.get(&fork) {
                return Some(self.get_account(*id, *offset));
            } else {
                if !walk_back {
                    return None;
                }
                let fork_infos = self.fork_infos.read().unwrap();
                if let Some(fork_info) = fork_infos.get(&fork) {
                    for parent_fork in fork_info.parents.iter() {
                        if let Some((id, offset)) = account_map.get(&parent_fork) {
                            return Some(self.get_account(*id, *offset));
                        }
                    }
                }
            }
        }
        None
    }

    fn load_by_program(
        &self,
        fork: Fork,
        program_id: &Pubkey,
        walk_back: bool,
    ) -> Vec<(Pubkey, Account)> {
        self.account_index
            .account_maps
            .read()
            .unwrap()
            .iter()
            .filter_map(|(pubkey, _)| {
                self.load(fork, pubkey, walk_back)
                    .filter(|account| account.owner == *program_id)
                    .map(|account| (*pubkey, account))
            })
            .collect()
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
                let path_idx = thread_rng().gen_range(0, self.paths.len());
                let storage = self.new_storage_entry(&self.paths[path_idx]);
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

        // Even if no lamports, need to preserve the account owner so
        // we can update the vote_accounts correctly if this account is purged
        // when squashing.
        let acc = &mut account.clone();
        if account.lamports == 0 {
            acc.userdata.resize(0, 0);
        }

        loop {
            let result: Option<u64>;
            {
                let av = &self.storage.read().unwrap()[id].accounts;
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
        let mut forks = map.write().unwrap();
        for fork in entries.iter() {
            if let Some((id, _)) = forks.remove(&fork) {
                let stores = self.storage.read().unwrap();
                stores[id].remove_account();
            }
        }
        forks.is_empty()
    }

    fn account_map_is_empty(pubkey: &Pubkey, account_maps: &HashMap<Pubkey, AccountMap>) -> bool {
        if let Some(account_map) = account_maps.get(pubkey) {
            if account_map.read().unwrap().len() == 0 {
                return true;
            }
        }
        false
    }

    fn update_vote_cache(
        &self,
        account: &Account,
        account_maps: &HashMap<Pubkey, AccountMap>,
        pubkey: &Pubkey,
    ) {
        if solana_vote_api::check_id(&account.owner) {
            if Self::account_map_is_empty(pubkey, account_maps) {
                self.account_index
                    .vote_accounts
                    .write()
                    .unwrap()
                    .remove(pubkey);
            } else {
                self.account_index
                    .vote_accounts
                    .write()
                    .unwrap()
                    .insert(*pubkey);
            }
        }
    }

    fn insert_account_entry(&self, fork: Fork, id: AppendVecId, offset: u64, map: &AccountMap) {
        let mut forks = map.write().unwrap();
        let stores = self.storage.read().unwrap();
        stores[id].add_account();
        if let Some((old_id, _)) = forks.insert(fork, (id, offset)) {
            stores[old_id].remove_account();
        }
    }

    /// Store the account update.
    fn store_account(&self, fork: Fork, pubkey: &Pubkey, account: &Account) {
        if account.lamports == 0 && self.is_squashed(fork) {
            // purge if balance is 0 and no checkpoints
            let account_maps = self.account_index.account_maps.read().unwrap();
            let map = account_maps.get(&pubkey).unwrap();
            self.remove_account_entries(&[fork], &map);
            self.update_vote_cache(account, &account_maps, pubkey);
        } else {
            let (id, offset) = self.append_account(account);

            let account_maps = self.account_index.account_maps.read().unwrap();

            let map = account_maps.get(&pubkey).unwrap();
            self.insert_account_entry(fork, id, offset, &map);
            self.update_vote_cache(account, &account_maps, pubkey);
        }
    }

    pub fn store(&self, fork: Fork, pubkey: &Pubkey, account: &Account) {
        {
            if !self
                .account_index
                .account_maps
                .read()
                .unwrap()
                .contains_key(&pubkey)
            {
                let mut waccount_maps = self.account_index.account_maps.write().unwrap();
                if !waccount_maps.contains_key(&pubkey) {
                    waccount_maps.insert(*pubkey, RwLock::new(HashMap::new()));
                }
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
            let account_maps = self.account_index.account_maps.read().unwrap();
            for (i, raccs) in loaded.iter().enumerate() {
                if res[i].is_err() || raccs.is_err() {
                    continue;
                }
                let tx = &txs[i];
                for key in tx.account_keys.iter() {
                    if !account_maps.contains_key(&key) {
                        keys.push(*key);
                    }
                }
            }
        }
        if !keys.is_empty() {
            let mut account_maps = self.account_index.account_maps.write().unwrap();
            for key in keys.iter() {
                account_maps.insert(*key, RwLock::new(HashMap::new()));
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
            Err(TransactionError::MissingSignatureForFee)
        } else {
            // Check for unique account keys
            if has_duplicates(&tx.account_keys) {
                error_counters.account_loaded_twice += 1;
                return Err(TransactionError::AccountLoadedTwice);
            }

            // There is no way to predict what program will execute without an error
            // If a fee can pay for execution then the program will be scheduled
            let mut called_accounts: Vec<Account> = vec![];
            for key in &tx.account_keys {
                called_accounts.push(self.load(fork, key, true).unwrap_or_default());
            }
            if called_accounts.is_empty() || called_accounts[0].lamports == 0 {
                error_counters.account_not_found += 1;
                Err(TransactionError::AccountNotFound)
            } else if called_accounts[0].lamports < tx.fee {
                error_counters.insufficient_funds += 1;
                Err(TransactionError::InsufficientFundsForFee)
            } else {
                called_accounts[0].lamports -= tx.fee;
                Ok(called_accounts)
            }
        }
    }

    fn load_executable_accounts(
        &self,
        fork: Fork,
        program_id: &Pubkey,
        error_counters: &mut ErrorCounters,
    ) -> Result<Vec<(Pubkey, Account)>> {
        let mut accounts = Vec::new();
        let mut depth = 0;
        let mut program_id = *program_id;
        loop {
            if native_loader::check_id(&program_id) {
                // at the root of the chain, ready to dispatch
                break;
            }

            if depth >= 5 {
                error_counters.call_chain_too_deep += 1;
                return Err(TransactionError::CallChainTooDeep);
            }
            depth += 1;

            let program = match self.load(fork, &program_id, true) {
                Some(program) => program,
                None => {
                    error_counters.account_not_found += 1;
                    return Err(TransactionError::AccountNotFound);
                }
            };
            if !program.executable || program.owner == Pubkey::default() {
                error_counters.account_not_found += 1;
                return Err(TransactionError::AccountNotFound);
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
                    return Err(TransactionError::AccountNotFound);
                }
                let program_id = tx.program_ids[ix.program_ids_index as usize];
                self.load_executable_accounts(fork, &program_id, error_counters)
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
        let mut fork_infos = self.fork_infos.write().unwrap();
        let fork_info = fork_infos.entry(fork).or_insert(ForkInfo::default());
        fork_info.transaction_count += tx_count as u64;
    }

    pub fn transaction_count(&self, fork: Fork) -> u64 {
        self.fork_infos
            .read()
            .unwrap()
            .get(&fork)
            .map_or(0, |fork_info| fork_info.transaction_count)
    }

    fn remove_parents(&self, fork: Fork) -> Vec<Fork> {
        let mut info = self.fork_infos.write().unwrap();
        let fork_info = info.get_mut(&fork).unwrap();
        fork_info.parents.split_off(0)
    }

    fn is_squashed(&self, fork: Fork) -> bool {
        self.fork_infos
            .read()
            .unwrap()
            .get(&fork)
            .unwrap()
            .parents
            .is_empty()
    }

    fn get_merged_account_map(
        &self,
        fork: Fork,
        parents: &[Fork],
        map: &AccountMap,
    ) -> Option<(Fork, AppendVecId, u64)> {
        let forks = map.read().unwrap();
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

        // for every account in all the parents, load latest and update self if
        // absent
        let mut keys = vec![];
        {
            let account_maps = self.account_index.account_maps.read().unwrap();
            account_maps.iter().for_each(|(pubkey, map)| {
                if let Some((parent_fork, id, offset)) =
                    self.get_merged_account_map(fork, &parents, &map)
                {
                    if parent_fork != fork {
                        self.insert_account_entry(fork, id, offset, &map);
                    } else {
                        let account = self.get_account(id, offset);
                        if account.lamports == 0 {
                            if self.remove_account_entries(&[fork], &map) {
                                keys.push(pubkey.clone());
                            }
                            self.update_vote_cache(&account, &account_maps, pubkey);
                        }
                    }
                }
            });
        }
        if !keys.is_empty() {
            let mut account_maps = self.account_index.account_maps.write().unwrap();
            for key in keys.iter() {
                account_maps.remove(&key);
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
        let (paths, own_paths) = if in_paths.is_none() {
            (Self::make_default_paths(), true)
        } else {
            (in_paths.unwrap(), false)
        };
        let accounts_db = AccountsDB::new(fork, &paths);
        Accounts {
            accounts_db,
            account_locks: Mutex::new(HashMap::new()),
            paths,
            own_paths,
        }
    }

    pub fn new_from_parent(&self, fork: Fork, parent: Fork) {
        self.accounts_db.add_fork(fork, Some(parent));
    }

    /// Slow because lock is held for 1 operation instead of many
    pub fn load_slow(&self, fork: Fork, pubkey: &Pubkey) -> Option<Account> {
        self.accounts_db
            .load(fork, pubkey, true)
            .filter(|acc| acc.lamports != 0)
    }

    /// Slow because lock is held for 1 operation instead of many
    pub fn load_slow_no_parent(&self, fork: Fork, pubkey: &Pubkey) -> Option<Account> {
        self.accounts_db
            .load(fork, pubkey, false)
            .filter(|acc| acc.lamports != 0)
    }

    /// Slow because lock is held for 1 operation instead of many
    pub fn load_by_program_slow_no_parent(
        &self,
        fork: Fork,
        program_id: &Pubkey,
    ) -> Vec<(Pubkey, Account)> {
        self.accounts_db
            .load_by_program(fork, program_id, false)
            .into_iter()
            .filter(|(_, acc)| acc.lamports != 0)
            .collect()
    }

    /// Slow because lock is held for 1 operation instead of many
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
                return Err(TransactionError::AccountInUse);
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
            Err(TransactionError::AccountInUse) => (),
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

    pub fn get_vote_accounts(&self, fork: Fork) -> impl Iterator<Item = (Pubkey, Account)> {
        self.accounts_db
            .get_vote_accounts(fork)
            .into_iter()
            .filter(|(_, acc)| acc.lamports != 0)
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

    fn cleanup_paths(paths: &str) {
        let paths = get_paths_vec(&paths);
        paths.iter().for_each(|p| {
            let _ignored = remove_dir_all(p);
        });
    }

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
        assert_eq!(loaded_accounts[0], Err(TransactionError::AccountNotFound));
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
        assert_eq!(loaded_accounts[0], Err(TransactionError::AccountNotFound));
    }

    #[test]
    fn test_load_accounts_unknown_program_id() {
        let mut accounts: Vec<(Pubkey, Account)> = Vec::new();
        let mut error_counters = ErrorCounters::default();

        let keypair = Keypair::new();
        let key0 = keypair.pubkey();
        let key1 = Pubkey::new(&[5u8; 32]);

        let account = Account::new(1, 1, &Pubkey::default());
        accounts.push((key0, account));

        let account = Account::new(2, 1, &Pubkey::default());
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
        assert_eq!(loaded_accounts[0], Err(TransactionError::AccountNotFound));
    }

    #[test]
    fn test_load_accounts_insufficient_funds() {
        let mut accounts: Vec<(Pubkey, Account)> = Vec::new();
        let mut error_counters = ErrorCounters::default();

        let keypair = Keypair::new();
        let key0 = keypair.pubkey();

        let account = Account::new(1, 1, &Pubkey::default());
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
        assert_eq!(
            loaded_accounts[0],
            Err(TransactionError::InsufficientFundsForFee)
        );
    }

    #[test]
    fn test_load_accounts_no_loaders() {
        let mut accounts: Vec<(Pubkey, Account)> = Vec::new();
        let mut error_counters = ErrorCounters::default();

        let keypair = Keypair::new();
        let key0 = keypair.pubkey();
        let key1 = Pubkey::new(&[5u8; 32]);

        let account = Account::new(1, 1, &Pubkey::default());
        accounts.push((key0, account));

        let account = Account::new(2, 1, &Pubkey::default());
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

        let account = Account::new(1, 1, &Pubkey::default());
        accounts.push((key0, account));

        let mut account = Account::new(40, 1, &Pubkey::default());
        account.executable = true;
        account.owner = native_loader::id();
        accounts.push((key1, account));

        let mut account = Account::new(41, 1, &Pubkey::default());
        account.executable = true;
        account.owner = key1;
        accounts.push((key2, account));

        let mut account = Account::new(42, 1, &Pubkey::default());
        account.executable = true;
        account.owner = key2;
        accounts.push((key3, account));

        let mut account = Account::new(43, 1, &Pubkey::default());
        account.executable = true;
        account.owner = key3;
        accounts.push((key4, account));

        let mut account = Account::new(44, 1, &Pubkey::default());
        account.executable = true;
        account.owner = key4;
        accounts.push((key5, account));

        let mut account = Account::new(45, 1, &Pubkey::default());
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
        assert_eq!(loaded_accounts[0], Err(TransactionError::CallChainTooDeep));
    }

    #[test]
    fn test_load_accounts_bad_program_id() {
        let mut accounts: Vec<(Pubkey, Account)> = Vec::new();
        let mut error_counters = ErrorCounters::default();

        let keypair = Keypair::new();
        let key0 = keypair.pubkey();
        let key1 = Pubkey::new(&[5u8; 32]);

        let account = Account::new(1, 1, &Pubkey::default());
        accounts.push((key0, account));

        let mut account = Account::new(40, 1, &Pubkey::default());
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
        assert_eq!(loaded_accounts[0], Err(TransactionError::AccountNotFound));
    }

    #[test]
    fn test_load_accounts_not_executable() {
        let mut accounts: Vec<(Pubkey, Account)> = Vec::new();
        let mut error_counters = ErrorCounters::default();

        let keypair = Keypair::new();
        let key0 = keypair.pubkey();
        let key1 = Pubkey::new(&[5u8; 32]);

        let account = Account::new(1, 1, &Pubkey::default());
        accounts.push((key0, account));

        let mut account = Account::new(40, 1, &Pubkey::default());
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
        assert_eq!(loaded_accounts[0], Err(TransactionError::AccountNotFound));
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

        let account = Account::new(1, 1, &Pubkey::default());
        accounts.push((key0, account));

        let mut account = Account::new(40, 1, &Pubkey::default());
        account.executable = true;
        account.owner = native_loader::id();
        accounts.push((key1, account));

        let mut account = Account::new(41, 1, &Pubkey::default());
        account.executable = true;
        account.owner = key1;
        accounts.push((key2, account));

        let mut account = Account::new(42, 1, &Pubkey::default());
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

        let account = Account::new(10, 1, &Pubkey::default());
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
        assert_eq!(
            loaded_accounts[0],
            Err(TransactionError::AccountLoadedTwice)
        );
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
        let out_dir = env::var("OUT_DIR").unwrap_or_else(|_| "target".to_string());
        let vpaths: Vec<_> = vpaths
            .iter()
            .map(|path| format!("{}/{}", out_dir, path))
            .collect();
        TempPaths {
            paths: vpaths.join(","),
        }
    }

    #[test]
    fn test_accountsdb_squash_one_fork() {
        let paths = get_tmp_accounts_path!();
        let db = AccountsDB::new(0, &paths.paths);
        let key = Pubkey::default();
        let account0 = Account::new(1, 0, &key);

        // store value 1 in the "root", i.e. db zero
        db.store(0, &key, &account0);

        db.add_fork(1, Some(0));
        db.add_fork(2, Some(0));

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
        //                          root0 -> key.lamports==1
        //                             \
        //                              \
        //  key.lamports==ANF <- root1     \
        //                              fork2 -> key.lamports==1 (from root0)
        //
        assert_eq!(db.load(1, &key, true), None); // purged
        assert_eq!(&db.load(2, &key, true).unwrap(), &account0); // original value
    }

    #[test]
    fn test_accountsdb_squash() {
        let paths = get_tmp_accounts_path!();
        let db = AccountsDB::new(0, &paths.paths);

        let mut pubkeys: Vec<Pubkey> = vec![];
        create_account(&db, &mut pubkeys, 100, 0);
        for _ in 1..100 {
            let idx = thread_rng().gen_range(0, 99);
            let account = db.load(0, &pubkeys[idx], true).unwrap();
            let mut default_account = Account::default();
            default_account.lamports = (idx + 1) as u64;
            assert_eq!(compare_account(&default_account, &account), true);
        }
        db.add_fork(1, Some(0));

        // now we have:
        //
        //                        root0 -> key[X].lamports==X
        //                         /
        //                        /
        //  key[X].lamports==X <- fork1
        //    (via root0)
        //

        // merge, which should whack key's account
        db.squash(1);

        // now we should have:
        //                root0 -> purged ??
        //
        //
        //  key[X].lamports==X <- root1
        //

        // check that all the accounts appear in parent after a squash ???
        for _ in 1..100 {
            let idx = thread_rng().gen_range(0, 99);
            let account0 = db.load(0, &pubkeys[idx], true).unwrap();
            let account1 = db.load(1, &pubkeys[idx], true).unwrap();
            let mut default_account = Account::default();
            default_account.lamports = (idx + 1) as u64;
            assert_eq!(&default_account, &account0);
            assert_eq!(&default_account, &account1);
        }
    }

    #[test]
    fn test_accounts_unsquashed() {
        let key = Pubkey::default();

        // 1 token in the "root", i.e. db zero
        let paths = get_tmp_accounts_path!();
        let db0 = AccountsDB::new(0, &paths.paths);
        let account0 = Account::new(1, 0, &key);
        db0.store(0, &key, &account0);

        db0.add_fork(1, Some(0));
        // 0 lamports in the child
        let account1 = Account::new(0, 0, &key);
        db0.store(1, &key, &account1);

        // masking accounts is done at the Accounts level, at accountsDB we see
        // original account
        assert_eq!(db0.load(1, &key, true), Some(account1));

        let mut accounts1 = Accounts::new(3, None);
        accounts1.accounts_db = db0;
        assert_eq!(accounts1.load_slow(1, &key), None);
        assert_eq!(accounts1.load_slow(0, &key), Some(account0));
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
            default_account.lamports = (t + 1) as u64;
            assert!(accounts.load(0, &pubkey, true).is_none());
            accounts.store(0, &pubkey, &default_account);
        }
        for t in 0..num_vote {
            let pubkey = Keypair::new().pubkey();
            let mut default_account = Account::default();
            pubkeys.push(pubkey.clone());
            default_account.owner = solana_vote_api::id();
            default_account.lamports = (num + t + 1) as u64;
            assert!(accounts.load(0, &pubkey, true).is_none());
            accounts.store(0, &pubkey, &default_account);
        }
    }

    fn update_accounts(accounts: &AccountsDB, pubkeys: Vec<Pubkey>, range: usize) {
        for _ in 1..1000 {
            let idx = thread_rng().gen_range(0, range);
            if let Some(mut account) = accounts.load(0, &pubkeys[idx], true) {
                account.lamports = account.lamports + 1;
                accounts.store(0, &pubkeys[idx], &account);
                if account.lamports == 0 {
                    assert!(accounts.load(0, &pubkeys[idx], true).is_none());
                } else {
                    let mut default_account = Account::default();
                    default_account.lamports = account.lamports;
                    assert_eq!(compare_account(&default_account, &account), true);
                }
            }
        }
    }

    fn compare_account(account1: &Account, account2: &Account) -> bool {
        if account1.userdata != account2.userdata
            || account1.owner != account2.owner
            || account1.executable != account2.executable
            || account1.lamports != account2.lamports
        {
            return false;
        }
        true
    }

    #[test]
    fn test_account_one() {
        let paths = get_tmp_accounts_path!();
        let accounts = AccountsDB::new(0, &paths.paths);
        let mut pubkeys: Vec<Pubkey> = vec![];
        create_account(&accounts, &mut pubkeys, 1, 0);
        let account = accounts.load(0, &pubkeys[0], true).unwrap();
        let mut default_account = Account::default();
        default_account.lamports = 1;
        assert_eq!(compare_account(&default_account, &account), true);
    }

    #[test]
    fn test_account_many() {
        let paths = get_tmp_accounts_path("many0,many1");
        let accounts = AccountsDB::new(0, &paths.paths);
        let mut pubkeys: Vec<Pubkey> = vec![];
        create_account(&accounts, &mut pubkeys, 100, 0);
        for _ in 1..100 {
            let idx = thread_rng().gen_range(0, 99);
            let account = accounts.load(0, &pubkeys[idx], true).unwrap();
            let mut default_account = Account::default();
            default_account.lamports = (idx + 1) as u64;
            assert_eq!(compare_account(&default_account, &account), true);
        }
    }

    #[test]
    fn test_account_update() {
        let paths = get_tmp_accounts_path!();
        let accounts = AccountsDB::new(0, &paths.paths);
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
    }

    #[test]
    fn test_account_grow_many() {
        let paths = get_tmp_accounts_path("many2,many3");
        let size = 4096;
        let accounts = AccountsDB::new_with_file_size(0, &paths.paths, size, 0);
        let mut keys = vec![];
        for i in 0..9 {
            let key = Keypair::new().pubkey();
            let account = Account::new(i + 1, size as usize / 4, &key);
            accounts.store(0, &key, &account);
            keys.push(key);
        }
        for (i, key) in keys.iter().enumerate() {
            assert_eq!(
                accounts.load(0, &key, false).unwrap().lamports,
                (i as u64) + 1
            );
        }

        let mut append_vec_histogram = HashMap::new();
        let account_maps = accounts.account_index.account_maps.read().unwrap();
        for map in account_maps.values() {
            *append_vec_histogram
                .entry(map.read().unwrap().get(&0).unwrap().0)
                .or_insert(0) += 1;
        }
        for count in append_vec_histogram.values() {
            assert!(*count >= 2);
        }
    }

    #[test]
    fn test_account_grow() {
        let paths = get_tmp_accounts_path!();
        let accounts = AccountsDB::new(0, &paths.paths);
        let count = [0, 1];
        let status = [
            AccountStorageStatus::StorageAvailable,
            AccountStorageStatus::StorageFull,
        ];
        let pubkey1 = Keypair::new().pubkey();
        let account1 = Account::new(1, ACCOUNT_DATA_FILE_SIZE as usize / 2, &pubkey1);
        accounts.store(0, &pubkey1, &account1);
        {
            let stores = accounts.storage.read().unwrap();
            assert_eq!(stores.len(), 1);
            assert_eq!(stores[0].count.load(Ordering::Relaxed), 1);
            assert_eq!(stores[0].get_status(), status[0]);
        }

        let pubkey2 = Keypair::new().pubkey();
        let account2 = Account::new(1, ACCOUNT_DATA_FILE_SIZE as usize / 2, &pubkey2);
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
    }

    #[test]
    fn test_accounts_vote_filter() {
        let accounts = Accounts::new(0, None);
        let mut vote_account = Account::new(1, 0, &solana_vote_api::id());
        let key = Keypair::new().pubkey();
        accounts.store_slow(0, &key, &vote_account);

        accounts.new_from_parent(1, 0);

        let mut vote_accounts: Vec<_> = accounts.get_vote_accounts(1).collect();
        assert_eq!(vote_accounts.len(), 1);

        vote_account.lamports = 0;
        accounts.store_slow(1, &key, &vote_account);

        vote_accounts = accounts.get_vote_accounts(1).collect();
        assert_eq!(vote_accounts.len(), 0);

        let mut vote_account1 = Account::new(2, 0, &solana_vote_api::id());
        let key1 = Keypair::new().pubkey();
        accounts.store_slow(1, &key1, &vote_account1);

        accounts.squash(1);
        vote_accounts = accounts.get_vote_accounts(0).collect();
        assert_eq!(vote_accounts.len(), 1);
        vote_accounts = accounts.get_vote_accounts(1).collect();
        assert_eq!(vote_accounts.len(), 1);

        vote_account1.lamports = 0;
        accounts.store_slow(1, &key1, &vote_account1);
        accounts.store_slow(0, &key, &vote_account);

        vote_accounts = accounts.get_vote_accounts(1).collect();
        assert_eq!(vote_accounts.len(), 0);
    }

    #[test]
    fn test_account_vote() {
        let paths = get_tmp_accounts_path!();
        let accounts_db = AccountsDB::new(0, &paths.paths);
        let mut pubkeys: Vec<Pubkey> = vec![];
        create_account(&accounts_db, &mut pubkeys, 0, 1);
        let accounts = accounts_db.get_vote_accounts(0);
        assert_eq!(accounts.len(), 1);
        accounts.iter().for_each(|(_, account)| {
            assert_eq!(account.owner, solana_vote_api::id());
        });
        let lastkey = Keypair::new().pubkey();
        let mut lastaccount = Account::new(1, 0, &solana_vote_api::id());
        accounts_db.store(0, &lastkey, &lastaccount);
        assert_eq!(accounts_db.get_vote_accounts(0).len(), 2);

        accounts_db.add_fork(1, Some(0));

        assert_eq!(
            accounts_db.get_vote_accounts(1),
            accounts_db.get_vote_accounts(0)
        );

        info!("storing lamports=0 to fork 1");
        // should store a lamports=0 account in 1
        lastaccount.lamports = 0;
        accounts_db.store(1, &lastkey, &lastaccount);
        // len == 2 because lamports=0 accounts are filtered at the Accounts interface.
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

        assert_eq!(
            accounts_db
                .account_index
                .vote_accounts
                .read()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(accounts_db.get_vote_accounts(1).len(), 1);
        assert_eq!(accounts_db.get_vote_accounts(2).len(), 1);
    }

    #[test]
    fn test_accounts_empty_hash_internal_state() {
        let paths = get_tmp_accounts_path!();
        let accounts = AccountsDB::new(0, &paths.paths);
        assert_eq!(accounts.hash_internal_state(0), None);
    }

    #[test]
    #[should_panic]
    fn test_accountsdb_duplicate_fork_should_panic() {
        let paths = get_tmp_accounts_path!();
        let accounts = AccountsDB::new(0, &paths.paths);
        cleanup_paths(&paths.paths);
        accounts.add_fork(0, None);
    }

    #[test]
    fn test_accountsdb_account_not_found() {
        let paths = get_tmp_accounts_path!();
        let accounts = AccountsDB::new(0, &paths.paths);
        let mut error_counters = ErrorCounters::default();
        assert_eq!(
            accounts.load_executable_accounts(0, &Keypair::new().pubkey(), &mut error_counters),
            Err(TransactionError::AccountNotFound)
        );
        assert_eq!(error_counters.account_not_found, 1);
    }

    #[test]
    fn test_accountsdb_inherit_tx_count() {
        let paths = get_tmp_accounts_path!();
        let accounts = AccountsDB::new(0, &paths.paths);
        assert_eq!(accounts.transaction_count(0), 0);
        accounts.increment_transaction_count(0, 1);
        assert_eq!(accounts.transaction_count(0), 1);
        // fork and check that tx count is inherited
        accounts.add_fork(1, Some(0));
        assert_eq!(accounts.transaction_count(1), 1);
        // Parent fork shouldn't change
        accounts.increment_transaction_count(1, 1);
        assert_eq!(accounts.transaction_count(1), 2);
        assert_eq!(accounts.transaction_count(0), 1);
        // Squash shouldn't effect tx count
        accounts.squash(1);
        assert_eq!(accounts.transaction_count(1), 2);
    }

    #[test]
    fn test_load_by_program() {
        let paths = get_tmp_accounts_path!();
        let accounts_db = AccountsDB::new(0, &paths.paths);

        // Load accounts owned by various programs into AccountsDB
        let pubkey0 = Keypair::new().pubkey();
        let account0 = Account::new(1, 0, &Pubkey::new(&[2; 32]));
        accounts_db.store(0, &pubkey0, &account0);
        let pubkey1 = Keypair::new().pubkey();
        let account1 = Account::new(1, 0, &Pubkey::new(&[2; 32]));
        accounts_db.store(0, &pubkey1, &account1);
        let pubkey2 = Keypair::new().pubkey();
        let account2 = Account::new(1, 0, &Pubkey::new(&[3; 32]));
        accounts_db.store(0, &pubkey2, &account2);

        let accounts = accounts_db.load_by_program(0, &Pubkey::new(&[2; 32]), false);
        assert_eq!(accounts.len(), 2);
        let accounts = accounts_db.load_by_program(0, &Pubkey::new(&[3; 32]), false);
        assert_eq!(accounts, vec![(pubkey2, account2)]);
        let accounts = accounts_db.load_by_program(0, &Pubkey::new(&[4; 32]), false);
        assert_eq!(accounts, vec![]);

        // Accounts method
        let mut accounts_proper = Accounts::new(0, None);
        accounts_proper.accounts_db = accounts_db;
        let accounts = accounts_proper.load_by_program_slow_no_parent(0, &Pubkey::new(&[2; 32]));
        assert_eq!(accounts.len(), 2);
        let accounts = accounts_proper.load_by_program_slow_no_parent(0, &Pubkey::new(&[4; 32]));
        assert_eq!(accounts, vec![]);
    }
}

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
use crate::append_vec::{AppendVec, StoredAccount};
use crate::message_processor::has_duplicates;
use bincode::serialize;
use hashbrown::{HashMap, HashSet};
use log::*;
use rand::{thread_rng, Rng};
use rayon::prelude::*;
use solana_metrics::counter::Counter;
use solana_sdk::account::Account;
use solana_sdk::fee_calculator::FeeCalculator;
use solana_sdk::hash::{hash, Hash, Hasher};
use solana_sdk::native_loader;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::transaction::Result;
use solana_sdk::transaction::{Transaction, TransactionError};
use std::env;
use std::fs::{create_dir_all, remove_dir_all};
use std::ops::Neg;
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
    pub invalid_account_index: usize,
    pub duplicate_signature: usize,
    pub call_chain_too_deep: usize,
    pub missing_signature_for_fee: usize,
}

const ACCOUNT_DATA_FILE_SIZE: u64 = 64 * 1024 * 1024;
const ACCOUNT_DATA_FILE: &str = "data";
const ACCOUNTSDB_DIR: &str = "accountsdb";
const NUM_ACCOUNT_DIRS: usize = 4;

/// An offset into the AccountsDB::storage vector
type AppendVecId = usize;

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

#[derive(Default, Clone)]
struct AccountInfo {
    /// index identifying the append storage
    id: AppendVecId,

    /// offset into the storage
    offset: usize,

    /// lamports in the account used when squashing kept for optimization
    /// purposes to remove accounts with zero balance.
    lamports: u64,
}

/// Persistent storage structure holding the accounts
struct AccountStorageEntry {
    fork_id: Fork,

    /// storage holding the accounts
    accounts: AppendVec,

    /// Keeps track of the number of accounts stored in a specific AppendVec.
    /// This is periodically checked to reuse the stores that do not have
    /// any accounts in it.
    count: AtomicUsize,

    /// status corresponding to the storage
    status: AtomicUsize,
}

impl AccountStorageEntry {
    pub fn new(path: &str, fork_id: Fork, id: usize, file_size: u64) -> Self {
        let p = format!("{}/{}", path, id);
        let path = Path::new(&p);
        let _ignored = remove_dir_all(path);
        create_dir_all(path).expect("Create directory failed");
        let accounts = AppendVec::new(&path.join(ACCOUNT_DATA_FILE), true, file_size as usize);

        AccountStorageEntry {
            fork_id,
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

    fn remove_account(&self) -> bool {
        if self.count.fetch_sub(1, Ordering::Relaxed) == 1 {
            self.accounts.reset();
            self.set_status(AccountStorageStatus::StorageAvailable);
            true
        } else {
            false
        }
    }
}

type AccountStorage = Vec<Arc<AccountStorageEntry>>;
type AccountStorageSlice = [Arc<AccountStorageEntry>];

// This structure handles the load/store of the accounts
#[derive(Default)]
pub struct AccountsDB {
    /// Keeps tracks of index into AppendVec on a per fork basis
    accounts_index: RwLock<AccountsIndex<AccountInfo>>,

    /// Account storage
    storage: RwLock<AccountStorage>,

    /// distribute the accounts across storage lists
    next_id: AtomicUsize,

    /// write version
    write_version: AtomicUsize,

    /// Set of storage paths to pick from
    paths: Vec<String>,

    /// Starting file size of appendvecs
    file_size: u64,
}

/// This structure handles synchronization for db
#[derive(Default)]
pub struct Accounts {
    /// Single global AccountsDB
    pub accounts_db: Arc<AccountsDB>,

    /// set of accounts which are currently in the pipeline
    account_locks: Mutex<HashSet<Pubkey>>,

    /// List of persistent stores
    paths: String,

    /// set to true if object created the directories in paths
    /// when true, delete parents of 'paths' on drop
    own_paths: bool,
}

fn get_paths_vec(paths: &str) -> Vec<String> {
    paths.split(',').map(ToString::to_string).collect()
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
    pub fn new_with_file_size(paths: &str, file_size: u64) -> Self {
        let paths = get_paths_vec(&paths);
        AccountsDB {
            accounts_index: RwLock::new(AccountsIndex::default()),
            storage: RwLock::new(vec![]),
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
        for x in self.storage.read().unwrap().iter() {
            if x.fork_id == fork && x.count.load(Ordering::Relaxed) > 0 {
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
            .iter()
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

    pub fn hash_internal_state(&self, fork_id: Fork) -> Option<Hash> {
        let accumulator: Vec<Vec<(Pubkey, u64, Hash)>> = self.scan_account_storage(
            fork_id,
            |stored_account: &StoredAccount, accum: &mut Vec<(Pubkey, u64, Hash)>| {
                accum.push((
                    stored_account.pubkey,
                    stored_account.write_version,
                    hash(&serialize(&stored_account.account).unwrap()),
                ));
            },
        );
        let mut account_hashes: Vec<_> = accumulator.into_iter().flat_map(|x| x).collect();
        account_hashes.sort_by_key(|s| (s.0, (s.1 as i64).neg()));
        account_hashes.dedup_by_key(|s| s.0);
        if account_hashes.is_empty() {
            None
        } else {
            let mut hasher = Hasher::default();
            for (_, _, hash) in account_hashes {
                hasher.hash(hash.as_ref());
            }
            Some(hasher.result())
        }
    }

    fn load(
        storage: &AccountStorageSlice,
        ancestors: &HashMap<Fork, usize>,
        accounts_index: &AccountsIndex<AccountInfo>,
        pubkey: &Pubkey,
    ) -> Option<Account> {
        let info = accounts_index.get(pubkey, ancestors)?;
        //TODO: thread this as a ref
        storage
            .get(info.id)
            .map(|store| store.accounts.get_account(info.offset).account.clone())
    }

    fn load_tx_accounts(
        storage: &AccountStorageSlice,
        ancestors: &HashMap<Fork, usize>,
        accounts_index: &AccountsIndex<AccountInfo>,
        tx: &Transaction,
        fee: u64,
        error_counters: &mut ErrorCounters,
    ) -> Result<Vec<Account>> {
        // Copy all the accounts
        let message = tx.message();
        if tx.signatures.is_empty() && fee != 0 {
            Err(TransactionError::MissingSignatureForFee)
        } else {
            // Check for unique account keys
            if has_duplicates(&message.account_keys) {
                error_counters.account_loaded_twice += 1;
                return Err(TransactionError::AccountLoadedTwice);
            }

            // There is no way to predict what program will execute without an error
            // If a fee can pay for execution then the program will be scheduled
            let mut called_accounts: Vec<Account> = vec![];
            for key in &message.account_keys {
                called_accounts
                    .push(Self::load(storage, ancestors, accounts_index, key).unwrap_or_default());
            }
            if called_accounts.is_empty() || called_accounts[0].lamports == 0 {
                error_counters.account_not_found += 1;
                Err(TransactionError::AccountNotFound)
            } else if called_accounts[0].lamports < fee {
                error_counters.insufficient_funds += 1;
                Err(TransactionError::InsufficientFundsForFee)
            } else {
                called_accounts[0].lamports -= fee;
                Ok(called_accounts)
            }
        }
    }

    fn load_executable_accounts(
        storage: &AccountStorageSlice,
        ancestors: &HashMap<Fork, usize>,
        accounts_index: &AccountsIndex<AccountInfo>,
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

            let program = match Self::load(storage, ancestors, accounts_index, &program_id) {
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
            program_id = program.owner;
            accounts.insert(0, (program_id, program));
        }
        Ok(accounts)
    }

    /// For each program_id in the transaction, load its loaders.
    fn load_loaders(
        storage: &AccountStorageSlice,
        ancestors: &HashMap<Fork, usize>,
        accounts_index: &AccountsIndex<AccountInfo>,
        tx: &Transaction,
        error_counters: &mut ErrorCounters,
    ) -> Result<Vec<Vec<(Pubkey, Account)>>> {
        let message = tx.message();
        message
            .instructions
            .iter()
            .map(|ix| {
                if message.program_ids().len() <= ix.program_ids_index as usize {
                    error_counters.account_not_found += 1;
                    return Err(TransactionError::AccountNotFound);
                }
                let program_id = message.program_ids()[ix.program_ids_index as usize];
                Self::load_executable_accounts(
                    storage,
                    ancestors,
                    accounts_index,
                    &program_id,
                    error_counters,
                )
            })
            .collect()
    }

    fn load_accounts(
        &self,
        ancestors: &HashMap<Fork, usize>,
        txs: &[Transaction],
        lock_results: Vec<Result<()>>,
        fee_calculator: &FeeCalculator,
        error_counters: &mut ErrorCounters,
    ) -> Vec<Result<(InstructionAccounts, InstructionLoaders)>> {
        //PERF: hold the lock to scan for the references, but not to clone the accounts
        //TODO: two locks usually leads to deadlocks, should this be one structure?
        let accounts_index = self.accounts_index.read().unwrap();
        let storage = self.storage.read().unwrap();
        txs.iter()
            .zip(lock_results.into_iter())
            .map(|etx| match etx {
                (tx, Ok(())) => {
                    let fee = fee_calculator.calculate_fee(tx.message());
                    let accounts = Self::load_tx_accounts(
                        &storage,
                        ancestors,
                        &accounts_index,
                        tx,
                        fee,
                        error_counters,
                    )?;
                    let loaders = Self::load_loaders(
                        &storage,
                        ancestors,
                        &accounts_index,
                        tx,
                        error_counters,
                    )?;
                    Ok((accounts, loaders))
                }
                (_, Err(e)) => Err(e),
            })
            .collect()
    }

    fn load_slow(&self, ancestors: &HashMap<Fork, usize>, pubkey: &Pubkey) -> Option<Account> {
        let accounts_index = self.accounts_index.read().unwrap();
        let storage = self.storage.read().unwrap();
        Self::load(&storage, ancestors, &accounts_index, pubkey)
    }

    fn get_storage_id(&self, fork_id: Fork, start: usize, current: usize) -> usize {
        let mut id = current;
        let len: usize;
        {
            let stores = self.storage.read().unwrap();
            len = stores.len();
            if len > 0 {
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
                    if fork_id == stores[id].fork_id
                        && stores[id].get_status() == AccountStorageStatus::StorageAvailable
                    {
                        break;
                    }
                    if id == start % len {
                        break;
                    }
                }
            }
        }
        if len == 0 || id == start % len {
            let mut stores = self.storage.write().unwrap();
            // check if new store was already created
            if stores.len() == len {
                let path_idx = thread_rng().gen_range(0, self.paths.len());
                let storage = self.new_storage_entry(fork_id, &self.paths[path_idx]);
                stores.push(Arc::new(storage));
            }
            id = stores.len() - 1;
        }
        id
    }

    fn append_account(&self, fork_id: Fork, pubkey: &Pubkey, account: &Account) -> (usize, usize) {
        let offset: usize;
        let start = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut id = self.get_storage_id(fork_id, start, std::usize::MAX);

        // Even if no lamports, need to preserve the account owner so
        // we can update the vote_accounts correctly if this account is purged
        // when squashing.
        let acc = &mut account.clone();
        if account.lamports == 0 {
            acc.data.resize(0, 0);
        }

        loop {
            let result: Option<usize>;
            {
                let accounts = &self.storage.read().unwrap()[id];
                let write_version = self.write_version.fetch_add(1, Ordering::Relaxed) as u64;
                let stored_account = StoredAccount {
                    write_version,
                    pubkey: *pubkey,
                    //TODO: fix all this copy
                    account: account.clone(),
                };
                result = accounts.accounts.append_account(&stored_account);
                accounts.add_account();
            }
            if let Some(val) = result {
                offset = val;
                break;
            } else {
                id = self.get_storage_id(fork_id, start, id);
            }
        }
        (id, offset)
    }

    pub fn purge_fork(&self, fork: Fork) {
        //add_root should be called first
        let is_root = self.accounts_index.read().unwrap().is_root(fork);
        trace!("PURGING {} {}", fork, is_root);
        if !is_root {
            self.storage.write().unwrap().retain(|x| {
                trace!("PURGING {} {}", x.fork_id, fork);
                x.fork_id != fork
            });
        }
    }

    /// Store the account update.
    pub fn store(&self, fork_id: Fork, accounts: &[(&Pubkey, &Account)]) {
        //TODO; these blocks should be separate functions and unit tested
        let infos: Vec<_> = accounts
            .iter()
            .map(|(pubkey, account)| {
                let (id, offset) = self.append_account(fork_id, pubkey, account);
                AccountInfo {
                    id,
                    offset,
                    lamports: account.lamports,
                }
            })
            .collect();

        let reclaims: Vec<(Fork, AccountInfo)> = {
            let mut index = self.accounts_index.write().unwrap();
            let mut reclaims = vec![];
            for (i, info) in infos.into_iter().enumerate() {
                let key = &accounts[i].0;
                reclaims.extend(index.insert(fork_id, key, info).into_iter())
            }
            reclaims
        };

        let dead_forks: HashSet<Fork> = {
            let stores = self.storage.read().unwrap();
            let mut cleared_forks: HashSet<Fork> = HashSet::new();
            for (fork_id, account_info) in reclaims {
                let cleared = stores[account_info.id].remove_account();
                if cleared {
                    cleared_forks.insert(fork_id);
                }
            }
            let live_forks: HashSet<Fork> = stores.iter().map(|x| x.fork_id).collect();
            cleared_forks.difference(&live_forks).cloned().collect()
        };
        {
            let mut index = self.accounts_index.write().unwrap();
            for fork in dead_forks {
                index.cleanup_dead_fork(fork);
            }
        }
    }

    pub fn store_accounts(
        &self,
        fork: Fork,
        txs: &[Transaction],
        res: &[Result<()>],
        loaded: &[Result<(InstructionAccounts, InstructionLoaders)>],
    ) {
        let mut accounts: Vec<(&Pubkey, &Account)> = vec![];
        for (i, raccs) in loaded.iter().enumerate() {
            if res[i].is_err() || raccs.is_err() {
                continue;
            }

            let message = &txs[i].message();
            let acc = raccs.as_ref().unwrap();
            for (key, account) in message.account_keys.iter().zip(acc.0.iter()) {
                accounts.push((key, account));
            }
        }
        self.store(fork, &accounts);
    }

    pub fn add_root(&self, fork: Fork) {
        self.accounts_index.write().unwrap().add_root(fork)
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

    pub fn new(in_paths: Option<String>) -> Self {
        let (paths, own_paths) = if in_paths.is_none() {
            (Self::make_default_paths(), true)
        } else {
            (in_paths.unwrap(), false)
        };
        let accounts_db = Arc::new(AccountsDB::new(&paths));
        Accounts {
            accounts_db,
            account_locks: Mutex::new(HashSet::new()),
            paths,
            own_paths,
        }
    }
    pub fn new_from_parent(parent: &Accounts) -> Self {
        let accounts_db = parent.accounts_db.clone();
        Accounts {
            accounts_db,
            account_locks: Mutex::new(HashSet::new()),
            paths: parent.paths.clone(),
            own_paths: parent.own_paths,
        }
    }

    /// Slow because lock is held for 1 operation instead of many
    pub fn load_slow(&self, ancestors: &HashMap<Fork, usize>, pubkey: &Pubkey) -> Option<Account> {
        self.accounts_db
            .load_slow(ancestors, pubkey)
            .filter(|acc| acc.lamports != 0)
    }

    pub fn load_by_program(&self, fork: Fork, program_id: &Pubkey) -> Vec<(Pubkey, Account)> {
        let accumulator: Vec<Vec<StoredAccount>> = self.accounts_db.scan_account_storage(
            fork,
            |stored_account: &StoredAccount, accum: &mut Vec<StoredAccount>| {
                if stored_account.account.owner == *program_id {
                    accum.push(stored_account.clone())
                }
            },
        );
        let mut versions: Vec<StoredAccount> = accumulator.into_iter().flat_map(|x| x).collect();
        versions.sort_by_key(|s| (s.pubkey, (s.write_version as i64).neg()));
        versions.dedup_by_key(|s| s.pubkey);
        versions
            .into_iter()
            .map(|s| (s.pubkey, s.account))
            .collect()
    }

    /// Slow because lock is held for 1 operation instead of many
    pub fn store_slow(&self, fork: Fork, pubkey: &Pubkey, account: &Account) {
        self.accounts_db.store(fork, &[(pubkey, account)]);
    }

    fn lock_account(
        locks: &mut HashSet<Pubkey>,
        keys: &[Pubkey],
        error_counters: &mut ErrorCounters,
    ) -> Result<()> {
        // Copy all the accounts
        for k in keys {
            if locks.contains(k) {
                error_counters.account_in_use += 1;
                debug!("Account in use: {:?}", k);
                return Err(TransactionError::AccountInUse);
            }
        }
        for k in keys {
            locks.insert(*k);
        }
        Ok(())
    }

    fn unlock_account(tx: &Transaction, result: &Result<()>, locks: &mut HashSet<Pubkey>) {
        match result {
            Err(TransactionError::AccountInUse) => (),
            _ => {
                for k in &tx.message().account_keys {
                    locks.remove(k);
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
    pub fn lock_accounts(&self, txs: &[Transaction]) -> Vec<Result<()>> {
        let mut account_locks = self.account_locks.lock().unwrap();
        let mut error_counters = ErrorCounters::default();
        let rv = txs
            .iter()
            .map(|tx| {
                Self::lock_account(
                    &mut account_locks,
                    &tx.message().account_keys,
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
    pub fn unlock_accounts(&self, txs: &[Transaction], results: &[Result<()>]) {
        let mut account_locks = self.account_locks.lock().unwrap();
        debug!("bank unlock accounts");
        txs.iter()
            .zip(results.iter())
            .for_each(|(tx, result)| Self::unlock_account(tx, result, &mut account_locks));
    }

    pub fn has_accounts(&self, fork: Fork) -> bool {
        self.accounts_db.has_accounts(fork)
    }

    pub fn load_accounts(
        &self,
        ancestors: &HashMap<Fork, usize>,
        txs: &[Transaction],
        results: Vec<Result<()>>,
        fee_calculator: &FeeCalculator,
        error_counters: &mut ErrorCounters,
    ) -> Vec<Result<(InstructionAccounts, InstructionLoaders)>> {
        self.accounts_db
            .load_accounts(ancestors, txs, results, fee_calculator, error_counters)
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

    /// Purge a fork if it is not a root
    /// Root forks cannot be purged
    pub fn purge_fork(&self, fork: Fork) {
        self.accounts_db.purge_fork(fork);
    }
    /// Add a fork to root.  Root forks cannot be purged
    pub fn add_root(&self, fork: Fork) {
        self.accounts_db.add_root(fork)
    }
}

#[cfg(test)]
mod tests {
    // TODO: all the bank tests are bank specific, issue: 2194

    use super::*;
    use rand::{thread_rng, Rng};
    use solana_sdk::account::Account;
    use solana_sdk::hash::Hash;
    use solana_sdk::instruction::CompiledInstruction;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::transaction::Transaction;

    fn cleanup_paths(paths: &str) {
        let paths = get_paths_vec(&paths);
        paths.iter().for_each(|p| {
            let _ignored = remove_dir_all(p);
        });
    }

    fn load_accounts_with_fee(
        tx: Transaction,
        ka: &Vec<(Pubkey, Account)>,
        fee_calculator: &FeeCalculator,
        error_counters: &mut ErrorCounters,
    ) -> Vec<Result<(InstructionAccounts, InstructionLoaders)>> {
        let accounts = Accounts::new(None);
        for ka in ka.iter() {
            accounts.store_slow(0, &ka.0, &ka.1);
        }

        let ancestors = vec![(0, 0)].into_iter().collect();
        let res = accounts.load_accounts(
            &ancestors,
            &[tx],
            vec![Ok(())],
            &fee_calculator,
            error_counters,
        );
        res
    }

    fn load_accounts(
        tx: Transaction,
        ka: &Vec<(Pubkey, Account)>,
        error_counters: &mut ErrorCounters,
    ) -> Vec<Result<(InstructionAccounts, InstructionLoaders)>> {
        let fee_calculator = FeeCalculator::default();
        load_accounts_with_fee(tx, ka, &fee_calculator, error_counters)
    }

    #[test]
    fn test_load_accounts_no_key() {
        let accounts: Vec<(Pubkey, Account)> = Vec::new();
        let mut error_counters = ErrorCounters::default();

        let instructions = vec![CompiledInstruction::new(1, &(), vec![0])];
        let tx = Transaction::new_with_compiled_instructions::<Keypair>(
            &[],
            &[],
            Hash::default(),
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

        let instructions = vec![CompiledInstruction::new(1, &(), vec![0])];
        let tx = Transaction::new_with_compiled_instructions(
            &[&keypair],
            &[],
            Hash::default(),
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

        let instructions = vec![CompiledInstruction::new(1, &(), vec![0])];
        let tx = Transaction::new_with_compiled_instructions(
            &[&keypair],
            &[],
            Hash::default(),
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

        let instructions = vec![CompiledInstruction::new(1, &(), vec![0])];
        let tx = Transaction::new_with_compiled_instructions(
            &[&keypair],
            &[],
            Hash::default(),
            vec![native_loader::id()],
            instructions,
        );

        let fee_calculator = FeeCalculator::new(10);
        assert_eq!(fee_calculator.calculate_fee(tx.message()), 10);

        let loaded_accounts =
            load_accounts_with_fee(tx, &accounts, &fee_calculator, &mut error_counters);

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

        let instructions = vec![CompiledInstruction::new(0, &(), vec![0, 1])];
        let tx = Transaction::new_with_compiled_instructions(
            &[&keypair],
            &[key1],
            Hash::default(),
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

        let instructions = vec![CompiledInstruction::new(0, &(), vec![0])];
        let tx = Transaction::new_with_compiled_instructions(
            &[&keypair],
            &[],
            Hash::default(),
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

        let instructions = vec![CompiledInstruction::new(0, &(), vec![0])];
        let tx = Transaction::new_with_compiled_instructions(
            &[&keypair],
            &[],
            Hash::default(),
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

        let instructions = vec![CompiledInstruction::new(0, &(), vec![0])];
        let tx = Transaction::new_with_compiled_instructions(
            &[&keypair],
            &[],
            Hash::default(),
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
            CompiledInstruction::new(0, &(), vec![0]),
            CompiledInstruction::new(1, &(), vec![0]),
        ];
        let tx = Transaction::new_with_compiled_instructions(
            &[&keypair],
            &[],
            Hash::default(),
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

        let instructions = vec![CompiledInstruction::new(0, &(), vec![0, 1])];
        // Simulate pay-to-self transaction, which loads the same account twice
        let tx = Transaction::new_with_compiled_instructions(
            &[&keypair],
            &[pubkey],
            Hash::default(),
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
    fn test_accountsdb_add_root() {
        solana_logger::setup();
        let paths = get_tmp_accounts_path!();
        let db = AccountsDB::new(&paths.paths);
        let key = Pubkey::default();
        let account0 = Account::new(1, 0, &key);

        db.store(0, &[(&key, &account0)]);
        db.add_root(0);
        let ancestors = vec![(1, 1)].into_iter().collect();
        assert_eq!(db.load_slow(&ancestors, &key), Some(account0));
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
        assert_eq!(&db.load_slow(&ancestors, &key).unwrap(), &account1);

        let ancestors = vec![(1, 1), (0, 0)].into_iter().collect();
        assert_eq!(&db.load_slow(&ancestors, &key).unwrap(), &account1);
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
        assert_eq!(&db.load_slow(&ancestors, &key).unwrap(), &account1);

        let ancestors = vec![(1, 1), (0, 0)].into_iter().collect();
        assert_eq!(&db.load_slow(&ancestors, &key).unwrap(), &account1);
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
        assert_eq!(&db.load_slow(&ancestors, &key).unwrap(), &account1);

        // we should see 1 token in fork 2
        let ancestors = vec![(0, 0), (2, 2)].into_iter().collect();
        assert_eq!(&db.load_slow(&ancestors, &key).unwrap(), &account0);

        db.add_root(0);

        let ancestors = vec![(1, 1)].into_iter().collect();
        assert_eq!(db.load_slow(&ancestors, &key), Some(account1));
        let ancestors = vec![(2, 2)].into_iter().collect();
        assert_eq!(db.load_slow(&ancestors, &key), Some(account0)); // original value
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
            assert_eq!(default_account, account);
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
            assert_eq!(&default_account, &account0);
            assert_eq!(&default_account, &account1);
        }
    }

    #[test]
    #[ignore]
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
            assert_eq!(stores[0].count.load(Ordering::Relaxed), 2);
            assert_eq!(stores[1].count.load(Ordering::Relaxed), 2);
        }
        db.add_root(1);
        {
            let stores = db.storage.read().unwrap();
            assert_eq!(stores.len(), 2);
            assert_eq!(stores[0].count.load(Ordering::Relaxed), 2);
            assert_eq!(stores[1].count.load(Ordering::Relaxed), 2);
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
        assert_eq!(db0.load_slow(&ancestors, &key), Some(account1));

        let mut accounts1 = Accounts::new(None);
        accounts1.accounts_db = Arc::new(db0);
        assert_eq!(accounts1.load_slow(&ancestors, &key), None);
        let ancestors = vec![(0, 0)].into_iter().collect();
        assert_eq!(accounts1.load_slow(&ancestors, &key), Some(account0));
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
            if let Some(mut account) = accounts.load_slow(&ancestors, &pubkeys[idx]) {
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
        assert_eq!(
            stores[0].get_status(),
            AccountStorageStatus::StorageAvailable
        );
        stores[0].count.load(Ordering::Relaxed) == count
    }

    fn check_accounts(accounts: &AccountsDB, pubkeys: &Vec<Pubkey>, fork: Fork) {
        for _ in 1..100 {
            let idx = thread_rng().gen_range(0, 99);
            let ancestors = vec![(fork, 0)].into_iter().collect();
            let account = accounts.load_slow(&ancestors, &pubkeys[idx]).unwrap();
            let mut default_account = Account::default();
            default_account.lamports = (idx + 1) as u64;
            assert_eq!(default_account, account);
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
        assert_eq!(default_account, account);
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
                accounts.load_slow(&ancestors, &key).unwrap().lamports,
                (i as u64) + 1
            );
        }

        let mut append_vec_histogram = HashMap::new();
        for storage in accounts.storage.read().unwrap().iter() {
            *append_vec_histogram.entry(storage.fork_id).or_insert(0) += 1;
        }
        for count in append_vec_histogram.values() {
            assert!(*count >= 2);
        }
    }

    #[test]
    #[ignore]
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
            assert_eq!(stores[0].count.load(Ordering::Relaxed), 1);
            assert_eq!(stores[0].get_status(), status[0]);
        }

        let pubkey2 = Pubkey::new_rand();
        let account2 = Account::new(1, ACCOUNT_DATA_FILE_SIZE as usize / 2, &pubkey2);
        accounts.store(0, &[(&pubkey2, &account2)]);
        {
            let stores = accounts.storage.read().unwrap();
            assert_eq!(stores.len(), 2);
            assert_eq!(stores[0].count.load(Ordering::Relaxed), 1);
            assert_eq!(stores[0].get_status(), status[1]);
            assert_eq!(stores[1].count.load(Ordering::Relaxed), 1);
            assert_eq!(stores[1].get_status(), status[0]);
        }
        let ancestors = vec![(0, 0)].into_iter().collect();
        assert_eq!(accounts.load_slow(&ancestors, &pubkey1).unwrap(), account1);
        assert_eq!(accounts.load_slow(&ancestors, &pubkey2).unwrap(), account2);

        for i in 0..25 {
            let index = i % 2;
            accounts.store(0, &[(&pubkey1, &account1)]);
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
            let ancestors = vec![(0, 0)].into_iter().collect();
            assert_eq!(accounts.load_slow(&ancestors, &pubkey1).unwrap(), account1);
            assert_eq!(accounts.load_slow(&ancestors, &pubkey2).unwrap(), account2);
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
    fn test_accounts_empty_hash_internal_state() {
        let paths = get_tmp_accounts_path!();
        let accounts = AccountsDB::new(&paths.paths);
        assert_eq!(accounts.hash_internal_state(0), None);
    }

    #[test]
    fn test_accountsdb_account_not_found() {
        let paths = get_tmp_accounts_path!();
        let accounts = AccountsDB::new(&paths.paths);
        let mut error_counters = ErrorCounters::default();
        let ancestors = vec![(0, 0)].into_iter().collect();

        let accounts_index = accounts.accounts_index.read().unwrap();
        let storage = accounts.storage.read().unwrap();
        assert_eq!(
            AccountsDB::load_executable_accounts(
                &storage,
                &ancestors,
                &accounts_index,
                &Pubkey::new_rand(),
                &mut error_counters
            ),
            Err(TransactionError::AccountNotFound)
        );
        assert_eq!(error_counters.account_not_found, 1);
    }

    #[test]
    fn test_load_by_program() {
        let paths = get_tmp_accounts_path!();
        let accounts_db = AccountsDB::new(&paths.paths);

        // Load accounts owned by various programs into AccountsDB
        let pubkey0 = Pubkey::new_rand();
        let account0 = Account::new(1, 0, &Pubkey::new(&[2; 32]));
        accounts_db.store(0, &[(&pubkey0, &account0)]);
        let pubkey1 = Pubkey::new_rand();
        let account1 = Account::new(1, 0, &Pubkey::new(&[2; 32]));
        accounts_db.store(0, &[(&pubkey1, &account1)]);
        let pubkey2 = Pubkey::new_rand();
        let account2 = Account::new(1, 0, &Pubkey::new(&[3; 32]));
        accounts_db.store(0, &[(&pubkey2, &account2)]);

        let mut accounts_proper = Accounts::new(None);
        accounts_proper.accounts_db = Arc::new(accounts_db);
        let accounts = accounts_proper.load_by_program(0, &Pubkey::new(&[2; 32]));
        assert_eq!(accounts.len(), 2);
        let accounts = accounts_proper.load_by_program(0, &Pubkey::new(&[3; 32]));
        assert_eq!(accounts, vec![(pubkey2, account2)]);
        let accounts = accounts_proper.load_by_program(0, &Pubkey::new(&[4; 32]));
        assert_eq!(accounts, vec![]);
    }
}

use crate::appendvec::AppendVec;
use crate::bank::{BankError, Result};
use crate::runtime::has_duplicates;
use bincode::serialize;
use hashbrown::{HashMap, HashSet};
use log::{debug, Level};
use solana_metrics::counter::Counter;
use solana_sdk::account::Account;
use solana_sdk::hash::{hash, Hash};
use solana_sdk::native_loader;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::transaction::Transaction;
use solana_sdk::vote_program;
use std::collections::BTreeMap;
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
const NUM_ACCOUNT_DIRS: usize = 4;

/// An offset into the AccountsDB::storage vector
type AppendVecId = usize;

type Fork = u64;

struct AccountMap(Vec<(Fork, (AppendVecId, u64))>);

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

// This structure handles the load/store of the accounts
pub struct AccountsDB {
    /// Keeps tracks of index into AppendVec on a per fork basis
    index_info: AccountIndexInfo,

    /// Account storage
    storage: RwLock<Vec<AccountStorage>>,

    /// distribute the accounts across storage lists
    next_id: AtomicUsize,

    /// The number of transactions the bank has processed without error since the
    /// start of the ledger.
    transaction_count: RwLock<u64>,
}

impl Default for AccountsDB {
    fn default() -> Self {
        let index_info = AccountIndexInfo {
            index: RwLock::new(HashMap::new()),
            vote_index: RwLock::new(HashSet::new()),
        };
        AccountsDB {
            index_info,
            storage: RwLock::new(vec![]),
            next_id: AtomicUsize::new(0),
            transaction_count: RwLock::new(0),
        }
    }
}

/// This structure handles synchronization for db
pub struct Accounts {
    pub accounts_db: AccountsDB,

    /// set of accounts which are currently in the pipeline
    account_locks: Mutex<HashSet<Pubkey>>,

    /// List of persistent stores
    paths: String,
}

impl Drop for Accounts {
    fn drop(&mut self) {
        let paths: Vec<String> = self.paths.split(',').map(|s| s.to_string()).collect();
        paths.iter().for_each(|p| {
            let _ignored = remove_dir_all(p);
        })
    }
}

impl AccountsDB {
    pub fn add_storage(&self, paths: &str) {
        let paths: Vec<String> = paths.split(',').map(|s| s.to_string()).collect();
        let mut stores: Vec<AccountStorage> = vec![];
        paths.iter().for_each(|p| {
            let path = format!("{}/{}", p, std::process::id());
            let storage = AccountStorage {
                appendvec: self.new_account_storage(&path),
                status: AtomicUsize::new(AccountStorageStatus::StorageAvailable as usize),
                count: AtomicUsize::new(0),
                path: path.to_string(),
            };
            stores.push(storage);
        });
        let _ignored = stores[0].appendvec.write().unwrap().grow_file();
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

    pub fn get_vote_accounts(&self, fork: Fork) -> Vec<Account> {
        let mut accounts: Vec<Account> = vec![];
        self.index_info
            .vote_index
            .read()
            .unwrap()
            .iter()
            .for_each(|p| {
                if let Some(forks) = self.index_info.index.read().unwrap().get(p) {
                    for (v_fork, (id, index)) in forks.0.iter() {
                        if fork == *v_fork {
                            accounts.push(
                                self.storage.read().unwrap()[*id]
                                    .appendvec
                                    .read()
                                    .unwrap()
                                    .get_account(*index)
                                    .unwrap(),
                            );
                        }
                        if fork > *v_fork {
                            break;
                        }
                    }
                }
            });
        accounts
    }

    pub fn hash_internal_state(&self, fork: Fork) -> Option<Hash> {
        let mut ordered_accounts = BTreeMap::new();
        let rindex = self.index_info.index.read().unwrap();
        rindex.iter().for_each(|(p, forks)| {
            for (v_fork, (id, index)) in forks.0.iter() {
                if fork == *v_fork {
                    let account = self.storage.read().unwrap()[*id]
                        .appendvec
                        .read()
                        .unwrap()
                        .get_account(*index)
                        .unwrap();
                    ordered_accounts.insert(*p, account);
                }
                if fork > *v_fork {
                    break;
                }
            }
        });

        if ordered_accounts.is_empty() {
            return None;
        }
        Some(hash(&serialize(&ordered_accounts).unwrap()))
    }

    fn load(&self, fork: Fork, pubkey: &Pubkey) -> Option<Account> {
        let index = self.index_info.index.read().unwrap();
        if let Some(forks) = index.get(pubkey) {
            // find most recent fork that is an ancestor of current_fork
            for (v_fork, (id, offset)) in forks.0.iter() {
                if *v_fork > fork {
                    continue;
                } else {
                    let appendvec = &self.storage.read().unwrap()[*id].appendvec;
                    let av = appendvec.read().unwrap();
                    return Some(av.get_account(*offset).unwrap());
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
        let mut acc = &Account::default();
        loop {
            let result: Option<u64>;
            {
                if account.tokens != 0 {
                    acc = account;
                }
                let av = &self.storage.read().unwrap()[id].appendvec;
                result = av.read().unwrap().append_account(&acc);
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

    /// Store the account update.  If the update is to delete the account because the token balance
    /// is 0, purge needs to be set to true for the delete to occur in place.
    pub fn store(&self, fork: Fork, purge: bool, pubkey: &Pubkey, account: &Account) {
        if account.tokens == 0 && purge {
            // purge if balance is 0 and no checkpoints
            let mut index = self.index_info.index.write().unwrap();
            if let Some(forks) = index.remove(pubkey) {
                let stores = self.storage.read().unwrap();
                for (_, (id, _)) in forks.0.iter() {
                    stores[*id].count.fetch_sub(1, Ordering::Relaxed);
                }
            }
            if vote_program::check_id(&account.owner) {
                self.index_info.vote_index.write().unwrap().remove(pubkey);
            }
        } else {
            let (id, offset) = self.append_account(&account);

            if vote_program::check_id(&account.owner) {
                let mut index = self.index_info.vote_index.write().unwrap();
                if account.tokens == 0 {
                    index.remove(pubkey);
                } else {
                    index.insert(*pubkey);
                }
            }

            let mut result: Option<usize> = None;
            {
                let mut insert: Option<usize> = None;
                let mut windex = self.index_info.index.write().unwrap();
                let forks = windex.entry(*pubkey).or_insert(AccountMap(vec![]));
                for (i, (v_fork, (v_id, _))) in forks.0.iter().enumerate() {
                    if *v_fork > fork {
                        continue;
                    }
                    if *v_fork == fork {
                        result = Some(*v_id);
                        forks.0[i] = (fork, (id, offset));
                        break;
                    }
                    insert = Some(i);
                    break;
                }
                if result.is_none() {
                    if let Some(index) = insert {
                        forks.0.insert(index, (fork, (id, offset)));
                    } else {
                        forks.0.push((fork, (id, offset)));
                    }
                }
            }
            let stores = self.storage.read().unwrap();
            stores[id].count.fetch_add(1, Ordering::Relaxed);
            if let Some(old_id) = result {
                if stores[old_id].count.fetch_sub(1, Ordering::Relaxed) == 1 {
                    stores[old_id].appendvec.write().unwrap().reset();
                    stores[old_id].set_status(AccountStorageStatus::StorageAvailable);
                }
            }
        }
    }

    pub fn store_accounts(
        &self,
        fork: Fork,
        purge: bool,
        txs: &[Transaction],
        res: &[Result<()>],
        loaded: &[Result<(InstructionAccounts, InstructionLoaders)>],
    ) {
        for (i, raccs) in loaded.iter().enumerate() {
            if res[i].is_err() || raccs.is_err() {
                continue;
            }

            let tx = &txs[i];
            let acc = raccs.as_ref().unwrap();
            for (key, account) in tx.account_keys.iter().zip(acc.0.iter()) {
                self.store(fork, purge, key, account);
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
                called_accounts.push(self.load(fork, key).unwrap_or_default());
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

            let program = match self.load(fork, &program_id) {
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

    pub fn increment_transaction_count(&self, tx_count: usize) {
        let mut tx = self.transaction_count.write().unwrap();
        *tx += tx_count as u64;
    }

    pub fn transaction_count(&self) -> u64 {
        let tx = self.transaction_count.read().unwrap();
        *tx
    }
    //pub fn account_values_slow(&self) -> Vec<(Pubkey, solana_sdk::account::Account)> {
    //    self.accounts.iter().map(|(x, y)| (*x, y.clone())).collect()
    //}
    //fn merge(&mut self, other: Self) {
    //    self.transaction_count += other.transaction_count;
    //    self.accounts.extend(other.accounts)
    //}
}

impl Accounts {
    pub fn new(in_paths: &str) -> Self {
        static ACCOUNT_DIR: AtomicUsize = AtomicUsize::new(0);
        let paths = if !in_paths.is_empty() {
            in_paths.to_string()
        } else {
            let mut dir: usize;
            dir = ACCOUNT_DIR.fetch_add(1, Ordering::Relaxed);
            let mut paths = dir.to_string();
            for _ in 1..NUM_ACCOUNT_DIRS {
                dir = ACCOUNT_DIR.fetch_add(1, Ordering::Relaxed);
                paths = format!("{},{}", paths, dir.to_string());
            }
            paths
        };
        let accounts_db = AccountsDB::default();
        accounts_db.add_storage(&paths);
        Accounts {
            accounts_db,
            account_locks: Mutex::new(HashSet::new()),
            paths,
        }
    }

    /// Slow because lock is held for 1 operation insted of many
    pub fn load_slow(&self, fork: Fork, pubkey: &Pubkey) -> Option<Account> {
        self.accounts_db.load(fork, pubkey)
    }

    /// Slow because lock is held for 1 operation insted of many
    /// * purge - if the account token value is 0 and purge is true then delete the account.
    /// purge should be set to false for overlays, and true for the root checkpoint.
    pub fn store_slow(&self, fork: Fork, purge: bool, pubkey: &Pubkey, account: &Account) {
        self.accounts_db.store(fork, purge, pubkey, account)
    }

    fn lock_account(
        account_locks: &mut HashSet<Pubkey>,
        keys: &[Pubkey],
        error_counters: &mut ErrorCounters,
    ) -> Result<()> {
        // Copy all the accounts
        for k in keys {
            if account_locks.contains(k) {
                error_counters.account_in_use += 1;
                return Err(BankError::AccountInUse);
            }
        }
        for k in keys {
            account_locks.insert(*k);
        }
        Ok(())
    }

    fn unlock_account(tx: &Transaction, result: &Result<()>, account_locks: &mut HashSet<Pubkey>) {
        match result {
            Err(BankError::AccountInUse) => (),
            _ => {
                for k in &tx.account_keys {
                    account_locks.remove(k);
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
            .map(|tx| Self::lock_account(&mut account_locks, &tx.account_keys, &mut error_counters))
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
    /// * purge - if the account token value is 0 and purge is true then delete the account.
    /// purge should be set to false for overlays, and true for the root checkpoint.
    pub fn store_accounts(
        &self,
        fork: Fork,
        purge: bool,
        txs: &[Transaction],
        res: &[Result<()>],
        loaded: &[Result<(InstructionAccounts, InstructionLoaders)>],
    ) {
        self.accounts_db
            .store_accounts(fork, purge, txs, res, loaded)
    }

    pub fn increment_transaction_count(&self, tx_count: usize) {
        self.accounts_db.increment_transaction_count(tx_count)
    }

    pub fn transaction_count(&self) -> u64 {
        self.accounts_db.transaction_count()
    }
    ///// accounts starts with an empty data structure for every fork
    ///// self is root, merge the fork into self
    //pub fn merge_into_root(&self, other: Self) {
    //    assert!(other.account_locks.lock().unwrap().is_empty());
    //    let db = other.accounts_db.into_inner().unwrap();
    //    let mut mydb = self.accounts_db.write().unwrap();
    //    mydb.merge(db)
    //}
    //pub fn copy_for_tpu(&self) -> Self {
    //    //TODO: deprecate this in favor of forks and merge_into_root
    //    let copy = Accounts::default();

    //    {
    //        let mut accounts_db = copy.accounts_db.write().unwrap();
    //        for (key, val) in self.accounts_db.read().unwrap().accounts.iter() {
    //            accounts_db.accounts.insert(key.clone(), val.clone());
    //        }
    //        accounts_db.transaction_count = self.transaction_count();
    //    }
    //    copy
    //}
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

    #[test]
    fn test_purge() {
        let paths = "purge".to_string();
        let db = AccountsDB::default();
        db.add_storage(&paths);
        let key = Pubkey::default();
        let account = Account::new(0, 0, Pubkey::default());
        // accounts are deleted when their token value is 0 and purge is true
        db.store(0, false, &key, &account);
        assert_eq!(db.load(0, &key), Some(account.clone()));
        // purge should be set to true for the root checkpoint
        db.store(0, true, &key, &account);
        assert_eq!(db.load(0, &key), None);
        cleanup_dirs(&paths);
    }

    fn load_accounts(
        tx: Transaction,
        ka: &Vec<(Pubkey, Account)>,
        error_counters: &mut ErrorCounters,
    ) -> Vec<Result<(InstructionAccounts, InstructionLoaders)>> {
        let accounts = Accounts::new("");
        for ka in ka.iter() {
            accounts.store_slow(0, true, &ka.0, &ka.1);
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

    fn create_account(
        accounts: &AccountsDB,
        pubkeys: &mut Vec<Pubkey>,
        num: usize,
        num_vote: usize,
    ) {
        let mut nvote = num_vote;
        for t in 0..num {
            let pubkey = Keypair::new().pubkey();
            let mut default_account = Account::default();
            pubkeys.push(pubkey.clone());
            default_account.tokens = (t + 1) as u64;
            if nvote > 0 && (t + 1) % nvote == 0 {
                default_account.owner = vote_program::id();
                nvote -= 1;
            }
            assert!(accounts.load(0, &pubkey).is_none());
            accounts.store(0, true, &pubkey, &default_account);
        }
    }

    fn update_accounts(accounts: &AccountsDB, pubkeys: Vec<Pubkey>, range: usize) {
        for _ in 1..1000 {
            let idx = thread_rng().gen_range(0, range);
            if let Some(mut account) = accounts.load(0, &pubkeys[idx]) {
                account.tokens = account.tokens + 1;
                accounts.store(0, true, &pubkeys[idx], &account);
                if account.tokens == 0 {
                    assert!(accounts.load(0, &pubkeys[idx]).is_none());
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

    fn cleanup_dirs(paths: &str) {
        let paths: Vec<String> = paths.split(',').map(|s| s.to_string()).collect();
        paths.iter().for_each(|p| {
            let _ignored = remove_dir_all(p);
        })
    }

    #[test]
    fn test_account_one() {
        let paths = "one".to_string();
        let accounts = AccountsDB::default();
        accounts.add_storage(&paths);
        let mut pubkeys: Vec<Pubkey> = vec![];
        create_account(&accounts, &mut pubkeys, 1, 0);
        let account = accounts.load(0, &pubkeys[0]).unwrap();
        let mut default_account = Account::default();
        default_account.tokens = 1;
        assert_eq!(compare_account(&default_account, &account), true);
        cleanup_dirs(&paths);
    }

    #[test]
    fn test_account_many() {
        let paths = "many0,many1".to_string();
        let accounts = AccountsDB::default();
        accounts.add_storage(&paths);
        let mut pubkeys: Vec<Pubkey> = vec![];
        create_account(&accounts, &mut pubkeys, 100, 0);
        for _ in 1..100 {
            let idx = thread_rng().gen_range(0, 99);
            let account = accounts.load(0, &pubkeys[idx]).unwrap();
            let mut default_account = Account::default();
            default_account.tokens = (idx + 1) as u64;
            assert_eq!(compare_account(&default_account, &account), true);
        }
        cleanup_dirs(&paths);
    }

    #[test]
    fn test_account_update() {
        let paths = "update0".to_string();
        let accounts = AccountsDB::default();
        accounts.add_storage(&paths);
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
        let accounts = AccountsDB::default();
        accounts.add_storage(&paths);
        let count = [0, 1];
        let status = [
            AccountStorageStatus::StorageAvailable,
            AccountStorageStatus::StorageFull,
        ];
        let pubkey1 = Keypair::new().pubkey();
        let account1 = Account::new(1, ACCOUNT_DATA_FILE_SIZE as usize / 2, pubkey1);
        accounts.store(0, true, &pubkey1, &account1);
        {
            let stores = accounts.storage.read().unwrap();
            assert_eq!(stores.len(), 1);
            assert_eq!(stores[0].count.load(Ordering::Relaxed), 1);
            assert_eq!(stores[0].get_status(), status[0]);
        }

        let pubkey2 = Keypair::new().pubkey();
        let account2 = Account::new(1, ACCOUNT_DATA_FILE_SIZE as usize / 2, pubkey2);
        accounts.store(0, true, &pubkey2, &account2);
        {
            let stores = accounts.storage.read().unwrap();
            assert_eq!(stores.len(), 2);
            assert_eq!(stores[0].count.load(Ordering::Relaxed), 1);
            assert_eq!(stores[0].get_status(), status[1]);
            assert_eq!(stores[1].count.load(Ordering::Relaxed), 1);
            assert_eq!(stores[1].get_status(), status[0]);
        }
        assert_eq!(accounts.load(0, &pubkey1).unwrap(), account1);
        assert_eq!(accounts.load(0, &pubkey2).unwrap(), account2);

        for i in 0..25 {
            let index = i % 2;
            accounts.store(0, true, &pubkey1, &account1);
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
            assert_eq!(accounts.load(0, &pubkey1).unwrap(), account1);
            assert_eq!(accounts.load(0, &pubkey2).unwrap(), account2);
        }
        cleanup_dirs(&paths);
    }

    #[test]
    fn test_account_vote() {
        let paths = "vote0".to_string();
        let accounts = AccountsDB::default();
        accounts.add_storage(&paths);
        let mut pubkeys: Vec<Pubkey> = vec![];
        create_account(&accounts, &mut pubkeys, 100, 6);
        let accounts = accounts.get_vote_accounts(0);
        assert_eq!(accounts.len(), 6);
        accounts.iter().for_each(|account| {
            assert_eq!(account.owner, vote_program::id());
        });
        cleanup_dirs(&paths);
    }
}

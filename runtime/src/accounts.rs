use crate::accounts_db::{
    get_paths_vec, AccountInfo, AccountStorage, AccountsDB, AppendVecId, ErrorCounters,
    InstructionAccounts, InstructionCredits, InstructionLoaders,
};
use crate::accounts_index::{AccountsIndex, Fork};
use crate::append_vec::StoredAccount;
use crate::blockhash_queue::BlockhashQueue;
use crate::message_processor::has_duplicates;
use bincode::serialize;
use log::*;
use solana_metrics::inc_new_counter_error;
use solana_sdk::account::Account;
use solana_sdk::hash::{Hash, Hasher};
use solana_sdk::message::Message;
use solana_sdk::native_loader;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::syscall;
use solana_sdk::system_program;
use solana_sdk::transaction::Result;
use solana_sdk::transaction::{Transaction, TransactionError};
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs::remove_dir_all;
use std::io::{BufReader, Read};
use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};

const ACCOUNTSDB_DIR: &str = "accountsdb";
const NUM_ACCOUNT_DIRS: usize = 4;

#[derive(Default, Debug)]
struct CreditOnlyLock {
    credits: AtomicU64,
    lock_count: Mutex<u64>,
}

/// This structure handles synchronization for db
#[derive(Default, Debug)]
pub struct Accounts {
    /// Single global AccountsDB
    pub accounts_db: Arc<AccountsDB>,

    /// set of credit-debit accounts which are currently in the pipeline
    account_locks: Mutex<HashSet<Pubkey>>,

    /// Set of credit-only accounts which are currently in the pipeline, caching account balance
    /// and number of locks. On commit_credits(), we do a take() on the option so that the hashmap
    /// is no longer available to be written to.
    credit_only_account_locks: Arc<RwLock<Option<HashMap<Pubkey, CreditOnlyLock>>>>,

    /// List of persistent stores
    pub paths: String,

    /// set to true if object created the directories in paths
    /// when true, delete parents of 'paths' on drop
    own_paths: bool,
}

impl Drop for Accounts {
    fn drop(&mut self) {
        if self.own_paths {
            let paths = get_paths_vec(&self.paths);
            paths.iter().for_each(|p| {
                let _ignored = remove_dir_all(p);

                // it is safe to delete the parent
                let path = Path::new(p);
                let _ignored = remove_dir_all(path.parent().unwrap());
            });
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
            credit_only_account_locks: Arc::new(RwLock::new(Some(HashMap::new()))),
            paths,
            own_paths,
        }
    }
    pub fn new_from_parent(parent: &Accounts) -> Self {
        let accounts_db = parent.accounts_db.clone();
        Accounts {
            accounts_db,
            account_locks: Mutex::new(HashSet::new()),
            credit_only_account_locks: Arc::new(RwLock::new(Some(HashMap::new()))),
            paths: parent.paths.clone(),
            own_paths: parent.own_paths,
        }
    }

    pub fn update_from_stream<R: Read>(
        &self,
        stream: &mut BufReader<R>,
    ) -> std::result::Result<(), std::io::Error> {
        self.accounts_db.update_from_stream(stream)
    }

    fn load_tx_accounts(
        storage: &AccountStorage,
        ancestors: &HashMap<Fork, usize>,
        accounts_index: &AccountsIndex<AccountInfo>,
        tx: &Transaction,
        fee: u64,
        error_counters: &mut ErrorCounters,
    ) -> Result<(Vec<Account>, InstructionCredits)> {
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
            let mut credits: InstructionCredits = vec![];
            for key in message.account_keys.iter() {
                if !message.program_ids().contains(&key) {
                    called_accounts.push(
                        AccountsDB::load(storage, ancestors, accounts_index, key)
                            .map(|(account, _)| account)
                            .unwrap_or_default(),
                    );
                    credits.push(0);
                }
            }
            if called_accounts.is_empty() || called_accounts[0].lamports == 0 {
                error_counters.account_not_found += 1;
                Err(TransactionError::AccountNotFound)
            } else if called_accounts[0].owner != system_program::id() {
                error_counters.invalid_account_for_fee += 1;
                Err(TransactionError::InvalidAccountForFee)
            } else if called_accounts[0].lamports < fee {
                error_counters.insufficient_funds += 1;
                Err(TransactionError::InsufficientFundsForFee)
            } else {
                called_accounts[0].lamports -= fee;
                Ok((called_accounts, credits))
            }
        }
    }

    fn load_executable_accounts(
        storage: &AccountStorage,
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

            let program = match AccountsDB::load(storage, ancestors, accounts_index, &program_id)
                .map(|(account, _)| account)
            {
                Some(program) => program,
                None => {
                    error_counters.account_not_found += 1;
                    return Err(TransactionError::ProgramAccountNotFound);
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
        storage: &AccountStorage,
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
                if message.account_keys.len() <= ix.program_id_index as usize {
                    error_counters.account_not_found += 1;
                    return Err(TransactionError::AccountNotFound);
                }
                let program_id = message.account_keys[ix.program_id_index as usize];
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

    pub fn load_accounts(
        &self,
        ancestors: &HashMap<Fork, usize>,
        txs: &[Transaction],
        lock_results: Vec<Result<()>>,
        hash_queue: &BlockhashQueue,
        error_counters: &mut ErrorCounters,
    ) -> Vec<Result<(InstructionAccounts, InstructionLoaders, InstructionCredits)>> {
        //PERF: hold the lock to scan for the references, but not to clone the accounts
        //TODO: two locks usually leads to deadlocks, should this be one structure?
        let accounts_index = self.accounts_db.accounts_index.read().unwrap();
        let storage = self.accounts_db.storage.read().unwrap();
        txs.iter()
            .zip(lock_results.into_iter())
            .map(|etx| match etx {
                (tx, Ok(())) => {
                    let fee_calculator = hash_queue
                        .get_fee_calculator(&tx.message().recent_blockhash)
                        .ok_or(TransactionError::BlockhashNotFound)?;

                    let fee = fee_calculator.calculate_fee(tx.message());
                    let (accounts, credits) = Self::load_tx_accounts(
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
                    Ok((accounts, loaders, credits))
                }
                (_, Err(e)) => Err(e),
            })
            .collect()
    }

    /// Slow because lock is held for 1 operation instead of many
    pub fn load_slow(
        &self,
        ancestors: &HashMap<Fork, usize>,
        pubkey: &Pubkey,
    ) -> Option<(Account, Fork)> {
        self.accounts_db
            .load_slow(ancestors, pubkey)
            .filter(|(acc, _)| acc.lamports != 0)
    }

    /// scans underlying accounts_db for this delta (fork) with a map function
    ///   from StoredAccount to B
    /// returns only the latest/current version of B for this fork
    fn scan_fork<F, B>(&self, fork: Fork, func: F) -> Vec<B>
    where
        F: Fn(&StoredAccount) -> Option<B>,
        F: Send + Sync,
        B: Send + Default,
    {
        let accumulator: Vec<Vec<(Pubkey, u64, B)>> = self.accounts_db.scan_account_storage(
            fork,
            |stored_account: &StoredAccount,
             _id: AppendVecId,
             accum: &mut Vec<(Pubkey, u64, B)>| {
                if let Some(val) = func(stored_account) {
                    accum.push((
                        stored_account.meta.pubkey,
                        std::u64::MAX - stored_account.meta.write_version,
                        val,
                    ));
                }
            },
        );

        let mut versions: Vec<(Pubkey, u64, B)> = accumulator.into_iter().flat_map(|x| x).collect();
        versions.sort_by_key(|s| (s.0, s.1));
        versions.dedup_by_key(|s| s.0);
        versions
            .into_iter()
            .map(|(_pubkey, _version, val)| val)
            .collect()
    }

    pub fn load_by_program_fork(&self, fork: Fork, program_id: &Pubkey) -> Vec<(Pubkey, Account)> {
        self.scan_fork(fork, |stored_account| {
            if stored_account.balance.owner == *program_id {
                Some((stored_account.meta.pubkey, stored_account.clone_account()))
            } else {
                None
            }
        })
    }

    pub fn load_by_program(
        &self,
        ancestors: &HashMap<Fork, usize>,
        program_id: &Pubkey,
    ) -> Vec<(Pubkey, Account)> {
        self.accounts_db.scan_accounts(
            ancestors,
            |collector: &mut Vec<(Pubkey, Account)>, option| {
                if let Some(data) = option
                    .filter(|(_, account, _)| account.owner == *program_id && account.lamports != 0)
                    .map(|(pubkey, account, _fork)| (*pubkey, account))
                {
                    collector.push(data)
                }
            },
        )
    }

    /// Slow because lock is held for 1 operation instead of many
    pub fn store_slow(&self, fork: Fork, pubkey: &Pubkey, account: &Account) {
        let mut accounts = HashMap::new();
        accounts.insert(pubkey, account);
        self.accounts_db.store(fork, &accounts);
    }

    fn get_read_access_credit_only<'a>(
        credit_only_locks: &'a RwLockReadGuard<Option<HashMap<Pubkey, CreditOnlyLock>>>,
    ) -> Result<&'a HashMap<Pubkey, CreditOnlyLock>> {
        credit_only_locks
            .as_ref()
            .ok_or(TransactionError::AccountInUse)
    }

    fn get_write_access_credit_only<'a>(
        credit_only_locks: &'a mut RwLockWriteGuard<Option<HashMap<Pubkey, CreditOnlyLock>>>,
    ) -> Result<&'a mut HashMap<Pubkey, CreditOnlyLock>> {
        credit_only_locks
            .as_mut()
            .ok_or(TransactionError::AccountInUse)
    }

    fn take_credit_only(
        credit_only_locks: &Arc<RwLock<Option<HashMap<Pubkey, CreditOnlyLock>>>>,
    ) -> Result<HashMap<Pubkey, CreditOnlyLock>> {
        let mut w_credit_only_locks = credit_only_locks.write().unwrap();
        w_credit_only_locks
            .take()
            .ok_or(TransactionError::AccountInUse)
    }

    fn lock_account(
        locks: &mut HashSet<Pubkey>,
        credit_only_locks: &Arc<RwLock<Option<HashMap<Pubkey, CreditOnlyLock>>>>,
        message: &Message,
        error_counters: &mut ErrorCounters,
    ) -> Result<()> {
        let (credit_debit_keys, credit_only_keys) = message.get_account_keys_by_lock_type();

        for k in credit_debit_keys.iter() {
            let r_credit_only_locks = credit_only_locks.read().unwrap();
            let r_credit_only_locks = Self::get_read_access_credit_only(&r_credit_only_locks)?;
            if locks.contains(k)
                || r_credit_only_locks
                    .get(&k)
                    .map_or(false, |lock| *lock.lock_count.lock().unwrap() > 0)
            {
                error_counters.account_in_use += 1;
                debug!("Account in use: {:?}", k);
                return Err(TransactionError::AccountInUse);
            }
        }
        for k in credit_only_keys.iter() {
            if locks.contains(k) {
                error_counters.account_in_use += 1;
                debug!("Account in use: {:?}", k);
                return Err(TransactionError::AccountInUse);
            }
        }

        for k in credit_debit_keys {
            locks.insert(*k);
        }
        let mut credit_only_writes: Vec<&Pubkey> = vec![];
        for k in credit_only_keys {
            let r_credit_only_locks = credit_only_locks.read().unwrap();
            let r_credit_only_locks = Self::get_read_access_credit_only(&r_credit_only_locks)?;
            if let Some(credit_only_lock) = r_credit_only_locks.get(&k) {
                *credit_only_lock.lock_count.lock().unwrap() += 1;
            } else {
                credit_only_writes.push(k);
            }
        }

        for k in credit_only_writes.iter() {
            let mut w_credit_only_locks = credit_only_locks.write().unwrap();
            let w_credit_only_locks = Self::get_write_access_credit_only(&mut w_credit_only_locks)?;
            assert!(w_credit_only_locks.get(&k).is_none());
            w_credit_only_locks.insert(
                **k,
                CreditOnlyLock {
                    credits: AtomicU64::new(0),
                    lock_count: Mutex::new(1),
                },
            );
        }

        Ok(())
    }

    fn unlock_account(
        tx: &Transaction,
        result: &Result<()>,
        locks: &mut HashSet<Pubkey>,
        credit_only_locks: &Arc<RwLock<Option<HashMap<Pubkey, CreditOnlyLock>>>>,
    ) {
        let (credit_debit_keys, credit_only_keys) = &tx.message().get_account_keys_by_lock_type();
        match result {
            Err(TransactionError::AccountInUse) => (),
            _ => {
                for k in credit_debit_keys {
                    locks.remove(k);
                }
                for k in credit_only_keys {
                    let r_credit_only_locks = credit_only_locks.read().unwrap();
                    let locks = Self::get_read_access_credit_only(&r_credit_only_locks);
                    if let Ok(locks) = locks {
                        if let Some(lock) = locks.get(&k) {
                            *lock.lock_count.lock().unwrap() -= 1;
                        }
                    }
                }
            }
        }
    }

    fn hash_account(stored_account: &StoredAccount) -> Hash {
        let mut hasher = Hasher::default();
        hasher.hash(&serialize(&stored_account.balance).unwrap());
        hasher.hash(stored_account.data);
        hasher.result()
    }

    pub fn hash_internal_state(&self, fork_id: Fork) -> Option<Hash> {
        let account_hashes = self.scan_fork(fork_id, |stored_account| {
            if !syscall::check_id(&stored_account.balance.owner) {
                Some(Self::hash_account(stored_account))
            } else {
                None
            }
        });

        if account_hashes.is_empty() {
            None
        } else {
            let mut hasher = Hasher::default();
            for hash in account_hashes {
                hasher.hash(hash.as_ref());
            }
            Some(hasher.result())
        }
    }

    /// This function will prevent multiple threads from modifying the same account state at the
    /// same time
    #[must_use]
    pub fn lock_accounts(&self, txs: &[Transaction]) -> Vec<Result<()>> {
        let mut error_counters = ErrorCounters::default();
        let rv = txs
            .iter()
            .map(|tx| {
                let message = &tx.message();
                Self::lock_account(
                    &mut self.account_locks.lock().unwrap(),
                    &self.credit_only_account_locks,
                    &message,
                    &mut error_counters,
                )
            })
            .collect();
        if error_counters.account_in_use != 0 {
            inc_new_counter_error!(
                "bank-process_transactions-account_in_use",
                error_counters.account_in_use,
                0,
                100
            );
        }
        rv
    }

    /// Once accounts are unlocked, new transactions that modify that state can enter the pipeline
    pub fn unlock_accounts(&self, txs: &[Transaction], results: &[Result<()>]) {
        let mut account_locks = self.account_locks.lock().unwrap();
        let credit_only_locks = self.credit_only_account_locks.clone();
        debug!("bank unlock accounts");
        txs.iter().zip(results.iter()).for_each(|(tx, result)| {
            Self::unlock_account(tx, result, &mut account_locks, &credit_only_locks)
        });
    }

    pub fn has_accounts(&self, fork: Fork) -> bool {
        self.accounts_db.has_accounts(fork)
    }

    /// Store the accounts into the DB
    pub fn store_accounts(
        &self,
        fork: Fork,
        txs: &[Transaction],
        res: &[Result<()>],
        loaded: &mut [Result<(InstructionAccounts, InstructionLoaders, InstructionCredits)>],
    ) {
        let accounts = self.collect_accounts(txs, res, loaded);
        // Only store credit-debit accounts immediately
        let mut accounts_to_store: HashMap<&Pubkey, &Account> = HashMap::new();

        for (pubkey, (account, is_debitable)) in accounts.iter() {
            if *is_debitable {
                accounts_to_store.insert(pubkey, account);
            }
        }
        self.accounts_db.store(fork, &accounts_to_store);
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

    /// Commit remaining credit-only changes, regardless of reference count
    ///
    /// We do a take() on `self.credit_only_account_locks` so that the hashmap is no longer
    /// available to be written to. This prevents any transactions from reinserting into the hashmap.
    /// Then there are then only 2 cases for interleaving with commit_credits and lock_accounts.
    /// Either:
    //  1) Any transactions that tries to lock after commit_credits will find the HashMap is None
    //     so will fail the lock
    //  2) Any transaction that grabs a lock and then commit_credits clears the HashMap will find
    //     the HashMap is None on unlock_accounts, and will perform a no-op.
    pub fn commit_credits(&self, ancestors: &HashMap<Fork, usize>, fork: Fork) {
        // Clear the credit only hashmap so that no further transactions can modify it
        let credit_only_account_locks = Self::take_credit_only(&self.credit_only_account_locks)
            .expect("Credit only locks didn't exist in commit_credits");
        self.store_credit_only_credits(credit_only_account_locks, ancestors, fork);
    }

    /// Used only for tests to store credit-only accounts after every transaction
    pub fn commit_credits_unsafe(&self, ancestors: &HashMap<Fork, usize>, fork: Fork) {
        // Clear the credit only hashmap so that no further transactions can modify it
        let mut w_credit_only_account_locks = self.credit_only_account_locks.write().unwrap();
        let w_credit_only_account_locks =
            Self::get_write_access_credit_only(&mut w_credit_only_account_locks)
                .expect("Credit only locks didn't exist in commit_credits");
        self.store_credit_only_credits(w_credit_only_account_locks.drain(), ancestors, fork);
    }

    fn store_credit_only_credits<I>(
        &self,
        credit_only_account_locks: I,
        ancestors: &HashMap<Fork, usize>,
        fork: Fork,
    ) where
        I: IntoIterator<Item = (Pubkey, CreditOnlyLock)>,
    {
        for (pubkey, lock) in credit_only_account_locks {
            let lock_count = *lock.lock_count.lock().unwrap();
            if lock_count != 0 {
                warn!(
                    "dropping credit-only lock on {}, still has {} locks",
                    pubkey, lock_count
                );
            }
            let credit = lock.credits.load(Ordering::Relaxed);
            if credit > 0 {
                let mut account = self
                    .load_slow(ancestors, &pubkey)
                    .map(|(account, _)| account)
                    .unwrap_or_default();
                account.lamports += credit;
                self.store_slow(fork, &pubkey, &account);
            }
        }
    }

    fn collect_accounts<'a>(
        &self,
        txs: &'a [Transaction],
        res: &'a [Result<()>],
        loaded: &'a mut [Result<(InstructionAccounts, InstructionLoaders, InstructionCredits)>],
    ) -> HashMap<&'a Pubkey, (&'a Account, bool)> {
        let mut accounts: HashMap<&Pubkey, (&Account, bool)> = HashMap::new();
        for (i, raccs) in loaded.iter_mut().enumerate() {
            if res[i].is_err() || raccs.is_err() {
                continue;
            }

            let message = &txs[i].message();
            let acc = raccs.as_mut().unwrap();
            for (((i, key), account), credit) in message
                .account_keys
                .iter()
                .enumerate()
                .zip(acc.0.iter())
                .zip(acc.2.iter())
            {
                if !accounts.contains_key(key) {
                    accounts.insert(key, (account, message.is_debitable(i)));
                }
                if *credit > 0 {
                    // Increment credit-only account balance Atomic
                    if accounts.get_mut(key).is_some() {
                        self.credit_only_account_locks
                            .read()
                            .unwrap()
                            .as_ref()
                            .expect("Collect accounts should only be called before a commit, and credit only account locks should exist before a commit")
                            .get(key)
                            .unwrap()
                            .credits
                            .fetch_add(*credit, Ordering::Relaxed);
                    }
                }
            }
        }
        accounts
    }
}

#[cfg(test)]
mod tests {
    // TODO: all the bank tests are bank specific, issue: 2194

    use super::*;
    use bincode::{serialize_into, serialized_size};
    use rand::{thread_rng, Rng};
    use solana_sdk::account::Account;
    use solana_sdk::fee_calculator::FeeCalculator;
    use solana_sdk::hash::Hash;
    use solana_sdk::instruction::CompiledInstruction;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::syscall;
    use solana_sdk::transaction::Transaction;
    use std::io::Cursor;
    use std::sync::atomic::AtomicBool;
    use std::{thread, time};

    fn load_accounts_with_fee(
        tx: Transaction,
        ka: &Vec<(Pubkey, Account)>,
        fee_calculator: &FeeCalculator,
        error_counters: &mut ErrorCounters,
    ) -> Vec<Result<(InstructionAccounts, InstructionLoaders, InstructionCredits)>> {
        let mut hash_queue = BlockhashQueue::new(100);
        hash_queue.register_hash(&tx.message().recent_blockhash, &fee_calculator);
        let accounts = Accounts::new(None);
        for ka in ka.iter() {
            accounts.store_slow(0, &ka.0, &ka.1);
        }

        let ancestors = vec![(0, 0)].into_iter().collect();
        let res =
            accounts.load_accounts(&ancestors, &[tx], vec![Ok(())], &hash_queue, error_counters);
        res
    }

    fn load_accounts(
        tx: Transaction,
        ka: &Vec<(Pubkey, Account)>,
        error_counters: &mut ErrorCounters,
    ) -> Vec<Result<(InstructionAccounts, InstructionLoaders, InstructionCredits)>> {
        let fee_calculator = FeeCalculator::default();
        load_accounts_with_fee(tx, ka, &fee_calculator, error_counters)
    }

    #[test]
    fn test_load_accounts_no_key() {
        let accounts: Vec<(Pubkey, Account)> = Vec::new();
        let mut error_counters = ErrorCounters::default();

        let instructions = vec![CompiledInstruction::new(0, &(), vec![0])];
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
        assert_eq!(
            loaded_accounts[0],
            Err(TransactionError::ProgramAccountNotFound)
        );
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
            loaded_accounts[0].clone(),
            Err(TransactionError::InsufficientFundsForFee)
        );
    }

    #[test]
    fn test_load_accounts_invalid_account_for_fee() {
        let mut accounts: Vec<(Pubkey, Account)> = Vec::new();
        let mut error_counters = ErrorCounters::default();

        let keypair = Keypair::new();
        let key0 = keypair.pubkey();

        let account = Account::new(1, 1, &Pubkey::new_rand()); // <-- owner is not the system program
        accounts.push((key0, account));

        let instructions = vec![CompiledInstruction::new(1, &(), vec![0])];
        let tx = Transaction::new_with_compiled_instructions(
            &[&keypair],
            &[],
            Hash::default(),
            vec![native_loader::id()],
            instructions,
        );

        let loaded_accounts = load_accounts(tx, &accounts, &mut error_counters);

        assert_eq!(error_counters.invalid_account_for_fee, 1);
        assert_eq!(loaded_accounts.len(), 1);
        assert_eq!(
            loaded_accounts[0],
            Err(TransactionError::InvalidAccountForFee)
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

        let instructions = vec![CompiledInstruction::new(2, &(), vec![0, 1])];
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
            Ok((instruction_accounts, instruction_loaders, instruction_credits)) => {
                assert_eq!(instruction_accounts.len(), 2);
                assert_eq!(instruction_accounts[0], accounts[0].1);
                assert_eq!(instruction_loaders.len(), 1);
                assert_eq!(instruction_loaders[0].len(), 0);
                assert_eq!(instruction_credits.len(), 2);
                assert_eq!(instruction_credits, &vec![0, 0]);
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

        let instructions = vec![CompiledInstruction::new(1, &(), vec![0])];
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

        let instructions = vec![CompiledInstruction::new(1, &(), vec![0])];
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
            CompiledInstruction::new(1, &(), vec![0]),
            CompiledInstruction::new(2, &(), vec![0]),
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
            Ok((instruction_accounts, instruction_loaders, instruction_credits)) => {
                assert_eq!(instruction_accounts.len(), 1);
                assert_eq!(instruction_accounts[0], accounts[0].1);
                assert_eq!(instruction_loaders.len(), 2);
                assert_eq!(instruction_loaders[0].len(), 1);
                assert_eq!(instruction_loaders[1].len(), 2);
                assert_eq!(instruction_credits.len(), 1);
                assert_eq!(instruction_credits, &vec![0]);
                for loaders in instruction_loaders.iter() {
                    for (i, accounts_subset) in loaders.iter().enumerate() {
                        // +1 to skip first not loader account
                        assert_eq![accounts_subset.1, accounts[i + 1].1];
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
        assert_eq!(
            loaded_accounts[0],
            Err(TransactionError::AccountLoadedTwice)
        );
    }

    #[test]
    fn test_load_by_program_fork() {
        let accounts = Accounts::new(None);

        // Load accounts owned by various programs into AccountsDB
        let pubkey0 = Pubkey::new_rand();
        let account0 = Account::new(1, 0, &Pubkey::new(&[2; 32]));
        accounts.store_slow(0, &pubkey0, &account0);
        let pubkey1 = Pubkey::new_rand();
        let account1 = Account::new(1, 0, &Pubkey::new(&[2; 32]));
        accounts.store_slow(0, &pubkey1, &account1);
        let pubkey2 = Pubkey::new_rand();
        let account2 = Account::new(1, 0, &Pubkey::new(&[3; 32]));
        accounts.store_slow(0, &pubkey2, &account2);

        let loaded = accounts.load_by_program_fork(0, &Pubkey::new(&[2; 32]));
        assert_eq!(loaded.len(), 2);
        let loaded = accounts.load_by_program_fork(0, &Pubkey::new(&[3; 32]));
        assert_eq!(loaded, vec![(pubkey2, account2)]);
        let loaded = accounts.load_by_program_fork(0, &Pubkey::new(&[4; 32]));
        assert_eq!(loaded, vec![]);
    }

    #[test]
    fn test_accounts_account_not_found() {
        let accounts = Accounts::new(None);
        let mut error_counters = ErrorCounters::default();
        let ancestors = vec![(0, 0)].into_iter().collect();

        let accounts_index = accounts.accounts_db.accounts_index.read().unwrap();
        let storage = accounts.accounts_db.storage.read().unwrap();
        assert_eq!(
            Accounts::load_executable_accounts(
                &storage,
                &ancestors,
                &accounts_index,
                &Pubkey::new_rand(),
                &mut error_counters
            ),
            Err(TransactionError::ProgramAccountNotFound)
        );
        assert_eq!(error_counters.account_not_found, 1);
    }

    #[test]
    fn test_accounts_empty_hash_internal_state() {
        let accounts = Accounts::new(None);
        assert_eq!(accounts.hash_internal_state(0), None);
        accounts.store_slow(0, &Pubkey::default(), &Account::new(1, 0, &syscall::id()));
        assert_eq!(accounts.hash_internal_state(0), None);
    }

    fn create_accounts(accounts: &Accounts, pubkeys: &mut Vec<Pubkey>, num: usize) {
        for t in 0..num {
            let pubkey = Pubkey::new_rand();
            let account = Account::new((t + 1) as u64, 0, &Account::default().owner);
            accounts.store_slow(0, &pubkey, &account);
            pubkeys.push(pubkey.clone());
        }
    }

    fn check_accounts(accounts: &Accounts, pubkeys: &Vec<Pubkey>, num: usize) {
        for _ in 1..num {
            let idx = thread_rng().gen_range(0, num - 1);
            let ancestors = vec![(0, 0)].into_iter().collect();
            let account = accounts.load_slow(&ancestors, &pubkeys[idx]).unwrap();
            let account1 = Account::new((idx + 1) as u64, 0, &Account::default().owner);
            assert_eq!(account, (account1, 0));
        }
    }

    #[test]
    fn test_accounts_serialize() {
        solana_logger::setup();
        let accounts = Accounts::new(Some("serialize_accounts".to_string()));

        let mut pubkeys: Vec<Pubkey> = vec![];
        create_accounts(&accounts, &mut pubkeys, 100);
        check_accounts(&accounts, &pubkeys, 100);
        accounts.add_root(0);

        let sz = serialized_size(&*accounts.accounts_db).unwrap();
        let mut buf = vec![0u8; sz as usize];
        let mut writer = Cursor::new(&mut buf[..]);
        serialize_into(&mut writer, &*accounts.accounts_db).unwrap();

        let mut reader = BufReader::new(&buf[..]);
        let daccounts = Accounts::new(Some("serialize_accounts".to_string()));
        assert!(daccounts.update_from_stream(&mut reader).is_ok());
        check_accounts(&daccounts, &pubkeys, 100);
        assert_eq!(
            accounts.hash_internal_state(0),
            daccounts.hash_internal_state(0)
        );
    }

    #[test]
    fn test_accounts_locks() {
        let keypair0 = Keypair::new();
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();
        let keypair3 = Keypair::new();

        let account0 = Account::new(1, 0, &Pubkey::default());
        let account1 = Account::new(2, 0, &Pubkey::default());
        let account2 = Account::new(3, 0, &Pubkey::default());
        let account3 = Account::new(4, 0, &Pubkey::default());

        let accounts = Accounts::new(None);
        accounts.store_slow(0, &keypair0.pubkey(), &account0);
        accounts.store_slow(0, &keypair1.pubkey(), &account1);
        accounts.store_slow(0, &keypair2.pubkey(), &account2);
        accounts.store_slow(0, &keypair3.pubkey(), &account3);

        let instructions = vec![CompiledInstruction::new(2, &(), vec![0, 1])];
        let message = Message::new_with_compiled_instructions(
            1,
            0,
            2,
            vec![keypair0.pubkey(), keypair1.pubkey(), native_loader::id()],
            Hash::default(),
            instructions,
        );
        let tx = Transaction::new(&[&keypair0], message, Hash::default());
        let results0 = accounts.lock_accounts(&[tx.clone()]);

        assert!(results0[0].is_ok());
        assert_eq!(
            *accounts
                .credit_only_account_locks
                .read()
                .unwrap()
                .as_ref()
                .unwrap()
                .get(&keypair1.pubkey())
                .unwrap()
                .lock_count
                .lock()
                .unwrap(),
            1
        );

        let instructions = vec![CompiledInstruction::new(2, &(), vec![0, 1])];
        let message = Message::new_with_compiled_instructions(
            1,
            0,
            2,
            vec![keypair2.pubkey(), keypair1.pubkey(), native_loader::id()],
            Hash::default(),
            instructions,
        );
        let tx0 = Transaction::new(&[&keypair2], message, Hash::default());
        let instructions = vec![CompiledInstruction::new(2, &(), vec![0, 1])];
        let message = Message::new_with_compiled_instructions(
            1,
            0,
            2,
            vec![keypair1.pubkey(), keypair3.pubkey(), native_loader::id()],
            Hash::default(),
            instructions,
        );
        let tx1 = Transaction::new(&[&keypair1], message, Hash::default());
        let txs = vec![tx0, tx1];
        let results1 = accounts.lock_accounts(&txs);

        assert!(results1[0].is_ok()); // Credit-only account (keypair1) can be referenced multiple times
        assert!(results1[1].is_err()); // Credit-only account (keypair1) cannot also be locked as credit-debit
        assert_eq!(
            *accounts
                .credit_only_account_locks
                .read()
                .unwrap()
                .as_ref()
                .unwrap()
                .get(&keypair1.pubkey())
                .unwrap()
                .lock_count
                .lock()
                .unwrap(),
            2
        );

        accounts.unlock_accounts(&[tx], &results0);
        accounts.unlock_accounts(&txs, &results1);

        let instructions = vec![CompiledInstruction::new(2, &(), vec![0, 1])];
        let message = Message::new_with_compiled_instructions(
            1,
            0,
            2,
            vec![keypair1.pubkey(), keypair3.pubkey(), native_loader::id()],
            Hash::default(),
            instructions,
        );
        let tx = Transaction::new(&[&keypair1], message, Hash::default());
        let results2 = accounts.lock_accounts(&[tx]);

        assert!(results2[0].is_ok()); // Now keypair1 account can be locked as credit-debit

        // Check that credit-only credits are still cached in accounts struct
        let credit_only_account_locks = accounts.credit_only_account_locks.read().unwrap();
        let credit_only_account_locks = credit_only_account_locks.as_ref().unwrap();
        let keypair1_lock = credit_only_account_locks.get(&keypair1.pubkey());
        assert!(keypair1_lock.is_some());
        assert_eq!(*keypair1_lock.unwrap().lock_count.lock().unwrap(), 0);
    }

    #[test]
    fn test_accounts_locks_multithreaded() {
        let counter = Arc::new(AtomicU64::new(0));
        let exit = Arc::new(AtomicBool::new(false));

        let keypair0 = Keypair::new();
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();

        let account0 = Account::new(1, 0, &Pubkey::default());
        let account1 = Account::new(2, 0, &Pubkey::default());
        let account2 = Account::new(3, 0, &Pubkey::default());

        let accounts = Accounts::new(None);
        accounts.store_slow(0, &keypair0.pubkey(), &account0);
        accounts.store_slow(0, &keypair1.pubkey(), &account1);
        accounts.store_slow(0, &keypair2.pubkey(), &account2);

        let accounts_arc = Arc::new(accounts);

        let instructions = vec![CompiledInstruction::new(2, &(), vec![0, 1])];
        let credit_only_message = Message::new_with_compiled_instructions(
            1,
            0,
            2,
            vec![keypair0.pubkey(), keypair1.pubkey(), native_loader::id()],
            Hash::default(),
            instructions,
        );
        let credit_only_tx = Transaction::new(&[&keypair0], credit_only_message, Hash::default());

        let instructions = vec![CompiledInstruction::new(2, &(), vec![0, 1])];
        let credit_debit_message = Message::new_with_compiled_instructions(
            1,
            0,
            2,
            vec![keypair1.pubkey(), keypair2.pubkey(), native_loader::id()],
            Hash::default(),
            instructions,
        );
        let credit_debit_tx = Transaction::new(&[&keypair1], credit_debit_message, Hash::default());

        let counter_clone = counter.clone();
        let accounts_clone = accounts_arc.clone();
        let exit_clone = exit.clone();
        thread::spawn(move || {
            let counter_clone = counter_clone.clone();
            let exit_clone = exit_clone.clone();
            loop {
                let txs = vec![credit_debit_tx.clone()];
                let results = accounts_clone.clone().lock_accounts(&txs);
                for result in results.iter() {
                    if result.is_ok() {
                        counter_clone.clone().fetch_add(1, Ordering::SeqCst);
                    }
                }
                accounts_clone.unlock_accounts(&txs, &results);
                if exit_clone.clone().load(Ordering::Relaxed) {
                    break;
                }
            }
        });
        let counter_clone = counter.clone();
        for _ in 0..5 {
            let txs = vec![credit_only_tx.clone()];
            let results = accounts_arc.clone().lock_accounts(&txs);
            if results[0].is_ok() {
                let counter_value = counter_clone.clone().load(Ordering::SeqCst);
                thread::sleep(time::Duration::from_millis(50));
                assert_eq!(counter_value, counter_clone.clone().load(Ordering::SeqCst));
            }
            accounts_arc.unlock_accounts(&txs, &results);
            thread::sleep(time::Duration::from_millis(50));
        }
        exit.store(true, Ordering::Relaxed);
    }

    #[test]
    fn test_commit_credits() {
        let pubkey0 = Pubkey::new_rand();
        let pubkey1 = Pubkey::new_rand();
        let pubkey2 = Pubkey::new_rand();

        let account0 = Account::new(1, 0, &Pubkey::default());
        let account1 = Account::new(2, 0, &Pubkey::default());

        let accounts = Accounts::new(None);
        accounts.store_slow(0, &pubkey0, &account0);
        accounts.store_slow(0, &pubkey1, &account1);

        {
            let mut credit_only_account_locks = accounts.credit_only_account_locks.write().unwrap();
            let credit_only_account_locks = credit_only_account_locks.as_mut().unwrap();
            credit_only_account_locks.insert(
                pubkey0,
                CreditOnlyLock {
                    credits: AtomicU64::new(0),
                    lock_count: Mutex::new(1),
                },
            );
            credit_only_account_locks.insert(
                pubkey1,
                CreditOnlyLock {
                    credits: AtomicU64::new(5),
                    lock_count: Mutex::new(1),
                },
            );
            credit_only_account_locks.insert(
                pubkey2,
                CreditOnlyLock {
                    credits: AtomicU64::new(10),
                    lock_count: Mutex::new(1),
                },
            );
        }

        let ancestors = vec![(0, 0)].into_iter().collect();
        accounts.commit_credits_unsafe(&ancestors, 0);

        // No change when CreditOnlyLock credits are 0
        assert_eq!(
            accounts.load_slow(&ancestors, &pubkey0).unwrap().0.lamports,
            1
        );
        // New balance should equal previous balance plus CreditOnlyLock credits
        assert_eq!(
            accounts.load_slow(&ancestors, &pubkey1).unwrap().0.lamports,
            7
        );
        // New account should be created
        assert_eq!(
            accounts.load_slow(&ancestors, &pubkey2).unwrap().0.lamports,
            10
        );
        // Account locks should be cleared
        assert_eq!(
            accounts
                .credit_only_account_locks
                .read()
                .unwrap()
                .as_ref()
                .unwrap()
                .len(),
            0
        );
    }

    #[test]
    fn test_collect_accounts() {
        let keypair0 = Keypair::new();
        let keypair1 = Keypair::new();
        let pubkey = Pubkey::new_rand();

        let instructions = vec![CompiledInstruction::new(2, &(), vec![0, 1])];
        let message = Message::new_with_compiled_instructions(
            1,
            0,
            2,
            vec![keypair0.pubkey(), pubkey, native_loader::id()],
            Hash::default(),
            instructions,
        );
        let tx0 = Transaction::new(&[&keypair0], message, Hash::default());

        let instructions = vec![CompiledInstruction::new(2, &(), vec![0, 1])];
        let message = Message::new_with_compiled_instructions(
            1,
            0,
            2,
            vec![keypair1.pubkey(), pubkey, native_loader::id()],
            Hash::default(),
            instructions,
        );
        let tx1 = Transaction::new(&[&keypair1], message, Hash::default());
        let txs = vec![tx0, tx1];

        let loaders = vec![Ok(()), Ok(())];

        let account0 = Account::new(1, 0, &Pubkey::default());
        let account1 = Account::new(2, 0, &Pubkey::default());
        let account2 = Account::new(3, 0, &Pubkey::default());

        let instruction_accounts0 = vec![account0, account2.clone()];
        let instruction_loaders0 = vec![];
        let instruction_credits0 = vec![0, 2];
        let loaded0 = Ok((
            instruction_accounts0,
            instruction_loaders0,
            instruction_credits0,
        ));

        let instruction_accounts1 = vec![account1, account2.clone()];
        let instruction_loaders1 = vec![];
        let instruction_credits1 = vec![0, 3];
        let loaded1 = Ok((
            instruction_accounts1,
            instruction_loaders1,
            instruction_credits1,
        ));

        let mut loaded = vec![loaded0, loaded1];

        let accounts = Accounts::new(None);
        {
            let mut credit_only_locks = accounts.credit_only_account_locks.write().unwrap();
            let credit_only_locks = credit_only_locks.as_mut().unwrap();
            credit_only_locks.insert(
                pubkey,
                CreditOnlyLock {
                    credits: AtomicU64::new(0),
                    lock_count: Mutex::new(1),
                },
            );
        }
        let collected_accounts = accounts.collect_accounts(&txs, &loaders, &mut loaded);
        assert_eq!(collected_accounts.len(), 3);
        assert!(collected_accounts.contains_key(&keypair0.pubkey()));
        assert!(collected_accounts.contains_key(&keypair1.pubkey()));
        assert!(collected_accounts.contains_key(&pubkey));

        let credit_debit_account0 = collected_accounts.get(&keypair0.pubkey()).unwrap();
        assert_eq!(credit_debit_account0.1, true);
        let credit_debit_account1 = collected_accounts.get(&keypair1.pubkey()).unwrap();
        assert_eq!(credit_debit_account1.1, true);

        let credit_only_account = collected_accounts.get(&pubkey).unwrap();
        assert_eq!(credit_only_account.1, false);
        // Ensure credit_only_lock reflects credits from both accounts: 2 + 3 = 5
        let credit_only_locks = accounts.credit_only_account_locks.read().unwrap();
        let credit_only_locks = credit_only_locks.as_ref().unwrap();
        assert_eq!(
            credit_only_locks
                .get(&pubkey)
                .unwrap()
                .credits
                .load(Ordering::Relaxed),
            5
        );
    }
}

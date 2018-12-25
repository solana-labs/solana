use crate::bank::BankError;
use crate::bank::Result;
use crate::counter::Counter;
use bincode::serialize;
use hashbrown::{HashMap, HashSet};
use log::Level;
use solana_sdk::account::Account;
use solana_sdk::hash::{hash, Hash};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::transaction::Transaction;
use std::collections::BTreeMap;
use std::ops::Deref;
use std::sync::atomic::AtomicUsize;
use std::sync::{Mutex, RwLock};

#[derive(Default)]
pub struct ErrorCounters {
    pub account_not_found: usize,
    pub account_in_use: usize,
    pub last_id_not_found: usize,
    pub last_id_too_old: usize,
    pub reserve_last_id: usize,
    pub insufficient_funds: usize,
    pub duplicate_signature: usize,
}

/// This structure handles the load/store of the accounts
pub struct AccountsDB {
    /// Mapping of known public keys/IDs to accounts
    pub accounts: HashMap<Pubkey, Account>,

    /// The number of transactions the bank has processed without error since the
    /// start of the ledger.
    transaction_count: u64,
}

/// This structure handles synchronization for db
pub struct Accounts {
    pub accounts_db: RwLock<AccountsDB>,

    /// set of accounts which are currently in the pipeline
    account_locks: Mutex<HashSet<Pubkey>>,
}

impl Default for AccountsDB {
    fn default() -> Self {
        Self {
            accounts: HashMap::new(),
            transaction_count: 0,
        }
    }
}

impl Default for Accounts {
    fn default() -> Self {
        Self {
            account_locks: Mutex::new(HashSet::new()),
            accounts_db: RwLock::new(AccountsDB::default()),
        }
    }
}

impl AccountsDB {
    pub fn hash_internal_state(&self) -> Hash {
        let mut ordered_accounts = BTreeMap::new();

        // only hash internal state of the part being voted upon, i.e. since last
        //  checkpoint
        for (pubkey, account) in &self.accounts {
            ordered_accounts.insert(*pubkey, account.clone());
        }

        hash(&serialize(&ordered_accounts).unwrap())
    }

    fn load<U>(checkpoints: &[U], pubkey: &Pubkey) -> Option<Account>
    where
        U: Deref<Target = Self>,
    {
        for db in checkpoints {
            if let Some(account) = db.accounts.get(pubkey) {
                return Some(account.clone());
            }
        }
        None
    }
    /// purge == checkpoints.is_empty()
    pub fn store(&mut self, purge: bool, pubkey: &Pubkey, account: &Account) {
        if account.tokens == 0 {
            if purge {
                // purge if balance is 0 and no checkpoints
                self.accounts.remove(pubkey);
            } else {
                // store default account if balance is 0 and there's a checkpoint
                self.accounts.insert(pubkey.clone(), Account::default());
            }
        } else {
            self.accounts.insert(pubkey.clone(), account.clone());
        }
    }
    pub fn store_accounts(
        &mut self,
        purge: bool,
        txs: &[Transaction],
        res: &[Result<()>],
        loaded: &[Result<Vec<Account>>],
    ) {
        for (i, racc) in loaded.iter().enumerate() {
            if res[i].is_err() || racc.is_err() {
                continue;
            }

            let tx = &txs[i];
            let acc = racc.as_ref().unwrap();
            for (key, account) in tx.account_keys.iter().zip(acc.iter()) {
                self.store(purge, key, account);
            }
        }
    }
    fn load_account<U>(
        checkpoints: &[U],
        tx: &Transaction,
        error_counters: &mut ErrorCounters,
    ) -> Result<Vec<Account>>
    where
        U: Deref<Target = Self>,
    {
        // Copy all the accounts
        if tx.signatures.is_empty() && tx.fee != 0 {
            Err(BankError::MissingSignatureForFee)
        } else {
            // There is no way to predict what program will execute without an error
            // If a fee can pay for execution then the program will be scheduled
            let mut called_accounts: Vec<Account> = vec![];
            for key in &tx.account_keys {
                called_accounts.push(Self::load(checkpoints, key).unwrap_or_default());
            }
            if called_accounts[0].tokens == 0 {
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
    /// For each program_id in the transaction, load its loaders.
    fn load_loaders<U>(
        checkpoints: &[U],
        txs: &[Transaction],
        results: &[Result<Vec<Account>>],
    ) -> HashMap<Pubkey, Account>
    where
        U: Deref<Target = Self>,
    {
        let mut keys: HashSet<Pubkey> = HashSet::new();
        txs.iter()
            .zip(results)
            .filter(|(_, rx)| rx.is_ok())
            .for_each(|(tx, _)| {
                for pk in &tx.program_ids {
                    keys.insert(*pk);
                }
            });
        keys.into_iter()
            .filter_map(|k| Self::load(checkpoints, &k).map(|a| (k, a)))
            .collect()
    }

    fn load_accounts<U>(
        checkpoints: &[U],
        txs: &[Transaction],
        results: Vec<Result<()>>,
        error_counters: &mut ErrorCounters,
    ) -> Vec<(Result<Vec<Account>>)>
    where
        U: Deref<Target = Self>,
    {
        txs.iter()
            .zip(results.into_iter())
            .map(|etx| match etx {
                (tx, Ok(())) => Self::load_account(checkpoints, tx, error_counters),
                (_, Err(e)) => Err(e),
            })
            .collect()
    }
    pub fn increment_transaction_count(&mut self, tx_count: usize) {
        self.transaction_count += tx_count as u64
    }
    pub fn transaction_count(&self) -> u64 {
        self.transaction_count
    }
    pub fn account_values_slow(&self) -> Vec<(Pubkey, solana_sdk::account::Account)> {
        self.accounts
            .iter()
            .map(|(x, y)| (x.clone(), y.clone()))
            .collect()
    }
    fn merge(&mut self, other: Self) {
        self.transaction_count += other.transaction_count;
        self.accounts.extend(other.accounts)
    }
}

impl Accounts {
    /// Slow because lock is held for 1 operation insted of many
    pub fn load_slow<U>(checkpoints: &[U], pubkey: &Pubkey) -> Option<Account>
    where
        U: Deref<Target = Self>,
    {
        let dbs: Vec<_> = checkpoints
            .iter()
            .map(|obj| obj.accounts_db.read().unwrap())
            .collect();
        AccountsDB::load(&dbs, pubkey)
    }
    /// Slow because lock is held for 1 operation insted of many
    pub fn store_slow(&self, purge: bool, pubkey: &Pubkey, account: &Account) {
        self.accounts_db
            .write()
            .unwrap()
            .store(purge, pubkey, account)
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
    pub fn hash_internal_state(&self) -> Hash {
        self.accounts_db.read().unwrap().hash_internal_state()
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

    pub fn load_accounts<U>(
        checkpoints: &[U],
        txs: &[Transaction],
        results: Vec<Result<()>>,
        error_counters: &mut ErrorCounters,
    ) -> Vec<(Result<Vec<Account>>)>
    where
        U: Deref<Target = Self>,
    {
        let dbs: Vec<_> = checkpoints
            .iter()
            .map(|obj| obj.accounts_db.read().unwrap())
            .collect();
        AccountsDB::load_accounts(&dbs, txs, results, error_counters)
    }

    pub fn store_accounts(
        &self,
        purge: bool,
        txs: &[Transaction],
        res: &[Result<()>],
        loaded: &[Result<Vec<Account>>],
    ) {
        self.accounts_db
            .write()
            .unwrap()
            .store_accounts(purge, txs, res, loaded)
    }
    pub fn load_loaders<U>(
        checkpoints: &[U],
        txs: &[Transaction],
        results: &[Result<Vec<Account>>],
    ) -> HashMap<Pubkey, Account>
    where
        U: Deref<Target = Self>,
    {
        let dbs: Vec<_> = checkpoints
            .iter()
            .map(|obj| obj.accounts_db.read().unwrap())
            .collect();
        AccountsDB::load_loaders(&dbs, txs, results)
    }
    pub fn increment_transaction_count(&self, tx_count: usize) {
        self.accounts_db
            .write()
            .unwrap()
            .increment_transaction_count(tx_count)
    }
    pub fn transaction_count(&self) -> u64 {
        self.accounts_db.read().unwrap().transaction_count()
    }
    /// accounts starts with an empty data structure for every fork
    /// self is trunk, merge the fork into self
    pub fn merge_into_trunk(&self, other: Self) {
        assert!(other.account_locks.lock().unwrap().is_empty());
        let db = other.accounts_db.into_inner().unwrap();
        let mut mydb = self.accounts_db.write().unwrap();
        mydb.merge(db)
    }
}

#[cfg(test)]
mod tests {
    // TODO: all the bank tests are bank specific, issue: 2194
}

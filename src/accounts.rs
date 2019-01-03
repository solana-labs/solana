use crate::bank::BankError;
use crate::bank::Result;
use crate::checkpoint::Checkpoint;
use crate::counter::Counter;
use crate::status_deque::{StatusDeque, StatusDequeError};
use bincode::serialize;
use hashbrown::{HashMap, HashSet};
use log::Level;
use solana_sdk::account::Account;
use solana_sdk::hash::{hash, Hash};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::transaction::Transaction;
use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::sync::atomic::AtomicUsize;
use std::sync::{Mutex, RwLock};

#[derive(Default)]
pub struct ErrorCounters {
    pub account_not_found: usize,
    pub account_in_use: usize,
    pub last_id_not_found: usize,
    pub reserve_last_id: usize,
    pub insufficient_funds: usize,
    pub duplicate_signature: usize,
}

/// This structure handles the load/store of the accounts
pub struct AccountsDB {
    /// Mapping of known public keys/IDs to accounts
    pub accounts: HashMap<Pubkey, Account>,

    /// list of prior states
    checkpoints: VecDeque<(HashMap<Pubkey, Account>, u64)>,

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
            checkpoints: VecDeque::new(),
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
    pub fn keys(&self) -> Vec<Pubkey> {
        self.accounts.keys().cloned().collect()
    }

    pub fn hash_internal_state(&self) -> Hash {
        let mut ordered_accounts = BTreeMap::new();

        // only hash internal state of the part being voted upon, i.e. since last
        //  checkpoint
        for (pubkey, account) in &self.accounts {
            ordered_accounts.insert(*pubkey, account.clone());
        }

        hash(&serialize(&ordered_accounts).unwrap())
    }

    fn load(&self, pubkey: &Pubkey) -> Option<&Account> {
        if let Some(account) = self.accounts.get(pubkey) {
            return Some(account);
        }

        for (accounts, _) in &self.checkpoints {
            if let Some(account) = accounts.get(pubkey) {
                return Some(account);
            }
        }
        None
    }

    pub fn store(&mut self, pubkey: &Pubkey, account: &Account) {
        if account.tokens == 0 {
            if self.checkpoints.is_empty() {
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
                self.store(key, account);
            }
        }
    }

    fn load_account(
        &self,
        tx: &Transaction,
        last_ids: &mut StatusDeque<Result<()>>,
        max_age: usize,
        error_counters: &mut ErrorCounters,
    ) -> Result<Vec<Account>> {
        // Copy all the accounts
        if tx.signatures.is_empty() && tx.fee != 0 {
            Err(BankError::MissingSignatureForFee)
        } else if self.load(&tx.account_keys[0]).is_none() {
            error_counters.account_not_found += 1;
            Err(BankError::AccountNotFound)
        } else if self.load(&tx.account_keys[0]).unwrap().tokens < tx.fee {
            error_counters.insufficient_funds += 1;
            Err(BankError::InsufficientFundsForFee)
        } else {
            if !last_ids.check_entry_id_age(tx.last_id, max_age) {
                error_counters.last_id_not_found += 1;
                return Err(BankError::LastIdNotFound);
            }

            // There is no way to predict what program will execute without an error
            // If a fee can pay for execution then the program will be scheduled
            last_ids
                .reserve_signature_with_last_id(&tx.last_id, &tx.signatures[0])
                .map_err(|err| match err {
                    StatusDequeError::LastIdNotFound => {
                        error_counters.reserve_last_id += 1;
                        BankError::LastIdNotFound
                    }
                    StatusDequeError::DuplicateSignature => {
                        error_counters.duplicate_signature += 1;
                        BankError::DuplicateSignature
                    }
                })?;

            let mut called_accounts: Vec<Account> = tx
                .account_keys
                .iter()
                .map(|key| self.load(key).cloned().unwrap_or_default())
                .collect();
            called_accounts[0].tokens -= tx.fee;
            Ok(called_accounts)
        }
    }

    fn load_accounts(
        &self,
        txs: &[Transaction],
        last_ids: &mut StatusDeque<Result<()>>,
        results: Vec<Result<()>>,
        max_age: usize,
        error_counters: &mut ErrorCounters,
    ) -> Vec<(Result<Vec<Account>>)> {
        txs.iter()
            .zip(results.into_iter())
            .map(|etx| match etx {
                (tx, Ok(())) => self.load_account(tx, last_ids, max_age, error_counters),
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
}

impl Accounts {
    pub fn keys(&self) -> Vec<Pubkey> {
        self.accounts_db.read().unwrap().keys()
    }

    /// Slow because lock is held for 1 operation instead of many
    pub fn load_slow(&self, pubkey: &Pubkey) -> Option<Account> {
        self.accounts_db.read().unwrap().load(pubkey).cloned()
    }

    /// Slow because lock is held for 1 operation instead of many
    pub fn store_slow(&self, pubkey: &Pubkey, account: &Account) {
        self.accounts_db.write().unwrap().store(pubkey, account)
    }
    
    fn lock_account(
        account_locks: &mut HashSet<Pubkey>,
        keys: &[Pubkey],
        error_counters: &mut ErrorCounters,
    ) -> Result<()> {
        // Bail if any of the accounts are already locked
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

    pub fn load_accounts(
        &self,
        txs: &[Transaction],
        last_ids: &mut StatusDeque<Result<()>>,
        results: Vec<Result<()>>,
        max_age: usize,
        error_counters: &mut ErrorCounters,
    ) -> Vec<(Result<Vec<Account>>)> {
        self.accounts_db.read().unwrap().load_accounts(
            txs,
            last_ids,
            results,
            max_age,
            error_counters,
        )
    }

    pub fn store_accounts(
        &self,
        txs: &[Transaction],
        res: &[Result<()>],
        loaded: &[Result<Vec<Account>>],
    ) {
        self.accounts_db
            .write()
            .unwrap()
            .store_accounts(txs, res, loaded)
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

    pub fn checkpoint(&self) {
        self.accounts_db.write().unwrap().checkpoint()
    }

    pub fn rollback(&self) {
        self.accounts_db.write().unwrap().rollback()
    }

    pub fn purge(&self, depth: usize) {
        self.accounts_db.write().unwrap().purge(depth)
    }

    pub fn depth(&self) -> usize {
        self.accounts_db.read().unwrap().depth()
    }
}

impl Checkpoint for AccountsDB {
    fn checkpoint(&mut self) {
        let mut accounts = HashMap::new();
        std::mem::swap(&mut self.accounts, &mut accounts);

        self.checkpoints
            .push_front((accounts, self.transaction_count()));
    }

    fn rollback(&mut self) {
        let (accounts, transaction_count) = self.checkpoints.pop_front().unwrap();
        self.accounts = accounts;
        self.transaction_count = transaction_count;
    }

    fn purge(&mut self, depth: usize) {
        fn merge(into: &mut HashMap<Pubkey, Account>, purge: &mut HashMap<Pubkey, Account>) {
            purge.retain(|pubkey, _| !into.contains_key(pubkey));
            into.extend(purge.drain());
            into.retain(|_, account| account.tokens != 0);
        }

        while self.depth() > depth {
            let (mut purge, _) = self.checkpoints.pop_back().unwrap();

            if let Some((into, _)) = self.checkpoints.back_mut() {
                merge(into, &mut purge);
                continue;
            }
            merge(&mut self.accounts, &mut purge);
        }
    }
    
    fn depth(&self) -> usize {
        self.checkpoints.len()
    }
}

#[cfg(test)]
mod tests {
    // TODO: all the bank tests are bank specific, issue: 2194
}

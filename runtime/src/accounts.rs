use crate::bank::{BankError, Result};
use crate::runtime::has_duplicates;
use bincode::serialize;
use hashbrown::{HashMap, HashSet};
use log::debug;
use solana_metrics::counter::Counter;
use solana_sdk::account::Account;
use solana_sdk::hash::{hash, Hash};
use solana_sdk::native_loader;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::transaction::Transaction;
use std::collections::BTreeMap;
use std::ops::Deref;
use std::sync::{Mutex, RwLock};

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
    /// Store the account update.  If the update is to delete the account because the token balance
    /// is 0, purge needs to be set to true for the delete to occur in place.
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
        loaded: &[Result<(InstructionAccounts, InstructionLoaders)>],
    ) {
        for (i, raccs) in loaded.iter().enumerate() {
            if res[i].is_err() || raccs.is_err() {
                continue;
            }

            let tx = &txs[i];
            let acc = raccs.as_ref().unwrap();
            for (key, account) in tx.account_keys.iter().zip(acc.0.iter()) {
                self.store(purge, key, account);
            }
        }
    }
    fn load_tx_accounts<U>(
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
            // Check for unique account keys
            if has_duplicates(&tx.account_keys) {
                error_counters.account_loaded_twice += 1;
                return Err(BankError::AccountLoadedTwice);
            }

            // There is no way to predict what program will execute without an error
            // If a fee can pay for execution then the program will be scheduled
            let mut called_accounts: Vec<Account> = vec![];
            for key in &tx.account_keys {
                called_accounts.push(Self::load(checkpoints, key).unwrap_or_default());
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

    fn load_executable_accounts<U>(
        checkpoints: &[U],
        mut program_id: Pubkey,
        error_counters: &mut ErrorCounters,
    ) -> Result<Vec<(Pubkey, Account)>>
    where
        U: Deref<Target = Self>,
    {
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

            let program = match Self::load(checkpoints, &program_id) {
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
    fn load_loaders<U>(
        checkpoints: &[U],
        tx: &Transaction,
        error_counters: &mut ErrorCounters,
    ) -> Result<Vec<Vec<(Pubkey, Account)>>>
    where
        U: Deref<Target = Self>,
    {
        tx.instructions
            .iter()
            .map(|ix| {
                if tx.program_ids.len() <= ix.program_ids_index as usize {
                    error_counters.account_not_found += 1;
                    return Err(BankError::AccountNotFound);
                }
                let program_id = tx.program_ids[ix.program_ids_index as usize];
                Self::load_executable_accounts(checkpoints, program_id, error_counters)
            })
            .collect()
    }

    fn load_accounts<U>(
        checkpoints: &[U],
        txs: &[Transaction],
        lock_results: Vec<Result<()>>,
        error_counters: &mut ErrorCounters,
    ) -> Vec<Result<(InstructionAccounts, InstructionLoaders)>>
    where
        U: Deref<Target = Self>,
    {
        txs.iter()
            .zip(lock_results.into_iter())
            .map(|etx| match etx {
                (tx, Ok(())) => {
                    let accounts = Self::load_tx_accounts(checkpoints, tx, error_counters)?;
                    let loaders = Self::load_loaders(checkpoints, tx, error_counters)?;
                    Ok((accounts, loaders))
                }
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

    /// become the root accountsDB
    fn squash<U>(&mut self, parents: &[U])
    where
        U: Deref<Target = Self>,
    {
        self.transaction_count += parents
            .iter()
            .fold(0, |sum, parent| sum + parent.transaction_count);

        // for every account in all the parents, load latest and update self if
        //   absent
        for pubkey in parents.iter().flat_map(|parent| parent.accounts.keys()) {
            // update self with data from parents unless in self
            if self.accounts.get(pubkey).is_none() {
                self.accounts
                    .insert(pubkey.clone(), Self::load(parents, pubkey).unwrap().clone());
            }
        }

        // toss any zero-balance accounts, since self is root now
        self.accounts.retain(|_, account| account.tokens != 0);
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
        AccountsDB::load(&dbs, pubkey).filter(|acc| acc.tokens != 0)
    }
    /// Slow because lock is held for 1 operation insted of many
    /// * purge - if the account token value is 0 and purge is true then delete the account.
    /// purge should be set to false for overlays, and true for the root checkpoint.
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
    ) -> Vec<Result<(InstructionAccounts, InstructionLoaders)>>
    where
        U: Deref<Target = Self>,
    {
        let dbs: Vec<_> = checkpoints
            .iter()
            .map(|obj| obj.accounts_db.read().unwrap())
            .collect();
        AccountsDB::load_accounts(&dbs, txs, results, error_counters)
    }

    /// Store the accounts into the DB
    /// * purge - if the account token value is 0 and purge is true then delete the account.
    /// purge should be set to false for overlays, and true for the root checkpoint.
    pub fn store_accounts(
        &self,
        purge: bool,
        txs: &[Transaction],
        res: &[Result<()>],
        loaded: &[Result<(InstructionAccounts, InstructionLoaders)>],
    ) {
        self.accounts_db
            .write()
            .unwrap()
            .store_accounts(purge, txs, res, loaded)
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

    /// accounts starts with an empty data structure for every child/fork
    ///   this function squashes all the parents into this instance
    pub fn squash<U>(&self, parents: &[U])
    where
        U: Deref<Target = Self>,
    {
        assert!(self.account_locks.lock().unwrap().is_empty());

        let dbs: Vec<_> = parents
            .iter()
            .map(|obj| obj.accounts_db.read().unwrap())
            .collect();

        self.accounts_db.write().unwrap().squash(&dbs);
    }
}

#[cfg(test)]
mod tests {
    // TODO: all the bank tests are bank specific, issue: 2194

    use super::*;
    use solana_sdk::account::Account;
    use solana_sdk::hash::Hash;
    use solana_sdk::signature::Keypair;
    use solana_sdk::signature::KeypairUtil;
    use solana_sdk::transaction::Instruction;
    use solana_sdk::transaction::Transaction;

    #[test]
    fn test_purge() {
        let mut db = AccountsDB::default();
        let key = Pubkey::default();
        let account = Account::new(0, 0, Pubkey::default());
        // accounts are deleted when their token value is 0 and purge is true
        db.store(false, &key, &account);
        assert_eq!(AccountsDB::load(&[&db], &key), Some(account.clone()));
        // purge should be set to true for the root checkpoint
        db.store(true, &key, &account);
        assert_eq!(AccountsDB::load(&[&db], &key), None);
    }

    fn load_accounts(
        tx: Transaction,
        ka: &Vec<(Pubkey, Account)>,
        error_counters: &mut ErrorCounters,
    ) -> Vec<Result<(InstructionAccounts, InstructionLoaders)>> {
        let accounts = Accounts::default();
        for ka in ka.iter() {
            accounts.store_slow(true, &ka.0, &ka.1);
        }

        Accounts::load_accounts(&[&accounts], &[tx], vec![Ok(())], error_counters)
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
    fn test_accountsdb_squash() {
        let mut db0 = AccountsDB::default();
        let key = Pubkey::default();
        let account0 = Account::new(1, 0, key);

        // store value 1 in the "root", i.e. db zero
        db0.store(true, &key, &account0);

        // store value 0 in the child, but don't purge (see purge test above)
        let mut db1 = AccountsDB::default();
        let account1 = Account::new(0, 0, key);
        db1.store(false, &key, &account1);

        // masking accounts is done at the Accounts level, at accountsDB we see
        // original account
        assert_eq!(AccountsDB::load(&[&db1, &db0], &key), Some(account1));

        // squash, which should whack key's account
        db1.squash(&[&db0]);
        assert_eq!(AccountsDB::load(&[&db1], &key), None);
    }

    #[test]
    fn test_accounts_unsquashed() {
        let key = Pubkey::default();

        // 1 token in the "root", i.e. db zero
        let mut db0 = AccountsDB::default();
        let account0 = Account::new(1, 0, key);
        db0.store(true, &key, &account0);

        // 0 tokens in the child
        let mut db1 = AccountsDB::default();
        let account1 = Account::new(0, 0, key);
        db1.store(false, &key, &account1);

        // masking accounts is done at the Accounts level, at accountsDB we see
        // original account
        assert_eq!(AccountsDB::load(&[&db1, &db0], &key), Some(account1));

        let mut accounts0 = Accounts::default();
        accounts0.accounts_db = RwLock::new(db0);
        let mut accounts1 = Accounts::default();
        accounts1.accounts_db = RwLock::new(db1);
        assert_eq!(Accounts::load_slow(&[&accounts1, &accounts0], &key), None);
    }

}

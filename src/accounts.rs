use crate::bank::BankError;
use crate::bank::Result;
use crate::checkpoint::Checkpoint;
use crate::counter::Counter;
use crate::serialize_object::{deserialize_object, serialize_object};
use crate::status_deque::{StatusDeque, StatusDequeError};
use bincode::ErrorKind;
use bincode::{deserialize, serialize};
use log::Level;
use solana_sdk::account::Account;
use solana_sdk::hash::{hash, Hash};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::transaction::Transaction;
use std::boxed::Box;
use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicUsize;
use std::sync::{Mutex, RwLock};

pub type InstructionAccounts = Vec<Account>;
pub type InstructionLoaders = Vec<Vec<(Pubkey, Account)>>;

#[derive(Debug, Default)]
pub struct ErrorCounters {
    pub account_not_found: usize,
    pub account_in_use: usize,
    pub last_id_not_found: usize,
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

    pub fn deserialize(&mut self, snapshot: &[u8]) -> core::result::Result<(), Box<ErrorKind>> {
        let mut current = 0;
        self.checkpoints = deserialize(&deserialize_object(&mut current, snapshot)?)?;
        self.accounts = deserialize(&deserialize_object(&mut current, snapshot)?)?;
        self.transaction_count = deserialize(&snapshot[current..current + 8])?;
        Ok(())
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut v = vec![];

        serialize_object(&mut v, serialize(&self.checkpoints).unwrap());
        serialize_object(&mut v, serialize(&self.accounts).unwrap());

        v.extend(serialize(&self.transaction_count).unwrap());
        v
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
        loaded: &[Result<(InstructionAccounts, InstructionLoaders)>],
    ) {
        for (i, raccs) in loaded.iter().enumerate() {
            if res[i].is_err() || raccs.is_err() {
                continue;
            }

            let tx = &txs[i];
            let accs = raccs.as_ref().unwrap();
            for (key, account) in tx.account_keys.iter().zip(accs.0.iter()) {
                self.store(key, account);
            }
        }
    }

    fn load_tx_accounts(
        &self,
        tx: &Transaction,
        last_ids: &mut StatusDeque<Result<()>>,
        max_age: usize,
        error_counters: &mut ErrorCounters,
    ) -> Result<Vec<Account>> {
        // Copy all the accounts
        if tx.signatures.is_empty() && tx.fee != 0 {
            Err(BankError::MissingSignatureForFee)
        } else if tx.account_keys.is_empty() || self.load(&tx.account_keys[0]).is_none() {
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

    fn load_executable_accounts(
        &self,
        mut program_id: Pubkey,
        error_counters: &mut ErrorCounters,
    ) -> Result<Vec<(Pubkey, Account)>> {
        let mut accounts = Vec::new();
        let mut depth = 0;
        loop {
            if solana_native_loader::check_id(&program_id) {
                // at the root of the chain, ready to dispatch
                break;
            }

            if depth >= 5 {
                error_counters.call_chain_too_deep += 1;
                return Err(BankError::CallChainTooDeep);
            }
            depth += 1;

            let program = match self.load(&program_id) {
                Some(program) => program,
                None => {
                    error_counters.account_not_found += 1;
                    return Err(BankError::AccountNotFound);
                }
            };
            if !program.executable || program.loader == Pubkey::default() {
                error_counters.account_not_found += 1;
                return Err(BankError::AccountNotFound);
            }

            // add loader to chain
            accounts.insert(0, (program_id, program.clone()));

            program_id = program.loader;
        }
        Ok(accounts)
    }

    /// For each program_id in the transaction, load its loaders.
    fn load_loaders(
        &self,
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
                self.load_executable_accounts(program_id, error_counters)
            })
            .collect()
    }

    fn load_accounts(
        &self,
        txs: &[Transaction],
        last_ids: &mut StatusDeque<Result<()>>,
        lock_results: Vec<Result<()>>,
        max_age: usize,
        error_counters: &mut ErrorCounters,
    ) -> Vec<Result<(InstructionAccounts, InstructionLoaders)>> {
        txs.iter()
            .zip(lock_results.into_iter())
            .map(|etx| match etx {
                (tx, Ok(())) => {
                    let accounts = self.load_tx_accounts(tx, last_ids, max_age, error_counters)?;
                    let loaders = self.load_loaders(tx, error_counters)?;
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
}

impl Accounts {
    pub fn deserialize(&self, snapshot: &[u8]) -> core::result::Result<(), Box<ErrorKind>> {
        self.accounts_db.write().unwrap().deserialize(snapshot)
    }

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

    pub fn serialize(&self) -> Vec<u8> {
        self.accounts_db.read().unwrap().serialize()
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
        lock_results: Vec<Result<()>>,
        max_age: usize,
        error_counters: &mut ErrorCounters,
    ) -> Vec<Result<(InstructionAccounts, InstructionLoaders)>> {
        self.accounts_db.read().unwrap().load_accounts(
            txs,
            last_ids,
            lock_results,
            max_age,
            error_counters,
        )
    }

    pub fn store_accounts(
        &self,
        txs: &[Transaction],
        res: &[Result<()>],
        loaded: &[Result<(InstructionAccounts, InstructionLoaders)>],
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

    use super::*;
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
        max_age: usize,
    ) -> Vec<Result<(InstructionAccounts, InstructionLoaders)>> {
        let accounts = Accounts::default();
        for ka in ka.iter() {
            accounts.store_slow(&ka.0, &ka.1);
        }

        let id = Default::default();
        let mut last_ids: StatusDeque<Result<()>> = StatusDeque::default();
        last_ids.register_tick(&id);

        accounts.load_accounts(&[tx], &mut last_ids, vec![Ok(())], max_age, error_counters)
    }

    fn assert_counters(error_counters: &ErrorCounters, expected: [usize; 8]) {
        assert_eq!(error_counters.account_not_found, expected[0]);
        assert_eq!(error_counters.account_in_use, expected[1]);
        assert_eq!(error_counters.last_id_not_found, expected[2]);
        assert_eq!(error_counters.reserve_last_id, expected[3]);
        assert_eq!(error_counters.insufficient_funds, expected[4]);
        assert_eq!(error_counters.duplicate_signature, expected[5]);
        assert_eq!(error_counters.call_chain_too_deep, expected[6]);
        assert_eq!(error_counters.missing_signature_for_fee, expected[7]);
    }

    #[test]
    fn test_load_accounts_index_out_of_bounds() {
        let mut accounts: Vec<(Pubkey, Account)> = Vec::new();
        let mut error_counters = ErrorCounters::default();

        let keypair = Keypair::new();
        let key0 = keypair.pubkey();

        let account = Account::new(1, 1, Pubkey::default());
        accounts.push((key0, account));

        let instructions = vec![Instruction::new(1, &(), vec![0, 1])];
        let tx = Transaction::new_with_instructions(
            &[&keypair],
            &[], // TODO this should contain a key, should fail
            Hash::default(),
            0,
            vec![solana_native_loader::id()],
            instructions,
        );

        let loaded_accounts = load_accounts(tx, &accounts, &mut error_counters, 10);

        assert_counters(&error_counters, [1, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(loaded_accounts.len(), 1);
        assert_eq!(loaded_accounts[0], Err(BankError::AccountNotFound));
    }

    #[test]
    fn test_load_accounts_no_key() {
        let accounts: Vec<(Pubkey, Account)> = Vec::new();
        let mut error_counters = ErrorCounters::default();

        let instructions = vec![Instruction::new(1, &(), vec![0])];
        let tx = Transaction::new_with_instructions(
            &[],
            &[],
            Hash::default(),
            0,
            vec![solana_native_loader::id()],
            instructions,
        );

        let loaded_accounts = load_accounts(tx, &accounts, &mut error_counters, 10);

        assert_counters(&error_counters, [1, 0, 0, 0, 0, 0, 0, 0]);
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
            vec![solana_native_loader::id()],
            instructions,
        );

        let loaded_accounts = load_accounts(tx, &accounts, &mut error_counters, 10);

        assert_counters(&error_counters, [1, 0, 0, 0, 0, 0, 0, 0]);
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

        let loaded_accounts = load_accounts(tx, &accounts, &mut error_counters, 10);

        assert_counters(&error_counters, [1, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(loaded_accounts.len(), 1);
        assert_eq!(loaded_accounts[0], Err(BankError::AccountNotFound));
    }

    #[test]
    fn test_load_accounts_max_age() {
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
            vec![solana_native_loader::id()],
            instructions,
        );

        let loaded_accounts = load_accounts(tx, &accounts, &mut error_counters, 0);

        assert_counters(&error_counters, [0, 0, 1, 0, 0, 0, 0, 0]);
        assert_eq!(loaded_accounts.len(), 1);
        assert_eq!(loaded_accounts[0], Err(BankError::LastIdNotFound));
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
            vec![solana_native_loader::id()],
            instructions,
        );

        let loaded_accounts = load_accounts(tx, &accounts, &mut error_counters, 10);

        assert_counters(&error_counters, [0, 0, 0, 0, 1, 0, 0, 0]);
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
            vec![solana_native_loader::id()],
            instructions,
        );

        let loaded_accounts = load_accounts(tx, &accounts, &mut error_counters, 10);

        assert_counters(&error_counters, [0, 0, 0, 0, 0, 0, 0, 0]);
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
        account.loader = solana_native_loader::id();
        accounts.push((key1, account));

        let mut account = Account::new(41, 1, Pubkey::default());
        account.executable = true;
        account.loader = key1;
        accounts.push((key2, account));

        let mut account = Account::new(42, 1, Pubkey::default());
        account.executable = true;
        account.loader = key2;
        accounts.push((key3, account));

        let mut account = Account::new(43, 1, Pubkey::default());
        account.executable = true;
        account.loader = key3;
        accounts.push((key4, account));

        let mut account = Account::new(44, 1, Pubkey::default());
        account.executable = true;
        account.loader = key4;
        accounts.push((key5, account));

        let mut account = Account::new(45, 1, Pubkey::default());
        account.executable = true;
        account.loader = key5;
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

        let loaded_accounts = load_accounts(tx, &accounts, &mut error_counters, 10);

        assert_counters(&error_counters, [0, 0, 0, 0, 0, 0, 1, 0]);
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
        account.loader = Pubkey::default();
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

        let loaded_accounts = load_accounts(tx, &accounts, &mut error_counters, 10);

        assert_counters(&error_counters, [1, 0, 0, 0, 0, 0, 0, 0]);
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
        account.loader = solana_native_loader::id();
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

        let loaded_accounts = load_accounts(tx, &accounts, &mut error_counters, 10);

        assert_counters(&error_counters, [1, 0, 0, 0, 0, 0, 0, 0]);
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
        account.loader = solana_native_loader::id();
        accounts.push((key1, account));

        let mut account = Account::new(41, 1, Pubkey::default());
        account.executable = true;
        account.loader = key1;
        accounts.push((key2, account));

        let mut account = Account::new(42, 1, Pubkey::default());
        account.executable = true;
        account.loader = key2;
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

        let loaded_accounts = load_accounts(tx, &accounts, &mut error_counters, 10);

        assert_counters(&error_counters, [0, 0, 0, 0, 0, 0, 0, 0]);
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
}

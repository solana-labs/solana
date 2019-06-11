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
use serde::{Deserialize, Serialize};
use solana_metrics::inc_new_counter_error;
use solana_sdk::account::{Account, LamportCredit};
use solana_sdk::hash::{Hash, Hasher};
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
use std::ops::Neg;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const ACCOUNTSDB_DIR: &str = "accountsdb";
const NUM_ACCOUNT_DIRS: usize = 4;

/// This structure handles synchronization for db
#[derive(Default, Debug, Serialize, Deserialize)]
pub struct Accounts {
    /// Single global AccountsDB
    #[serde(skip)]
    pub accounts_db: Arc<AccountsDB>,

    /// set of accounts which are currently in the pipeline
    account_locks: Mutex<HashSet<Pubkey>>,

    /// List of persistent stores
    paths: String,

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
            for key in &message.account_keys {
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
                if message.account_keys.len() <= ix.program_ids_index as usize {
                    error_counters.account_not_found += 1;
                    return Err(TransactionError::AccountNotFound);
                }
                let program_id = message.account_keys[ix.program_ids_index as usize];
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

    pub fn load_by_program(&self, fork: Fork, program_id: &Pubkey) -> Vec<(Pubkey, Account)> {
        let accumulator: Vec<Vec<(Pubkey, u64, Account)>> = self.accounts_db.scan_account_storage(
            fork,
            |stored_account: &StoredAccount,
             _id: AppendVecId,
             accum: &mut Vec<(Pubkey, u64, Account)>| {
                if stored_account.balance.owner == *program_id {
                    let val = (
                        stored_account.meta.pubkey,
                        stored_account.meta.write_version,
                        stored_account.clone_account(),
                    );
                    accum.push(val)
                }
            },
        );
        let mut versions: Vec<(Pubkey, u64, Account)> =
            accumulator.into_iter().flat_map(|x| x).collect();
        versions.sort_by_key(|s| (s.0, (s.1 as i64).neg()));
        versions.dedup_by_key(|s| s.0);
        versions.into_iter().map(|s| (s.0, s.2)).collect()
    }

    /// Slow because lock is held for 1 operation instead of many
    pub fn store_slow(&self, fork: Fork, pubkey: &Pubkey, account: &Account) {
        let mut accounts = HashMap::new();
        accounts.insert(pubkey, (account, 0));
        self.accounts_db.store(fork, &accounts);
    }

    fn lock_account(
        locks: &mut HashSet<Pubkey>,
        keys: &[&Pubkey],
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
            locks.insert(**k);
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

    fn hash_account(stored_account: &StoredAccount) -> Hash {
        let mut hasher = Hasher::default();
        hasher.hash(&serialize(&stored_account.balance).unwrap());
        hasher.hash(stored_account.data);
        hasher.result()
    }

    pub fn hash_internal_state(&self, fork_id: Fork) -> Option<Hash> {
        let accumulator: Vec<Vec<(Pubkey, u64, Hash)>> = self.accounts_db.scan_account_storage(
            fork_id,
            |stored_account: &StoredAccount,
             _id: AppendVecId,
             accum: &mut Vec<(Pubkey, u64, Hash)>| {
                if !syscall::check_id(&stored_account.balance.owner) {
                    accum.push((
                        stored_account.meta.pubkey,
                        stored_account.meta.write_version,
                        Self::hash_account(stored_account),
                    ));
                }
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
                    &message.get_account_keys_by_lock_type().0,
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
        debug!("bank unlock accounts");
        txs.iter()
            .zip(results.iter())
            .for_each(|(tx, result)| Self::unlock_account(tx, result, &mut account_locks));
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
        loaded: &[Result<(InstructionAccounts, InstructionLoaders, InstructionCredits)>],
    ) {
        let accounts = collect_accounts(txs, res, loaded);
        self.accounts_db.store(fork, &accounts);
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

fn collect_accounts<'a>(
    txs: &'a [Transaction],
    res: &'a [Result<()>],
    loaded: &'a [Result<(InstructionAccounts, InstructionLoaders, InstructionCredits)>],
) -> HashMap<&'a Pubkey, (&'a Account, LamportCredit)> {
    let mut accounts: HashMap<&Pubkey, (&Account, LamportCredit)> = HashMap::new();
    for (i, raccs) in loaded.iter().enumerate() {
        if res[i].is_err() || raccs.is_err() {
            continue;
        }

        let message = &txs[i].message();
        let acc = raccs.as_ref().unwrap();
        for ((key, account), credit) in message
            .account_keys
            .iter()
            .zip(acc.0.iter())
            .zip(acc.2.iter())
        {
            if !accounts.contains_key(key) {
                accounts.insert(key, (account, 0));
            } else if *credit > 0 {
                // Credit-only accounts may be referenced by multiple transactions
                // Collect credits to update account lamport balance before store.
                if let Some((_, c)) = accounts.get_mut(key) {
                    *c += credit;
                }
            }
        }
    }
    accounts
}

#[cfg(test)]
mod tests {
    // TODO: all the bank tests are bank specific, issue: 2194

    use super::*;
    use bincode::{deserialize_from, serialize_into, serialized_size};
    use rand::{thread_rng, Rng};
    use solana_sdk::account::Account;
    use solana_sdk::fee_calculator::FeeCalculator;
    use solana_sdk::hash::Hash;
    use solana_sdk::instruction::CompiledInstruction;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::syscall;
    use solana_sdk::transaction::Transaction;
    use std::io::Cursor;

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
            loaded_accounts[0],
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
        loaded_accounts[0].clone().unwrap_err();
        assert_eq!(
            loaded_accounts[0],
            Err(TransactionError::AccountLoadedTwice)
        );
    }

    #[test]
    fn test_load_by_program() {
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

        let loaded = accounts.load_by_program(0, &Pubkey::new(&[2; 32]));
        assert_eq!(loaded.len(), 2);
        let loaded = accounts.load_by_program(0, &Pubkey::new(&[3; 32]));
        assert_eq!(loaded, vec![(pubkey2, account2)]);
        let loaded = accounts.load_by_program(0, &Pubkey::new(&[4; 32]));
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

        let sz =
            serialized_size(&accounts).unwrap() + serialized_size(&*accounts.accounts_db).unwrap();
        let mut buf = vec![0u8; sz as usize];
        let mut writer = Cursor::new(&mut buf[..]);
        serialize_into(&mut writer, &accounts).unwrap();
        serialize_into(&mut writer, &*accounts.accounts_db).unwrap();

        let mut reader = Cursor::new(&mut buf[..]);
        let mut daccounts: Accounts = deserialize_from(&mut reader).unwrap();
        let accounts_db: AccountsDB = deserialize_from(&mut reader).unwrap();
        daccounts.accounts_db = Arc::new(accounts_db);
        check_accounts(&daccounts, &pubkeys, 100);
        assert_eq!(
            accounts.hash_internal_state(0),
            daccounts.hash_internal_state(0)
        );
    }

    #[test]
    fn test_collect_accounts() {
        let keypair0 = Keypair::new();
        let keypair1 = Keypair::new();
        let pubkey = Pubkey::new_rand();

        let instructions = vec![CompiledInstruction::new(0, &(), vec![0, 1])];
        let tx0 = Transaction::new_with_compiled_instructions(
            &[&keypair0],
            &[pubkey],
            Hash::default(),
            vec![native_loader::id()],
            instructions,
        );
        let instructions = vec![CompiledInstruction::new(0, &(), vec![0, 1])];
        let tx1 = Transaction::new_with_compiled_instructions(
            &[&keypair1],
            &[pubkey],
            Hash::default(),
            vec![native_loader::id()],
            instructions,
        );
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
        let instruction_credits1 = vec![2, 3];
        let loaded1 = Ok((
            instruction_accounts1,
            instruction_loaders1,
            instruction_credits1,
        ));

        let loaded = vec![loaded0, loaded1];

        let accounts = collect_accounts(&txs, &loaders, &loaded);
        assert_eq!(accounts.len(), 3);
        assert!(accounts.contains_key(&keypair0.pubkey()));
        assert!(accounts.contains_key(&keypair1.pubkey()));
        assert!(accounts.contains_key(&pubkey));

        // Accounts referenced once are assumed to have the correct lamport balance, even if a
        // credit amount is reported (as in a credit-only account)
        assert_eq!(accounts.get(&keypair0.pubkey()).unwrap().1, 0);
        assert_eq!(accounts.get(&keypair1.pubkey()).unwrap().1, 0);
        // Accounts referenced more than once need to pass the accumulated credits of additional
        // references on to store
        assert_eq!(accounts.get(&pubkey).unwrap().1, 3);
    }
}

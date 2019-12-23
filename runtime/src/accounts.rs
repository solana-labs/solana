use crate::accounts_db::{AccountInfo, AccountStorage, AccountsDB, AppendVecId, ErrorCounters};
use crate::accounts_index::AccountsIndex;
use crate::append_vec::StoredAccount;
use crate::bank::{HashAgeKind, TransactionProcessResult};
use crate::blockhash_queue::BlockhashQueue;
use crate::message_processor::has_duplicates;
use crate::rent_collector::RentCollector;
use log::*;
use rayon::slice::ParallelSliceMut;
use solana_metrics::inc_new_counter_error;
use solana_sdk::account::Account;
use solana_sdk::bank_hash::BankHash;
use solana_sdk::clock::Slot;
use solana_sdk::native_loader;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::system_program;
use solana_sdk::transaction::Result;
use solana_sdk::transaction::{Transaction, TransactionError};
use std::collections::{HashMap, HashSet};
use std::io::{BufReader, Error as IOError, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use crate::transaction_utils::OrderedIterator;

#[derive(Default, Debug)]
struct ReadonlyLock {
    lock_count: Mutex<u64>,
}

/// This structure handles synchronization for db
#[derive(Default, Debug)]
pub struct Accounts {
    /// my slot
    pub slot: Slot,

    /// Single global AccountsDB
    pub accounts_db: Arc<AccountsDB>,

    /// set of writable accounts which are currently in the pipeline
    account_locks: Mutex<HashSet<Pubkey>>,

    /// Set of read-only accounts which are currently in the pipeline, caching number of locks.
    readonly_locks: Arc<RwLock<Option<HashMap<Pubkey, ReadonlyLock>>>>,
}

// for the load instructions
pub type TransactionAccounts = Vec<Account>;
pub type TransactionRent = u64;
pub type TransactionLoaders = Vec<Vec<(Pubkey, Account)>>;

pub type TransactionLoadResult = (TransactionAccounts, TransactionLoaders, TransactionRent);

impl Accounts {
    pub fn new(paths: Vec<PathBuf>) -> Self {
        let accounts_db = Arc::new(AccountsDB::new(paths));

        Accounts {
            slot: 0,
            accounts_db,
            account_locks: Mutex::new(HashSet::new()),
            readonly_locks: Arc::new(RwLock::new(Some(HashMap::new()))),
        }
    }
    pub fn new_from_parent(parent: &Accounts, slot: Slot, parent_slot: Slot) -> Self {
        let accounts_db = parent.accounts_db.clone();
        accounts_db.set_hash(slot, parent_slot);
        Accounts {
            slot,
            accounts_db,
            account_locks: Mutex::new(HashSet::new()),
            readonly_locks: Arc::new(RwLock::new(Some(HashMap::new()))),
        }
    }

    pub fn accounts_from_stream<R: Read, P: AsRef<Path>>(
        &self,
        stream: &mut BufReader<R>,
        local_paths: &[PathBuf],
        append_vecs_path: P,
    ) -> std::result::Result<(), IOError> {
        self.accounts_db
            .accounts_from_stream(stream, local_paths, append_vecs_path)
    }

    fn load_tx_accounts(
        &self,
        storage: &AccountStorage,
        ancestors: &HashMap<Slot, usize>,
        accounts_index: &AccountsIndex<AccountInfo>,
        tx: &Transaction,
        fee: u64,
        error_counters: &mut ErrorCounters,
        rent_collector: &RentCollector,
    ) -> Result<(TransactionAccounts, TransactionRent)> {
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
            let mut accounts: TransactionAccounts = Vec::with_capacity(message.account_keys.len());
            let mut tx_rent: TransactionRent = 0;
            for (i, key) in message
                .account_keys
                .iter()
                .enumerate()
                .filter(|(_, key)| !message.program_ids().contains(key))
            {
                let (account, rent) = AccountsDB::load(storage, ancestors, accounts_index, key)
                    .and_then(|(mut account, _)| {
                        let rent_due: u64;
                        if message.is_writable(i) {
                            rent_due = rent_collector.update(&mut account);
                            Some((account, rent_due))
                        } else {
                            Some((account, 0))
                        }
                    })
                    .unwrap_or_default();

                accounts.push(account);
                tx_rent += rent;
            }

            if accounts.is_empty() || accounts[0].lamports == 0 {
                error_counters.account_not_found += 1;
                Err(TransactionError::AccountNotFound)
            } else if accounts[0].owner != system_program::id() {
                error_counters.invalid_account_for_fee += 1;
                Err(TransactionError::InvalidAccountForFee)
            } else if accounts[0].lamports < fee {
                error_counters.insufficient_funds += 1;
                Err(TransactionError::InsufficientFundsForFee)
            } else {
                accounts[0].lamports -= fee;
                Ok((accounts, tx_rent))
            }
        }
    }

    fn load_executable_accounts(
        storage: &AccountStorage,
        ancestors: &HashMap<Slot, usize>,
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
            let program_owner = program.owner;
            accounts.insert(0, (program_id, program));
            program_id = program_owner;
        }
        Ok(accounts)
    }

    /// For each program_id in the transaction, load its loaders.
    fn load_loaders(
        storage: &AccountStorage,
        ancestors: &HashMap<Slot, usize>,
        accounts_index: &AccountsIndex<AccountInfo>,
        tx: &Transaction,
        error_counters: &mut ErrorCounters,
    ) -> Result<TransactionLoaders> {
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
        ancestors: &HashMap<Slot, usize>,
        txs: &[Transaction],
        txs_iteration_order: Option<&[usize]>,
        lock_results: Vec<TransactionProcessResult>,
        hash_queue: &BlockhashQueue,
        error_counters: &mut ErrorCounters,
        rent_collector: &RentCollector,
    ) -> Vec<(Result<TransactionLoadResult>, Option<HashAgeKind>)> {
        //PERF: hold the lock to scan for the references, but not to clone the accounts
        //TODO: two locks usually leads to deadlocks, should this be one structure?
        let accounts_index = self.accounts_db.accounts_index.read().unwrap();
        let storage = self.accounts_db.storage.read().unwrap();
        OrderedIterator::new(txs, txs_iteration_order)
            .zip(lock_results.into_iter())
            .map(|etx| match etx {
                (tx, (Ok(()), hash_age_kind)) => {
                    let fee_hash = if let Some(HashAgeKind::DurableNonce) = hash_age_kind {
                        hash_queue.last_hash()
                    } else {
                        tx.message().recent_blockhash
                    };
                    let fee = if let Some(fee_calculator) = hash_queue.get_fee_calculator(&fee_hash)
                    {
                        fee_calculator.calculate_fee(tx.message())
                    } else {
                        return (Err(TransactionError::BlockhashNotFound), hash_age_kind);
                    };

                    let load_res = self.load_tx_accounts(
                        &storage,
                        ancestors,
                        &accounts_index,
                        tx,
                        fee,
                        error_counters,
                        rent_collector,
                    );
                    let (accounts, rents) = match load_res {
                        Ok((a, r)) => (a, r),
                        Err(e) => return (Err(e), hash_age_kind),
                    };

                    let load_res = Self::load_loaders(
                        &storage,
                        ancestors,
                        &accounts_index,
                        tx,
                        error_counters,
                    );
                    let loaders = match load_res {
                        Ok(loaders) => loaders,
                        Err(e) => return (Err(e), hash_age_kind),
                    };

                    (Ok((accounts, loaders, rents)), hash_age_kind)
                }
                (_, (Err(e), hash_age_kind)) => (Err(e), hash_age_kind),
            })
            .collect()
    }

    /// Slow because lock is held for 1 operation instead of many
    pub fn load_slow(
        &self,
        ancestors: &HashMap<Slot, usize>,
        pubkey: &Pubkey,
    ) -> Option<(Account, Slot)> {
        let (account, slot) = self
            .accounts_db
            .load_slow(ancestors, pubkey)
            .unwrap_or((Account::default(), self.slot));

        if account.lamports > 0 {
            Some((account, slot))
        } else {
            None
        }
    }

    /// scans underlying accounts_db for this delta (slot) with a map function
    ///   from StoredAccount to B
    /// returns only the latest/current version of B for this slot
    fn scan_slot<F, B>(&self, slot: Slot, func: F) -> Vec<B>
    where
        F: Fn(&StoredAccount) -> Option<B> + Send + Sync,
        B: Send + Default,
    {
        let accumulator: Vec<Vec<(Pubkey, u64, B)>> = self.accounts_db.scan_account_storage(
            slot,
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

        let mut versions: Vec<(Pubkey, u64, B)> = accumulator.into_iter().flatten().collect();
        self.accounts_db.thread_pool.install(|| {
            versions.par_sort_by_key(|s| (s.0, s.1));
        });
        versions.dedup_by_key(|s| s.0);
        versions
            .into_iter()
            .map(|(_pubkey, _version, val)| val)
            .collect()
    }

    pub fn load_by_program_slot(&self, slot: Slot, program_id: &Pubkey) -> Vec<(Pubkey, Account)> {
        self.scan_slot(slot, |stored_account| {
            if stored_account.account_meta.owner == *program_id {
                Some((stored_account.meta.pubkey, stored_account.clone_account()))
            } else {
                None
            }
        })
    }

    pub fn verify_bank_hash(&self, slot: Slot, ancestors: &HashMap<Slot, usize>) -> bool {
        self.accounts_db.verify_bank_hash(slot, ancestors).is_ok()
    }

    pub fn load_by_program(
        &self,
        ancestors: &HashMap<Slot, usize>,
        program_id: &Pubkey,
    ) -> Vec<(Pubkey, Account)> {
        self.accounts_db.scan_accounts(
            ancestors,
            |collector: &mut Vec<(Pubkey, Account)>, option| {
                if let Some(data) = option
                    .filter(|(_, account, _)| account.owner == *program_id && account.lamports != 0)
                    .map(|(pubkey, account, _slot)| (*pubkey, account))
                {
                    collector.push(data)
                }
            },
        )
    }

    /// Slow because lock is held for 1 operation instead of many
    pub fn store_slow(&self, slot: Slot, pubkey: &Pubkey, account: &Account) {
        self.accounts_db.store(slot, &[(pubkey, account)]);
    }

    fn is_locked_readonly(&self, key: &Pubkey) -> bool {
        self.readonly_locks
            .read()
            .unwrap()
            .as_ref()
            .map_or(false, |locks| {
                locks
                    .get(key)
                    .map_or(false, |lock| *lock.lock_count.lock().unwrap() > 0)
            })
    }

    fn unlock_readonly(&self, key: &Pubkey) {
        self.readonly_locks.read().unwrap().as_ref().map(|locks| {
            locks
                .get(key)
                .map(|lock| *lock.lock_count.lock().unwrap() -= 1)
        });
    }

    fn lock_readonly(&self, key: &Pubkey) -> bool {
        self.readonly_locks
            .read()
            .unwrap()
            .as_ref()
            .map_or(false, |locks| {
                locks.get(key).map_or(false, |lock| {
                    *lock.lock_count.lock().unwrap() += 1;
                    true
                })
            })
    }

    fn insert_readonly(&self, key: &Pubkey, lock: ReadonlyLock) -> bool {
        self.readonly_locks
            .write()
            .unwrap()
            .as_mut()
            .map_or(false, |locks| {
                assert!(locks.get(key).is_none());
                locks.insert(*key, lock);
                true
            })
    }

    fn lock_account(
        &self,
        locks: &mut HashSet<Pubkey>,
        writable_keys: Vec<&Pubkey>,
        readonly_keys: Vec<&Pubkey>,
        error_counters: &mut ErrorCounters,
    ) -> Result<()> {
        for k in writable_keys.iter() {
            if locks.contains(k) || self.is_locked_readonly(k) {
                error_counters.account_in_use += 1;
                debug!("CD Account in use: {:?}", k);
                return Err(TransactionError::AccountInUse);
            }
        }
        for k in readonly_keys.iter() {
            if locks.contains(k) {
                error_counters.account_in_use += 1;
                debug!("CO Account in use: {:?}", k);
                return Err(TransactionError::AccountInUse);
            }
        }

        for k in writable_keys {
            locks.insert(*k);
        }

        let readonly_writes: Vec<&&Pubkey> = readonly_keys
            .iter()
            .filter(|k| !self.lock_readonly(k))
            .collect();

        for k in readonly_writes.iter() {
            self.insert_readonly(
                *k,
                ReadonlyLock {
                    lock_count: Mutex::new(1),
                },
            );
        }

        Ok(())
    }

    fn unlock_account(&self, tx: &Transaction, result: &Result<()>, locks: &mut HashSet<Pubkey>) {
        let (writable_keys, readonly_keys) = &tx.message().get_account_keys_by_lock_type();
        match result {
            Err(TransactionError::AccountInUse) => (),
            _ => {
                for k in writable_keys {
                    locks.remove(k);
                }
                for k in readonly_keys {
                    self.unlock_readonly(k);
                }
            }
        }
    }

    pub fn bank_hash_at(&self, slot_id: Slot) -> BankHash {
        let bank_hashes = self.accounts_db.bank_hashes.read().unwrap();
        *bank_hashes
            .get(&slot_id)
            .expect("No bank hash was found for this bank, that should not be possible")
    }

    /// This function will prevent multiple threads from modifying the same account state at the
    /// same time
    #[must_use]
    pub fn lock_accounts(
        &self,
        txs: &[Transaction],
        txs_iteration_order: Option<&[usize]>,
    ) -> Vec<Result<()>> {
        let mut error_counters = ErrorCounters::default();
        let keys: Vec<_> = OrderedIterator::new(txs, txs_iteration_order)
            .map(|tx| {
                let message = &tx.message();
                message.get_account_keys_by_lock_type()
            })
            .collect();

        let rv = {
            let mut account_locks = &mut self.account_locks.lock().unwrap();
            keys.into_iter()
                .map(|(writable_keys, readonly_keys)| {
                    self.lock_account(
                        &mut account_locks,
                        writable_keys,
                        readonly_keys,
                        &mut error_counters,
                    )
                })
                .collect()
        };
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
    pub fn unlock_accounts(
        &self,
        txs: &[Transaction],
        txs_iteration_order: Option<&[usize]>,
        results: &[Result<()>],
    ) {
        let mut account_locks = self.account_locks.lock().unwrap();
        debug!("bank unlock accounts");

        OrderedIterator::new(txs, txs_iteration_order)
            .zip(results.iter())
            .for_each(|(tx, result)| self.unlock_account(tx, result, &mut account_locks));
    }

    pub fn has_accounts(&self, slot: Slot) -> bool {
        self.accounts_db.has_accounts(slot)
    }

    /// Store the accounts into the DB
    pub fn store_accounts(
        &self,
        slot: Slot,
        txs: &[Transaction],
        txs_iteration_order: Option<&[usize]>,
        res: &[TransactionProcessResult],
        loaded: &mut [(Result<TransactionLoadResult>, Option<HashAgeKind>)],
        rent_collector: &RentCollector,
    ) {
        let accounts_to_store =
            self.collect_accounts_to_store(txs, txs_iteration_order, res, loaded, rent_collector);
        self.accounts_db.store(slot, &accounts_to_store);
    }

    /// Purge a slot if it is not a root
    /// Root slots cannot be purged
    pub fn purge_slot(&self, slot: Slot) {
        self.accounts_db.purge_slot(slot);
    }
    /// Add a slot to root.  Root slots cannot be purged
    pub fn add_root(&self, slot: Slot) {
        self.accounts_db.add_root(slot)
    }

    fn collect_accounts_to_store<'a>(
        &self,
        txs: &'a [Transaction],
        txs_iteration_order: Option<&'a [usize]>,
        res: &'a [TransactionProcessResult],
        loaded: &'a mut [(Result<TransactionLoadResult>, Option<HashAgeKind>)],
        rent_collector: &RentCollector,
    ) -> Vec<(&'a Pubkey, &'a Account)> {
        let mut accounts = Vec::with_capacity(loaded.len());
        for (i, ((raccs, _hash_age_kind), tx)) in loaded
            .iter_mut()
            .zip(OrderedIterator::new(txs, txs_iteration_order))
            .enumerate()
        {
            let (res, _hash_age_kind) = &res[i];
            if res.is_err() || raccs.is_err() {
                continue;
            }

            let message = &tx.message();
            let acc = raccs.as_mut().unwrap();
            for ((i, key), account) in message
                .account_keys
                .iter()
                .enumerate()
                .zip(acc.0.iter_mut())
            {
                if message.is_writable(i) {
                    if account.rent_epoch == 0 {
                        account.rent_epoch = rent_collector.epoch;
                        acc.2 += rent_collector.update(account);
                    }
                    accounts.push((key, &*account));
                }
            }
        }
        accounts
    }
}

pub fn create_test_accounts(
    accounts: &Accounts,
    pubkeys: &mut Vec<Pubkey>,
    num: usize,
    slot: Slot,
) {
    for t in 0..num {
        let pubkey = Pubkey::new_rand();
        let account = Account::new((t + 1) as u64, 0, &Account::default().owner);
        accounts.store_slow(slot, &pubkey, &account);
        pubkeys.push(pubkey);
    }
}

#[cfg(test)]
mod tests {
    // TODO: all the bank tests are bank specific, issue: 2194

    use super::*;
    use crate::accounts_db::tests::copy_append_vecs;
    use crate::accounts_db::{get_temp_accounts_paths, AccountsDBSerialize};
    use crate::bank::HashAgeKind;
    use bincode::serialize_into;
    use rand::{thread_rng, Rng};
    use solana_sdk::account::Account;
    use solana_sdk::fee_calculator::FeeCalculator;
    use solana_sdk::hash::Hash;
    use solana_sdk::instruction::CompiledInstruction;
    use solana_sdk::message::Message;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::sysvar;
    use solana_sdk::transaction::Transaction;
    use std::io::Cursor;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::{thread, time};
    use tempfile::TempDir;

    fn load_accounts_with_fee(
        tx: Transaction,
        ka: &Vec<(Pubkey, Account)>,
        fee_calculator: &FeeCalculator,
        error_counters: &mut ErrorCounters,
    ) -> Vec<(Result<TransactionLoadResult>, Option<HashAgeKind>)> {
        let mut hash_queue = BlockhashQueue::new(100);
        hash_queue.register_hash(&tx.message().recent_blockhash, &fee_calculator);
        let accounts = Accounts::new(Vec::new());
        for ka in ka.iter() {
            accounts.store_slow(0, &ka.0, &ka.1);
        }

        let ancestors = vec![(0, 0)].into_iter().collect();
        let rent_collector = RentCollector::default();
        let res = accounts.load_accounts(
            &ancestors,
            &[tx],
            None,
            vec![(Ok(()), Some(HashAgeKind::Extant))],
            &hash_queue,
            error_counters,
            &rent_collector,
        );
        res
    }

    fn load_accounts(
        tx: Transaction,
        ka: &Vec<(Pubkey, Account)>,
        error_counters: &mut ErrorCounters,
    ) -> Vec<(Result<TransactionLoadResult>, Option<HashAgeKind>)> {
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
        assert_eq!(
            loaded_accounts[0],
            (
                Err(TransactionError::AccountNotFound),
                Some(HashAgeKind::Extant)
            )
        );
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
        assert_eq!(
            loaded_accounts[0],
            (
                Err(TransactionError::AccountNotFound),
                Some(HashAgeKind::Extant)
            ),
        );
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
            (
                Err(TransactionError::ProgramAccountNotFound),
                Some(HashAgeKind::Extant)
            )
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

        let fee_calculator = FeeCalculator::new(10, 0);
        assert_eq!(fee_calculator.calculate_fee(tx.message()), 10);

        let loaded_accounts =
            load_accounts_with_fee(tx, &accounts, &fee_calculator, &mut error_counters);

        assert_eq!(error_counters.insufficient_funds, 1);
        assert_eq!(loaded_accounts.len(), 1);
        assert_eq!(
            loaded_accounts[0].clone(),
            (
                Err(TransactionError::InsufficientFundsForFee),
                Some(HashAgeKind::Extant)
            ),
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
            (
                Err(TransactionError::InvalidAccountForFee),
                Some(HashAgeKind::Extant)
            ),
        );
    }

    #[test]
    fn test_load_accounts_no_loaders() {
        let mut accounts: Vec<(Pubkey, Account)> = Vec::new();
        let mut error_counters = ErrorCounters::default();

        let keypair = Keypair::new();
        let key0 = keypair.pubkey();
        let key1 = Pubkey::new(&[5u8; 32]);

        let mut account = Account::new(1, 1, &Pubkey::default());
        account.rent_epoch = 1;
        accounts.push((key0, account));

        let mut account = Account::new(2, 1, &Pubkey::default());
        account.rent_epoch = 1;
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
            (
                Ok((transaction_accounts, transaction_loaders, _transaction_rents)),
                _hash_age_kind,
            ) => {
                assert_eq!(transaction_accounts.len(), 2);
                assert_eq!(transaction_accounts[0], accounts[0].1);
                assert_eq!(transaction_loaders.len(), 1);
                assert_eq!(transaction_loaders[0].len(), 0);
            }
            (Err(e), _hash_age_kind) => Err(e).unwrap(),
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
        assert_eq!(
            loaded_accounts[0],
            (
                Err(TransactionError::CallChainTooDeep),
                Some(HashAgeKind::Extant)
            )
        );
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
        assert_eq!(
            loaded_accounts[0],
            (
                Err(TransactionError::AccountNotFound),
                Some(HashAgeKind::Extant)
            )
        );
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
        assert_eq!(
            loaded_accounts[0],
            (
                Err(TransactionError::AccountNotFound),
                Some(HashAgeKind::Extant)
            )
        );
    }

    #[test]
    fn test_load_accounts_multiple_loaders() {
        let mut accounts: Vec<(Pubkey, Account)> = Vec::new();
        let mut error_counters = ErrorCounters::default();

        let keypair = Keypair::new();
        let key0 = keypair.pubkey();
        let key1 = Pubkey::new(&[5u8; 32]);
        let key2 = Pubkey::new(&[6u8; 32]);

        let mut account = Account::new(1, 1, &Pubkey::default());
        account.rent_epoch = 1;
        accounts.push((key0, account));

        let mut account = Account::new(40, 1, &Pubkey::default());
        account.executable = true;
        account.rent_epoch = 1;
        account.owner = native_loader::id();
        accounts.push((key1, account));

        let mut account = Account::new(41, 1, &Pubkey::default());
        account.executable = true;
        account.rent_epoch = 1;
        account.owner = key1;
        accounts.push((key2, account));

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
            (
                Ok((transaction_accounts, transaction_loaders, _transaction_rents)),
                _hash_age_kind,
            ) => {
                assert_eq!(transaction_accounts.len(), 1);
                assert_eq!(transaction_accounts[0], accounts[0].1);
                assert_eq!(transaction_loaders.len(), 2);
                assert_eq!(transaction_loaders[0].len(), 1);
                assert_eq!(transaction_loaders[1].len(), 2);
                for loaders in transaction_loaders.iter() {
                    for (i, accounts_subset) in loaders.iter().enumerate() {
                        // +1 to skip first not loader account
                        assert_eq!(*accounts_subset, accounts[i + 1]);
                    }
                }
            }
            (Err(e), _hash_age_kind) => Err(e).unwrap(),
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
            (
                Err(TransactionError::AccountLoadedTwice),
                Some(HashAgeKind::Extant)
            )
        );
    }

    #[test]
    fn test_load_by_program_slot() {
        let accounts = Accounts::new(Vec::new());

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

        let loaded = accounts.load_by_program_slot(0, &Pubkey::new(&[2; 32]));
        assert_eq!(loaded.len(), 2);
        let loaded = accounts.load_by_program_slot(0, &Pubkey::new(&[3; 32]));
        assert_eq!(loaded, vec![(pubkey2, account2)]);
        let loaded = accounts.load_by_program_slot(0, &Pubkey::new(&[4; 32]));
        assert_eq!(loaded, vec![]);
    }

    #[test]
    fn test_accounts_account_not_found() {
        let accounts = Accounts::new(Vec::new());
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
    #[should_panic]
    fn test_accounts_empty_bank_hash() {
        let accounts = Accounts::new(Vec::new());
        accounts.bank_hash_at(0);
    }

    #[test]
    #[should_panic]
    fn test_accounts_empty_account_bank_hash() {
        let accounts = Accounts::new(Vec::new());
        accounts.store_slow(0, &Pubkey::default(), &Account::new(1, 0, &sysvar::id()));
        accounts.bank_hash_at(0);
    }

    fn check_accounts(accounts: &Accounts, pubkeys: &Vec<Pubkey>, num: usize) {
        for _ in 1..num {
            let idx = thread_rng().gen_range(0, num - 1);
            let ancestors = vec![(0, 0)].into_iter().collect();
            let account = accounts.load_slow(&ancestors, &pubkeys[idx]);
            let account1 = Some((
                Account::new((idx + 1) as u64, 0, &Account::default().owner),
                0,
            ));
            assert_eq!(account, account1);
        }
    }

    #[test]
    fn test_accounts_serialize() {
        solana_logger::setup();
        let (_accounts_dir, paths) = get_temp_accounts_paths(4).unwrap();
        let accounts = Accounts::new(paths);

        let mut pubkeys: Vec<Pubkey> = vec![];
        create_test_accounts(&accounts, &mut pubkeys, 100, 0);
        check_accounts(&accounts, &pubkeys, 100);
        accounts.add_root(0);

        let mut writer = Cursor::new(vec![]);
        serialize_into(
            &mut writer,
            &AccountsDBSerialize::new(&*accounts.accounts_db, 0),
        )
        .unwrap();

        let copied_accounts = TempDir::new().unwrap();

        // Simulate obtaining a copy of the AppendVecs from a tarball
        copy_append_vecs(&accounts.accounts_db, copied_accounts.path()).unwrap();

        let buf = writer.into_inner();
        let mut reader = BufReader::new(&buf[..]);
        let (_accounts_dir, daccounts_paths) = get_temp_accounts_paths(2).unwrap();
        let daccounts = Accounts::new(daccounts_paths.clone());
        assert!(daccounts
            .accounts_from_stream(&mut reader, &daccounts_paths, copied_accounts.path())
            .is_ok());
        check_accounts(&daccounts, &pubkeys, 100);
        assert_eq!(accounts.bank_hash_at(0), daccounts.bank_hash_at(0));
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

        let accounts = Accounts::new(Vec::new());
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
        let results0 = accounts.lock_accounts(&[tx.clone()], None);

        assert!(results0[0].is_ok());
        assert_eq!(
            *accounts
                .readonly_locks
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
        let results1 = accounts.lock_accounts(&txs, None);

        assert!(results1[0].is_ok()); // Read-only account (keypair1) can be referenced multiple times
        assert!(results1[1].is_err()); // Read-only account (keypair1) cannot also be locked as writable
        assert_eq!(
            *accounts
                .readonly_locks
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

        accounts.unlock_accounts(&[tx], None, &results0);
        accounts.unlock_accounts(&txs, None, &results1);

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
        let results2 = accounts.lock_accounts(&[tx], None);

        assert!(results2[0].is_ok()); // Now keypair1 account can be locked as writable

        // Check that read-only locks are still cached in accounts struct
        let readonly_locks = accounts.readonly_locks.read().unwrap();
        let readonly_locks = readonly_locks.as_ref().unwrap();
        let keypair1_lock = readonly_locks.get(&keypair1.pubkey());
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

        let accounts = Accounts::new(Vec::new());
        accounts.store_slow(0, &keypair0.pubkey(), &account0);
        accounts.store_slow(0, &keypair1.pubkey(), &account1);
        accounts.store_slow(0, &keypair2.pubkey(), &account2);

        let accounts_arc = Arc::new(accounts);

        let instructions = vec![CompiledInstruction::new(2, &(), vec![0, 1])];
        let readonly_message = Message::new_with_compiled_instructions(
            1,
            0,
            2,
            vec![keypair0.pubkey(), keypair1.pubkey(), native_loader::id()],
            Hash::default(),
            instructions,
        );
        let readonly_tx = Transaction::new(&[&keypair0], readonly_message, Hash::default());

        let instructions = vec![CompiledInstruction::new(2, &(), vec![0, 1])];
        let writable_message = Message::new_with_compiled_instructions(
            1,
            0,
            2,
            vec![keypair1.pubkey(), keypair2.pubkey(), native_loader::id()],
            Hash::default(),
            instructions,
        );
        let writable_tx = Transaction::new(&[&keypair1], writable_message, Hash::default());

        let counter_clone = counter.clone();
        let accounts_clone = accounts_arc.clone();
        let exit_clone = exit.clone();
        thread::spawn(move || {
            let counter_clone = counter_clone.clone();
            let exit_clone = exit_clone.clone();
            loop {
                let txs = vec![writable_tx.clone()];
                let results = accounts_clone.clone().lock_accounts(&txs, None);
                for result in results.iter() {
                    if result.is_ok() {
                        counter_clone.clone().fetch_add(1, Ordering::SeqCst);
                    }
                }
                accounts_clone.unlock_accounts(&txs, None, &results);
                if exit_clone.clone().load(Ordering::Relaxed) {
                    break;
                }
            }
        });
        let counter_clone = counter.clone();
        for _ in 0..5 {
            let txs = vec![readonly_tx.clone()];
            let results = accounts_arc.clone().lock_accounts(&txs, None);
            if results[0].is_ok() {
                let counter_value = counter_clone.clone().load(Ordering::SeqCst);
                thread::sleep(time::Duration::from_millis(50));
                assert_eq!(counter_value, counter_clone.clone().load(Ordering::SeqCst));
            }
            accounts_arc.unlock_accounts(&txs, None, &results);
            thread::sleep(time::Duration::from_millis(50));
        }
        exit.store(true, Ordering::Relaxed);
    }

    #[test]
    fn test_collect_accounts_to_store() {
        let keypair0 = Keypair::new();
        let keypair1 = Keypair::new();
        let pubkey = Pubkey::new_rand();

        let rent_collector = RentCollector::default();

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

        let loaders = vec![
            (Ok(()), Some(HashAgeKind::Extant)),
            (Ok(()), Some(HashAgeKind::Extant)),
        ];

        let account0 = Account::new(1, 0, &Pubkey::default());
        let account1 = Account::new(2, 0, &Pubkey::default());
        let account2 = Account::new(3, 0, &Pubkey::default());

        let transaction_accounts0 = vec![account0, account2.clone()];
        let transaction_loaders0 = vec![];
        let transaction_rent0 = 0;
        let loaded0 = (
            Ok((
                transaction_accounts0,
                transaction_loaders0,
                transaction_rent0,
            )),
            Some(HashAgeKind::Extant),
        );

        let transaction_accounts1 = vec![account1, account2.clone()];
        let transaction_loaders1 = vec![];
        let transaction_rent1 = 0;
        let loaded1 = (
            Ok((
                transaction_accounts1,
                transaction_loaders1,
                transaction_rent1,
            )),
            Some(HashAgeKind::Extant),
        );

        let mut loaded = vec![loaded0, loaded1];

        let accounts = Accounts::new(Vec::new());
        {
            let mut readonly_locks = accounts.readonly_locks.write().unwrap();
            let readonly_locks = readonly_locks.as_mut().unwrap();
            readonly_locks.insert(
                pubkey,
                ReadonlyLock {
                    lock_count: Mutex::new(1),
                },
            );
        }
        let collected_accounts =
            accounts.collect_accounts_to_store(&txs, None, &loaders, &mut loaded, &rent_collector);
        assert_eq!(collected_accounts.len(), 2);
        assert!(collected_accounts
            .iter()
            .find(|(pubkey, _account)| *pubkey == &keypair0.pubkey())
            .is_some());
        assert!(collected_accounts
            .iter()
            .find(|(pubkey, _account)| *pubkey == &keypair1.pubkey())
            .is_some());

        // Ensure readonly_lock reflects lock
        let readonly_locks = accounts.readonly_locks.read().unwrap();
        let readonly_locks = readonly_locks.as_ref().unwrap();
        assert_eq!(
            *readonly_locks
                .get(&pubkey)
                .unwrap()
                .lock_count
                .lock()
                .unwrap(),
            1
        );
    }
}

use crate::accounts::{Accounts, ErrorCounters};
use crate::bank::{BankError, Result};
use crate::counter::Counter;
use crate::entry::Entry;
use crate::entry_queue::{EntryQueue, MAX_ENTRY_IDS};
use crate::poh_recorder::PohRecorder;
use crate::replay_stage::BLOCK_TICK_COUNT;
use crate::runtime::{self, RuntimeError};
use crate::status_cache::StatusCache;
use crate::subscriptions::Subscriptions;
use hashbrown::HashMap;
use log::Level;
use rayon::prelude::*;
use solana_native_loader;
use solana_sdk::account::Account;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_sdk::timing::duration_as_us;
use solana_sdk::transaction::Transaction;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::sync::RwLock;
use std::time::Instant;

pub struct BankCheckpoint {
    /// accounts database
    pub accounts: Accounts,
    /// entries
    entry_q: RwLock<EntryQueue>,
    /// status cache
    status_cache: RwLock<StatusCache>,
    finalized: AtomicBool,
    fork_id: AtomicUsize,
}

impl std::fmt::Debug for BankCheckpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "BankCheckpoint {{ fork_id: {} }}", self.fork_id())
    }
}

impl BankCheckpoint {
    // last_id id is used by the status_cache to filter duplicate signatures
    pub fn new(fork_id: u64, last_id: &Hash) -> Self {
        BankCheckpoint {
            accounts: Accounts::default(),
            entry_q: RwLock::new(EntryQueue::default()),
            status_cache: RwLock::new(StatusCache::new(last_id)),
            finalized: AtomicBool::new(false),
            fork_id: AtomicUsize::new(fork_id as usize),
        }
    }
    /// Create an Bank using a deposit.
    pub fn new_from_accounts(fork: u64, accounts: &[(Pubkey, Account)], last_id: &Hash) -> Self {
        let bank_state = BankCheckpoint::new(fork, last_id);
        for (to, account) in accounts {
            bank_state.accounts.store_slow(false, &to, &account);
        }
        bank_state
    }
    pub fn store_slow(&self, purge: bool, pubkey: &Pubkey, account: &Account) {
        self.accounts.store_slow(purge, pubkey, account)
    }

    /// Forget all signatures. Useful for benchmarking.
    pub fn clear_signatures(&self) {
        self.entry_q.write().unwrap().clear();
        self.status_cache.write().unwrap().clear();
    }
    /// Return the last entry ID registered.
    pub fn last_id(&self) -> Hash {
        self.entry_q
            .read()
            .unwrap()
            .last_id
            .expect("no last_id has been set")
    }

    pub fn transaction_count(&self) -> u64 {
        self.accounts.transaction_count()
    }

    fn finalize(&self) {
        self.finalized.store(true, Ordering::Relaxed);
    }
    pub fn finalized(&self) -> bool {
        self.finalized.load(Ordering::Relaxed)
    }

    /// Look through the last_ids and find all the valid ids
    /// This is batched to avoid holding the lock for a significant amount of time
    ///
    /// Return a vec of tuple of (valid index, timestamp)
    /// index is into the passed ids slice to avoid copying hashes
    pub fn count_valid_ids(&self, ids: &[Hash]) -> Vec<(usize, u64)> {
        let entry_q = self.entry_q.read().unwrap();
        entry_q.count_valid_ids(ids)
    }

    /// Looks through a list of tick heights and stakes, and finds the latest
    /// tick that has achieved finality
    pub fn get_finality_timestamp(
        &self,
        ticks_and_stakes: &mut [(u64, u64)],
        supermajority_stake: u64,
    ) -> Option<u64> {
        let entry_q = self.entry_q.read().unwrap();
        entry_q.get_finality_timestamp(ticks_and_stakes, supermajority_stake)
    }
    pub fn get_signature_status(&self, signature: &Signature) -> Option<Result<()>> {
        self.status_cache
            .read()
            .unwrap()
            .get_signature_status(signature)
    }
    pub fn has_signature(&self, signature: &Signature) -> bool {
        self.status_cache.read().unwrap().has_signature(signature)
    }

    pub fn tick_height(&self) -> u64 {
        self.entry_q.read().unwrap().tick_height
    }

    /// Tell the bank which Entry IDs exist on the ledger. This function
    /// assumes subsequent calls correspond to later entries, and will boot
    /// the oldest ones once its internal cache is full. Once boot, the
    /// bank will reject transactions using that `last_id`.
    pub fn register_tick(&self, last_id: &Hash) {
        let mut entry_q = self.entry_q.write().unwrap();
        inc_new_counter_info!("bank-register_tick-registered", 1);
        entry_q.register_tick(last_id)
    }
    fn lock_accounts(&self, txs: &[Transaction]) -> Vec<Result<()>> {
        self.accounts.lock_accounts(txs)
    }
    fn unlock_accounts(&self, txs: &[Transaction], results: &[Result<()>]) {
        self.accounts.unlock_accounts(txs, results)
    }

    /// Check the transactions last_id age.
    fn check_last_id_age(
        &self,
        txs: &[Transaction],
        max_age: usize,
        results: Vec<Result<()>>,
        error_counters: &mut ErrorCounters,
    ) -> Vec<Result<()>> {
        let entry_q = self.entry_q.read().unwrap();
        txs.iter()
            .zip(results.into_iter())
            .map(|etx| match etx {
                (tx, Ok(())) if entry_q.check_entry_id_age(tx.last_id, max_age) => Ok(()),
                (_, Ok(())) => {
                    error_counters.last_id_too_old += 1;
                    Err(BankError::LastIdNotFound)
                }
                (_, Err(e)) => Err(e),
            })
            .collect()
    }

    /// Add signature to the current checkpoint
    fn add_signatures(&self, txs: &[Transaction], results: &[Result<Vec<Account>>]) {
        let mut status_cache = self.status_cache.write().unwrap();
        for (tx, result) in txs.iter().zip(results) {
            if result.is_ok() {
                status_cache.add(&tx.signatures[0]);
            }
        }
    }

    pub fn first_err(results: &[Result<()>]) -> Result<()> {
        for r in results {
            r.clone()?;
        }
        Ok(())
    }

    fn update_transaction_statuses(&self, txs: &[Transaction], res: &[Result<()>]) {
        let mut status_cache = self.status_cache.write().unwrap();
        for (i, tx) in txs.iter().enumerate() {
            if res[i].is_err() {
                status_cache.save_failure_status(&tx.signatures[0], res[i].clone().err().unwrap());
            }
        }
    }
    pub fn hash_internal_state(&self) -> Hash {
        self.accounts.hash_internal_state()
    }

    pub fn fork_id(&self) -> u64 {
        self.fork_id.load(Ordering::Relaxed) as u64
    }
    /// create a new fork for the bank state
    pub fn fork(&self, fork_id: u64, last_id: &Hash) -> Self {
        Self {
            accounts: Accounts::default(),
            entry_q: RwLock::new(self.entry_q.read().unwrap().fork()),
            status_cache: RwLock::new(StatusCache::new(last_id)),
            finalized: AtomicBool::new(false),
            fork_id: AtomicUsize::new(fork_id as usize),
        }
    }
    /// consume the checkpoint into the trunk state
    /// self becomes the new trunk and its fork_id is updated
    pub fn merge_into_trunk(&self, other: Self) {
        let (accounts, entry_q, status_cache, fork_id) = {
            (
                other.accounts,
                other.entry_q,
                other.status_cache,
                other.fork_id,
            )
        };
        self.accounts.merge_into_trunk(accounts);
        self.entry_q
            .write()
            .unwrap()
            .merge_into_trunk(entry_q.into_inner().unwrap());
        self.status_cache
            .write()
            .unwrap()
            .merge_into_trunk(status_cache.into_inner().unwrap());
        self.fork_id
            .store(fork_id.load(Ordering::Relaxed), Ordering::Relaxed);
    }
}

pub struct BankState {
    pub checkpoints: Vec<Arc<BankCheckpoint>>,
}

impl BankState {
    pub fn head(&self) -> &Arc<BankCheckpoint> {
        self.checkpoints
            .first()
            .expect("at least 1 checkpoint needs to be available for the state")
    }
    pub fn load_slow(&self, pubkey: &Pubkey) -> Option<Account> {
        let accounts: Vec<&Accounts> = self.checkpoints.iter().map(|c| &c.accounts).collect();
        Accounts::load_slow(&accounts, pubkey)
    }

    /// For each program_id in the transaction, load its loaders.
    fn load_loaders(
        &self,
        txs: &[Transaction],
        results: &[Result<Vec<Account>>],
    ) -> HashMap<Pubkey, Account> {
        let accounts: Vec<&Accounts> = self.checkpoints.iter().map(|c| &c.accounts).collect();
        Accounts::load_loaders(&accounts, txs, results)
    }

    fn load_accounts(
        &self,
        txs: &[Transaction],
        results: Vec<Result<()>>,
        error_counters: &mut ErrorCounters,
    ) -> Vec<(Result<Vec<Account>>)> {
        let accounts: Vec<&Accounts> = self.checkpoints.iter().map(|c| &c.accounts).collect();
        Accounts::load_accounts(&accounts, txs, results, error_counters)
    }

    /// Look through all the checkpoints to check for a duplicate signature
    fn check_duplicate_signatures(
        &self,
        txs: &[Transaction],
        mut results: Vec<Result<()>>,
        error_counters: &mut ErrorCounters,
    ) -> Vec<Result<()>> {
        for c in &self.checkpoints {
            let status_cache = c.status_cache.read().unwrap();
            for (i, tx) in txs.iter().enumerate() {
                if results[i] == Ok(()) && status_cache.has_signature(&tx.signatures[0]) {
                    results[i] = Err(BankError::DuplicateSignature);
                    error_counters.duplicate_signature += 1;
                }
            }
        }
        results
    }

    /// Process a batch of transactions.
    #[must_use]
    fn execute_and_commit_transactions(
        &self,
        maybe_subs: Option<&Subscriptions>,
        txs: &[Transaction],
        locked_accounts: Vec<Result<()>>,
        max_age: usize,
    ) -> Vec<Result<()>> {
        let head = &self.checkpoints[0];
        debug!("processing transactions: {}", txs.len());
        let purge = self.checkpoints.is_empty();
        let mut error_counters = ErrorCounters::default();
        let now = Instant::now();
        let results = head.check_last_id_age(txs, max_age, locked_accounts, &mut error_counters);
        let results = self.check_duplicate_signatures(txs, results, &mut error_counters);
        let mut loaded_accounts = self.load_accounts(txs, results, &mut error_counters);
        head.add_signatures(txs, &loaded_accounts);
        let tick_height = head.tick_height();

        let loaders = self.load_loaders(txs, &loaded_accounts);
        let load_elapsed = now.elapsed();
        let now = Instant::now();
        let executed: Vec<Result<()>> = loaded_accounts
            .iter_mut()
            .zip(txs.iter())
            .map(|(acc, tx)| match acc {
                Err(e) => Err(e.clone()),
                Ok(ref mut accounts) => {
                    let mut tx_loaders = Self::load_tx_loaders(&loaders, tx)?;
                    runtime::execute_transaction(tx, &mut tx_loaders, accounts, tick_height)
                        .map_err(|RuntimeError::ProgramError(index, err)| {
                            BankError::ProgramError(index, err)
                        })
                }
            })
            .collect();
        let execution_elapsed = now.elapsed();
        let now = Instant::now();
        head.accounts
            .store_accounts(purge, txs, &executed, &loaded_accounts);

        if let Some(subs) = maybe_subs {
            // Check account subscriptions and send notifications
            subs.send_account_notifications(txs, &executed, &loaded_accounts);
        }

        // once committed there is no way to unroll
        let write_elapsed = now.elapsed();
        debug!(
            "load: {}us execute: {}us store: {}us txs_len={}",
            duration_as_us(&load_elapsed),
            duration_as_us(&execution_elapsed),
            duration_as_us(&write_elapsed),
            txs.len(),
        );
        head.update_transaction_statuses(txs, &executed);
        if let Some(subs) = maybe_subs {
            subs.update_transaction_statuses(txs, &executed);
        }
        let mut tx_count = 0;
        let mut err_count = 0;
        for (r, tx) in executed.iter().zip(txs.iter()) {
            if r.is_ok() {
                tx_count += 1;
            } else {
                if err_count == 0 {
                    info!("tx error: {:?} {:?}", r, tx);
                }
                err_count += 1;
            }
        }
        if err_count > 0 {
            info!("{} errors of {} txs", err_count, err_count + tx_count);
            inc_new_counter_info!(
                "bank-process_transactions-account_not_found",
                error_counters.account_not_found
            );
            inc_new_counter_info!("bank-process_transactions-error_count", err_count);
        }

        head.accounts.increment_transaction_count(tx_count);

        inc_new_counter_info!("bank-process_transactions-txs", tx_count);
        if 0 != error_counters.last_id_not_found {
            inc_new_counter_info!(
                "bank-process_transactions-error-last_id_not_found",
                error_counters.last_id_not_found
            );
        }
        if 0 != error_counters.reserve_last_id {
            inc_new_counter_info!(
                "bank-process_transactions-error-reserve_last_id",
                error_counters.reserve_last_id
            );
        }
        if 0 != error_counters.duplicate_signature {
            inc_new_counter_info!(
                "bank-process_transactions-error-duplicate_signature",
                error_counters.duplicate_signature
            );
        }
        if 0 != error_counters.insufficient_funds {
            inc_new_counter_info!(
                "bank-process_transactions-error-insufficient_funds",
                error_counters.insufficient_funds
            );
        }
        executed
    }

    fn load_executable_accounts(
        loaders: &HashMap<Pubkey, Account>,
        mut program_id: Pubkey,
    ) -> Result<Vec<(Pubkey, Account)>> {
        let mut accounts = Vec::new();
        let mut depth = 0;
        loop {
            if solana_native_loader::check_id(&program_id) {
                // at the root of the chain, ready to dispatch
                break;
            }

            if depth >= 5 {
                return Err(BankError::CallChainTooDeep);
            }
            depth += 1;

            let program = match loaders.get(&program_id) {
                Some(program) => program,
                None => return Err(BankError::AccountNotFound),
            };
            if !program.executable || program.loader == Pubkey::default() {
                return Err(BankError::AccountNotFound);
            }

            // add loader to chain
            accounts.insert(0, (program_id, program.clone()));

            program_id = program.loader;
        }
        Ok(accounts)
    }

    /// For each program_id in the transaction, load its loaders.
    fn load_tx_loaders(
        loaders: &HashMap<Pubkey, Account>,
        tx: &Transaction,
    ) -> Result<Vec<Vec<(Pubkey, Account)>>> {
        tx.instructions
            .iter()
            .map(|ix| {
                let program_id = tx.program_ids[ix.program_ids_index as usize];
                Self::load_executable_accounts(loaders, program_id)
            })
            .collect()
    }

    pub fn process_and_record_transactions(
        &self,
        subs: Option<&Subscriptions>,
        txs: &[Transaction],
        recorder: Option<&PohRecorder>,
    ) -> Result<(Vec<Result<()>>)> {
        let head = &self.checkpoints[0];
        let now = Instant::now();
        // Once accounts are locked, other threads cannot encode transactions that will modify the
        // same account state
        let locked_accounts = head.lock_accounts(txs);
        let lock_time = now.elapsed();
        let now = Instant::now();
        // Use a shorter maximum age when adding transactions into the pipeline.  This will reduce
        // the likelyhood of any single thread getting starved and processing old ids.
        // TODO: Banking stage threads should be prioritized to complete faster then this queue
        // expires.
        let results = self.execute_and_commit_transactions(
            subs,
            txs,
            locked_accounts,
            MAX_ENTRY_IDS as usize / 2,
        );
        let process_time = now.elapsed();
        let now = Instant::now();
        if let Some(poh) = recorder {
            Self::record_transactions(txs, &results, poh)?;
        }
        let record_time = now.elapsed();
        let now = Instant::now();
        // Once the accounts are unlocked new transactions can enter the pipeline to process them
        head.unlock_accounts(&txs, &results);
        let unlock_time = now.elapsed();
        debug!(
            "lock: {}us process: {}us record: {}us unlock: {}us txs_len={}",
            duration_as_us(&lock_time),
            duration_as_us(&process_time),
            duration_as_us(&record_time),
            duration_as_us(&unlock_time),
            txs.len(),
        );
        Ok(results)
    }

    pub fn par_execute_entries(&self, entries: &[(&Entry, Vec<Result<()>>)]) -> Result<()> {
        let head = &self.checkpoints[0];
        inc_new_counter_info!("bank-par_execute_entries-count", entries.len());
        let results: Vec<Result<()>> = entries
            .into_par_iter()
            .map(|(e, locks)| {
                // Fork sanity check
                //TODO: this sanity check needs to be fixed once forks contain the PoH ticks that
                //connect them to the previous fork.  We need a way to identify the fork from the
                //entry itself, or have that information passed through.
                assert_eq!(e.tick_height / BLOCK_TICK_COUNT, head.fork_id());
                let results = self.execute_and_commit_transactions(
                    None,
                    &e.transactions,
                    locks.to_vec(),
                    MAX_ENTRY_IDS,
                );
                head.unlock_accounts(&e.transactions, &results);
                BankCheckpoint::first_err(&results)
            })
            .collect();
        BankCheckpoint::first_err(&results)
    }

    /// process entries in parallel
    /// The entries must be for the same checkpoint
    /// 1. In order lock accounts for each entry while the lock succeeds, up to a Tick entry
    /// 2. Process the locked group in parallel
    /// 3. Register the `Tick` if it's available, goto 1
    pub fn par_process_entries(&self, entries: &[Entry]) -> Result<()> {
        let head = &self.checkpoints[0];
        inc_new_counter_info!("bank-par_process_entries-count", entries.len());
        // accumulator for entries that can be processed in parallel
        let mut mt_group = vec![];
        for entry in entries {
            if entry.is_tick() {
                // if its a tick, execute the group and register the tick
                self.par_execute_entries(&mt_group)?;
                head.register_tick(&entry.id);
                mt_group = vec![];
                continue;
            }
            // try to lock the accounts
            let locked = head.lock_accounts(&entry.transactions);
            // if any of the locks error out
            // execute the current group
            if BankCheckpoint::first_err(&locked).is_err() {
                self.par_execute_entries(&mt_group)?;
                mt_group = vec![];
                //reset the lock and push the entry
                head.unlock_accounts(&entry.transactions, &locked);
                let locked = head.lock_accounts(&entry.transactions);
                mt_group.push((entry, locked));
            } else {
                // push the entry to the mt_group
                mt_group.push((entry, locked));
            }
        }
        self.par_execute_entries(&mt_group)?;

        if !entries.is_empty() {
            let finish = entries.last().unwrap().tick_height;
            // Fork sanity check
            //TODO: same as the other fork sanity check
            assert_eq!(finish / BLOCK_TICK_COUNT, head.fork_id());
            if (finish + 1) / BLOCK_TICK_COUNT != head.fork_id() {
                head.finalize();
            }
        }
        Ok(())
    }

    fn record_transactions(
        txs: &[Transaction],
        results: &[Result<()>],
        poh: &PohRecorder,
    ) -> Result<()> {
        let processed_transactions: Vec<_> = results
            .iter()
            .zip(txs.iter())
            .filter_map(|(r, x)| match r {
                Ok(_) => Some(x.clone()),
                Err(ref e) => {
                    debug!("process transaction failed {:?}", e);
                    None
                }
            })
            .collect();
        // unlock all the accounts with errors which are filtered by the above `filter_map`
        if !processed_transactions.is_empty() {
            let hash = Transaction::hash(&processed_transactions);
            debug!("processed ok: {} {}", processed_transactions.len(), hash);
            // record and unlock will unlock all the successfull transactions
            poh.record(hash, processed_transactions).map_err(|e| {
                warn!("record failure: {:?}", e);
                BankError::RecordFailure
            })?;
        }
        Ok(())
    }
    pub fn get_signature_status(&self, signature: &Signature) -> Option<Result<()>> {
        for c in &self.checkpoints {
            if let Some(status) = c.get_signature_status(signature) {
                return Some(status);
            }
        }
        None
    }
    pub fn has_signature(&self, signature: &Signature) -> bool {
        for c in &self.checkpoints {
            if c.has_signature(signature) {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use solana_sdk::signature::Keypair;
    use solana_sdk::signature::KeypairUtil;
    use solana_sdk::system_transaction::SystemTransaction;

    /// Create, sign, and process a Transaction from `keypair` to `to` of
    /// `n` tokens where `last_id` is the last Entry ID observed by the client.
    pub fn transfer(
        bank: &BankState,
        n: u64,
        keypair: &Keypair,
        to: Pubkey,
        last_id: Hash,
    ) -> Result<Signature> {
        let tx = Transaction::system_new(keypair, to, n, last_id);
        let signature = tx.signatures[0];
        let e = bank.process_and_record_transactions(None, &[tx], None)?;
        match &e[0] {
            Ok(_) => Ok(signature),
            Err(e) => Err(e.clone()),
        }
    }

    fn new_state(mint: &Keypair, tokens: u64, last_id: &Hash) -> BankState {
        let accounts = [(mint.pubkey(), Account::new(tokens, 0, Pubkey::default()))];
        let bank = Arc::new(BankCheckpoint::new_from_accounts(0, &accounts, &last_id));
        BankState {
            checkpoints: vec![bank],
        }
    }

    #[test]
    fn test_interleaving_locks() {
        let last_id = Hash::default();
        let mint = Keypair::new();
        let alice = Keypair::new();
        let bob = Keypair::new();
        let bank = new_state(&mint, 3, &last_id);

        let tx1 = Transaction::system_new(&mint, alice.pubkey(), 1, last_id);
        let pay_alice = vec![tx1];

        let locked_alice = bank.head().lock_accounts(&pay_alice);
        let results_alice =
            bank.execute_and_commit_transactions(None, &pay_alice, locked_alice, MAX_ENTRY_IDS);
        assert_eq!(results_alice[0], Ok(()));

        // try executing an interleaved transfer twice
        assert_eq!(
            transfer(&bank, 1, &mint, bob.pubkey(), last_id),
            Err(BankError::AccountInUse)
        );
        // the second time shoudl fail as well
        // this verifies that `unlock_accounts` doesn't unlock `AccountInUse` accounts
        assert_eq!(
            transfer(&bank, 1, &mint, bob.pubkey(), last_id),
            Err(BankError::AccountInUse)
        );

        bank.head().unlock_accounts(&pay_alice, &results_alice);

        assert_matches!(transfer(&bank, 2, &mint, bob.pubkey(), last_id), Ok(_));
    }
}

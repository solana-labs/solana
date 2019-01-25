use crate::accounts::{Accounts, ErrorCounters, InstructionAccounts, InstructionLoaders};
use crate::bank::{BankError, Result};
use crate::counter::Counter;
use crate::entry::Entry;
use crate::last_id_queue::{LastIdQueue, MAX_ENTRY_IDS};
use crate::leader_scheduler::TICKS_PER_BLOCK;
use crate::poh_recorder::PohRecorder;
use crate::runtime::{self, RuntimeError};
use crate::status_cache::StatusCache;
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
    entry_q: RwLock<LastIdQueue>,
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
            entry_q: RwLock::new(LastIdQueue::default()),
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
    pub fn finalize(&self) {
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
    pub fn get_confirmation_timestamp(
        &self,
        ticks_and_stakes: &mut [(u64, u64)],
        supermajority_stake: u64,
    ) -> Option<u64> {
        let entry_q = self.entry_q.read().unwrap();
        entry_q.get_confirmation_timestamp(ticks_and_stakes, supermajority_stake)
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
    fn add_signatures(&self, txs: &[Transaction], results: &[Result<()>]) {
        let mut status_cache = self.status_cache.write().unwrap();
        for (i, tx) in txs.iter().enumerate() {
            match results[i] {
                Err(BankError::LastIdNotFound) => (),
                Err(BankError::DuplicateSignature) => (),
                Err(BankError::AccountNotFound) => (),
                _ => status_cache.add(&tx.signatures[0]),
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
            match res[i] {
                Ok(_) => (),
                Err(BankError::LastIdNotFound) => (),
                Err(BankError::DuplicateSignature) => (),
                Err(BankError::AccountNotFound) => (),
                _ => status_cache
                    .save_failure_status(&tx.signatures[0], res[i].clone().err().unwrap()),
            }
            if res[i].is_err() {}
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

    fn load_accounts(
        &self,
        txs: &[Transaction],
        results: Vec<Result<()>>,
        error_counters: &mut ErrorCounters,
    ) -> Vec<Result<(InstructionAccounts, InstructionLoaders)>> {
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
    #[allow(clippy::type_complexity)]
    fn load_and_execute_transactions(
        &self,
        txs: &[Transaction],
        lock_results: Vec<Result<()>>,
        max_age: usize,
    ) -> (
        Vec<Result<(InstructionAccounts, InstructionLoaders)>>,
        Vec<Result<()>>,
    ) {
        let head = &self.checkpoints[0];
        debug!("processing transactions: {}", txs.len());
        let mut error_counters = ErrorCounters::default();
        let now = Instant::now();
        let results = head.check_last_id_age(txs, max_age, lock_results, &mut error_counters);
        let results = self.check_duplicate_signatures(txs, results, &mut error_counters);
        head.add_signatures(txs, &results);
        let mut loaded_accounts = self.load_accounts(txs, results, &mut error_counters);
        let tick_height = head.tick_height();

        let load_elapsed = now.elapsed();
        let now = Instant::now();
        let executed: Vec<Result<()>> = loaded_accounts
            .iter_mut()
            .zip(txs.iter())
            .map(|(accs, tx)| match accs {
                Err(e) => Err(e.clone()),
                Ok((ref mut accounts, ref mut loaders)) => {
                    runtime::execute_transaction(tx, loaders, accounts, tick_height).map_err(
                        |RuntimeError::ProgramError(index, err)| {
                            BankError::ProgramError(index, err)
                        },
                    )
                }
            })
            .collect();

        let execution_elapsed = now.elapsed();

        debug!(
            "load: {}us execute: {}us txs_len={}",
            duration_as_us(&load_elapsed),
            duration_as_us(&execution_elapsed),
            txs.len(),
        );
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
        (loaded_accounts, executed)
    }

    fn commit_transactions(
        &self,
        txs: &[Transaction],
        loaded_accounts: &[Result<(InstructionAccounts, InstructionLoaders)>],
        executed: &[Result<()>],
    ) {
        let head = &self.checkpoints[0];
        let now = Instant::now();
        let purge = self.checkpoints.len() == 1;
        head.accounts
            .store_accounts(purge, txs, executed, loaded_accounts);

        // TODO: Thread subscriptions here
        // // Check account subscriptions and send notifications
        // if let Some(subs) = maybe_subs {
        //     subs.send_account_notifications(txs, executed, loaded_accounts);
        // }

        // once committed there is no way to unroll
        let write_elapsed = now.elapsed();
        debug!(
            "store: {}us txs_len={}",
            duration_as_us(&write_elapsed),
            txs.len(),
        );
        head.update_transaction_statuses(txs, &executed);
    }

    /// Process a batch of transactions.
    #[must_use]
    pub fn load_execute_and_commit_transactions(
        &self,
        txs: &[Transaction],
        lock_results: Vec<Result<()>>,
        max_age: usize,
    ) -> Vec<Result<()>> {
        let (loaded_accounts, executed) =
            self.load_and_execute_transactions(txs, lock_results, max_age);

        self.commit_transactions(txs, &loaded_accounts, &executed);
        executed
    }

    pub fn process_and_record_transactions(
        &self,
        txs: &[Transaction],
        recorder: Option<&PohRecorder>,
    ) -> Result<(Vec<Result<()>>)> {
        let now = Instant::now();
        // Once accounts are locked, other threads cannot encode transactions that will modify the
        // same account state
        let head = &self.checkpoints[0];
        let lock_results = head.lock_accounts(txs);
        let lock_time = now.elapsed();

        let now = Instant::now();
        // Use a shorter maximum age when adding transactions into the pipeline.  This will reduce
        // the likelihood of any single thread getting starved and processing old ids.
        // TODO: Banking stage threads should be prioritized to complete faster then this queue
        // expires.
        let (loaded_accounts, results) =
            self.load_and_execute_transactions(txs, lock_results, MAX_ENTRY_IDS as usize / 2);
        let load_execute_time = now.elapsed();

        let record_time = {
            let now = Instant::now();
            if let Some(poh) = recorder {
                Self::record_transactions(txs, &results, poh)?;
            }
            now.elapsed()
        };

        let commit_time = {
            let now = Instant::now();
            self.commit_transactions(txs, &loaded_accounts, &results);
            now.elapsed()
        };

        let now = Instant::now();
        // Once the accounts are new transactions can enter the pipeline to process them
        head.unlock_accounts(&txs, &results);
        let unlock_time = now.elapsed();
        debug!(
            "lock: {}us load_execute: {}us record: {}us commit: {}us unlock: {}us txs_len: {}",
            duration_as_us(&lock_time),
            duration_as_us(&load_execute_time),
            duration_as_us(&record_time),
            duration_as_us(&commit_time),
            duration_as_us(&unlock_time),
            txs.len(),
        );
        Ok(results)
    }
    fn ignore_program_errors(results: Vec<Result<()>>) -> Vec<Result<()>> {
        results
            .into_iter()
            .map(|result| match result {
                // Entries that result in a ProgramError are still valid and are written in the
                // ledger so map them to an ok return value
                Err(BankError::ProgramError(index, err)) => {
                    info!("program error {:?}, {:?}", index, err);
                    inc_new_counter_info!("bank-ignore_program_err", 1);
                    Ok(())
                }
                _ => result,
            })
            .collect()
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
                assert_eq!(e.tick_height / TICKS_PER_BLOCK, head.fork_id());
                let results = self.load_execute_and_commit_transactions(
                    &e.transactions,
                    locks.to_vec(),
                    MAX_ENTRY_IDS,
                );
                let results = BankState::ignore_program_errors(results);
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
            assert_eq!(finish / TICKS_PER_BLOCK, head.fork_id());
            if (finish + 1) / TICKS_PER_BLOCK != head.fork_id() {
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
    pub fn get_signature_status(&self, sig: &Signature) -> Option<Result<()>> {
        let checkpoints: Vec<_> = self
            .checkpoints
            .iter()
            .map(|c| c.status_cache.read().unwrap())
            .collect();
        StatusCache::get_signature_status_all(&checkpoints, sig)
    }
    pub fn has_signature(&self, sig: &Signature) -> bool {
        let checkpoints: Vec<_> = self
            .checkpoints
            .iter()
            .map(|c| c.status_cache.read().unwrap())
            .collect();
        StatusCache::has_signature_all(&checkpoints, sig)
    }
    pub fn checkpoint_depth(&self) -> usize {
        self.checkpoints.len()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use solana_sdk::native_program::ProgramError;
    use solana_sdk::signature::Keypair;
    use solana_sdk::signature::KeypairUtil;
    use solana_sdk::system_program;
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
        let e = bank.process_and_record_transactions(&[tx], None)?;
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

    fn add_system_program(checkpoint: &BankCheckpoint) {
        let system_program_account = Account {
            tokens: 1,
            owner: system_program::id(),
            userdata: b"solana_system_program".to_vec(),
            executable: true,
            loader: solana_native_loader::id(),
        };
        checkpoint.store_slow(false, &system_program::id(), &system_program_account);
    }

    #[test]
    fn test_interleaving_locks() {
        let last_id = Hash::default();
        let mint = Keypair::new();
        let alice = Keypair::new();
        let bob = Keypair::new();
        let bank = new_state(&mint, 3, &last_id);
        bank.head().register_tick(&last_id);
        add_system_program(bank.head());

        let tx1 = Transaction::system_new(&mint, alice.pubkey(), 1, last_id);
        let pay_alice = vec![tx1];

        let locked_alice = bank.head().lock_accounts(&pay_alice);
        assert!(locked_alice[0].is_ok());
        let results_alice =
            bank.load_execute_and_commit_transactions(&pay_alice, locked_alice, MAX_ENTRY_IDS);
        assert_eq!(results_alice[0], Ok(()));

        // try executing an interleaved transfer twice
        assert_eq!(
            transfer(&bank, 1, &mint, bob.pubkey(), last_id),
            Err(BankError::AccountInUse)
        );
        // the second time should fail as well
        // this verifies that `unlock_accounts` doesn't unlock `AccountInUse` accounts
        assert_eq!(
            transfer(&bank, 1, &mint, bob.pubkey(), last_id),
            Err(BankError::AccountInUse)
        );

        bank.head().unlock_accounts(&pay_alice, &results_alice);

        assert_matches!(transfer(&bank, 2, &mint, bob.pubkey(), last_id), Ok(_));
    }
    #[test]
    fn test_bank_ignore_program_errors() {
        let expected_results = vec![Ok(()), Ok(())];
        let results = vec![Ok(()), Ok(())];
        let updated_results = BankState::ignore_program_errors(results);
        assert_eq!(updated_results, expected_results);

        let results = vec![
            Err(BankError::ProgramError(
                1,
                ProgramError::ResultWithNegativeTokens,
            )),
            Ok(()),
        ];
        let updated_results = BankState::ignore_program_errors(results);
        assert_eq!(updated_results, expected_results);

        // Other BankErrors should not be ignored
        let results = vec![Err(BankError::AccountNotFound), Ok(())];
        let updated_results = BankState::ignore_program_errors(results);
        assert_ne!(updated_results, expected_results);
    }

    //#[test]
    //fn test_bank_record_transactions() {
    //    let mint = Mint::new(10_000);
    //    let bank = Arc::new(Bank::new(&mint));
    //    let (entry_sender, entry_receiver) = channel();
    //    let poh_recorder = PohRecorder::new(bank.clone(), entry_sender, bank.last_id(), None);
    //    let pubkey = Keypair::new().pubkey();

    //    let transactions = vec![
    //        Transaction::system_move(&mint.keypair(), pubkey, 1, mint.last_id(), 0),
    //        Transaction::system_move(&mint.keypair(), pubkey, 1, mint.last_id(), 0),
    //    ];

    //    let mut results = vec![Ok(()), Ok(())];
    //    BankStater::record_transactions(&transactions, &results, &poh_recorder)
    //        .unwrap();
    //    let entries = entry_receiver.recv().unwrap();
    //    assert_eq!(entries[0].transactions.len(), transactions.len());

    //    // ProgramErrors should still be recorded
    //    results[0] = Err(BankError::ProgramError(
    //        1,
    //        ProgramError::ResultWithNegativeTokens,
    //    ));
    //    BankState::record_transactions(&transactions, &results, &poh_recorder)
    //        .unwrap();
    //    let entries = entry_receiver.recv().unwrap();
    //    assert_eq!(entries[0].transactions.len(), transactions.len());

    //    // Other BankErrors should not be recorded
    //    results[0] = Err(BankError::AccountNotFound);
    //    BankState::record_transactions(&transactions, &results, &poh_recorder)
    //        .unwrap();
    //    let entries = entry_receiver.recv().unwrap();
    //    assert_eq!(entries[0].transactions.len(), transactions.len() - 1);
    //}
    //
    // #[test]
    // fn test_bank_process_and_record_transactions() {
    //     let mint = Mint::new(10_000);
    //     let bank = Arc::new(Bank::new(&mint));
    //     let pubkey = Keypair::new().pubkey();

    //     let transactions = vec![Transaction::system_move(
    //         &mint.keypair(),
    //         pubkey,
    //         1,
    //         mint.last_id(),
    //         0,
    //     )];

    //     let (entry_sender, entry_receiver) = channel();
    //     let mut poh_recorder = PohRecorder::new(
    //         bank.clone(),
    //         entry_sender,
    //         bank.last_id(),
    //         Some(bank.tick_height() + 1),
    //     );

    //     bank.process_and_record_transactions(&transactions, &poh_recorder)
    //         .unwrap();
    //     poh_recorder.tick().unwrap();

    //     let mut need_tick = true;
    //     // read entries until I find mine, might be ticks...
    //     while need_tick {
    //         let entries = entry_receiver.recv().unwrap();
    //         for entry in entries {
    //             if !entry.is_tick() {
    //                 assert_eq!(entry.transactions.len(), transactions.len());
    //                 assert_eq!(bank.get_balance(&pubkey), 1);
    //             } else {
    //                 need_tick = false;
    //             }
    //         }
    //     }

    //     let transactions = vec![Transaction::system_move(
    //         &mint.keypair(),
    //         pubkey,
    //         2,
    //         mint.last_id(),
    //         0,
    //     )];

    //     assert_eq!(
    //         bank.process_and_record_transactions(&transactions, &poh_recorder),
    //         Err(BankError::RecordFailure)
    //     );

    //     assert_eq!(bank.get_balance(&pubkey), 1);
    // }

}

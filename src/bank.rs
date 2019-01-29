//! The `bank` module tracks client accounts and the progress of on-chain
//! programs. It offers a high-level API that signs transactions
//! on behalf of the caller, and a low-level API for when they have
//! already been signed and verified.

use crate::accounts::{Accounts, ErrorCounters, InstructionAccounts, InstructionLoaders};
use crate::counter::Counter;
use crate::entry::Entry;
use crate::entry::EntrySlice;
use crate::genesis_block::GenesisBlock;
use crate::leader_scheduler::LeaderScheduler;
use crate::poh_recorder::PohRecorder;
use crate::runtime::{self, RuntimeError};
use crate::status_deque::{Status, StatusDeque, MAX_ENTRY_IDS};
use bincode::deserialize;
use itertools::Itertools;
use log::Level;
use rayon::prelude::*;
use solana_native_loader;
use solana_sdk::account::Account;
use solana_sdk::bpf_loader;
use solana_sdk::budget_program;
use solana_sdk::hash::Hash;
use solana_sdk::native_program::ProgramError;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Keypair;
use solana_sdk::signature::Signature;
use solana_sdk::storage_program;
use solana_sdk::system_program;
use solana_sdk::system_transaction::SystemTransaction;
use solana_sdk::timing::duration_as_us;
use solana_sdk::token_program;
use solana_sdk::transaction::Transaction;
use solana_sdk::vote_program;
use std;
use std::result;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Instant;

/// Reasons a transaction might be rejected.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum BankError {
    /// This Pubkey is being processed in another transaction
    AccountInUse,

    /// Attempt to debit from `Pubkey`, but no found no record of a prior credit.
    AccountNotFound,

    /// The from `Pubkey` does not have sufficient balance to pay the fee to schedule the transaction
    InsufficientFundsForFee,

    /// The bank has seen `Signature` before. This can occur under normal operation
    /// when a UDP packet is duplicated, as a user error from a client not updating
    /// its `last_id`, or as a double-spend attack.
    DuplicateSignature,

    /// The bank has not seen the given `last_id` or the transaction is too old and
    /// the `last_id` has been discarded.
    LastIdNotFound,

    /// Proof of History verification failed.
    LedgerVerificationFailed,

    /// The program returned an error
    ProgramError(u8, ProgramError),

    /// Recoding into PoH failed
    RecordFailure,

    /// Loader call chain too deep
    CallChainTooDeep,

    /// Transaction has a fee but has no signature present
    MissingSignatureForFee,
}

pub type Result<T> = result::Result<T, BankError>;

pub const VERIFY_BLOCK_SIZE: usize = 16;

pub trait BankSubscriptions {
    fn check_account(&self, pubkey: &Pubkey, account: &Account);
    fn check_signature(&self, signature: &Signature, status: &Result<()>);
}

struct LocalSubscriptions {}
impl Default for LocalSubscriptions {
    fn default() -> Self {
        LocalSubscriptions {}
    }
}

impl BankSubscriptions for LocalSubscriptions {
    fn check_account(&self, _pubkey: &Pubkey, _account: &Account) {}
    fn check_signature(&self, _signature: &Signature, _status: &Result<()>) {}
}

/// Manager for the state of all accounts and programs after processing its entries.
pub struct Bank {
    pub accounts: Accounts,

    /// FIFO queue of `last_id` items
    last_ids: RwLock<StatusDeque<Result<()>>>,

    // The latest confirmation time for the network
    confirmation_time: AtomicUsize,

    /// Tracks and updates the leader schedule based on the votes and account stakes
    /// processed by the bank
    pub leader_scheduler: Arc<RwLock<LeaderScheduler>>,

    subscriptions: RwLock<Box<Arc<BankSubscriptions + Send + Sync>>>,
}

impl Default for Bank {
    fn default() -> Self {
        Bank {
            accounts: Accounts::default(),
            last_ids: RwLock::new(StatusDeque::default()),
            confirmation_time: AtomicUsize::new(std::usize::MAX),
            leader_scheduler: Arc::new(RwLock::new(LeaderScheduler::default())),
            subscriptions: RwLock::new(Box::new(Arc::new(LocalSubscriptions::default()))),
        }
    }
}

impl Bank {
    pub fn new(genesis_block: &GenesisBlock) -> Self {
        let bank = Self::default();
        bank.process_genesis_block(genesis_block);
        bank.add_builtin_programs();
        bank
    }
    pub fn set_subscriptions(&self, subscriptions: Box<Arc<BankSubscriptions + Send + Sync>>) {
        let mut sub = self.subscriptions.write().unwrap();
        *sub = subscriptions
    }

    pub fn copy_for_tpu(&self) -> Self {
        Self {
            accounts: self.accounts.copy_for_tpu(),
            last_ids: RwLock::new(self.last_ids.read().unwrap().clone()),
            confirmation_time: AtomicUsize::new(self.confirmation_time()),
            leader_scheduler: self.leader_scheduler.clone(),
            subscriptions: RwLock::new(Box::new(Arc::new(LocalSubscriptions::default()))),
        }
    }

    fn process_genesis_block(&self, genesis_block: &GenesisBlock) {
        assert!(genesis_block.mint_id != Pubkey::default());
        assert!(genesis_block.tokens >= genesis_block.bootstrap_leader_tokens);

        let mut mint_account = Account::default();
        let mut bootstrap_leader_account = Account::default();
        mint_account.tokens += genesis_block.tokens;

        if genesis_block.bootstrap_leader_id != Pubkey::default() {
            mint_account.tokens -= genesis_block.bootstrap_leader_tokens;
            bootstrap_leader_account.tokens += genesis_block.bootstrap_leader_tokens;
            self.accounts.store_slow(
                &genesis_block.bootstrap_leader_id,
                &bootstrap_leader_account,
            );
        };

        self.accounts
            .store_slow(&genesis_block.mint_id, &mint_account);
        self.register_tick(&genesis_block.last_id());
    }

    fn add_system_program(&self) {
        let system_program_account = Account {
            tokens: 1,
            owner: system_program::id(),
            userdata: b"solana_system_program".to_vec(),
            executable: true,
            loader: solana_native_loader::id(),
        };
        self.accounts
            .store_slow(&system_program::id(), &system_program_account);
    }

    fn add_builtin_programs(&self) {
        self.add_system_program();

        // Vote program
        let vote_program_account = Account {
            tokens: 1,
            owner: vote_program::id(),
            userdata: b"solana_vote_program".to_vec(),
            executable: true,
            loader: solana_native_loader::id(),
        };
        self.accounts
            .store_slow(&vote_program::id(), &vote_program_account);

        // Storage program
        let storage_program_account = Account {
            tokens: 1,
            owner: storage_program::id(),
            userdata: b"solana_storage_program".to_vec(),
            executable: true,
            loader: solana_native_loader::id(),
        };
        self.accounts
            .store_slow(&storage_program::id(), &storage_program_account);

        let storage_system_account = Account {
            tokens: 1,
            owner: storage_program::system_id(),
            userdata: vec![0; 16 * 1024],
            executable: false,
            loader: Pubkey::default(),
        };
        self.accounts
            .store_slow(&storage_program::system_id(), &storage_system_account);

        // Bpf Loader
        let bpf_loader_account = Account {
            tokens: 1,
            owner: bpf_loader::id(),
            userdata: b"solana_bpf_loader".to_vec(),
            executable: true,
            loader: solana_native_loader::id(),
        };

        self.accounts
            .store_slow(&bpf_loader::id(), &bpf_loader_account);

        // Budget program
        let budget_program_account = Account {
            tokens: 1,
            owner: budget_program::id(),
            userdata: b"solana_budget_program".to_vec(),
            executable: true,
            loader: solana_native_loader::id(),
        };
        self.accounts
            .store_slow(&budget_program::id(), &budget_program_account);

        // Erc20 token program
        let erc20_account = Account {
            tokens: 1,
            owner: token_program::id(),
            userdata: b"solana_erc20".to_vec(),
            executable: true,
            loader: solana_native_loader::id(),
        };

        self.accounts
            .store_slow(&token_program::id(), &erc20_account);
    }

    /// Return the last entry ID registered.
    pub fn last_id(&self) -> Hash {
        self.last_ids
            .read()
            .unwrap()
            .last_id
            .expect("no last_id has been set")
    }

    pub fn get_storage_entry_height(&self) -> u64 {
        match self.get_account(&storage_program::system_id()) {
            Some(storage_system_account) => {
                let state = deserialize(&storage_system_account.userdata);
                if let Ok(state) = state {
                    let state: storage_program::StorageProgramState = state;
                    return state.entry_height;
                }
            }
            None => {
                info!("error in reading entry_height");
            }
        }
        0
    }

    pub fn get_storage_last_id(&self) -> Hash {
        if let Some(storage_system_account) = self.get_account(&storage_program::system_id()) {
            let state = deserialize(&storage_system_account.userdata);
            if let Ok(state) = state {
                let state: storage_program::StorageProgramState = state;
                return state.id;
            }
        }
        Hash::default()
    }

    /// Forget all signatures. Useful for benchmarking.
    pub fn clear_signatures(&self) {
        self.last_ids.write().unwrap().clear_signatures();
    }

    fn update_transaction_statuses(&self, txs: &[Transaction], res: &[Result<()>]) {
        let mut last_ids = self.last_ids.write().unwrap();
        for (i, tx) in txs.iter().enumerate() {
            last_ids.update_signature_status_with_last_id(&tx.signatures[0], &res[i], &tx.last_id);
            self.subscriptions
                .read()
                .unwrap()
                .check_signature(&tx.signatures[0], &res[i]);
        }
    }

    /// Looks through a list of tick heights and stakes, and finds the latest
    /// tick that has achieved confirmation
    pub fn get_confirmation_timestamp(
        &self,
        ticks_and_stakes: &mut [(u64, u64)],
        supermajority_stake: u64,
    ) -> Option<u64> {
        let last_ids = self.last_ids.read().unwrap();
        last_ids.get_confirmation_timestamp(ticks_and_stakes, supermajority_stake)
    }

    /// Tell the bank which Entry IDs exist on the ledger. This function
    /// assumes subsequent calls correspond to later entries, and will boot
    /// the oldest ones once its internal cache is full. Once boot, the
    /// bank will reject transactions using that `last_id`.
    pub fn register_tick(&self, last_id: &Hash) {
        let mut last_ids = self.last_ids.write().unwrap();
        inc_new_counter_info!("bank-register_tick-registered", 1);
        last_ids.register_tick(last_id)
    }

    /// Process a Transaction. This is used for unit tests and simply calls the vector Bank::process_transactions method.
    pub fn process_transaction(&self, tx: &Transaction) -> Result<()> {
        let txs = vec![tx.clone()];
        match self.process_transactions(&txs)[0] {
            Err(ref e) => {
                info!("process_transaction error: {:?}", e);
                Err((*e).clone())
            }
            Ok(_) => Ok(()),
        }
    }

    fn lock_accounts(&self, txs: &[Transaction]) -> Vec<Result<()>> {
        self.accounts.lock_accounts(txs)
    }

    fn unlock_accounts(&self, txs: &[Transaction], results: &[Result<()>]) {
        self.accounts.unlock_accounts(txs, results)
    }

    pub fn process_and_record_transactions(
        &self,
        txs: &[Transaction],
        poh: &PohRecorder,
    ) -> Result<()> {
        let now = Instant::now();
        // Once accounts are locked, other threads cannot encode transactions that will modify the
        // same account state
        let lock_results = self.lock_accounts(txs);
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
            self.record_transactions(txs, &results, poh)?;
            now.elapsed()
        };

        let commit_time = {
            let now = Instant::now();
            self.commit_transactions(txs, &loaded_accounts, &results);
            now.elapsed()
        };

        let now = Instant::now();
        // Once the accounts are new transactions can enter the pipeline to process them
        self.unlock_accounts(&txs, &results);
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
        Ok(())
    }

    fn record_transactions(
        &self,
        txs: &[Transaction],
        results: &[Result<()>],
        poh: &PohRecorder,
    ) -> Result<()> {
        let processed_transactions: Vec<_> = results
            .iter()
            .zip(txs.iter())
            .filter_map(|(r, x)| match r {
                Ok(_) => Some(x.clone()),
                Err(BankError::ProgramError(index, err)) => {
                    info!("program error {:?}, {:?}", index, err);
                    Some(x.clone())
                }
                Err(ref e) => {
                    debug!("process transaction failed {:?}", e);
                    None
                }
            })
            .collect();
        debug!("processed: {} ", processed_transactions.len());
        // unlock all the accounts with errors which are filtered by the above `filter_map`
        if !processed_transactions.is_empty() {
            let hash = Transaction::hash(&processed_transactions);
            // record and unlock will unlock all the successfull transactions
            poh.record(hash, processed_transactions).map_err(|e| {
                warn!("record failure: {:?}", e);
                BankError::RecordFailure
            })?;
        }
        Ok(())
    }

    fn load_accounts(
        &self,
        txs: &[Transaction],
        lock_results: Vec<Result<()>>,
        max_age: usize,
        error_counters: &mut ErrorCounters,
    ) -> Vec<Result<(InstructionAccounts, InstructionLoaders)>> {
        let mut last_ids = self.last_ids.write().unwrap();
        self.accounts
            .load_accounts(txs, &mut last_ids, lock_results, max_age, error_counters)
    }

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
        debug!("processing transactions: {}", txs.len());
        let mut error_counters = ErrorCounters::default();
        let now = Instant::now();
        let mut loaded_accounts =
            self.load_accounts(txs, lock_results, max_age, &mut error_counters);
        let tick_height = self.tick_height();

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

        self.accounts.increment_transaction_count(tx_count);

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
        let now = Instant::now();
        self.accounts.store_accounts(txs, executed, loaded_accounts);

        // Check account subscriptions and send notifications
        self.send_account_notifications(txs, executed, loaded_accounts);

        // once committed there is no way to unroll
        let write_elapsed = now.elapsed();
        debug!(
            "store: {}us txs_len={}",
            duration_as_us(&write_elapsed),
            txs.len(),
        );
        self.update_transaction_statuses(txs, &executed);
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

    #[must_use]
    pub fn process_transactions(&self, txs: &[Transaction]) -> Vec<Result<()>> {
        let lock_results = self.lock_accounts(txs);
        let results = self.load_execute_and_commit_transactions(txs, lock_results, MAX_ENTRY_IDS);
        self.unlock_accounts(txs, &results);
        results
    }

    pub fn process_entry(&self, entry: &Entry) -> Result<()> {
        if !entry.is_tick() {
            for result in self.process_transactions(&entry.transactions) {
                match result {
                    // Entries that result in a ProgramError are still valid and are written in the
                    // ledger so map them to an ok return value
                    Err(BankError::ProgramError(_, _)) => Ok(()),
                    _ => result,
                }?;
            }
        } else {
            self.register_tick(&entry.id);
            self.leader_scheduler
                .write()
                .unwrap()
                .update_height(self.tick_height(), self);
        }

        Ok(())
    }

    /// Process an ordered list of entries.
    pub fn process_entries(&self, entries: &[Entry]) -> Result<()> {
        self.par_process_entries(entries)
    }

    pub fn first_err(results: &[Result<()>]) -> Result<()> {
        for r in results {
            r.clone()?;
        }
        Ok(())
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

    fn par_execute_entries(&self, entries: &[(&Entry, Vec<Result<()>>)]) -> Result<()> {
        inc_new_counter_info!("bank-par_execute_entries-count", entries.len());
        let results: Vec<Result<()>> = entries
            .into_par_iter()
            .map(|(e, lock_results)| {
                let old_results = self.load_execute_and_commit_transactions(
                    &e.transactions,
                    lock_results.to_vec(),
                    MAX_ENTRY_IDS,
                );
                let results = Bank::ignore_program_errors(old_results);
                self.unlock_accounts(&e.transactions, &results);
                Self::first_err(&results)
            })
            .collect();
        Self::first_err(&results)
    }

    /// process entries in parallel
    /// 1. In order lock accounts for each entry while the lock succeeds, up to a Tick entry
    /// 2. Process the locked group in parallel
    /// 3. Register the `Tick` if it's available, goto 1
    pub fn par_process_entries(&self, entries: &[Entry]) -> Result<()> {
        // accumulator for entries that can be processed in parallel
        let mut mt_group = vec![];
        for entry in entries {
            if entry.is_tick() {
                // if its a tick, execute the group and register the tick
                self.par_execute_entries(&mt_group)?;
                self.register_tick(&entry.id);
                self.leader_scheduler
                    .write()
                    .unwrap()
                    .update_height(self.tick_height(), self);
                mt_group = vec![];
                continue;
            }
            // try to lock the accounts
            let lock_results = self.lock_accounts(&entry.transactions);
            // if any of the locks error out
            // execute the current group
            if Self::first_err(&lock_results).is_err() {
                self.par_execute_entries(&mt_group)?;
                mt_group = vec![];
                //reset the lock and push the entry
                self.unlock_accounts(&entry.transactions, &lock_results);
                let lock_results = self.lock_accounts(&entry.transactions);
                mt_group.push((entry, lock_results));
            } else {
                // push the entry to the mt_group
                mt_group.push((entry, lock_results));
            }
        }
        self.par_execute_entries(&mt_group)?;
        Ok(())
    }

    /// Process an ordered list of entries, populating a circular buffer "tail"
    /// as we go.
    fn process_block(&self, entries: &[Entry]) -> Result<()> {
        for entry in entries {
            self.process_entry(entry)?;
        }

        Ok(())
    }

    /// Append entry blocks to the ledger, verifying them along the way.
    pub fn process_ledger<I>(&self, entries: I) -> Result<(u64, Hash)>
    where
        I: IntoIterator<Item = Entry>,
    {
        // these magic numbers are from genesis of the mint, could pull them
        //  back out of this loop.
        let mut entry_height = 0;
        let mut last_id = self.last_id();

        // Ledger verification needs to be parallelized, but we can't pull the whole
        // thing into memory. We therefore chunk it.
        for block in &entries.into_iter().chunks(VERIFY_BLOCK_SIZE) {
            let block: Vec<_> = block.collect();

            if !block.verify(&last_id) {
                warn!("Ledger proof of history failed at entry: {}", entry_height);
                return Err(BankError::LedgerVerificationFailed);
            }

            self.process_block(&block)?;

            last_id = block.last().unwrap().id;
            entry_height += block.len() as u64;
        }
        Ok((entry_height, last_id))
    }

    /// Create, sign, and process a Transaction from `keypair` to `to` of
    /// `n` tokens where `last_id` is the last Entry ID observed by the client.
    pub fn transfer(
        &self,
        n: u64,
        keypair: &Keypair,
        to: Pubkey,
        last_id: Hash,
    ) -> Result<Signature> {
        let tx = Transaction::system_new(keypair, to, n, last_id);
        let signature = tx.signatures[0];
        self.process_transaction(&tx).map(|_| signature)
    }

    pub fn read_balance(account: &Account) -> u64 {
        // TODO: Re-instate budget_program special case?
        /*
        if budget_program::check_id(&account.owner) {
           return budget_program::get_balance(account);
        }
        */
        account.tokens
    }
    /// Each program would need to be able to introspect its own state
    /// this is hard-coded to the Budget language
    pub fn get_balance(&self, pubkey: &Pubkey) -> u64 {
        self.get_account(pubkey)
            .map(|x| Self::read_balance(&x))
            .unwrap_or(0)
    }

    pub fn get_account(&self, pubkey: &Pubkey) -> Option<Account> {
        self.accounts.load_slow(pubkey)
    }

    pub fn transaction_count(&self) -> u64 {
        self.accounts.transaction_count()
    }

    pub fn get_signature_status(&self, signature: &Signature) -> Option<Status<Result<()>>> {
        self.last_ids
            .read()
            .unwrap()
            .get_signature_status(signature)
    }

    pub fn has_signature(&self, signature: &Signature) -> bool {
        self.last_ids.read().unwrap().has_signature(signature)
    }

    pub fn get_signature(
        &self,
        last_id: &Hash,
        signature: &Signature,
    ) -> Option<Status<Result<()>>> {
        self.last_ids
            .read()
            .unwrap()
            .get_signature(last_id, signature)
    }

    /// Hash the `accounts` HashMap. This represents a validator's interpretation
    ///  of the delta of the ledger since the last vote and up to now
    pub fn hash_internal_state(&self) -> Hash {
        self.accounts.hash_internal_state()
    }

    pub fn confirmation_time(&self) -> usize {
        self.confirmation_time.load(Ordering::Relaxed)
    }

    pub fn set_confirmation_time(&self, confirmation: usize) {
        self.confirmation_time
            .store(confirmation, Ordering::Relaxed);
    }

    fn send_account_notifications(
        &self,
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
                self.subscriptions
                    .read()
                    .unwrap()
                    .check_account(&key, account);
            }
        }
    }

    pub fn get_current_leader(&self) -> Option<(Pubkey, u64)> {
        self.leader_scheduler
            .read()
            .unwrap()
            .get_scheduled_leader(self.tick_height() + 1)
    }

    pub fn tick_height(&self) -> u64 {
        self.last_ids.read().unwrap().tick_height
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::{next_entries, next_entry, Entry};
    use crate::gen_keys::GenKeys;
    use bincode::serialize;
    use hashbrown::HashSet;
    use solana_sdk::hash::hash;
    use solana_sdk::native_program::ProgramError;
    use solana_sdk::signature::Keypair;
    use solana_sdk::signature::KeypairUtil;
    use solana_sdk::storage_program::{StorageTransaction, ENTRIES_PER_SEGMENT};
    use solana_sdk::system_instruction::SystemInstruction;
    use solana_sdk::system_transaction::SystemTransaction;
    use solana_sdk::transaction::Instruction;
    use std;
    use std::sync::mpsc::channel;

    #[test]
    fn test_bank_new() {
        let (genesis_block, _) = GenesisBlock::new(10_000);
        let bank = Bank::new(&genesis_block);
        assert_eq!(bank.get_balance(&genesis_block.mint_id), 10_000);
    }

    #[test]
    fn test_bank_new_with_leader() {
        let dummy_leader_id = Keypair::new().pubkey();
        let dummy_leader_tokens = 1;
        let (genesis_block, _) =
            GenesisBlock::new_with_leader(10_000, dummy_leader_id, dummy_leader_tokens);
        let bank = Bank::new(&genesis_block);
        assert_eq!(bank.get_balance(&genesis_block.mint_id), 9999);
        assert_eq!(bank.get_balance(&dummy_leader_id), 1);
    }

    #[test]
    fn test_two_payments_to_one_party() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(10_000);
        let pubkey = Keypair::new().pubkey();
        let bank = Bank::new(&genesis_block);
        assert_eq!(bank.last_id(), genesis_block.last_id());

        bank.transfer(1_000, &mint_keypair, pubkey, genesis_block.last_id())
            .unwrap();
        assert_eq!(bank.get_balance(&pubkey), 1_000);

        bank.transfer(500, &mint_keypair, pubkey, genesis_block.last_id())
            .unwrap();
        assert_eq!(bank.get_balance(&pubkey), 1_500);
        assert_eq!(bank.transaction_count(), 2);
    }

    #[test]
    fn test_one_source_two_tx_one_batch() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(1);
        let key1 = Keypair::new().pubkey();
        let key2 = Keypair::new().pubkey();
        let bank = Bank::new(&genesis_block);
        assert_eq!(bank.last_id(), genesis_block.last_id());

        let t1 = Transaction::system_move(&mint_keypair, key1, 1, genesis_block.last_id(), 0);
        let t2 = Transaction::system_move(&mint_keypair, key2, 1, genesis_block.last_id(), 0);
        let res = bank.process_transactions(&vec![t1.clone(), t2.clone()]);
        assert_eq!(res.len(), 2);
        assert_eq!(res[0], Ok(()));
        assert_eq!(res[1], Err(BankError::AccountInUse));
        assert_eq!(bank.get_balance(&mint_keypair.pubkey()), 0);
        assert_eq!(bank.get_balance(&key1), 1);
        assert_eq!(bank.get_balance(&key2), 0);
        assert_eq!(
            bank.get_signature(&t1.last_id, &t1.signatures[0]),
            Some(Status::Complete(Ok(())))
        );
        // TODO: Transactions that fail to pay a fee could be dropped silently
        assert_eq!(
            bank.get_signature(&t2.last_id, &t2.signatures[0]),
            Some(Status::Complete(Err(BankError::AccountInUse)))
        );
    }

    #[test]
    fn test_one_tx_two_out_atomic_fail() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(1);
        let key1 = Keypair::new().pubkey();
        let key2 = Keypair::new().pubkey();
        let bank = Bank::new(&genesis_block);
        let spend = SystemInstruction::Move { tokens: 1 };
        let instructions = vec![
            Instruction {
                program_ids_index: 0,
                userdata: serialize(&spend).unwrap(),
                accounts: vec![0, 1],
            },
            Instruction {
                program_ids_index: 0,
                userdata: serialize(&spend).unwrap(),
                accounts: vec![0, 2],
            },
        ];

        let t1 = Transaction::new_with_instructions(
            &[&mint_keypair],
            &[key1, key2],
            genesis_block.last_id(),
            0,
            vec![system_program::id()],
            instructions,
        );
        let res = bank.process_transactions(&vec![t1.clone()]);
        assert_eq!(res.len(), 1);
        assert_eq!(
            res[0],
            Err(BankError::ProgramError(
                1,
                ProgramError::ResultWithNegativeTokens
            ))
        );
        assert_eq!(bank.get_balance(&mint_keypair.pubkey()), 1);
        assert_eq!(bank.get_balance(&key1), 0);
        assert_eq!(bank.get_balance(&key2), 0);
        assert_eq!(
            bank.get_signature(&t1.last_id, &t1.signatures[0]),
            Some(Status::Complete(Err(BankError::ProgramError(
                1,
                ProgramError::ResultWithNegativeTokens
            ))))
        );
    }

    #[test]
    fn test_one_tx_two_out_atomic_pass() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(2);
        let key1 = Keypair::new().pubkey();
        let key2 = Keypair::new().pubkey();
        let bank = Bank::new(&genesis_block);
        let t1 = Transaction::system_move_many(
            &mint_keypair,
            &[(key1, 1), (key2, 1)],
            genesis_block.last_id(),
            0,
        );
        let res = bank.process_transactions(&vec![t1.clone()]);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0], Ok(()));
        assert_eq!(bank.get_balance(&mint_keypair.pubkey()), 0);
        assert_eq!(bank.get_balance(&key1), 1);
        assert_eq!(bank.get_balance(&key2), 1);
        assert_eq!(
            bank.get_signature(&t1.last_id, &t1.signatures[0]),
            Some(Status::Complete(Ok(())))
        );
    }

    // TODO: This test demonstrates that fees are not paid when a program fails.
    // See github issue 1157 (https://github.com/solana-labs/solana/issues/1157)
    #[test]
    fn test_detect_failed_duplicate_transactions_issue_1157() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(1);
        let bank = Bank::new(&genesis_block);
        let dest = Keypair::new();

        // source with 0 program context
        let tx = Transaction::system_create(
            &mint_keypair,
            dest.pubkey(),
            genesis_block.last_id(),
            2,
            0,
            Pubkey::default(),
            1,
        );
        let signature = tx.signatures[0];
        assert!(!bank.has_signature(&signature));
        let res = bank.process_transaction(&tx);

        // Result failed, but signature is registered
        assert!(res.is_err());
        assert!(bank.has_signature(&signature));
        assert_matches!(
            bank.get_signature_status(&signature),
            Some(Status::Complete(Err(BankError::ProgramError(
                0,
                ProgramError::ResultWithNegativeTokens
            ))))
        );

        // The tokens didn't move, but the from address paid the transaction fee.
        assert_eq!(bank.get_balance(&dest.pubkey()), 0);

        // BUG: This should be the original balance minus the transaction fee.
        //assert_eq!(bank.get_balance(&mint_keypair.pubkey()), 0);
    }

    #[test]
    fn test_account_not_found() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(1);
        let bank = Bank::new(&genesis_block);
        let keypair = Keypair::new();
        assert_eq!(
            bank.transfer(1, &keypair, mint_keypair.pubkey(), genesis_block.last_id()),
            Err(BankError::AccountNotFound)
        );
        assert_eq!(bank.transaction_count(), 0);
    }

    #[test]
    fn test_insufficient_funds() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(11_000);
        let bank = Bank::new(&genesis_block);
        let pubkey = Keypair::new().pubkey();
        bank.transfer(1_000, &mint_keypair, pubkey, genesis_block.last_id())
            .unwrap();
        assert_eq!(bank.transaction_count(), 1);
        assert_eq!(bank.get_balance(&pubkey), 1_000);
        assert_matches!(
            bank.transfer(10_001, &mint_keypair, pubkey, genesis_block.last_id()),
            Err(BankError::ProgramError(
                0,
                ProgramError::ResultWithNegativeTokens
            ))
        );
        assert_eq!(bank.transaction_count(), 1);

        let mint_pubkey = mint_keypair.pubkey();
        assert_eq!(bank.get_balance(&mint_pubkey), 10_000);
        assert_eq!(bank.get_balance(&pubkey), 1_000);
    }

    #[test]
    fn test_transfer_to_newb() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(10_000);
        let bank = Bank::new(&genesis_block);
        let pubkey = Keypair::new().pubkey();
        bank.transfer(500, &mint_keypair, pubkey, genesis_block.last_id())
            .unwrap();
        assert_eq!(bank.get_balance(&pubkey), 500);
    }

    #[test]
    fn test_debits_before_credits() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(2);
        let bank = Bank::new(&genesis_block);
        let keypair = Keypair::new();
        let tx0 =
            Transaction::system_new(&mint_keypair, keypair.pubkey(), 2, genesis_block.last_id());
        let tx1 =
            Transaction::system_new(&keypair, mint_keypair.pubkey(), 1, genesis_block.last_id());
        let txs = vec![tx0, tx1];
        let results = bank.process_transactions(&txs);
        assert!(results[1].is_err());

        // Assert bad transactions aren't counted.
        assert_eq!(bank.transaction_count(), 1);
    }

    #[test]
    fn test_process_empty_entry_is_registered() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(1);
        let bank = Bank::new(&genesis_block);
        let keypair = Keypair::new();
        let entry = next_entry(&genesis_block.last_id(), 1, vec![]);
        let tx = Transaction::system_new(&mint_keypair, keypair.pubkey(), 1, entry.id);

        // First, ensure the TX is rejected because of the unregistered last ID
        assert_eq!(
            bank.process_transaction(&tx),
            Err(BankError::LastIdNotFound)
        );

        // Now ensure the TX is accepted despite pointing to the ID of an empty entry.
        bank.process_entries(&[entry]).unwrap();
        assert_eq!(bank.process_transaction(&tx), Ok(()));
    }

    #[test]
    fn test_process_genesis() {
        let dummy_leader_id = Keypair::new().pubkey();
        let dummy_leader_tokens = 1;
        let (genesis_block, _) =
            GenesisBlock::new_with_leader(5, dummy_leader_id, dummy_leader_tokens);
        let bank = Bank::default();
        bank.process_genesis_block(&genesis_block);
        assert_eq!(bank.get_balance(&genesis_block.mint_id), 4);
        assert_eq!(bank.get_balance(&dummy_leader_id), 1);
        // TODO: Restore next assert_eq() once leader scheduler configuration is stored in the
        // genesis block
        /*
        assert_eq!(
            bank.leader_scheduler.read().unwrap().bootstrap_leader,
            dummy_leader_id
        );
        */
    }

    fn create_sample_block_with_next_entries_using_keypairs(
        genesis_block: &GenesisBlock,
        mint_keypair: &Keypair,
        keypairs: &[Keypair],
    ) -> impl Iterator<Item = Entry> {
        let mut last_id = genesis_block.last_id();
        let mut hash = genesis_block.last_id();
        let mut entries: Vec<Entry> = vec![];
        let num_hashes = 1;
        for k in keypairs {
            let txs = vec![Transaction::system_new(
                mint_keypair,
                k.pubkey(),
                1,
                last_id,
            )];
            let mut e = next_entries(&hash, 0, txs);
            entries.append(&mut e);
            hash = entries.last().unwrap().id;
            let tick = Entry::new(&hash, 0, num_hashes, vec![]);
            hash = tick.id;
            last_id = hash;
            entries.push(tick);
        }
        entries.into_iter()
    }

    // create a ledger with tick entries every `ticks` entries
    fn create_sample_block_with_ticks(
        genesis_block: &GenesisBlock,
        mint_keypair: &Keypair,
        length: usize,
        ticks: usize,
    ) -> impl Iterator<Item = Entry> {
        let mut entries = Vec::with_capacity(length);
        let mut last_id = genesis_block.last_id();
        let mut hash = genesis_block.last_id();
        let num_hashes = 1;
        for i in 0..length {
            let keypair = Keypair::new();
            let tx = Transaction::system_new(mint_keypair, keypair.pubkey(), 1, last_id);
            let entry = Entry::new(&hash, 0, num_hashes, vec![tx]);
            hash = entry.id;
            entries.push(entry);

            // Add a second Transaction that will produce a
            // ProgramError<0, ResultWithNegativeTokens> error when processed
            let keypair2 = Keypair::new();
            let tx = Transaction::system_new(&keypair, keypair2.pubkey(), 42, last_id);
            let entry = Entry::new(&hash, 0, num_hashes, vec![tx]);
            hash = entry.id;
            entries.push(entry);

            if (i + 1) % ticks == 0 {
                let tick = Entry::new(&hash, 0, num_hashes, vec![]);
                hash = tick.id;
                last_id = hash;
                entries.push(tick);
            }
        }
        entries.into_iter()
    }

    fn create_sample_ledger(length: usize) -> (GenesisBlock, Keypair, impl Iterator<Item = Entry>) {
        let mint_keypair = Keypair::new();
        let genesis_block = GenesisBlock {
            bootstrap_leader_id: Keypair::new().pubkey(),
            bootstrap_leader_tokens: 1,
            mint_id: mint_keypair.pubkey(),
            tokens: length as u64 + 2,
        };
        let block = create_sample_block_with_ticks(&genesis_block, &mint_keypair, length, length);
        (genesis_block, mint_keypair, block)
    }

    #[test]
    fn test_process_ledger_simple() {
        let (genesis_block, mint_keypair, ledger) = create_sample_ledger(1);
        let bank = Bank::default();
        bank.process_genesis_block(&genesis_block);
        bank.add_system_program();
        let (ledger_height, last_id) = bank.process_ledger(ledger).unwrap();
        assert_eq!(bank.get_balance(&mint_keypair.pubkey()), 1);
        assert_eq!(ledger_height, 3);
        assert_eq!(bank.tick_height(), 2);
        assert_eq!(bank.last_id(), last_id);
    }

    #[test]
    fn test_hash_internal_state() {
        let mint_keypair = Keypair::new();
        let genesis_block = GenesisBlock {
            bootstrap_leader_id: Keypair::new().pubkey(),
            bootstrap_leader_tokens: 1,
            mint_id: mint_keypair.pubkey(),
            tokens: 2_000,
        };
        let seed = [0u8; 32];
        let mut rnd = GenKeys::new(seed);
        let keypairs = rnd.gen_n_keypairs(5);
        let ledger0 = create_sample_block_with_next_entries_using_keypairs(
            &genesis_block,
            &mint_keypair,
            &keypairs,
        );
        let ledger1 = create_sample_block_with_next_entries_using_keypairs(
            &genesis_block,
            &mint_keypair,
            &keypairs,
        );

        let bank0 = Bank::default();
        bank0.add_system_program();
        bank0.process_genesis_block(&genesis_block);
        bank0.process_ledger(ledger0).unwrap();
        let bank1 = Bank::default();
        bank1.add_system_program();
        bank1.process_genesis_block(&genesis_block);
        bank1.process_ledger(ledger1).unwrap();

        let initial_state = bank0.hash_internal_state();

        assert_eq!(bank1.hash_internal_state(), initial_state);

        let pubkey = keypairs[0].pubkey();
        bank0
            .transfer(1_000, &mint_keypair, pubkey, genesis_block.last_id())
            .unwrap();
        assert_ne!(bank0.hash_internal_state(), initial_state);
        bank1
            .transfer(1_000, &mint_keypair, pubkey, genesis_block.last_id())
            .unwrap();
        assert_eq!(bank0.hash_internal_state(), bank1.hash_internal_state());
    }
    #[test]
    fn test_confirmation_time() {
        let def_bank = Bank::default();
        assert_eq!(def_bank.confirmation_time(), std::usize::MAX);
        def_bank.set_confirmation_time(90);
        assert_eq!(def_bank.confirmation_time(), 90);
    }
    #[test]
    fn test_interleaving_locks() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(3);
        let bank = Bank::new(&genesis_block);
        let alice = Keypair::new();
        let bob = Keypair::new();

        let tx1 =
            Transaction::system_new(&mint_keypair, alice.pubkey(), 1, genesis_block.last_id());
        let pay_alice = vec![tx1];

        let lock_result = bank.lock_accounts(&pay_alice);
        let results_alice =
            bank.load_execute_and_commit_transactions(&pay_alice, lock_result, MAX_ENTRY_IDS);
        assert_eq!(results_alice[0], Ok(()));

        // try executing an interleaved transfer twice
        assert_eq!(
            bank.transfer(1, &mint_keypair, bob.pubkey(), genesis_block.last_id()),
            Err(BankError::AccountInUse)
        );
        // the second time should fail as well
        // this verifies that `unlock_accounts` doesn't unlock `AccountInUse` accounts
        assert_eq!(
            bank.transfer(1, &mint_keypair, bob.pubkey(), genesis_block.last_id()),
            Err(BankError::AccountInUse)
        );

        bank.unlock_accounts(&pay_alice, &results_alice);

        assert_matches!(
            bank.transfer(2, &mint_keypair, bob.pubkey(), genesis_block.last_id()),
            Ok(_)
        );
    }

    #[test]
    fn test_first_err() {
        assert_eq!(Bank::first_err(&[Ok(())]), Ok(()));
        assert_eq!(
            Bank::first_err(&[Ok(()), Err(BankError::DuplicateSignature)]),
            Err(BankError::DuplicateSignature)
        );
        assert_eq!(
            Bank::first_err(&[
                Ok(()),
                Err(BankError::DuplicateSignature),
                Err(BankError::AccountInUse)
            ]),
            Err(BankError::DuplicateSignature)
        );
        assert_eq!(
            Bank::first_err(&[
                Ok(()),
                Err(BankError::AccountInUse),
                Err(BankError::DuplicateSignature)
            ]),
            Err(BankError::AccountInUse)
        );
        assert_eq!(
            Bank::first_err(&[
                Err(BankError::AccountInUse),
                Ok(()),
                Err(BankError::DuplicateSignature)
            ]),
            Err(BankError::AccountInUse)
        );
    }
    #[test]
    fn test_par_process_entries_tick() {
        let (genesis_block, _mint_keypair) = GenesisBlock::new(1000);
        let bank = Bank::new(&genesis_block);

        // ensure bank can process a tick
        let tick = next_entry(&genesis_block.last_id(), 1, vec![]);
        assert_eq!(bank.par_process_entries(&[tick.clone()]), Ok(()));
        assert_eq!(bank.last_id(), tick.id);
    }
    #[test]
    fn test_par_process_entries_2_entries_collision() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(1000);
        let bank = Bank::new(&genesis_block);
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();

        let last_id = bank.last_id();

        // ensure bank can process 2 entries that have a common account and no tick is registered
        let tx = Transaction::system_new(&mint_keypair, keypair1.pubkey(), 2, bank.last_id());
        let entry_1 = next_entry(&last_id, 1, vec![tx]);
        let tx = Transaction::system_new(&mint_keypair, keypair2.pubkey(), 2, bank.last_id());
        let entry_2 = next_entry(&entry_1.id, 1, vec![tx]);
        assert_eq!(bank.par_process_entries(&[entry_1, entry_2]), Ok(()));
        assert_eq!(bank.get_balance(&keypair1.pubkey()), 2);
        assert_eq!(bank.get_balance(&keypair2.pubkey()), 2);
        assert_eq!(bank.last_id(), last_id);
    }
    #[test]
    fn test_par_process_entries_2_txes_collision() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(1000);
        let bank = Bank::new(&genesis_block);
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();
        let keypair3 = Keypair::new();
        println!("KP1 {:?}", keypair1.pubkey());
        println!("KP2 {:?}", keypair2.pubkey());
        println!("KP3 {:?}", keypair3.pubkey());
        println!("GenesisBlock {:?}", mint_keypair.pubkey());

        // fund: put 4 in each of 1 and 2
        assert_matches!(
            bank.transfer(4, &mint_keypair, keypair1.pubkey(), bank.last_id()),
            Ok(_)
        );
        assert_matches!(
            bank.transfer(4, &mint_keypair, keypair2.pubkey(), bank.last_id()),
            Ok(_)
        );

        // construct an Entry whose 2nd transaction would cause a lock conflict with previous entry
        let entry_1_to_mint = next_entry(
            &bank.last_id(),
            1,
            vec![Transaction::system_new(
                &keypair1,
                mint_keypair.pubkey(),
                1,
                bank.last_id(),
            )],
        );

        let entry_2_to_3_mint_to_1 = next_entry(
            &entry_1_to_mint.id,
            1,
            vec![
                Transaction::system_new(&keypair2, keypair3.pubkey(), 2, bank.last_id()), // should be fine
                Transaction::system_new(&keypair1, mint_keypair.pubkey(), 2, bank.last_id()), // will collide
            ],
        );

        assert_eq!(
            bank.par_process_entries(&[entry_1_to_mint, entry_2_to_3_mint_to_1]),
            Ok(())
        );

        assert_eq!(bank.get_balance(&keypair1.pubkey()), 1);
        assert_eq!(bank.get_balance(&keypair2.pubkey()), 2);
        assert_eq!(bank.get_balance(&keypair3.pubkey()), 2);
    }
    #[test]
    fn test_par_process_entries_2_entries_par() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(1000);
        let bank = Bank::new(&genesis_block);
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();
        let keypair3 = Keypair::new();
        let keypair4 = Keypair::new();

        //load accounts
        let tx = Transaction::system_new(&mint_keypair, keypair1.pubkey(), 1, bank.last_id());
        assert_eq!(bank.process_transaction(&tx), Ok(()));
        let tx = Transaction::system_new(&mint_keypair, keypair2.pubkey(), 1, bank.last_id());
        assert_eq!(bank.process_transaction(&tx), Ok(()));

        // ensure bank can process 2 entries that do not have a common account and no tick is registered
        let last_id = bank.last_id();
        let tx = Transaction::system_new(&keypair1, keypair3.pubkey(), 1, bank.last_id());
        let entry_1 = next_entry(&last_id, 1, vec![tx]);
        let tx = Transaction::system_new(&keypair2, keypair4.pubkey(), 1, bank.last_id());
        let entry_2 = next_entry(&entry_1.id, 1, vec![tx]);
        assert_eq!(bank.par_process_entries(&[entry_1, entry_2]), Ok(()));
        assert_eq!(bank.get_balance(&keypair3.pubkey()), 1);
        assert_eq!(bank.get_balance(&keypair4.pubkey()), 1);
        assert_eq!(bank.last_id(), last_id);
    }
    #[test]
    fn test_par_process_entries_2_entries_tick() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(1000);
        let bank = Bank::new(&genesis_block);
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();
        let keypair3 = Keypair::new();
        let keypair4 = Keypair::new();

        //load accounts
        let tx = Transaction::system_new(&mint_keypair, keypair1.pubkey(), 1, bank.last_id());
        assert_eq!(bank.process_transaction(&tx), Ok(()));
        let tx = Transaction::system_new(&mint_keypair, keypair2.pubkey(), 1, bank.last_id());
        assert_eq!(bank.process_transaction(&tx), Ok(()));

        let last_id = bank.last_id();

        // ensure bank can process 2 entries that do not have a common account and tick is registered
        let tx = Transaction::system_new(&keypair2, keypair3.pubkey(), 1, bank.last_id());
        let entry_1 = next_entry(&last_id, 1, vec![tx]);
        let tick = next_entry(&entry_1.id, 1, vec![]);
        let tx = Transaction::system_new(&keypair1, keypair4.pubkey(), 1, tick.id);
        let entry_2 = next_entry(&tick.id, 1, vec![tx]);
        assert_eq!(
            bank.par_process_entries(&[entry_1.clone(), tick.clone(), entry_2.clone()]),
            Ok(())
        );
        assert_eq!(bank.get_balance(&keypair3.pubkey()), 1);
        assert_eq!(bank.get_balance(&keypair4.pubkey()), 1);
        assert_eq!(bank.last_id(), tick.id);
        // ensure that an error is returned for an empty account (keypair2)
        let tx = Transaction::system_new(&keypair2, keypair3.pubkey(), 1, tick.id);
        let entry_3 = next_entry(&entry_2.id, 1, vec![tx]);
        assert_eq!(
            bank.par_process_entries(&[entry_3]),
            Err(BankError::AccountNotFound)
        );
    }

    #[test]
    fn test_program_ids() {
        let system = Pubkey::new(&[
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0,
        ]);
        let native = Pubkey::new(&[
            1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0,
        ]);
        let bpf = Pubkey::new(&[
            128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0,
        ]);
        let budget = Pubkey::new(&[
            129, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0,
        ]);
        let storage = Pubkey::new(&[
            130, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0,
        ]);
        let token = Pubkey::new(&[
            131, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0,
        ]);
        let vote = Pubkey::new(&[
            132, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0,
        ]);
        let storage_system = Pubkey::new(&[
            133, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0,
        ]);

        assert_eq!(system_program::id(), system);
        assert_eq!(solana_native_loader::id(), native);
        assert_eq!(bpf_loader::id(), bpf);
        assert_eq!(budget_program::id(), budget);
        assert_eq!(storage_program::id(), storage);
        assert_eq!(token_program::id(), token);
        assert_eq!(vote_program::id(), vote);
        assert_eq!(storage_program::system_id(), storage_system);
    }

    #[test]
    fn test_program_id_uniqueness() {
        let mut unique = HashSet::new();
        let ids = vec![
            system_program::id(),
            solana_native_loader::id(),
            bpf_loader::id(),
            budget_program::id(),
            storage_program::id(),
            token_program::id(),
            vote_program::id(),
            storage_program::system_id(),
        ];
        assert!(ids.into_iter().all(move |id| unique.insert(id)));
    }

    #[test]
    fn test_bank_record_transactions() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(10_000);
        let bank = Arc::new(Bank::new(&genesis_block));
        let (entry_sender, entry_receiver) = channel();
        let poh_recorder = PohRecorder::new(bank.clone(), entry_sender, bank.last_id(), None);
        let pubkey = Keypair::new().pubkey();

        let transactions = vec![
            Transaction::system_move(&mint_keypair, pubkey, 1, genesis_block.last_id(), 0),
            Transaction::system_move(&mint_keypair, pubkey, 1, genesis_block.last_id(), 0),
        ];

        let mut results = vec![Ok(()), Ok(())];
        bank.record_transactions(&transactions, &results, &poh_recorder)
            .unwrap();
        let entries = entry_receiver.recv().unwrap();
        assert_eq!(entries[0].transactions.len(), transactions.len());

        // ProgramErrors should still be recorded
        results[0] = Err(BankError::ProgramError(
            1,
            ProgramError::ResultWithNegativeTokens,
        ));
        bank.record_transactions(&transactions, &results, &poh_recorder)
            .unwrap();
        let entries = entry_receiver.recv().unwrap();
        assert_eq!(entries[0].transactions.len(), transactions.len());

        // Other BankErrors should not be recorded
        results[0] = Err(BankError::AccountNotFound);
        bank.record_transactions(&transactions, &results, &poh_recorder)
            .unwrap();
        let entries = entry_receiver.recv().unwrap();
        assert_eq!(entries[0].transactions.len(), transactions.len() - 1);
    }

    #[test]
    fn test_bank_ignore_program_errors() {
        let expected_results = vec![Ok(()), Ok(())];
        let results = vec![Ok(()), Ok(())];
        let updated_results = Bank::ignore_program_errors(results);
        assert_eq!(updated_results, expected_results);

        let results = vec![
            Err(BankError::ProgramError(
                1,
                ProgramError::ResultWithNegativeTokens,
            )),
            Ok(()),
        ];
        let updated_results = Bank::ignore_program_errors(results);
        assert_eq!(updated_results, expected_results);

        // Other BankErrors should not be ignored
        let results = vec![Err(BankError::AccountNotFound), Ok(())];
        let updated_results = Bank::ignore_program_errors(results);
        assert_ne!(updated_results, expected_results);
    }

    #[test]
    fn test_bank_storage() {
        solana_logger::setup();
        let (genesis_block, alice) = GenesisBlock::new(1000);
        let bank = Bank::new(&genesis_block);

        let bob = Keypair::new();
        let jack = Keypair::new();
        let jill = Keypair::new();

        let x = 42;
        let last_id = hash(&[x]);
        let x2 = x * 2;
        let storage_last_id = hash(&[x2]);

        bank.register_tick(&last_id);

        bank.transfer(10, &alice, jill.pubkey(), last_id).unwrap();

        bank.transfer(10, &alice, bob.pubkey(), last_id).unwrap();
        bank.transfer(10, &alice, jack.pubkey(), last_id).unwrap();

        let tx = Transaction::storage_new_advertise_last_id(
            &bob,
            storage_last_id,
            last_id,
            ENTRIES_PER_SEGMENT,
        );

        bank.process_transaction(&tx).unwrap();

        let entry_height = 0;

        let tx = Transaction::storage_new_mining_proof(
            &jack,
            Hash::default(),
            last_id,
            entry_height,
            Signature::default(),
        );

        bank.process_transaction(&tx).unwrap();

        assert_eq!(bank.get_storage_entry_height(), ENTRIES_PER_SEGMENT);
        assert_eq!(bank.get_storage_last_id(), storage_last_id);
    }

    #[test]
    fn test_bank_process_and_record_transactions() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(10_000);
        let bank = Arc::new(Bank::new(&genesis_block));
        let pubkey = Keypair::new().pubkey();

        let transactions = vec![Transaction::system_move(
            &mint_keypair,
            pubkey,
            1,
            genesis_block.last_id(),
            0,
        )];

        let (entry_sender, entry_receiver) = channel();
        let mut poh_recorder = PohRecorder::new(
            bank.clone(),
            entry_sender,
            bank.last_id(),
            Some(bank.tick_height() + 1),
        );

        bank.process_and_record_transactions(&transactions, &poh_recorder)
            .unwrap();
        poh_recorder.tick().unwrap();

        let mut need_tick = true;
        // read entries until I find mine, might be ticks...
        while need_tick {
            let entries = entry_receiver.recv().unwrap();
            for entry in entries {
                if !entry.is_tick() {
                    assert_eq!(entry.transactions.len(), transactions.len());
                    assert_eq!(bank.get_balance(&pubkey), 1);
                } else {
                    need_tick = false;
                }
            }
        }

        let transactions = vec![Transaction::system_move(
            &mint_keypair,
            pubkey,
            2,
            genesis_block.last_id(),
            0,
        )];

        assert_eq!(
            bank.process_and_record_transactions(&transactions, &poh_recorder),
            Err(BankError::RecordFailure)
        );

        assert_eq!(bank.get_balance(&pubkey), 1);
    }

}

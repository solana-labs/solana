//! The `bank` module tracks client accounts and the progress of on-chain
//! programs. It offers a high-level API that signs transactions
//! on behalf of the caller, and a low-level API for when they have
//! already been signed and verified.

use crate::accounts::{Accounts, ErrorCounters, InstructionAccounts, InstructionLoaders};
use crate::hash_queue::HashQueue;
use crate::runtime::{self, RuntimeError};
use crate::status_cache::StatusCache;
use bincode::serialize;
use hashbrown::HashMap;
use log::*;
use solana_budget_api;
use solana_metrics::counter::Counter;
use solana_sdk::account::Account;
use solana_sdk::bpf_loader;
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::hash::{extend_and_hash, Hash};
use solana_sdk::native_loader;
use solana_sdk::native_program::ProgramError;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signature};
use solana_sdk::storage_program;
use solana_sdk::system_program;
use solana_sdk::system_transaction::SystemTransaction;
use solana_sdk::timing::{duration_as_us, MAX_RECENT_BLOCKHASHES, NUM_TICKS_PER_SECOND};
use solana_sdk::token_program;
use solana_sdk::transaction::Transaction;
use solana_vote_api;
use solana_vote_api::vote_instruction::Vote;
use solana_vote_api::vote_state::{Lockout, VoteState};
use std::result;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Instant;

/// Reasons a transaction might be rejected.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum BankError {
    /// This Pubkey is being processed in another transaction
    AccountInUse,

    /// Pubkey appears twice in the same transaction, typically in a pay-to-self
    /// transaction.
    AccountLoadedTwice,

    /// Attempt to debit from `Pubkey`, but no found no record of a prior credit.
    AccountNotFound,

    /// The from `Pubkey` does not have sufficient balance to pay the fee to schedule the transaction
    InsufficientFundsForFee,

    /// The bank has seen `Signature` before. This can occur under normal operation
    /// when a UDP packet is duplicated, as a user error from a client not updating
    /// its `recent_blockhash`, or as a double-spend attack.
    DuplicateSignature,

    /// The bank has not seen the given `recent_blockhash` or the transaction is too old and
    /// the `recent_blockhash` has been discarded.
    BlockhashNotFound,

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

type BankStatusCache = StatusCache<BankError>;

/// Manager for the state of all accounts and programs after processing its entries.
#[derive(Default)]
pub struct Bank {
    accounts: Option<Arc<Accounts>>,

    /// A cache of signature statuses
    status_cache: RwLock<BankStatusCache>,

    /// FIFO queue of `recent_blockhash` items
    blockhash_queue: RwLock<HashQueue>,

    /// Previous checkpoint of this bank
    parent: RwLock<Option<Arc<Bank>>>,

    /// Hash of this Bank's state. Only meaningful after freezing.
    hash: RwLock<Hash>,

    /// Hash of this Bank's parent's state
    parent_hash: Hash,

    /// Bank tick height
    tick_height: AtomicUsize, // TODO: Use AtomicU64 if/when available

    /// Bank fork (i.e. slot, i.e. block)
    slot: u64,

    /// The number of ticks in each slot.
    ticks_per_slot: u64,

    /// The number of slots in each epoch.
    slots_per_epoch: u64,

    /// A number of slots before slot_index 0. Used to calculate finalized staked nodes.
    stakers_slot_offset: u64,

    /// The pubkey to send transactions fees to.
    collector_id: Pubkey,

    /// staked nodes on epoch boundaries, saved off when a bank.slot() is at
    ///   a leader schedule boundary
    epoch_vote_accounts: HashMap<u64, HashMap<Pubkey, Account>>,

    /// Bank accounts fork id
    accounts_id: u64,
}

impl Default for HashQueue {
    fn default() -> Self {
        Self::new(MAX_RECENT_BLOCKHASHES)
    }
}

impl Bank {
    pub fn new(genesis_block: &GenesisBlock) -> Self {
        Self::new_with_paths(&genesis_block, None)
    }

    pub fn new_with_paths(genesis_block: &GenesisBlock, paths: Option<String>) -> Self {
        let mut bank = Self::default();
        bank.accounts = Some(Arc::new(Accounts::new(bank.slot, paths)));
        bank.process_genesis_block(genesis_block);
        bank.add_builtin_programs();

        // genesis needs stakes for all epochs up to the epoch implied by
        //  slot = 0 and genesis configuration
        let vote_accounts: HashMap<_, _> = bank.vote_accounts().collect();
        for i in 0..=bank.epoch_from_stakers_slot_offset() {
            bank.epoch_vote_accounts.insert(i, vote_accounts.clone());
        }

        bank
    }

    /// Create a new bank that points to an immutable checkpoint of another bank.
    pub fn new_from_parent(parent: &Arc<Bank>, collector_id: Pubkey, slot: u64) -> Self {
        parent.freeze();

        assert_ne!(slot, parent.slot());

        let mut bank = Self::default();
        bank.blockhash_queue = RwLock::new(parent.blockhash_queue.read().unwrap().clone());
        bank.tick_height
            .store(parent.tick_height.load(Ordering::SeqCst), Ordering::SeqCst);
        bank.ticks_per_slot = parent.ticks_per_slot;
        bank.slots_per_epoch = parent.slots_per_epoch;
        bank.stakers_slot_offset = parent.stakers_slot_offset;

        bank.slot = slot;
        bank.parent = RwLock::new(Some(parent.clone()));
        bank.parent_hash = parent.hash();
        bank.collector_id = collector_id;

        // Accounts needs a unique id
        static BANK_ACCOUNTS_ID: AtomicUsize = AtomicUsize::new(1);
        bank.accounts_id = BANK_ACCOUNTS_ID.fetch_add(1, Ordering::Relaxed) as u64;
        bank.accounts = Some(parent.accounts());
        bank.accounts()
            .new_from_parent(bank.accounts_id, parent.accounts_id);

        bank.epoch_vote_accounts = {
            let mut epoch_vote_accounts = parent.epoch_vote_accounts.clone();
            let epoch = bank.epoch_from_stakers_slot_offset();
            // update epoch_vote_states cache
            //  if my parent didn't populate for this epoch, we've
            //  crossed a boundary
            if epoch_vote_accounts.get(&epoch).is_none() {
                epoch_vote_accounts.insert(epoch, bank.vote_accounts().collect());
            }
            epoch_vote_accounts
        };

        bank
    }

    pub fn collector_id(&self) -> Pubkey {
        self.collector_id
    }

    pub fn slot(&self) -> u64 {
        self.slot
    }

    pub fn hash(&self) -> Hash {
        *self.hash.read().unwrap()
    }

    pub fn is_frozen(&self) -> bool {
        *self.hash.read().unwrap() != Hash::default()
    }

    pub fn freeze(&self) {
        let mut hash = self.hash.write().unwrap();

        if *hash == Hash::default() {
            //  freeze is a one-way trip, idempotent
            *hash = self.hash_internal_state();
        }
    }

    /// squash the parent's state up into this Bank,
    ///   this Bank becomes a root
    pub fn squash(&self) {
        self.freeze();

        let parents = self.parents();
        *self.parent.write().unwrap() = None;

        self.accounts().squash(self.accounts_id);

        let parent_caches: Vec<_> = parents
            .iter()
            .map(|b| b.status_cache.read().unwrap())
            .collect();
        self.status_cache.write().unwrap().squash(&parent_caches);
    }

    /// Return the more recent checkpoint of this bank instance.
    pub fn parent(&self) -> Option<Arc<Bank>> {
        self.parent.read().unwrap().clone()
    }

    fn process_genesis_block(&mut self, genesis_block: &GenesisBlock) {
        assert!(genesis_block.mint_id != Pubkey::default());
        assert!(genesis_block.bootstrap_leader_id != Pubkey::default());
        assert!(genesis_block.bootstrap_leader_vote_account_id != Pubkey::default());
        assert!(genesis_block.tokens >= genesis_block.bootstrap_leader_tokens);
        assert!(genesis_block.bootstrap_leader_tokens >= 3);

        // Bootstrap leader collects fees until `new_from_parent` is called.
        self.collector_id = genesis_block.bootstrap_leader_id;

        let mint_tokens = genesis_block.tokens - genesis_block.bootstrap_leader_tokens;
        self.deposit(&genesis_block.mint_id, mint_tokens);

        let bootstrap_leader_tokens = 2;
        let bootstrap_leader_stake =
            genesis_block.bootstrap_leader_tokens - bootstrap_leader_tokens;
        self.deposit(&genesis_block.bootstrap_leader_id, bootstrap_leader_tokens);

        // Construct a vote account for the bootstrap_leader such that the leader_scheduler
        // will be forced to select it as the leader for height 0
        let mut bootstrap_leader_vote_account = Account {
            tokens: bootstrap_leader_stake,
            userdata: vec![0; VoteState::max_size() as usize],
            owner: solana_vote_api::id(),
            executable: false,
        };

        let mut vote_state = VoteState::new(genesis_block.bootstrap_leader_id);
        vote_state.votes.push_back(Lockout::new(&Vote::new(0)));
        vote_state
            .serialize(&mut bootstrap_leader_vote_account.userdata)
            .unwrap();

        self.accounts().store_slow(
            self.accounts_id,
            &genesis_block.bootstrap_leader_vote_account_id,
            &bootstrap_leader_vote_account,
        );

        self.blockhash_queue
            .write()
            .unwrap()
            .genesis_hash(&genesis_block.hash());

        self.ticks_per_slot = genesis_block.ticks_per_slot;
        self.slots_per_epoch = genesis_block.slots_per_epoch;
        self.stakers_slot_offset = genesis_block.stakers_slot_offset;
    }

    pub fn add_native_program(&self, name: &str, program_id: &Pubkey) {
        let account = native_loader::create_program_account(name);
        self.accounts()
            .store_slow(self.accounts_id, program_id, &account);
    }

    fn add_builtin_programs(&self) {
        self.add_native_program("solana_system_program", &system_program::id());
        self.add_native_program("solana_vote_program", &solana_vote_api::id());
        self.add_native_program("solana_storage_program", &storage_program::id());
        self.add_native_program("solana_bpf_loader", &bpf_loader::id());
        self.add_native_program("solana_budget_program", &solana_budget_api::id());
        self.add_native_program("solana_token_program", &token_program::id());
    }

    /// Return the last block hash registered.
    pub fn last_blockhash(&self) -> Hash {
        self.blockhash_queue.read().unwrap().last_hash()
    }

    /// Forget all signatures. Useful for benchmarking.
    pub fn clear_signatures(&self) {
        self.status_cache.write().unwrap().clear();
    }

    fn update_transaction_statuses(&self, txs: &[Transaction], res: &[Result<()>]) {
        let mut status_cache = self.status_cache.write().unwrap();
        for (i, tx) in txs.iter().enumerate() {
            match &res[i] {
                Ok(_) => {
                    if !tx.signatures.is_empty() {
                        status_cache.add(&tx.signatures[0]);
                    }
                }
                Err(BankError::BlockhashNotFound) => (),
                Err(BankError::DuplicateSignature) => (),
                Err(BankError::AccountNotFound) => (),
                Err(e) => {
                    if !tx.signatures.is_empty() {
                        status_cache.add(&tx.signatures[0]);
                        status_cache.save_failure_status(&tx.signatures[0], e.clone());
                    }
                }
            }
        }
    }

    /// Looks through a list of tick heights and stakes, and finds the latest
    /// tick that has achieved confirmation
    pub fn get_confirmation_timestamp(
        &self,
        mut slots_and_stakes: Vec<(u64, u64)>,
        supermajority_stake: u64,
    ) -> Option<u64> {
        // Sort by slot height
        slots_and_stakes.sort_by(|a, b| a.0.cmp(&b.0));

        let max_slot = self.slot_height();
        let min_slot = max_slot.saturating_sub(MAX_RECENT_BLOCKHASHES as u64);

        let mut total_stake = 0;
        for (slot, stake) in slots_and_stakes.iter() {
            if *slot >= min_slot && *slot <= max_slot {
                total_stake += stake;
                if total_stake > supermajority_stake {
                    return self
                        .blockhash_queue
                        .read()
                        .unwrap()
                        .hash_height_to_timestamp(*slot);
                }
            }
        }

        None
    }

    /// Tell the bank which Entry IDs exist on the ledger. This function
    /// assumes subsequent calls correspond to later entries, and will boot
    /// the oldest ones once its internal cache is full. Once boot, the
    /// bank will reject transactions using that `hash`.
    pub fn register_tick(&self, hash: &Hash) {
        if self.is_frozen() {
            warn!("=========== FIXME: register_tick() working on a frozen bank! ================");
        }

        // TODO: put this assert back in
        // assert!(!self.is_frozen());

        let current_tick_height = {
            self.tick_height.fetch_add(1, Ordering::SeqCst);
            self.tick_height.load(Ordering::SeqCst) as u64
        };
        inc_new_counter_info!("bank-register_tick-registered", 1);

        // Register a new block hash if at the last tick in the slot
        if current_tick_height % self.ticks_per_slot == self.ticks_per_slot - 1 {
            let mut blockhash_queue = self.blockhash_queue.write().unwrap();
            blockhash_queue.register_hash(hash);
        }

        if current_tick_height % NUM_TICKS_PER_SECOND == 0 {
            self.status_cache.write().unwrap().new_cache(hash);
        }
    }

    /// Process a Transaction. This is used for unit tests and simply calls the vector Bank::process_transactions method.
    pub fn process_transaction(&self, tx: &Transaction) -> Result<()> {
        let txs = vec![tx.clone()];
        self.process_transactions(&txs)[0].clone()?;
        tx.signatures
            .get(0)
            .map_or(Ok(()), |sig| self.get_signature_status(sig).unwrap())
    }

    pub fn lock_accounts(&self, txs: &[Transaction]) -> Vec<Result<()>> {
        if self.is_frozen() {
            warn!("=========== FIXME: lock_accounts() working on a frozen bank! ================");
        }
        // TODO: put this assert back in
        // assert!(!self.is_frozen());
        self.accounts().lock_accounts(self.accounts_id, txs)
    }

    pub fn unlock_accounts(&self, txs: &[Transaction], results: &[Result<()>]) {
        self.accounts()
            .unlock_accounts(self.accounts_id, txs, results)
    }

    fn load_accounts(
        &self,
        txs: &[Transaction],
        results: Vec<Result<()>>,
        error_counters: &mut ErrorCounters,
    ) -> Vec<Result<(InstructionAccounts, InstructionLoaders)>> {
        self.accounts()
            .load_accounts(self.accounts_id, txs, results, error_counters)
    }
    fn check_age(
        &self,
        txs: &[Transaction],
        lock_results: Vec<Result<()>>,
        max_age: usize,
        error_counters: &mut ErrorCounters,
    ) -> Vec<Result<()>> {
        let hash_queue = self.blockhash_queue.read().unwrap();
        txs.iter()
            .zip(lock_results.into_iter())
            .map(|(tx, lock_res)| {
                if lock_res.is_ok() && !hash_queue.check_entry_age(tx.recent_blockhash, max_age) {
                    error_counters.reserve_blockhash += 1;
                    Err(BankError::BlockhashNotFound)
                } else {
                    lock_res
                }
            })
            .collect()
    }
    fn check_signatures(
        &self,
        txs: &[Transaction],
        lock_results: Vec<Result<()>>,
        error_counters: &mut ErrorCounters,
    ) -> Vec<Result<()>> {
        let parents = self.parents();
        let mut caches = vec![self.status_cache.read().unwrap()];
        caches.extend(parents.iter().map(|b| b.status_cache.read().unwrap()));
        txs.iter()
            .zip(lock_results.into_iter())
            .map(|(tx, lock_res)| {
                if tx.signatures.is_empty() {
                    return lock_res;
                }
                if lock_res.is_ok() && StatusCache::has_signature_all(&caches, &tx.signatures[0]) {
                    error_counters.duplicate_signature += 1;
                    Err(BankError::DuplicateSignature)
                } else {
                    lock_res
                }
            })
            .collect()
    }
    #[allow(clippy::type_complexity)]
    pub fn load_and_execute_transactions(
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
        let age_results = self.check_age(txs, lock_results, max_age, &mut error_counters);
        let sig_results = self.check_signatures(txs, age_results, &mut error_counters);
        let mut loaded_accounts = self.load_accounts(txs, sig_results, &mut error_counters);
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

        self.accounts()
            .increment_transaction_count(self.accounts_id, tx_count);

        inc_new_counter_info!("bank-process_transactions-txs", tx_count);
        if 0 != error_counters.blockhash_not_found {
            inc_new_counter_info!(
                "bank-process_transactions-error-blockhash_not_found",
                error_counters.blockhash_not_found
            );
        }
        if 0 != error_counters.reserve_blockhash {
            inc_new_counter_info!(
                "bank-process_transactions-error-reserve_blockhash",
                error_counters.reserve_blockhash
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
        if 0 != error_counters.account_loaded_twice {
            inc_new_counter_info!(
                "bank-process_transactions-account_loaded_twice",
                error_counters.account_loaded_twice
            );
        }
        (loaded_accounts, executed)
    }

    fn filter_program_errors_and_collect_fee(
        &self,
        txs: &[Transaction],
        executed: &[Result<()>],
    ) -> Vec<Result<()>> {
        let mut fees = 0;
        let results = txs
            .iter()
            .zip(executed.iter())
            .map(|(tx, res)| match *res {
                Err(BankError::ProgramError(_, _)) => {
                    // Charge the transaction fee even in case of ProgramError
                    self.withdraw(&tx.account_keys[0], tx.fee)?;
                    fees += tx.fee;
                    Ok(())
                }
                Ok(()) => {
                    fees += tx.fee;
                    Ok(())
                }
                _ => res.clone(),
            })
            .collect();
        self.deposit(&self.collector_id, fees);
        results
    }

    pub fn commit_transactions(
        &self,
        txs: &[Transaction],
        loaded_accounts: &[Result<(InstructionAccounts, InstructionLoaders)>],
        executed: &[Result<()>],
    ) -> Vec<Result<()>> {
        if self.is_frozen() {
            warn!("=========== FIXME: commit_transactions() working on a frozen bank! ================");
        }
        // TODO: put this assert back in
        // assert!(!self.is_frozen());
        let now = Instant::now();
        self.accounts()
            .store_accounts(self.accounts_id, txs, executed, loaded_accounts);

        // once committed there is no way to unroll
        let write_elapsed = now.elapsed();
        debug!(
            "store: {}us txs_len={}",
            duration_as_us(&write_elapsed),
            txs.len(),
        );
        self.update_transaction_statuses(txs, &executed);
        self.filter_program_errors_and_collect_fee(txs, executed)
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

        self.commit_transactions(txs, &loaded_accounts, &executed)
    }

    #[must_use]
    pub fn process_transactions(&self, txs: &[Transaction]) -> Vec<Result<()>> {
        let lock_results = self.lock_accounts(txs);
        let results =
            self.load_execute_and_commit_transactions(txs, lock_results, MAX_RECENT_BLOCKHASHES);
        self.unlock_accounts(txs, &results);
        results
    }

    /// Create, sign, and process a Transaction from `keypair` to `to` of
    /// `n` tokens where `blockhash` is the last Entry ID observed by the client.
    pub fn transfer(
        &self,
        n: u64,
        keypair: &Keypair,
        to: Pubkey,
        blockhash: Hash,
    ) -> Result<Signature> {
        let tx = SystemTransaction::new_account(keypair, to, n, blockhash, 0);
        let signature = tx.signatures[0];
        self.process_transaction(&tx).map(|_| signature)
    }

    pub fn read_balance(account: &Account) -> u64 {
        account.tokens
    }
    /// Each program would need to be able to introspect its own state
    /// this is hard-coded to the Budget language
    pub fn get_balance(&self, pubkey: &Pubkey) -> u64 {
        self.get_account(pubkey)
            .map(|x| Self::read_balance(&x))
            .unwrap_or(0)
    }

    /// Compute all the parents of the bank in order
    pub fn parents(&self) -> Vec<Arc<Bank>> {
        let mut parents = vec![];
        let mut bank = self.parent();
        while let Some(parent) = bank {
            parents.push(parent.clone());
            bank = parent.parent();
        }
        parents
    }

    pub fn withdraw(&self, pubkey: &Pubkey, tokens: u64) -> Result<()> {
        match self.get_account(pubkey) {
            Some(mut account) => {
                if tokens > account.tokens {
                    return Err(BankError::InsufficientFundsForFee);
                }

                account.tokens -= tokens;
                self.accounts()
                    .store_slow(self.accounts_id, pubkey, &account);
                Ok(())
            }
            None => Err(BankError::AccountNotFound),
        }
    }

    pub fn deposit(&self, pubkey: &Pubkey, tokens: u64) {
        let mut account = self.get_account(pubkey).unwrap_or_default();
        account.tokens += tokens;
        self.accounts()
            .store_slow(self.accounts_id, pubkey, &account);
    }

    fn accounts(&self) -> Arc<Accounts> {
        if let Some(accounts) = &self.accounts {
            accounts.clone()
        } else {
            panic!("no accounts!");
        }
    }

    pub fn get_account(&self, pubkey: &Pubkey) -> Option<Account> {
        self.accounts().load_slow(self.accounts_id, pubkey)
    }

    pub fn get_account_modified_since_parent(&self, pubkey: &Pubkey) -> Option<Account> {
        self.accounts()
            .load_slow_no_parent(self.accounts_id, pubkey)
    }

    pub fn transaction_count(&self) -> u64 {
        self.accounts().transaction_count(self.accounts_id)
    }

    pub fn get_signature_status(&self, signature: &Signature) -> Option<Result<()>> {
        let parents = self.parents();
        let mut caches = vec![self.status_cache.read().unwrap()];
        caches.extend(parents.iter().map(|b| b.status_cache.read().unwrap()));
        StatusCache::get_signature_status_all(&caches, signature)
    }

    pub fn has_signature(&self, signature: &Signature) -> bool {
        let parents = self.parents();
        let mut caches = vec![self.status_cache.read().unwrap()];
        caches.extend(parents.iter().map(|b| b.status_cache.read().unwrap()));
        StatusCache::has_signature_all(&caches, signature)
    }

    /// Hash the `accounts` HashMap. This represents a validator's interpretation
    ///  of the delta of the ledger since the last vote and up to now
    fn hash_internal_state(&self) -> Hash {
        // If there are no accounts, return the same hash as we did before
        // checkpointing.
        if !self.accounts().has_accounts(self.accounts_id) {
            return self.parent_hash;
        }

        let accounts_delta_hash = self.accounts().hash_internal_state(self.accounts_id);
        extend_and_hash(&self.parent_hash, &serialize(&accounts_delta_hash).unwrap())
    }

    /// Return the number of slots in advance of an epoch that a leader scheduler
    /// should be generated.
    pub fn stakers_slot_offset(&self) -> u64 {
        self.stakers_slot_offset
    }

    /// Return the number of ticks per slot that should be used calls to slot_height().
    pub fn ticks_per_slot(&self) -> u64 {
        self.ticks_per_slot
    }

    /// Return the number of ticks since genesis.
    pub fn tick_height(&self) -> u64 {
        // tick_height is using an AtomicUSize because AtomicU64 is not yet a stable API.
        // Until we can switch to AtomicU64, fail if usize is not the same as u64
        assert_eq!(std::usize::MAX, 0xFFFF_FFFF_FFFF_FFFF);

        self.tick_height.load(Ordering::SeqCst) as u64
    }

    /// Return the number of ticks since the last slot boundary.
    pub fn tick_index(&self) -> u64 {
        self.tick_height() % self.ticks_per_slot()
    }

    /// Return the slot_height of the last registered tick.
    pub fn slot_height(&self) -> u64 {
        self.tick_height() / self.ticks_per_slot()
    }

    /// Return the number of slots per tick.
    pub fn slots_per_epoch(&self) -> u64 {
        self.slots_per_epoch
    }

    /// returns the epoch for which this bank's stakers_slot_offset and slot would
    ///  need to cache stakers
    fn epoch_from_stakers_slot_offset(&self) -> u64 {
        (self.slot + self.stakers_slot_offset) / self.slots_per_epoch
    }

    /// current vote accounts for this bank
    pub fn vote_accounts(&self) -> impl Iterator<Item = (Pubkey, Account)> {
        self.accounts().get_vote_accounts(self.accounts_id)
    }

    ///  vote accounts for the specific epoch
    pub fn epoch_vote_accounts(&self, epoch: u64) -> Option<&HashMap<Pubkey, Account>> {
        self.epoch_vote_accounts.get(&epoch)
    }

    /// Return the number of slots since the last epoch boundary.
    pub fn slot_index(&self) -> u64 {
        self.slot_height() % self.slots_per_epoch()
    }

    /// Return the epoch height of the last registered tick.
    pub fn epoch_height(&self) -> u64 {
        self.slot_height() / self.slots_per_epoch()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hashbrown::HashSet;
    use solana_sdk::genesis_block::BOOTSTRAP_LEADER_TOKENS;
    use solana_sdk::native_program::ProgramError;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_instruction::SystemInstruction;
    use solana_sdk::system_transaction::SystemTransaction;
    use solana_sdk::transaction::Instruction;

    #[test]
    fn test_bank_new() {
        let (genesis_block, _) = GenesisBlock::new(10_000);
        let bank = Bank::new(&genesis_block);
        assert_eq!(bank.get_balance(&genesis_block.mint_id), 10_000);
    }

    #[test]
    fn test_bank_new_with_leader() {
        let dummy_leader_id = Keypair::new().pubkey();
        let dummy_leader_tokens = BOOTSTRAP_LEADER_TOKENS;
        let (genesis_block, _) =
            GenesisBlock::new_with_leader(10_000, dummy_leader_id, dummy_leader_tokens);
        assert_eq!(genesis_block.bootstrap_leader_tokens, dummy_leader_tokens);
        let bank = Bank::new(&genesis_block);
        assert_eq!(
            bank.get_balance(&genesis_block.mint_id),
            10_000 - dummy_leader_tokens
        );
        assert_eq!(
            bank.get_balance(&dummy_leader_id),
            dummy_leader_tokens - 1 /* 1 token goes to the vote account associated with dummy_leader_tokens */
        );
    }

    #[test]
    fn test_two_payments_to_one_party() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(10_000);
        let pubkey = Keypair::new().pubkey();
        let bank = Bank::new(&genesis_block);
        assert_eq!(bank.last_blockhash(), genesis_block.hash());

        bank.transfer(1_000, &mint_keypair, pubkey, genesis_block.hash())
            .unwrap();
        assert_eq!(bank.get_balance(&pubkey), 1_000);

        bank.transfer(500, &mint_keypair, pubkey, genesis_block.hash())
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
        assert_eq!(bank.last_blockhash(), genesis_block.hash());

        let t1 = SystemTransaction::new_move(&mint_keypair, key1, 1, genesis_block.hash(), 0);
        let t2 = SystemTransaction::new_move(&mint_keypair, key2, 1, genesis_block.hash(), 0);
        let res = bank.process_transactions(&vec![t1.clone(), t2.clone()]);
        assert_eq!(res.len(), 2);
        assert_eq!(res[0], Ok(()));
        assert_eq!(res[1], Err(BankError::AccountInUse));
        assert_eq!(bank.get_balance(&mint_keypair.pubkey()), 0);
        assert_eq!(bank.get_balance(&key1), 1);
        assert_eq!(bank.get_balance(&key2), 0);
        assert_eq!(bank.get_signature_status(&t1.signatures[0]), Some(Ok(())));
        // TODO: Transactions that fail to pay a fee could be dropped silently
        assert_eq!(
            bank.get_signature_status(&t2.signatures[0]),
            Some(Err(BankError::AccountInUse))
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
            genesis_block.hash(),
            0,
            vec![system_program::id()],
            instructions,
        );
        let res = bank.process_transactions(&vec![t1.clone()]);
        assert_eq!(res.len(), 1);
        assert_eq!(bank.get_balance(&mint_keypair.pubkey()), 1);
        assert_eq!(bank.get_balance(&key1), 0);
        assert_eq!(bank.get_balance(&key2), 0);
        assert_eq!(
            bank.get_signature_status(&t1.signatures[0]),
            Some(Err(BankError::ProgramError(
                1,
                ProgramError::ResultWithNegativeTokens
            )))
        );
    }

    #[test]
    fn test_one_tx_two_out_atomic_pass() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(2);
        let key1 = Keypair::new().pubkey();
        let key2 = Keypair::new().pubkey();
        let bank = Bank::new(&genesis_block);
        let t1 = SystemTransaction::new_move_many(
            &mint_keypair,
            &[(key1, 1), (key2, 1)],
            genesis_block.hash(),
            0,
        );
        let res = bank.process_transactions(&vec![t1.clone()]);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0], Ok(()));
        assert_eq!(bank.get_balance(&mint_keypair.pubkey()), 0);
        assert_eq!(bank.get_balance(&key1), 1);
        assert_eq!(bank.get_balance(&key2), 1);
        assert_eq!(bank.get_signature_status(&t1.signatures[0]), Some(Ok(())));
    }

    // This test demonstrates that fees are paid even when a program fails.
    #[test]
    fn test_detect_failed_duplicate_transactions() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(2);
        let bank = Bank::new(&genesis_block);
        let dest = Keypair::new();

        // source with 0 program context
        let tx = SystemTransaction::new_account(
            &mint_keypair,
            dest.pubkey(),
            2,
            genesis_block.hash(),
            1,
        );
        let signature = tx.signatures[0];
        assert!(!bank.has_signature(&signature));

        assert_eq!(
            bank.process_transaction(&tx),
            Err(BankError::ProgramError(
                0,
                ProgramError::ResultWithNegativeTokens
            ))
        );

        // The tokens didn't move, but the from address paid the transaction fee.
        assert_eq!(bank.get_balance(&dest.pubkey()), 0);

        // This should be the original balance minus the transaction fee.
        assert_eq!(bank.get_balance(&mint_keypair.pubkey()), 1);
    }

    #[test]
    fn test_account_not_found() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(0);
        let bank = Bank::new(&genesis_block);
        let keypair = Keypair::new();
        assert_eq!(
            bank.transfer(1, &keypair, mint_keypair.pubkey(), genesis_block.hash()),
            Err(BankError::AccountNotFound)
        );
        assert_eq!(bank.transaction_count(), 0);
    }

    #[test]
    fn test_insufficient_funds() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(11_000);
        let bank = Bank::new(&genesis_block);
        let pubkey = Keypair::new().pubkey();
        bank.transfer(1_000, &mint_keypair, pubkey, genesis_block.hash())
            .unwrap();
        assert_eq!(bank.transaction_count(), 1);
        assert_eq!(bank.get_balance(&pubkey), 1_000);
        assert_eq!(
            bank.transfer(10_001, &mint_keypair, pubkey, genesis_block.hash()),
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
        bank.transfer(500, &mint_keypair, pubkey, genesis_block.hash())
            .unwrap();
        assert_eq!(bank.get_balance(&pubkey), 500);
    }

    #[test]
    fn test_bank_deposit() {
        let (genesis_block, _mint_keypair) = GenesisBlock::new(100);
        let bank = Bank::new(&genesis_block);

        // Test new account
        let key = Keypair::new();
        bank.deposit(&key.pubkey(), 10);
        assert_eq!(bank.get_balance(&key.pubkey()), 10);

        // Existing account
        bank.deposit(&key.pubkey(), 3);
        assert_eq!(bank.get_balance(&key.pubkey()), 13);
    }

    #[test]
    fn test_bank_withdraw() {
        let (genesis_block, _mint_keypair) = GenesisBlock::new(100);
        let bank = Bank::new(&genesis_block);

        // Test no account
        let key = Keypair::new();
        assert_eq!(
            bank.withdraw(&key.pubkey(), 10),
            Err(BankError::AccountNotFound)
        );

        bank.deposit(&key.pubkey(), 3);
        assert_eq!(bank.get_balance(&key.pubkey()), 3);

        // Low balance
        assert_eq!(
            bank.withdraw(&key.pubkey(), 10),
            Err(BankError::InsufficientFundsForFee)
        );

        // Enough balance
        assert_eq!(bank.withdraw(&key.pubkey(), 2), Ok(()));
        assert_eq!(bank.get_balance(&key.pubkey()), 1);
    }

    #[test]
    fn test_bank_tx_fee() {
        let leader = Keypair::new().pubkey();
        let (genesis_block, mint_keypair) = GenesisBlock::new_with_leader(100, leader, 3);
        let bank = Bank::new(&genesis_block);
        let key1 = Keypair::new();
        let key2 = Keypair::new();

        let tx =
            SystemTransaction::new_move(&mint_keypair, key1.pubkey(), 2, genesis_block.hash(), 3);
        let initial_balance = bank.get_balance(&leader);
        assert_eq!(bank.process_transaction(&tx), Ok(()));
        assert_eq!(bank.get_balance(&leader), initial_balance + 3);
        assert_eq!(bank.get_balance(&key1.pubkey()), 2);
        assert_eq!(bank.get_balance(&mint_keypair.pubkey()), 100 - 5 - 3);

        let tx = SystemTransaction::new_move(&key1, key2.pubkey(), 1, genesis_block.hash(), 1);
        assert_eq!(bank.process_transaction(&tx), Ok(()));
        assert_eq!(bank.get_balance(&leader), initial_balance + 4);
        assert_eq!(bank.get_balance(&key1.pubkey()), 0);
        assert_eq!(bank.get_balance(&key2.pubkey()), 1);
        assert_eq!(bank.get_balance(&mint_keypair.pubkey()), 100 - 5 - 3);
    }

    #[test]
    fn test_filter_program_errors_and_collect_fee() {
        let leader = Keypair::new().pubkey();
        let (genesis_block, mint_keypair) = GenesisBlock::new_with_leader(100, leader, 3);
        let bank = Bank::new(&genesis_block);

        let key = Keypair::new();
        let tx1 =
            SystemTransaction::new_move(&mint_keypair, key.pubkey(), 2, genesis_block.hash(), 3);
        let tx2 =
            SystemTransaction::new_move(&mint_keypair, key.pubkey(), 5, genesis_block.hash(), 1);

        let results = vec![
            Ok(()),
            Err(BankError::ProgramError(
                1,
                ProgramError::ResultWithNegativeTokens,
            )),
        ];

        let initial_balance = bank.get_balance(&leader);
        let results = bank.filter_program_errors_and_collect_fee(&vec![tx1, tx2], &results);
        assert_eq!(bank.get_balance(&leader), initial_balance + 3 + 1);
        assert_eq!(results[0], Ok(()));
        assert_eq!(results[1], Ok(()));
    }

    #[test]
    fn test_debits_before_credits() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(2);
        let bank = Bank::new(&genesis_block);
        let keypair = Keypair::new();
        let tx0 = SystemTransaction::new_account(
            &mint_keypair,
            keypair.pubkey(),
            2,
            genesis_block.hash(),
            0,
        );
        let tx1 = SystemTransaction::new_account(
            &keypair,
            mint_keypair.pubkey(),
            1,
            genesis_block.hash(),
            0,
        );
        let txs = vec![tx0, tx1];
        let results = bank.process_transactions(&txs);
        assert!(results[1].is_err());

        // Assert bad transactions aren't counted.
        assert_eq!(bank.transaction_count(), 1);
    }

    #[test]
    fn test_process_genesis() {
        let dummy_leader_id = Keypair::new().pubkey();
        let dummy_leader_tokens = 3;
        let (genesis_block, _) =
            GenesisBlock::new_with_leader(5, dummy_leader_id, dummy_leader_tokens);
        let bank = Bank::new(&genesis_block);
        assert_eq!(bank.get_balance(&genesis_block.mint_id), 2);
        assert_eq!(bank.get_balance(&dummy_leader_id), 2);
    }

    // Register n ticks and return the tick, slot and epoch indexes.
    fn register_ticks(bank: &Bank, n: u64) -> (u64, u64, u64) {
        for _ in 0..n {
            bank.register_tick(&Hash::default());
        }
        (bank.tick_index(), bank.slot_index(), bank.epoch_height())
    }

    #[test]
    fn test_tick_slot_epoch_indexes() {
        let (genesis_block, _) = GenesisBlock::new(5);
        let bank = Bank::new(&genesis_block);
        let ticks_per_slot = bank.ticks_per_slot();
        let slots_per_epoch = bank.slots_per_epoch();
        let ticks_per_epoch = ticks_per_slot * slots_per_epoch;

        // All indexes are zero-based.
        assert_eq!(register_ticks(&bank, 0), (0, 0, 0));

        // Slot index remains zero through the last tick.
        assert_eq!(
            register_ticks(&bank, ticks_per_slot - 1),
            (ticks_per_slot - 1, 0, 0)
        );

        // Cross a slot boundary.
        assert_eq!(register_ticks(&bank, 1), (0, 1, 0));

        // Cross an epoch boundary.
        assert_eq!(register_ticks(&bank, ticks_per_epoch), (0, 1, 1));
    }

    #[test]
    fn test_interleaving_locks() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(3);
        let bank = Bank::new(&genesis_block);
        let alice = Keypair::new();
        let bob = Keypair::new();

        let tx1 = SystemTransaction::new_account(
            &mint_keypair,
            alice.pubkey(),
            1,
            genesis_block.hash(),
            0,
        );
        let pay_alice = vec![tx1];

        let lock_result = bank.lock_accounts(&pay_alice);
        let results_alice = bank.load_execute_and_commit_transactions(
            &pay_alice,
            lock_result,
            MAX_RECENT_BLOCKHASHES,
        );
        assert_eq!(results_alice[0], Ok(()));

        // try executing an interleaved transfer twice
        assert_eq!(
            bank.transfer(1, &mint_keypair, bob.pubkey(), genesis_block.hash()),
            Err(BankError::AccountInUse)
        );
        // the second time should fail as well
        // this verifies that `unlock_accounts` doesn't unlock `AccountInUse` accounts
        assert_eq!(
            bank.transfer(1, &mint_keypair, bob.pubkey(), genesis_block.hash()),
            Err(BankError::AccountInUse)
        );

        bank.unlock_accounts(&pay_alice, &results_alice);

        assert!(bank
            .transfer(2, &mint_keypair, bob.pubkey(), genesis_block.hash())
            .is_ok());
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

        assert_eq!(system_program::id(), system);
        assert_eq!(native_loader::id(), native);
        assert_eq!(bpf_loader::id(), bpf);
        assert_eq!(solana_budget_api::id(), budget);
        assert_eq!(storage_program::id(), storage);
        assert_eq!(token_program::id(), token);
        assert_eq!(solana_vote_api::id(), vote);
    }

    #[test]
    fn test_program_id_uniqueness() {
        let mut unique = HashSet::new();
        let ids = vec![
            system_program::id(),
            native_loader::id(),
            bpf_loader::id(),
            solana_budget_api::id(),
            storage_program::id(),
            token_program::id(),
            solana_vote_api::id(),
        ];
        assert!(ids.into_iter().all(move |id| unique.insert(id)));
    }

    #[test]
    fn test_bank_pay_to_self() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(1);
        let key1 = Keypair::new();
        let bank = Bank::new(&genesis_block);

        bank.transfer(1, &mint_keypair, key1.pubkey(), genesis_block.hash())
            .unwrap();
        assert_eq!(bank.get_balance(&key1.pubkey()), 1);
        let tx = SystemTransaction::new_move(&key1, key1.pubkey(), 1, genesis_block.hash(), 0);
        let res = bank.process_transactions(&vec![tx.clone()]);
        assert_eq!(res.len(), 1);
        assert_eq!(bank.get_balance(&key1.pubkey()), 1);
        res[0].clone().unwrap_err();
    }

    fn new_from_parent(parent: &Arc<Bank>) -> Bank {
        Bank::new_from_parent(parent, Pubkey::default(), parent.slot() + 1)
    }

    /// Verify that the parent's vector is computed correctly
    #[test]
    fn test_bank_parents() {
        let (genesis_block, _) = GenesisBlock::new(1);
        let parent = Arc::new(Bank::new(&genesis_block));

        let bank = new_from_parent(&parent);
        assert!(Arc::ptr_eq(&bank.parents()[0], &parent));
    }

    /// Verifies that last ids and status cache are correctly referenced from parent
    #[test]
    fn test_bank_parent_duplicate_signature() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(2);
        let key1 = Keypair::new();
        let parent = Arc::new(Bank::new(&genesis_block));

        let tx =
            SystemTransaction::new_move(&mint_keypair, key1.pubkey(), 1, genesis_block.hash(), 0);
        assert_eq!(parent.process_transaction(&tx), Ok(()));
        let bank = new_from_parent(&parent);
        assert_eq!(
            bank.process_transaction(&tx),
            Err(BankError::DuplicateSignature)
        );
    }

    /// Verifies that last ids and accounts are correctly referenced from parent
    #[test]
    fn test_bank_parent_account_spend() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(2);
        let key1 = Keypair::new();
        let key2 = Keypair::new();
        let parent = Arc::new(Bank::new(&genesis_block));

        let tx =
            SystemTransaction::new_move(&mint_keypair, key1.pubkey(), 1, genesis_block.hash(), 0);
        assert_eq!(parent.process_transaction(&tx), Ok(()));
        let bank = new_from_parent(&parent);
        let tx = SystemTransaction::new_move(&key1, key2.pubkey(), 1, genesis_block.hash(), 0);
        assert_eq!(bank.process_transaction(&tx), Ok(()));
        assert_eq!(parent.get_signature_status(&tx.signatures[0]), None);
    }

    #[test]
    fn test_bank_hash_internal_state() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(2_000);
        let bank0 = Bank::new(&genesis_block);
        let bank1 = Bank::new(&genesis_block);
        let initial_state = bank0.hash_internal_state();
        assert_eq!(bank1.hash_internal_state(), initial_state);

        let pubkey = Keypair::new().pubkey();
        bank0
            .transfer(1_000, &mint_keypair, pubkey, bank0.last_blockhash())
            .unwrap();
        assert_ne!(bank0.hash_internal_state(), initial_state);
        bank1
            .transfer(1_000, &mint_keypair, pubkey, bank1.last_blockhash())
            .unwrap();
        assert_eq!(bank0.hash_internal_state(), bank1.hash_internal_state());

        // Checkpointing should not change its state
        let bank2 = new_from_parent(&Arc::new(bank1));
        assert_eq!(bank0.hash_internal_state(), bank2.hash_internal_state());
    }

    #[test]
    fn test_hash_internal_state_genesis() {
        let bank0 = Bank::new(&GenesisBlock::new(10).0);
        let bank1 = Bank::new(&GenesisBlock::new(20).0);
        assert_ne!(bank0.hash_internal_state(), bank1.hash_internal_state());
    }

    #[test]
    fn test_bank_hash_internal_state_squash() {
        let collector_id = Pubkey::default();
        let bank0 = Arc::new(Bank::new(&GenesisBlock::new(10).0));
        let bank1 = Bank::new_from_parent(&bank0, collector_id, 1);

        // no delta in bank1, hashes match
        assert_eq!(bank0.hash_internal_state(), bank1.hash_internal_state());

        // remove parent
        bank1.squash();
        assert!(bank1.parents().is_empty());

        // hash should still match
        assert_eq!(bank0.hash(), bank1.hash());
    }

    /// Verifies that last ids and accounts are correctly referenced from parent
    #[test]
    fn test_bank_squash() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(2);
        let key1 = Keypair::new();
        let key2 = Keypair::new();
        let parent = Arc::new(Bank::new(&genesis_block));

        let tx_move_mint_to_1 =
            SystemTransaction::new_move(&mint_keypair, key1.pubkey(), 1, genesis_block.hash(), 0);
        assert_eq!(parent.process_transaction(&tx_move_mint_to_1), Ok(()));
        assert_eq!(parent.transaction_count(), 1);

        let bank = new_from_parent(&parent);
        assert_eq!(bank.transaction_count(), 0);
        let tx_move_1_to_2 =
            SystemTransaction::new_move(&key1, key2.pubkey(), 1, genesis_block.hash(), 0);
        assert_eq!(bank.process_transaction(&tx_move_1_to_2), Ok(()));
        assert_eq!(bank.transaction_count(), 1);
        assert_eq!(
            parent.get_signature_status(&tx_move_1_to_2.signatures[0]),
            None
        );

        for _ in 0..3 {
            // first time these should match what happened above, assert that parents are ok
            assert_eq!(bank.get_balance(&key1.pubkey()), 0);
            assert_eq!(bank.get_account(&key1.pubkey()), None);
            assert_eq!(bank.get_balance(&key2.pubkey()), 1);
            assert_eq!(
                bank.get_signature_status(&tx_move_mint_to_1.signatures[0]),
                Some(Ok(()))
            );
            assert_eq!(
                bank.get_signature_status(&tx_move_1_to_2.signatures[0]),
                Some(Ok(()))
            );

            // works iteration 0, no-ops on iteration 1 and 2
            bank.squash();

            assert_eq!(parent.transaction_count(), 1);
            assert_eq!(bank.transaction_count(), 2);
        }
    }

    #[test]
    fn test_bank_get_account_in_parent_after_squash() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(500);
        let parent = Arc::new(Bank::new(&genesis_block));

        let key1 = Keypair::new();

        parent
            .transfer(1, &mint_keypair, key1.pubkey(), genesis_block.hash())
            .unwrap();
        assert_eq!(parent.get_balance(&key1.pubkey()), 1);
        let bank = new_from_parent(&parent);
        bank.squash();
        assert_eq!(parent.get_balance(&key1.pubkey()), 1);
    }

    #[test]
    fn test_bank_epoch_vote_accounts() {
        let leader_id = Keypair::new().pubkey();
        let leader_tokens = 3;
        let (mut genesis_block, _) = GenesisBlock::new_with_leader(5, leader_id, leader_tokens);

        // set this up weird, forces:
        //  1. genesis bank to cover epochs 0, 1, *and* 2
        //  2. child banks to cover epochs in their future
        //
        const SLOTS_PER_EPOCH: u64 = 8;
        const STAKERS_SLOT_OFFSET: u64 = 21;
        genesis_block.slots_per_epoch = SLOTS_PER_EPOCH;
        genesis_block.stakers_slot_offset = STAKERS_SLOT_OFFSET;

        let parent = Arc::new(Bank::new(&genesis_block));

        let vote_accounts0: Option<HashMap<_, _>> = parent.epoch_vote_accounts(0).map(|accounts| {
            accounts
                .iter()
                .filter_map(|(pubkey, account)| {
                    if let Ok(vote_state) = VoteState::deserialize(&account.userdata) {
                        if vote_state.delegate_id == leader_id {
                            Some((*pubkey, true))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect()
        });
        assert!(vote_accounts0.is_some());
        assert!(vote_accounts0.iter().len() != 0);

        let mut i = 1;
        loop {
            if i > STAKERS_SLOT_OFFSET / SLOTS_PER_EPOCH {
                break;
            }
            assert!(parent.epoch_vote_accounts(i).is_some());
            i += 1;
        }

        // child crosses epoch boundary and is the first slot in the epoch
        let child = Bank::new_from_parent(
            &parent,
            leader_id,
            SLOTS_PER_EPOCH - (STAKERS_SLOT_OFFSET % SLOTS_PER_EPOCH),
        );

        assert!(child.epoch_vote_accounts(i).is_some());

        // child crosses epoch boundary but isn't the first slot in the epoch
        let child = Bank::new_from_parent(
            &parent,
            leader_id,
            SLOTS_PER_EPOCH - (STAKERS_SLOT_OFFSET % SLOTS_PER_EPOCH) + 1,
        );
        assert!(child.epoch_vote_accounts(i).is_some());
    }

    #[test]
    fn test_zero_signatures() {
        solana_logger::setup();
        let (genesis_block, mint_keypair) = GenesisBlock::new(500);
        let bank = Arc::new(Bank::new(&genesis_block));
        let key = Keypair::new();

        let move_tokens = SystemInstruction::Move { tokens: 1 };

        let mut tx = Transaction::new_unsigned(
            &mint_keypair.pubkey(),
            &[key.pubkey()],
            system_program::id(),
            &move_tokens,
            bank.last_blockhash(),
            2,
        );

        assert_eq!(
            bank.process_transaction(&tx),
            Err(BankError::MissingSignatureForFee)
        );

        // Set the fee to 0, this should give a ProgramError
        // but since no signature we cannot look up the error.
        tx.fee = 0;

        assert_eq!(bank.process_transaction(&tx), Ok(()));
        assert_eq!(bank.get_balance(&key.pubkey()), 0);
    }

}

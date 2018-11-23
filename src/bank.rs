//! The `bank` module tracks client accounts and the progress of smart
//! contracts. It offers a high-level API that signs transactions
//! on behalf of the caller, and a low-level API for when they have
//! already been signed and verified.

use bincode::deserialize;
use bincode::serialize;
use bpf_loader;
use budget_program;
use counter::Counter;
use entry::Entry;
use itertools::Itertools;
use jsonrpc_macros::pubsub::Sink;
use leader_scheduler::LeaderScheduler;
use ledger::Block;
use log::Level;
use mint::Mint;
use native_loader;
use payment_plan::Payment;
use poh_recorder::PohRecorder;
use poh_service::NUM_TICKS_PER_SECOND;
use rayon::prelude::*;
use rpc::RpcSignatureStatus;
use signature::Keypair;
use signature::Signature;
use solana_sdk::account::{create_keyed_accounts, Account, KeyedAccount};
use solana_sdk::hash::{hash, Hash};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::system_instruction::SystemInstruction;
use solana_sdk::timing::{duration_as_us, timestamp};
use std;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::result;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;
use storage_program;
use system_program;
use system_transaction::SystemTransaction;
use token_program;
use tokio::prelude::Future;
use transaction::Transaction;
use vote_program;

/// The number of most recent `last_id` values that the bank will track the signatures
/// of. Once the bank discards a `last_id`, it will reject any transactions that use
/// that `last_id` in a transaction. Lowering this value reduces memory consumption,
/// but requires clients to update its `last_id` more frequently. Raising the value
/// lengthens the time a client must wait to be certain a missing transaction will
/// not be processed by the network.
pub const MAX_ENTRY_IDS: usize = NUM_TICKS_PER_SECOND * 120;

pub const VERIFY_BLOCK_SIZE: usize = 16;

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

    /// The bank has not seen a transaction with the given `Signature` or the transaction is
    /// too old and has been discarded.
    SignatureNotFound,

    /// A transaction with this signature has been received but not yet executed
    SignatureReserved,

    /// Proof of History verification failed.
    LedgerVerificationFailed,

    /// Contract's instruction token balance does not equal the balance after the instruction
    UnbalancedInstruction(u8),

    /// Contract's transactions resulted in an account with a negative balance
    /// The difference from InsufficientFundsForFee is that the transaction was executed by the
    /// contract
    ResultWithNegativeTokens(u8),

    /// Contract id is unknown
    UnknownContractId(u8),

    /// Contract modified an accounts contract id
    ModifiedContractId(u8),

    /// Contract spent the tokens of an account that doesn't belong to it
    ExternalAccountTokenSpend(u8),

    /// The program returned an error
    ProgramRuntimeError(u8),

    /// Recoding into PoH failed
    RecordFailure,

    /// Loader call chain too deep
    CallChainTooDeep,

    /// Transaction has a fee but has no signature present
    MissingSignatureForFee,
}

pub type Result<T> = result::Result<T, BankError>;
type SignatureStatusMap = HashMap<Signature, Result<()>>;

#[derive(Default)]
struct ErrorCounters {
    account_not_found: usize,
    account_in_use: usize,
    last_id_not_found: usize,
    reserve_last_id: usize,
    insufficient_funds: usize,
    duplicate_signature: usize,
}

pub trait Checkpoint {
    /// add a checkpoint to this data at current state
    fn checkpoint(&mut self);

    /// rollback to previous state, panics if no prior checkpoint
    fn rollback(&mut self);

    /// cull checkpoints to depth, that is depth of zero means
    ///  no checkpoints, only current state
    fn purge(&mut self, depth: usize);

    /// returns the number of checkpoints
    fn depth(&self) -> usize;
}

/// a record of a tick, from register_tick
#[derive(Clone)]
pub struct LastIdEntry {
    /// when the id was registered, according to network time
    tick_height: u64,

    /// timestamp when this id was registered, used for stats/finality
    timestamp: u64,

    /// a map of signature status, used for duplicate detection
    signature_status: SignatureStatusMap,
}

pub struct LastIds {
    /// A FIFO queue of `last_id` items, where each item is a set of signatures
    /// that have been processed using that `last_id`. Rejected `last_id`
    /// values are so old that the `last_id` has been pulled out of the queue.

    /// updated whenever an id is registered, at each tick ;)
    tick_height: u64,

    /// last tick to be registered
    last_id: Option<Hash>,

    /// Mapping of hashes to signature sets along with timestamp and what tick_height
    /// was when the id was added. The bank uses this data to
    /// reject transactions with signatures it's seen before and to reject
    /// transactions that are too old (nth is too small)
    entries: HashMap<Hash, LastIdEntry>,

    checkpoints: VecDeque<(u64, Option<Hash>, HashMap<Hash, LastIdEntry>)>,
}

impl Default for LastIds {
    fn default() -> Self {
        LastIds {
            tick_height: 0,
            last_id: None,
            entries: HashMap::new(),
            checkpoints: VecDeque::new(),
        }
    }
}

impl Checkpoint for LastIds {
    fn checkpoint(&mut self) {
        self.checkpoints
            .push_front((self.tick_height, self.last_id, self.entries.clone()));
    }
    fn rollback(&mut self) {
        let (tick_height, last_id, entries) = self.checkpoints.pop_front().unwrap();
        self.tick_height = tick_height;
        self.last_id = last_id;
        self.entries = entries;
    }
    fn purge(&mut self, depth: usize) {
        while self.depth() > depth {
            self.checkpoints.pop_back().unwrap();
        }
    }
    fn depth(&self) -> usize {
        self.checkpoints.len()
    }
}

#[derive(Default)]
pub struct Accounts {
    // TODO: implement values() or something? take this back to private
    //  from the voting/leader/finality code
    //  issue #1701
    pub accounts: HashMap<Pubkey, Account>,

    /// The number of transactions the bank has processed without error since the
    /// start of the ledger.
    transaction_count: u64,

    /// list of prior states
    checkpoints: VecDeque<(HashMap<Pubkey, Account>, u64)>,
}

impl Accounts {
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

    fn store(&mut self, pubkey: &Pubkey, account: &Account) {
        // purge if balance is 0 and no checkpoints
        if account.tokens == 0 && self.checkpoints.is_empty() {
            self.accounts.remove(pubkey);
        } else {
            self.accounts.insert(pubkey.clone(), account.clone());
        }
    }

    fn increment_transaction_count(&mut self, tx_count: usize) {
        self.transaction_count += tx_count as u64;
    }
    fn transaction_count(&self) -> u64 {
        self.transaction_count
    }
}

impl Checkpoint for Accounts {
    fn checkpoint(&mut self) {
        let mut accounts = HashMap::new();
        std::mem::swap(&mut self.accounts, &mut accounts);

        self.checkpoints
            .push_front((accounts, self.transaction_count));
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

/// Manager for the state of all accounts and contracts after processing its entries.
pub struct Bank {
    /// A map of account public keys to the balance in that account.
    pub accounts: RwLock<Accounts>,

    /// FIFO queue of `last_id` items
    last_ids: RwLock<LastIds>,

    /// set of accounts which are currently in the pipeline
    account_locks: Mutex<HashSet<Pubkey>>,

    // The latest finality time for the network
    finality_time: AtomicUsize,

    // Mapping of account ids to Subscriber ids and sinks to notify on userdata update
    account_subscriptions: RwLock<HashMap<Pubkey, HashMap<Pubkey, Sink<Account>>>>,

    // Mapping of signatures to Subscriber ids and sinks to notify on confirmation
    signature_subscriptions: RwLock<HashMap<Signature, HashMap<Pubkey, Sink<RpcSignatureStatus>>>>,

    /// Tracks and updates the leader schedule based on the votes and account stakes
    /// processed by the bank
    pub leader_scheduler: Arc<RwLock<LeaderScheduler>>,
}

impl Default for Bank {
    fn default() -> Self {
        Bank {
            accounts: RwLock::new(Accounts::default()),
            last_ids: RwLock::new(LastIds::default()),
            account_locks: Mutex::new(HashSet::new()),
            finality_time: AtomicUsize::new(std::usize::MAX),
            account_subscriptions: RwLock::new(HashMap::new()),
            signature_subscriptions: RwLock::new(HashMap::new()),
            leader_scheduler: Arc::new(RwLock::new(LeaderScheduler::default())),
        }
    }
}

impl Bank {
    /// Create an Bank with built-in programs.
    pub fn new_with_builtin_programs() -> Self {
        let bank = Self::default();
        bank.add_builtin_programs();
        bank
    }

    /// Create an Bank using a deposit.
    pub fn new_from_deposits(deposits: &[Payment]) -> Self {
        let bank = Self::default();
        for deposit in deposits {
            let mut accounts = bank.accounts.write().unwrap();

            let mut account = Account::default();
            account.tokens += deposit.tokens;

            accounts.store(&deposit.to, &account);
        }
        bank.add_builtin_programs();
        bank
    }

    pub fn checkpoint(&self) {
        self.accounts.write().unwrap().checkpoint();
        self.last_ids.write().unwrap().checkpoint();
    }
    pub fn purge(&self, depth: usize) {
        self.accounts.write().unwrap().purge(depth);
        self.last_ids.write().unwrap().purge(depth);
    }

    pub fn rollback(&self) {
        let rolled_back_pubkeys: Vec<Pubkey> = self
            .accounts
            .read()
            .unwrap()
            .accounts
            .keys()
            .cloned()
            .collect();

        self.accounts.write().unwrap().rollback();

        rolled_back_pubkeys.iter().for_each(|pubkey| {
            if let Some(account) = self.accounts.read().unwrap().load(&pubkey) {
                self.check_account_subscriptions(&pubkey, account)
            }
        });

        self.last_ids.write().unwrap().rollback();
    }
    pub fn checkpoint_depth(&self) -> usize {
        self.accounts.read().unwrap().depth()
    }

    /// Create an Bank with only a Mint. Typically used by unit tests.
    pub fn new(mint: &Mint) -> Self {
        let mint_tokens = if mint.bootstrap_leader_id != Pubkey::default() {
            mint.tokens - mint.bootstrap_leader_tokens
        } else {
            mint.tokens
        };

        let mint_deposit = Payment {
            to: mint.pubkey(),
            tokens: mint_tokens,
        };

        let deposits = if mint.bootstrap_leader_id != Pubkey::default() {
            let leader_deposit = Payment {
                to: mint.bootstrap_leader_id,
                tokens: mint.bootstrap_leader_tokens,
            };
            vec![mint_deposit, leader_deposit]
        } else {
            vec![mint_deposit]
        };
        let bank = Self::new_from_deposits(&deposits);
        bank.register_tick(&mint.last_id());
        bank
    }

    fn add_builtin_programs(&self) {
        let mut accounts = self.accounts.write().unwrap();

        // Preload Bpf Loader account
        accounts.store(&bpf_loader::id(), &bpf_loader::account());

        // Preload Erc20 token program
        accounts.store(&token_program::id(), &token_program::account());
    }

    /// Return the last entry ID registered.
    pub fn last_id(&self) -> Hash {
        self.last_ids
            .read()
            .unwrap()
            .last_id
            .expect("no last_id has been set")
    }

    /// Store the given signature. The bank will reject any transaction with the same signature.
    fn reserve_signature(signatures: &mut SignatureStatusMap, signature: &Signature) -> Result<()> {
        if let Some(_result) = signatures.get(signature) {
            return Err(BankError::DuplicateSignature);
        }
        signatures.insert(*signature, Err(BankError::SignatureReserved));
        Ok(())
    }

    /// Forget all signatures. Useful for benchmarking.
    pub fn clear_signatures(&self) {
        for entry in &mut self.last_ids.write().unwrap().entries.values_mut() {
            entry.signature_status.clear();
        }
    }

    /// Check if the age of the entry_id is within the max_age
    /// return false for any entries with an age equal to or above max_age
    fn check_entry_id_age(last_ids: &LastIds, entry_id: Hash, max_age: usize) -> bool {
        let entry = last_ids.entries.get(&entry_id);

        match entry {
            Some(entry) => last_ids.tick_height - entry.tick_height < max_age as u64,
            _ => false,
        }
    }

    fn reserve_signature_with_last_id(
        last_ids: &mut LastIds,
        last_id: &Hash,
        sig: &Signature,
    ) -> Result<()> {
        if let Some(entry) = last_ids.entries.get_mut(last_id) {
            if last_ids.tick_height - entry.tick_height < MAX_ENTRY_IDS as u64 {
                return Self::reserve_signature(&mut entry.signature_status, sig);
            }
        }
        Err(BankError::LastIdNotFound)
    }

    #[cfg(test)]
    fn reserve_signature_with_last_id_test(&self, sig: &Signature, last_id: &Hash) -> Result<()> {
        let mut last_ids = self.last_ids.write().unwrap();
        Self::reserve_signature_with_last_id(&mut last_ids, last_id, sig)
    }

    fn update_signature_status_with_last_id(
        last_ids_sigs: &mut HashMap<Hash, LastIdEntry>,
        signature: &Signature,
        result: &Result<()>,
        last_id: &Hash,
    ) {
        if let Some(entry) = last_ids_sigs.get_mut(last_id) {
            entry.signature_status.insert(*signature, result.clone());
        }
    }

    fn update_transaction_statuses(&self, txs: &[Transaction], res: &[Result<()>]) {
        let mut last_ids = self.last_ids.write().unwrap();
        for (i, tx) in txs.iter().enumerate() {
            Self::update_signature_status_with_last_id(
                &mut last_ids.entries,
                &tx.signatures[0],
                &res[i],
                &tx.last_id,
            );
            if res[i] != Err(BankError::SignatureNotFound) {
                let status = match res[i] {
                    Ok(_) => RpcSignatureStatus::Confirmed,
                    Err(BankError::AccountInUse) => RpcSignatureStatus::AccountInUse,
                    Err(BankError::ProgramRuntimeError(_)) => {
                        RpcSignatureStatus::ProgramRuntimeError
                    }
                    Err(_) => RpcSignatureStatus::GenericFailure,
                };
                if status != RpcSignatureStatus::SignatureNotFound {
                    self.check_signature_subscriptions(&tx.signatures[0], status);
                }
            }
        }
    }

    /// Maps a tick height to a timestamp
    fn tick_height_to_timestamp(last_ids: &LastIds, tick_height: u64) -> Option<u64> {
        for entry in last_ids.entries.values() {
            if entry.tick_height == tick_height {
                return Some(entry.timestamp);
            }
        }
        None
    }

    /// Look through the last_ids and find all the valid ids
    /// This is batched to avoid holding the lock for a significant amount of time
    ///
    /// Return a vec of tuple of (valid index, timestamp)
    /// index is into the passed ids slice to avoid copying hashes
    pub fn count_valid_ids(&self, ids: &[Hash]) -> Vec<(usize, u64)> {
        let last_ids = self.last_ids.read().unwrap();
        let mut ret = Vec::new();
        for (i, id) in ids.iter().enumerate() {
            if let Some(entry) = last_ids.entries.get(id) {
                if last_ids.tick_height - entry.tick_height < MAX_ENTRY_IDS as u64 {
                    ret.push((i, entry.timestamp));
                }
            }
        }
        ret
    }

    /// Looks through a list of tick heights and stakes, and finds the latest
    /// tick that has achieved finality
    pub fn get_finality_timestamp(
        &self,
        ticks_and_stakes: &mut [(u64, u64)],
        supermajority_stake: u64,
    ) -> Option<u64> {
        // Sort by tick height
        ticks_and_stakes.sort_by(|a, b| a.0.cmp(&b.0));
        let last_ids = self.last_ids.read().unwrap();
        let current_tick_height = last_ids.tick_height;
        let mut total = 0;
        for (tick_height, stake) in ticks_and_stakes.iter() {
            if ((current_tick_height - tick_height) as usize) < MAX_ENTRY_IDS {
                total += stake;
                if total > supermajority_stake {
                    return Self::tick_height_to_timestamp(&last_ids, *tick_height);
                }
            }
        }
        None
    }

    /// Tell the bank which Entry IDs exist on the ledger. This function
    /// assumes subsequent calls correspond to later entries, and will boot
    /// the oldest ones once its internal cache is full. Once boot, the
    /// bank will reject transactions using that `last_id`.
    pub fn register_tick(&self, last_id: &Hash) {
        let mut last_ids = self.last_ids.write().unwrap();

        last_ids.tick_height += 1;
        let tick_height = last_ids.tick_height;

        // this clean up can be deferred until sigs gets larger
        //  because we verify entry.nth every place we check for validity
        if last_ids.entries.len() >= MAX_ENTRY_IDS as usize {
            last_ids
                .entries
                .retain(|_, entry| tick_height - entry.tick_height <= MAX_ENTRY_IDS as u64);
        }

        last_ids.entries.insert(
            *last_id,
            LastIdEntry {
                tick_height,
                timestamp: timestamp(),
                signature_status: HashMap::new(),
            },
        );

        last_ids.last_id = Some(*last_id);

        inc_new_counter_info!("bank-register_tick-registered", 1);
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

    fn load_account(
        &self,
        tx: &Transaction,
        accounts: &Accounts,
        last_ids: &mut LastIds,
        max_age: usize,
        error_counters: &mut ErrorCounters,
    ) -> Result<Vec<Account>> {
        // Copy all the accounts
        if tx.signatures.is_empty() && tx.fee != 0 {
            Err(BankError::MissingSignatureForFee)
        } else if accounts.load(&tx.account_keys[0]).is_none() {
            error_counters.account_not_found += 1;
            Err(BankError::AccountNotFound)
        } else if accounts.load(&tx.account_keys[0]).unwrap().tokens < tx.fee {
            error_counters.insufficient_funds += 1;
            Err(BankError::InsufficientFundsForFee)
        } else {
            if !Self::check_entry_id_age(&last_ids, tx.last_id, max_age) {
                error_counters.last_id_not_found += 1;
                return Err(BankError::LastIdNotFound);
            }

            // There is no way to predict what contract will execute without an error
            // If a fee can pay for execution then the contract will be scheduled
            let err =
                Self::reserve_signature_with_last_id(last_ids, &tx.last_id, &tx.signatures[0]);
            if let Err(BankError::LastIdNotFound) = err {
                error_counters.reserve_last_id += 1;
            } else if let Err(BankError::DuplicateSignature) = err {
                error_counters.duplicate_signature += 1;
            }
            err?;

            let mut called_accounts: Vec<Account> = tx
                .account_keys
                .iter()
                .map(|key| accounts.load(key).cloned().unwrap_or_default())
                .collect();
            called_accounts[0].tokens -= tx.fee;
            Ok(called_accounts)
        }
    }

    /// This function will prevent multiple threads from modifying the same account state at the
    /// same time
    #[must_use]
    fn lock_accounts(&self, txs: &[Transaction]) -> Vec<Result<()>> {
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
    fn unlock_accounts(&self, txs: &[Transaction], results: &[Result<()>]) {
        debug!("bank unlock accounts");
        let mut account_locks = self.account_locks.lock().unwrap();
        txs.iter()
            .zip(results.iter())
            .for_each(|(tx, result)| Self::unlock_account(tx, result, &mut account_locks));
    }

    fn load_accounts(
        &self,
        txs: &[Transaction],
        results: Vec<Result<()>>,
        max_age: usize,
        error_counters: &mut ErrorCounters,
    ) -> Vec<(Result<Vec<Account>>)> {
        let accounts = self.accounts.read().unwrap();
        let mut last_ids = self.last_ids.write().unwrap();
        txs.iter()
            .zip(results.into_iter())
            .map(|etx| match etx {
                (tx, Ok(())) => {
                    self.load_account(tx, &accounts, &mut last_ids, max_age, error_counters)
                }
                (_, Err(e)) => Err(e),
            }).collect()
    }

    pub fn verify_instruction(
        instruction_index: usize,
        program_id: &Pubkey,
        pre_program_id: &Pubkey,
        pre_tokens: u64,
        account: &Account,
    ) -> Result<()> {
        // Verify the transaction

        // Make sure that program_id is still the same or this was just assigned by the system call contract
        if *pre_program_id != account.owner && !system_program::check_id(&program_id) {
            return Err(BankError::ModifiedContractId(instruction_index as u8));
        }
        // For accounts unassigned to the contract, the individual balance of each accounts cannot decrease.
        if *program_id != account.owner && pre_tokens > account.tokens {
            return Err(BankError::ExternalAccountTokenSpend(
                instruction_index as u8,
            ));
        }
        Ok(())
    }

    /// Execute a function with a subset of accounts as writable references.
    /// Since the subset can point to the same references, in any order there is no way
    /// for the borrow checker to track them with regards to the original set.
    fn with_subset<F, A>(accounts: &mut [Account], ixes: &[u8], func: F) -> A
    where
        F: Fn(&mut [&mut Account]) -> A,
    {
        let mut subset: Vec<&mut Account> = ixes
            .iter()
            .map(|ix| {
                let ptr = &mut accounts[*ix as usize] as *mut Account;
                // lifetime of this unsafe is only within the scope of the closure
                // there is no way to reorder them without breaking borrow checker rules
                unsafe { &mut *ptr }
            }).collect();
        func(&mut subset)
    }

    fn load_executable_accounts(&self, mut program_id: Pubkey) -> Result<Vec<(Pubkey, Account)>> {
        let mut accounts = Vec::new();
        let mut depth = 0;
        loop {
            if native_loader::check_id(&program_id) {
                // at the root of the chain, ready to dispatch
                break;
            }

            if depth >= 5 {
                return Err(BankError::CallChainTooDeep);
            }
            depth += 1;

            let program = match self.get_account(&program_id) {
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

    /// Execute an instruction
    /// This method calls the instruction's program entry pont method and verifies that the result of
    /// the call does not violate the bank's accounting rules.
    /// The accounts are committed back to the bank only if this function returns Ok(_).
    fn execute_instruction(
        &self,
        tx: &Transaction,
        instruction_index: usize,
        program_accounts: &mut [&mut Account],
    ) -> Result<()> {
        let program_id = tx.program_id(instruction_index);
        // TODO: the runtime should be checking read/write access to memory
        // we are trusting the hard coded contracts not to clobber or allocate
        let pre_total: u64 = program_accounts.iter().map(|a| a.tokens).sum();
        let pre_data: Vec<_> = program_accounts
            .iter_mut()
            .map(|a| (a.owner, a.tokens))
            .collect();

        // Call the contract method
        // It's up to the contract to implement its own rules on moving funds
        if system_program::check_id(&program_id) {
            if let Err(err) =
                system_program::process_instruction(&tx, instruction_index, program_accounts)
            {
                let err = match err {
                    system_program::Error::ResultWithNegativeTokens => {
                        BankError::ResultWithNegativeTokens(instruction_index as u8)
                    }
                    _ => BankError::ProgramRuntimeError(instruction_index as u8),
                };
                return Err(err);
            }
        } else if budget_program::check_id(&program_id) {
            if budget_program::process_instruction(&tx, instruction_index, program_accounts)
                .is_err()
            {
                return Err(BankError::ProgramRuntimeError(instruction_index as u8));
            }
        } else if storage_program::check_id(&program_id) {
            if storage_program::process_instruction(&tx, instruction_index, program_accounts)
                .is_err()
            {
                return Err(BankError::ProgramRuntimeError(instruction_index as u8));
            }
        } else if vote_program::check_id(&program_id) {
            if vote_program::process_instruction(&tx, instruction_index, program_accounts).is_err()
            {
                return Err(BankError::ProgramRuntimeError(instruction_index as u8));
            }
        } else {
            let mut accounts = self.load_executable_accounts(tx.program_ids[instruction_index])?;
            let mut keyed_accounts = create_keyed_accounts(&mut accounts);
            let mut keyed_accounts2: Vec<_> = tx.instructions[instruction_index]
                .accounts
                .iter()
                .map(|&index| &tx.account_keys[index as usize])
                .zip(program_accounts.iter_mut())
                .map(|(key, account)| KeyedAccount { key, account })
                .collect();
            keyed_accounts.append(&mut keyed_accounts2);

            if !native_loader::process_instruction(
                &program_id,
                &mut keyed_accounts,
                &tx.instructions[instruction_index].userdata,
                self.tick_height(),
            ) {
                return Err(BankError::ProgramRuntimeError(instruction_index as u8));
            }
        }

        // Verify the instruction
        for ((pre_program_id, pre_tokens), post_account) in
            pre_data.iter().zip(program_accounts.iter())
        {
            Self::verify_instruction(
                instruction_index,
                &program_id,
                pre_program_id,
                *pre_tokens,
                post_account,
            )?;
        }
        // The total sum of all the tokens in all the accounts cannot change.
        let post_total: u64 = program_accounts.iter().map(|a| a.tokens).sum();
        if pre_total != post_total {
            Err(BankError::UnbalancedInstruction(instruction_index as u8))
        } else {
            Ok(())
        }
    }

    /// Execute a transaction.
    /// This method calls each instruction in the transaction over the set of loaded Accounts
    /// The accounts are committed back to the bank only if every instruction succeeds
    fn execute_transaction(&self, tx: &Transaction, tx_accounts: &mut [Account]) -> Result<()> {
        for (instruction_index, instruction) in tx.instructions.iter().enumerate() {
            Self::with_subset(tx_accounts, &instruction.accounts, |program_accounts| {
                self.execute_instruction(tx, instruction_index, program_accounts)
            })?;
        }
        Ok(())
    }

    pub fn store_accounts(
        &self,
        txs: &[Transaction],
        res: &[Result<()>],
        loaded: &[Result<Vec<Account>>],
    ) {
        let mut accounts = self.accounts.write().unwrap();
        for (i, racc) in loaded.iter().enumerate() {
            if res[i].is_err() || racc.is_err() {
                continue;
            }

            let tx = &txs[i];
            let acc = racc.as_ref().unwrap();
            for (key, account) in tx.account_keys.iter().zip(acc.iter()) {
                accounts.store(key, account);
            }
        }
    }

    pub fn process_and_record_transactions(
        &self,
        txs: &[Transaction],
        poh: &PohRecorder,
    ) -> Result<()> {
        let now = Instant::now();
        // Once accounts are locked, other threads cannot encode transactions that will modify the
        // same account state
        let locked_accounts = self.lock_accounts(txs);
        let lock_time = now.elapsed();
        let now = Instant::now();
        // Use a shorter maximum age when adding transactions into the pipeline.  This will reduce
        // the likelyhood of any single thread getting starved and processing old ids.
        // TODO: Banking stage threads should be prioritized to complete faster then this queue
        // expires.
        let results =
            self.execute_and_commit_transactions(txs, locked_accounts, MAX_ENTRY_IDS as usize / 2);
        let process_time = now.elapsed();
        let now = Instant::now();
        self.record_transactions(txs, &results, poh)?;
        let record_time = now.elapsed();
        let now = Instant::now();
        // Once the accounts are unlocked new transactions can enter the pipeline to process them
        self.unlock_accounts(&txs, &results);
        let unlock_time = now.elapsed();
        debug!(
            "lock: {}us process: {}us record: {}us unlock: {}us txs_len={}",
            duration_as_us(&lock_time),
            duration_as_us(&process_time),
            duration_as_us(&record_time),
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
                Err(ref e) => {
                    debug!("process transaction failed {:?}", e);
                    None
                }
            }).collect();
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

    /// Process a batch of transactions.
    #[must_use]
    pub fn execute_and_commit_transactions(
        &self,
        txs: &[Transaction],
        locked_accounts: Vec<Result<()>>,
        max_age: usize,
    ) -> Vec<Result<()>> {
        debug!("processing transactions: {}", txs.len());
        let mut error_counters = ErrorCounters::default();
        let now = Instant::now();
        let mut loaded_accounts =
            self.load_accounts(txs, locked_accounts.clone(), max_age, &mut error_counters);

        let load_elapsed = now.elapsed();
        let now = Instant::now();
        let executed: Vec<Result<()>> = loaded_accounts
            .iter_mut()
            .zip(txs.iter())
            .map(|(acc, tx)| match acc {
                Err(e) => Err(e.clone()),
                Ok(ref mut accounts) => self.execute_transaction(tx, accounts),
            }).collect();
        let execution_elapsed = now.elapsed();
        let now = Instant::now();
        self.store_accounts(txs, &executed, &loaded_accounts);

        // Check account subscriptions and send notifications
        self.send_account_notifications(txs, locked_accounts);

        // once committed there is no way to unroll
        let write_elapsed = now.elapsed();
        debug!(
            "load: {}us execute: {}us store: {}us txs_len={}",
            duration_as_us(&load_elapsed),
            duration_as_us(&execution_elapsed),
            duration_as_us(&write_elapsed),
            txs.len(),
        );
        self.update_transaction_statuses(txs, &executed);
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

        self.accounts
            .write()
            .unwrap()
            .increment_transaction_count(tx_count);

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

    #[must_use]
    pub fn process_transactions(&self, txs: &[Transaction]) -> Vec<Result<()>> {
        let locked_accounts = self.lock_accounts(txs);
        let results = self.execute_and_commit_transactions(txs, locked_accounts, MAX_ENTRY_IDS);
        self.unlock_accounts(txs, &results);
        results
    }

    pub fn process_entry(&self, entry: &Entry) -> Result<()> {
        if !entry.is_tick() {
            for result in self.process_transactions(&entry.transactions) {
                result?;
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
    pub fn par_execute_entries(&self, entries: &[(&Entry, Vec<Result<()>>)]) -> Result<()> {
        inc_new_counter_info!("bank-par_execute_entries-count", entries.len());
        let results: Vec<Result<()>> = entries
            .into_par_iter()
            .map(|(e, locks)| {
                let results = self.execute_and_commit_transactions(
                    &e.transactions,
                    locks.to_vec(),
                    MAX_ENTRY_IDS,
                );
                self.unlock_accounts(&e.transactions, &results);
                Self::first_err(&results)
            }).collect();
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
                mt_group = vec![];
                continue;
            }
            // try to lock the accounts
            let locked = self.lock_accounts(&entry.transactions);
            // if any of the locks error out
            // execute the current group
            if Self::first_err(&locked).is_err() {
                self.par_execute_entries(&mt_group)?;
                mt_group = vec![];
                //reset the lock and push the entry
                let locked = self.lock_accounts(&entry.transactions);
                mt_group.push((entry, locked));
            } else {
                // push the entry to the mt_group
                mt_group.push((entry, locked));
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
    fn process_ledger_blocks<I>(
        &self,
        start_hash: Hash,
        entry_height: u64,
        entries: I,
    ) -> Result<(u64, Hash)>
    where
        I: IntoIterator<Item = Entry>,
    {
        // these magic numbers are from genesis of the mint, could pull them
        //  back out of this loop.
        let mut entry_height = entry_height;
        let mut last_id = start_hash;

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

    /// Process a full ledger.
    pub fn process_ledger<I>(&self, entries: I) -> Result<(u64, Hash)>
    where
        I: IntoIterator<Item = Entry>,
    {
        let mut entries = entries.into_iter();

        // The first item in the ledger is required to be an entry with zero num_hashes,
        // which implies its id can be used as the ledger's seed.
        let entry0 = entries.next().expect("invalid ledger: empty");

        // The second item in the ledger consists of a transaction with
        // two special instructions:
        // 1) The first is a special move instruction where the to and from
        // fields are the same. That entry should be treated as a deposit, not a
        // transfer to oneself.
        // 2) The second is a move instruction that acts as a payment to the first
        // leader from the mint. This bootstrap leader will stay in power during the
        // bootstrapping period of the network
        let entry1 = entries
            .next()
            .expect("invalid ledger: need at least 2 entries");

        // genesis should conform to PoH
        assert!(entry1.verify(&entry0.id));

        {
            // Process the first transaction
            let tx = &entry1.transactions[0];
            assert!(system_program::check_id(tx.program_id(0)), "Invalid ledger");
            assert!(system_program::check_id(tx.program_id(1)), "Invalid ledger");
            let mut instruction: SystemInstruction = deserialize(tx.userdata(0)).unwrap();
            let mint_deposit = if let SystemInstruction::Move { tokens } = instruction {
                Some(tokens)
            } else {
                None
            }.expect("invalid ledger, needs to start with mint deposit");

            instruction = deserialize(tx.userdata(1)).unwrap();
            let leader_payment = if let SystemInstruction::Move { tokens } = instruction {
                Some(tokens)
            } else {
                None
            }.expect("invalid ledger, bootstrap leader payment expected");

            assert!(leader_payment <= mint_deposit);
            assert!(leader_payment > 0);

            {
                // 1) Deposit into the mint
                let mut accounts = self.accounts.write().unwrap();

                let mut account = accounts
                    .load(&tx.account_keys[0])
                    .cloned()
                    .unwrap_or_default();
                account.tokens += mint_deposit - leader_payment;
                accounts.store(&tx.account_keys[0], &account);
                trace!(
                    "applied genesis payment {:?} => {:?}",
                    mint_deposit - leader_payment,
                    account
                );

                // 2) Transfer tokens to the bootstrap leader. The first two
                // account keys will both be the mint (because the mint is the source
                // for this transaction and the first move instruction is to the the
                // mint itself), so we look at the third account key to find the first
                // leader id.
                let bootstrap_leader_id = tx.account_keys[2];
                let mut account = accounts
                    .load(&bootstrap_leader_id)
                    .cloned()
                    .unwrap_or_default();
                account.tokens += leader_payment;
                accounts.store(&bootstrap_leader_id, &account);

                self.leader_scheduler.write().unwrap().bootstrap_leader = bootstrap_leader_id;

                trace!(
                    "applied genesis payment to bootstrap leader {:?} => {:?}",
                    leader_payment,
                    account
                );
            }
        }

        Ok(self.process_ledger_blocks(entry1.id, 2, entries)?)
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
        if system_program::check_id(&account.owner) {
            system_program::get_balance(account)
        } else if budget_program::check_id(&account.owner) {
            budget_program::get_balance(account)
        } else {
            account.tokens
        }
    }
    /// Each contract would need to be able to introspect its own state
    /// this is hard coded to the budget contract language
    pub fn get_balance(&self, pubkey: &Pubkey) -> u64 {
        self.get_account(pubkey)
            .map(|x| Self::read_balance(&x))
            .unwrap_or(0)
    }

    /// TODO: Need to implement a real staking contract to hold node stake.
    /// Right now this just gets the account balances. See github issue #1655.
    pub fn get_stake(&self, pubkey: &Pubkey) -> u64 {
        self.get_balance(pubkey)
    }

    pub fn get_account(&self, pubkey: &Pubkey) -> Option<Account> {
        let accounts = self
            .accounts
            .read()
            .expect("'accounts' read lock in get_balance");
        accounts.load(pubkey).cloned()
    }

    pub fn transaction_count(&self) -> u64 {
        self.accounts.read().unwrap().transaction_count()
    }

    pub fn get_signature_status(&self, signature: &Signature) -> Result<()> {
        let last_ids = self.last_ids.read().unwrap();
        for entry in last_ids.entries.values() {
            if let Some(res) = entry.signature_status.get(signature) {
                return res.clone();
            }
        }
        Err(BankError::SignatureNotFound)
    }

    pub fn has_signature(&self, signature: &Signature) -> bool {
        self.get_signature_status(signature) != Err(BankError::SignatureNotFound)
    }

    pub fn get_signature(&self, last_id: &Hash, signature: &Signature) -> Option<Result<()>> {
        self.last_ids
            .read()
            .unwrap()
            .entries
            .get(last_id)
            .and_then(|entry| entry.signature_status.get(signature).cloned())
    }

    /// Hash the `accounts` HashMap. This represents a validator's interpretation
    ///  of the delta of the ledger since the last vote and up to now
    pub fn hash_internal_state(&self) -> Hash {
        let mut ordered_accounts = BTreeMap::new();

        // only hash internal state of the part being voted upon, i.e. since last
        //  checkpoint
        for (pubkey, account) in &self.accounts.read().unwrap().accounts {
            ordered_accounts.insert(*pubkey, account.clone());
        }

        hash(&serialize(&ordered_accounts).unwrap())
    }

    pub fn finality(&self) -> usize {
        self.finality_time.load(Ordering::Relaxed)
    }

    pub fn set_finality(&self, finality: usize) {
        self.finality_time.store(finality, Ordering::Relaxed);
    }

    fn send_account_notifications(&self, txs: &[Transaction], locked_accounts: Vec<Result<()>>) {
        let accounts = self.accounts.read().unwrap();
        txs.iter()
            .zip(locked_accounts.into_iter())
            .filter(|(_, result)| result.is_ok())
            .flat_map(|(tx, _)| &tx.account_keys)
            .for_each(|pubkey| {
                let account = accounts.load(pubkey).cloned().unwrap_or_default();
                self.check_account_subscriptions(&pubkey, &account);
            });
    }
    pub fn add_account_subscription(
        &self,
        bank_sub_id: Pubkey,
        pubkey: Pubkey,
        sink: Sink<Account>,
    ) {
        let mut subscriptions = self.account_subscriptions.write().unwrap();
        if let Some(current_hashmap) = subscriptions.get_mut(&pubkey) {
            current_hashmap.insert(bank_sub_id, sink);
            return;
        }
        let mut hashmap = HashMap::new();
        hashmap.insert(bank_sub_id, sink);
        subscriptions.insert(pubkey, hashmap);
    }

    pub fn remove_account_subscription(&self, bank_sub_id: &Pubkey, pubkey: &Pubkey) -> bool {
        let mut subscriptions = self.account_subscriptions.write().unwrap();
        match subscriptions.get_mut(pubkey) {
            Some(ref current_hashmap) if current_hashmap.len() == 1 => {}
            Some(current_hashmap) => {
                return current_hashmap.remove(bank_sub_id).is_some();
            }
            None => {
                return false;
            }
        }
        subscriptions.remove(pubkey).is_some()
    }

    pub fn get_current_leader(&self) -> Option<(Pubkey, u64)> {
        self.leader_scheduler
            .read()
            .unwrap()
            .get_scheduled_leader(self.tick_height())
    }

    pub fn tick_height(&self) -> u64 {
        self.last_ids.read().unwrap().tick_height
    }

    fn check_account_subscriptions(&self, pubkey: &Pubkey, account: &Account) {
        let subscriptions = self.account_subscriptions.read().unwrap();
        if let Some(hashmap) = subscriptions.get(pubkey) {
            for (_bank_sub_id, sink) in hashmap.iter() {
                sink.notify(Ok(account.clone())).wait().unwrap();
            }
        }
    }

    pub fn add_signature_subscription(
        &self,
        bank_sub_id: Pubkey,
        signature: Signature,
        sink: Sink<RpcSignatureStatus>,
    ) {
        let mut subscriptions = self.signature_subscriptions.write().unwrap();
        if let Some(current_hashmap) = subscriptions.get_mut(&signature) {
            current_hashmap.insert(bank_sub_id, sink);
            return;
        }
        let mut hashmap = HashMap::new();
        hashmap.insert(bank_sub_id, sink);
        subscriptions.insert(signature, hashmap);
    }

    pub fn remove_signature_subscription(
        &self,
        bank_sub_id: &Pubkey,
        signature: &Signature,
    ) -> bool {
        let mut subscriptions = self.signature_subscriptions.write().unwrap();
        match subscriptions.get_mut(signature) {
            Some(ref current_hashmap) if current_hashmap.len() == 1 => {}
            Some(current_hashmap) => {
                return current_hashmap.remove(bank_sub_id).is_some();
            }
            None => {
                return false;
            }
        }
        subscriptions.remove(signature).is_some()
    }

    fn check_signature_subscriptions(&self, signature: &Signature, status: RpcSignatureStatus) {
        let mut subscriptions = self.signature_subscriptions.write().unwrap();
        if let Some(hashmap) = subscriptions.get(signature) {
            for (_bank_sub_id, sink) in hashmap.iter() {
                sink.notify(Ok(status)).wait().unwrap();
            }
        }
        subscriptions.remove(&signature);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::serialize;
    use entry::next_entry;
    use entry::Entry;
    use jsonrpc_macros::pubsub::{Subscriber, SubscriptionId};
    use ledger;
    use signature::Keypair;
    use signature::{GenKeys, KeypairUtil};
    use solana_sdk::hash::hash;
    use std;
    use system_transaction::SystemTransaction;
    use tokio::prelude::{Async, Stream};
    use transaction::Instruction;

    #[test]
    fn test_bank_new() {
        let mint = Mint::new(10_000);
        let bank = Bank::new(&mint);
        assert_eq!(bank.get_balance(&mint.pubkey()), 10_000);
    }

    #[test]
    fn test_bank_new_with_leader() {
        let dummy_leader_id = Keypair::new().pubkey();
        let dummy_leader_tokens = 1;
        let mint = Mint::new_with_leader(10_000, dummy_leader_id, dummy_leader_tokens);
        let bank = Bank::new(&mint);
        assert_eq!(bank.get_balance(&mint.pubkey()), 9999);
        assert_eq!(bank.get_balance(&dummy_leader_id), 1);
    }

    #[test]
    fn test_two_payments_to_one_party() {
        let mint = Mint::new(10_000);
        let pubkey = Keypair::new().pubkey();
        let bank = Bank::new(&mint);
        assert_eq!(bank.last_id(), mint.last_id());

        bank.transfer(1_000, &mint.keypair(), pubkey, mint.last_id())
            .unwrap();
        assert_eq!(bank.get_balance(&pubkey), 1_000);

        bank.transfer(500, &mint.keypair(), pubkey, mint.last_id())
            .unwrap();
        assert_eq!(bank.get_balance(&pubkey), 1_500);
        assert_eq!(bank.transaction_count(), 2);
    }

    #[test]
    fn test_one_source_two_tx_one_batch() {
        let mint = Mint::new(1);
        let key1 = Keypair::new().pubkey();
        let key2 = Keypair::new().pubkey();
        let bank = Bank::new(&mint);
        assert_eq!(bank.last_id(), mint.last_id());

        let t1 = Transaction::system_move(&mint.keypair(), key1, 1, mint.last_id(), 0);
        let t2 = Transaction::system_move(&mint.keypair(), key2, 1, mint.last_id(), 0);
        let res = bank.process_transactions(&vec![t1.clone(), t2.clone()]);
        assert_eq!(res.len(), 2);
        assert_eq!(res[0], Ok(()));
        assert_eq!(res[1], Err(BankError::AccountInUse));
        assert_eq!(bank.get_balance(&mint.pubkey()), 0);
        assert_eq!(bank.get_balance(&key1), 1);
        assert_eq!(bank.get_balance(&key2), 0);
        assert_eq!(
            bank.get_signature(&t1.last_id, &t1.signatures[0]),
            Some(Ok(()))
        );
        // TODO: Transactions that fail to pay a fee could be dropped silently
        assert_eq!(
            bank.get_signature(&t2.last_id, &t2.signatures[0]),
            Some(Err(BankError::AccountInUse))
        );
    }

    #[test]
    fn test_one_tx_two_out_atomic_fail() {
        let mint = Mint::new(1);
        let key1 = Keypair::new().pubkey();
        let key2 = Keypair::new().pubkey();
        let bank = Bank::new(&mint);
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
            &[&mint.keypair()],
            &[key1, key2],
            mint.last_id(),
            0,
            vec![system_program::id()],
            instructions,
        );
        let res = bank.process_transactions(&vec![t1.clone()]);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0], Err(BankError::ResultWithNegativeTokens(1)));
        assert_eq!(bank.get_balance(&mint.pubkey()), 1);
        assert_eq!(bank.get_balance(&key1), 0);
        assert_eq!(bank.get_balance(&key2), 0);
        assert_eq!(
            bank.get_signature(&t1.last_id, &t1.signatures[0]),
            Some(Err(BankError::ResultWithNegativeTokens(1)))
        );
    }

    #[test]
    fn test_one_tx_two_out_atomic_pass() {
        let mint = Mint::new(2);
        let key1 = Keypair::new().pubkey();
        let key2 = Keypair::new().pubkey();
        let bank = Bank::new(&mint);
        let t1 = Transaction::system_move_many(
            &mint.keypair(),
            &[(key1, 1), (key2, 1)],
            mint.last_id(),
            0,
        );
        let res = bank.process_transactions(&vec![t1.clone()]);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0], Ok(()));
        assert_eq!(bank.get_balance(&mint.pubkey()), 0);
        assert_eq!(bank.get_balance(&key1), 1);
        assert_eq!(bank.get_balance(&key2), 1);
        assert_eq!(
            bank.get_signature(&t1.last_id, &t1.signatures[0]),
            Some(Ok(()))
        );
    }

    // TODO: This test demonstrates that fees are not paid when a program fails.
    // See github issue 1157 (https://github.com/solana-labs/solana/issues/1157)
    #[test]
    fn test_detect_failed_duplicate_transactions_issue_1157() {
        let mint = Mint::new(1);
        let bank = Bank::new(&mint);
        let dest = Keypair::new();

        // source with 0 contract context
        let tx = Transaction::system_create(
            &mint.keypair(),
            dest.pubkey(),
            mint.last_id(),
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
            Err(BankError::ResultWithNegativeTokens(0))
        );

        // The tokens didn't move, but the from address paid the transaction fee.
        assert_eq!(bank.get_balance(&dest.pubkey()), 0);

        // BUG: This should be the original balance minus the transaction fee.
        //assert_eq!(bank.get_balance(&mint.pubkey()), 0);
    }

    #[test]
    fn test_account_not_found() {
        let mint = Mint::new(1);
        let bank = Bank::new(&mint);
        let keypair = Keypair::new();
        assert_eq!(
            bank.transfer(1, &keypair, mint.pubkey(), mint.last_id()),
            Err(BankError::AccountNotFound)
        );
        assert_eq!(bank.transaction_count(), 0);
    }

    #[test]
    fn test_insufficient_funds() {
        let mint = Mint::new(11_000);
        let bank = Bank::new(&mint);
        let pubkey = Keypair::new().pubkey();
        bank.transfer(1_000, &mint.keypair(), pubkey, mint.last_id())
            .unwrap();
        assert_eq!(bank.transaction_count(), 1);
        assert_eq!(bank.get_balance(&pubkey), 1_000);
        assert_matches!(
            bank.transfer(10_001, &mint.keypair(), pubkey, mint.last_id()),
            Err(BankError::ResultWithNegativeTokens(0))
        );
        assert_eq!(bank.transaction_count(), 1);

        let mint_pubkey = mint.keypair().pubkey();
        assert_eq!(bank.get_balance(&mint_pubkey), 10_000);
        assert_eq!(bank.get_balance(&pubkey), 1_000);
    }

    #[test]
    fn test_transfer_to_newb() {
        let mint = Mint::new(10_000);
        let bank = Bank::new(&mint);
        let pubkey = Keypair::new().pubkey();
        bank.transfer(500, &mint.keypair(), pubkey, mint.last_id())
            .unwrap();
        assert_eq!(bank.get_balance(&pubkey), 500);
    }

    #[test]
    fn test_duplicate_transaction_signature() {
        let mint = Mint::new(1);
        let bank = Bank::new(&mint);
        let signature = Signature::default();
        assert_eq!(
            bank.reserve_signature_with_last_id_test(&signature, &mint.last_id()),
            Ok(())
        );
        assert_eq!(
            bank.reserve_signature_with_last_id_test(&signature, &mint.last_id()),
            Err(BankError::DuplicateSignature)
        );
    }

    #[test]
    fn test_clear_signatures() {
        let mint = Mint::new(1);
        let bank = Bank::new(&mint);
        let signature = Signature::default();
        bank.reserve_signature_with_last_id_test(&signature, &mint.last_id())
            .unwrap();
        bank.clear_signatures();
        assert_eq!(
            bank.reserve_signature_with_last_id_test(&signature, &mint.last_id()),
            Ok(())
        );
    }

    #[test]
    fn test_get_signature_status() {
        let mint = Mint::new(1);
        let bank = Bank::new(&mint);
        let signature = Signature::default();
        bank.reserve_signature_with_last_id_test(&signature, &mint.last_id())
            .expect("reserve signature");
        assert_eq!(
            bank.get_signature_status(&signature),
            Err(BankError::SignatureReserved)
        );
    }

    #[test]
    fn test_has_signature() {
        let mint = Mint::new(1);
        let bank = Bank::new(&mint);
        let signature = Signature::default();
        bank.reserve_signature_with_last_id_test(&signature, &mint.last_id())
            .expect("reserve signature");
        assert!(bank.has_signature(&signature));
    }

    #[test]
    fn test_reject_old_last_id() {
        let mint = Mint::new(1);
        let bank = Bank::new(&mint);
        let signature = Signature::default();
        for i in 0..MAX_ENTRY_IDS {
            let last_id = hash(&serialize(&i).unwrap()); // Unique hash
            bank.register_tick(&last_id);
        }
        // Assert we're no longer able to use the oldest entry ID.
        assert_eq!(
            bank.reserve_signature_with_last_id_test(&signature, &mint.last_id()),
            Err(BankError::LastIdNotFound)
        );
    }

    #[test]
    fn test_count_valid_ids() {
        let mint = Mint::new(1);
        let bank = Bank::new(&mint);
        let ids: Vec<_> = (0..MAX_ENTRY_IDS)
            .map(|i| {
                let last_id = hash(&serialize(&i).unwrap()); // Unique hash
                bank.register_tick(&last_id);
                last_id
            }).collect();
        assert_eq!(bank.count_valid_ids(&[]).len(), 0);
        assert_eq!(bank.count_valid_ids(&[mint.last_id()]).len(), 0);
        for (i, id) in bank.count_valid_ids(&ids).iter().enumerate() {
            assert_eq!(id.0, i);
        }
    }

    #[test]
    fn test_debits_before_credits() {
        let mint = Mint::new(2);
        let bank = Bank::new(&mint);
        let keypair = Keypair::new();
        let tx0 = Transaction::system_new(&mint.keypair(), keypair.pubkey(), 2, mint.last_id());
        let tx1 = Transaction::system_new(&keypair, mint.pubkey(), 1, mint.last_id());
        let txs = vec![tx0, tx1];
        let results = bank.process_transactions(&txs);
        assert!(results[1].is_err());

        // Assert bad transactions aren't counted.
        assert_eq!(bank.transaction_count(), 1);
    }

    #[test]
    fn test_process_empty_entry_is_registered() {
        let mint = Mint::new(1);
        let bank = Bank::new(&mint);
        let keypair = Keypair::new();
        let entry = next_entry(&mint.last_id(), 1, vec![]);
        let tx = Transaction::system_new(&mint.keypair(), keypair.pubkey(), 1, entry.id);

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
        let mint = Mint::new_with_leader(5, dummy_leader_id, dummy_leader_tokens);
        let genesis = mint.create_entries();
        let bank = Bank::default();
        bank.process_ledger(genesis).unwrap();
        assert_eq!(bank.get_balance(&mint.pubkey()), 4);
        assert_eq!(bank.get_balance(&dummy_leader_id), 1);
        assert_eq!(
            bank.leader_scheduler.read().unwrap().bootstrap_leader,
            dummy_leader_id
        );
    }

    fn create_sample_block_with_next_entries_using_keypairs(
        mint: &Mint,
        keypairs: &[Keypair],
    ) -> impl Iterator<Item = Entry> {
        let mut last_id = mint.last_id();
        let mut hash = mint.last_id();
        let mut entries: Vec<Entry> = vec![];
        let num_hashes = 1;
        for k in keypairs {
            let txs = vec![Transaction::system_new(
                &mint.keypair(),
                k.pubkey(),
                1,
                last_id,
            )];
            let mut e = ledger::next_entries(&hash, 0, txs);
            entries.append(&mut e);
            hash = entries.last().unwrap().id;
            let tick = Entry::new(&hash, num_hashes, vec![]);
            hash = tick.id;
            last_id = hash;
            entries.push(tick);
        }
        entries.into_iter()
    }

    // create a ledger with tick entries every `ticks` entries
    fn create_sample_block_with_ticks(
        mint: &Mint,
        length: usize,
        ticks: usize,
    ) -> impl Iterator<Item = Entry> {
        let mut entries = Vec::with_capacity(length);
        let mut hash = mint.last_id();
        let mut last_id = mint.last_id();
        let num_hashes = 1;
        for i in 0..length {
            let keypair = Keypair::new();
            let tx = Transaction::system_new(&mint.keypair(), keypair.pubkey(), 1, last_id);
            let entry = Entry::new(&hash, num_hashes, vec![tx]);
            hash = entry.id;
            entries.push(entry);
            if (i + 1) % ticks == 0 {
                let tick = Entry::new(&hash, num_hashes, vec![]);
                hash = tick.id;
                last_id = hash;
                entries.push(tick);
            }
        }
        entries.into_iter()
    }

    fn create_sample_ledger(length: usize) -> (impl Iterator<Item = Entry>, Pubkey) {
        let dummy_leader_id = Keypair::new().pubkey();
        let dummy_leader_tokens = 1;
        let mint = Mint::new_with_leader(
            length as u64 + 1 + dummy_leader_tokens,
            dummy_leader_id,
            dummy_leader_tokens,
        );
        let genesis = mint.create_entries();
        let block = create_sample_block_with_ticks(&mint, length, length);
        (genesis.into_iter().chain(block), mint.pubkey())
    }

    fn create_sample_ledger_with_mint_and_keypairs(
        mint: &Mint,
        keypairs: &[Keypair],
    ) -> impl Iterator<Item = Entry> {
        let genesis = mint.create_entries();
        let block = create_sample_block_with_next_entries_using_keypairs(mint, keypairs);
        genesis.into_iter().chain(block)
    }

    #[test]
    fn test_process_ledger_simple() {
        let (ledger, pubkey) = create_sample_ledger(1);
        let bank = Bank::default();
        let (ledger_height, last_id) = bank.process_ledger(ledger).unwrap();
        assert_eq!(bank.get_balance(&pubkey), 1);
        assert_eq!(ledger_height, 5);
        assert_eq!(bank.tick_height(), 2);
        assert_eq!(bank.last_id(), last_id);
    }

    #[test]
    fn test_hash_internal_state() {
        let dummy_leader_id = Keypair::new().pubkey();
        let dummy_leader_tokens = 1;
        let mint = Mint::new_with_leader(2_000, dummy_leader_id, dummy_leader_tokens);

        let seed = [0u8; 32];
        let mut rnd = GenKeys::new(seed);
        let keypairs = rnd.gen_n_keypairs(5);
        let ledger0 = create_sample_ledger_with_mint_and_keypairs(&mint, &keypairs);
        let ledger1 = create_sample_ledger_with_mint_and_keypairs(&mint, &keypairs);

        let bank0 = Bank::default();
        bank0.process_ledger(ledger0).unwrap();
        let bank1 = Bank::default();
        bank1.process_ledger(ledger1).unwrap();

        let initial_state = bank0.hash_internal_state();

        assert_eq!(bank1.hash_internal_state(), initial_state);

        let pubkey = keypairs[0].pubkey();
        bank0
            .transfer(1_000, &mint.keypair(), pubkey, mint.last_id())
            .unwrap();
        assert_ne!(bank0.hash_internal_state(), initial_state);
        bank1
            .transfer(1_000, &mint.keypair(), pubkey, mint.last_id())
            .unwrap();
        assert_eq!(bank0.hash_internal_state(), bank1.hash_internal_state());
    }
    #[test]
    fn test_finality() {
        let def_bank = Bank::default();
        assert_eq!(def_bank.finality(), std::usize::MAX);
        def_bank.set_finality(90);
        assert_eq!(def_bank.finality(), 90);
    }
    #[test]
    fn test_interleaving_locks() {
        let mint = Mint::new(3);
        let bank = Bank::new(&mint);
        let alice = Keypair::new();
        let bob = Keypair::new();

        let tx1 = Transaction::system_new(&mint.keypair(), alice.pubkey(), 1, mint.last_id());
        let pay_alice = vec![tx1];

        let locked_alice = bank.lock_accounts(&pay_alice);
        let results_alice =
            bank.execute_and_commit_transactions(&pay_alice, locked_alice, MAX_ENTRY_IDS);
        assert_eq!(results_alice[0], Ok(()));

        // try executing an interleaved transfer twice
        assert_eq!(
            bank.transfer(1, &mint.keypair(), bob.pubkey(), mint.last_id()),
            Err(BankError::AccountInUse)
        );
        // the second time shoudl fail as well
        // this verifies that `unlock_accounts` doesn't unlock `AccountInUse` accounts
        assert_eq!(
            bank.transfer(1, &mint.keypair(), bob.pubkey(), mint.last_id()),
            Err(BankError::AccountInUse)
        );

        bank.unlock_accounts(&pay_alice, &results_alice);

        assert_matches!(
            bank.transfer(2, &mint.keypair(), bob.pubkey(), mint.last_id()),
            Ok(_)
        );
    }
    #[test]
    fn test_bank_account_subscribe() {
        let mint = Mint::new(100);
        let bank = Bank::new(&mint);
        let alice = Keypair::new();
        let bank_sub_id = Keypair::new().pubkey();
        let last_id = bank.last_id();
        let tx = Transaction::system_create(
            &mint.keypair(),
            alice.pubkey(),
            last_id,
            1,
            16,
            budget_program::id(),
            0,
        );
        bank.process_transaction(&tx).unwrap();

        let (subscriber, _id_receiver, mut transport_receiver) =
            Subscriber::new_test("accountNotification");
        let sub_id = SubscriptionId::Number(0 as u64);
        let sink = subscriber.assign_id(sub_id.clone()).unwrap();
        bank.add_account_subscription(bank_sub_id, alice.pubkey(), sink);

        assert!(
            bank.account_subscriptions
                .write()
                .unwrap()
                .contains_key(&alice.pubkey())
        );

        let account = bank.get_account(&alice.pubkey()).unwrap();
        bank.check_account_subscriptions(&alice.pubkey(), &account);
        let string = transport_receiver.poll();
        assert!(string.is_ok());
        if let Async::Ready(Some(response)) = string.unwrap() {
            let expected = format!(r#"{{"jsonrpc":"2.0","method":"accountNotification","params":{{"result":{{"executable":false,"loader":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],"owner":[129,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],"tokens":1,"userdata":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]}},"subscription":0}}}}"#);
            assert_eq!(expected, response);
        }

        bank.remove_account_subscription(&bank_sub_id, &alice.pubkey());
        assert!(
            !bank
                .account_subscriptions
                .write()
                .unwrap()
                .contains_key(&alice.pubkey())
        );
    }
    #[test]
    fn test_bank_signature_subscribe() {
        let mint = Mint::new(100);
        let bank = Bank::new(&mint);
        let alice = Keypair::new();
        let bank_sub_id = Keypair::new().pubkey();
        let last_id = bank.last_id();
        let tx = Transaction::system_move(&mint.keypair(), alice.pubkey(), 20, last_id, 0);
        let signature = tx.signatures[0];
        bank.process_transaction(&tx).unwrap();

        let (subscriber, _id_receiver, mut transport_receiver) =
            Subscriber::new_test("signatureNotification");
        let sub_id = SubscriptionId::Number(0 as u64);
        let sink = subscriber.assign_id(sub_id.clone()).unwrap();
        bank.add_signature_subscription(bank_sub_id, signature, sink);

        assert!(
            bank.signature_subscriptions
                .write()
                .unwrap()
                .contains_key(&signature)
        );

        bank.check_signature_subscriptions(&signature, RpcSignatureStatus::Confirmed);
        let string = transport_receiver.poll();
        assert!(string.is_ok());
        if let Async::Ready(Some(response)) = string.unwrap() {
            let expected = format!(r#"{{"jsonrpc":"2.0","method":"signatureNotification","params":{{"result":"Confirmed","subscription":0}}}}"#);
            assert_eq!(expected, response);
        }

        bank.remove_signature_subscription(&bank_sub_id, &signature);
        assert!(
            !bank
                .signature_subscriptions
                .write()
                .unwrap()
                .contains_key(&signature)
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
        let mint = Mint::new(1000);
        let bank = Bank::new(&mint);

        // ensure bank can process a tick
        let tick = next_entry(&mint.last_id(), 1, vec![]);
        assert_eq!(bank.par_process_entries(&[tick.clone()]), Ok(()));
        assert_eq!(bank.last_id(), tick.id);
    }
    #[test]
    fn test_par_process_entries_2_entries_collision() {
        let mint = Mint::new(1000);
        let bank = Bank::new(&mint);
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();

        let last_id = bank.last_id();

        // ensure bank can process 2 entries that have a common account and no tick is registered
        let tx = Transaction::system_new(&mint.keypair(), keypair1.pubkey(), 2, bank.last_id());
        let entry_1 = next_entry(&last_id, 1, vec![tx]);
        let tx = Transaction::system_new(&mint.keypair(), keypair2.pubkey(), 2, bank.last_id());
        let entry_2 = next_entry(&entry_1.id, 1, vec![tx]);
        assert_eq!(bank.par_process_entries(&[entry_1, entry_2]), Ok(()));
        assert_eq!(bank.get_balance(&keypair1.pubkey()), 2);
        assert_eq!(bank.get_balance(&keypair2.pubkey()), 2);
        assert_eq!(bank.last_id(), last_id);
    }
    #[test]
    fn test_par_process_entries_2_entries_par() {
        let mint = Mint::new(1000);
        let bank = Bank::new(&mint);
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();
        let keypair3 = Keypair::new();
        let keypair4 = Keypair::new();

        //load accounts
        let tx = Transaction::system_new(&mint.keypair(), keypair1.pubkey(), 1, bank.last_id());
        assert_eq!(bank.process_transaction(&tx), Ok(()));
        let tx = Transaction::system_new(&mint.keypair(), keypair2.pubkey(), 1, bank.last_id());
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
        let mint = Mint::new(1000);
        let bank = Bank::new(&mint);
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();
        let keypair3 = Keypair::new();
        let keypair4 = Keypair::new();

        //load accounts
        let tx = Transaction::system_new(&mint.keypair(), keypair1.pubkey(), 1, bank.last_id());
        assert_eq!(bank.process_transaction(&tx), Ok(()));
        let tx = Transaction::system_new(&mint.keypair(), keypair2.pubkey(), 1, bank.last_id());
        assert_eq!(bank.process_transaction(&tx), Ok(()));

        let last_id = bank.last_id();

        // ensure bank can process 2 entries that do not have a common account and tick is registered
        let tx = Transaction::system_new(&keypair2, keypair3.pubkey(), 1, bank.last_id());
        let entry_1 = next_entry(&last_id, 1, vec![tx]);
        let new_tick = next_entry(&entry_1.id, 1, vec![]);
        let tx = Transaction::system_new(&keypair1, keypair4.pubkey(), 1, new_tick.id);
        let entry_2 = next_entry(&new_tick.id, 1, vec![tx]);
        assert_eq!(
            bank.par_process_entries(&[entry_1.clone(), new_tick.clone(), entry_2]),
            Ok(())
        );
        assert_eq!(bank.get_balance(&keypair3.pubkey()), 1);
        assert_eq!(bank.get_balance(&keypair4.pubkey()), 1);
        assert_eq!(bank.last_id(), new_tick.id);
        // ensure that errors are returned
        assert_eq!(
            bank.par_process_entries(&[entry_1]),
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

        assert_eq!(system_program::id(), system);
        assert_eq!(native_loader::id(), native);
        assert_eq!(bpf_loader::id(), bpf);
        assert_eq!(budget_program::id(), budget);
        assert_eq!(storage_program::id(), storage);
        assert_eq!(token_program::id(), token);
        assert_eq!(vote_program::id(), vote);
    }

    #[test]
    fn test_program_id_uniqueness() {
        let mut unique = HashSet::new();
        let ids = vec![
            system_program::id(),
            native_loader::id(),
            bpf_loader::id(),
            budget_program::id(),
            storage_program::id(),
            token_program::id(),
            vote_program::id(),
        ];
        assert!(ids.into_iter().all(move |id| unique.insert(id)));
    }

    #[test]
    fn test_bank_purge() {
        let alice = Mint::new(10_000);
        let bank = Bank::new(&alice);
        let bob = Keypair::new();
        let charlie = Keypair::new();

        // bob should have 500
        bank.transfer(500, &alice.keypair(), bob.pubkey(), alice.last_id())
            .unwrap();
        assert_eq!(bank.get_balance(&bob.pubkey()), 500);

        bank.transfer(500, &alice.keypair(), charlie.pubkey(), alice.last_id())
            .unwrap();
        assert_eq!(bank.get_balance(&charlie.pubkey()), 500);

        bank.checkpoint();
        bank.checkpoint();
        assert_eq!(bank.checkpoint_depth(), 2);
        assert_eq!(bank.get_balance(&bob.pubkey()), 500);
        assert_eq!(bank.get_balance(&alice.pubkey()), 9_000);
        assert_eq!(bank.get_balance(&charlie.pubkey()), 500);
        assert_eq!(bank.transaction_count(), 2);

        // transfer money back, so bob has zero
        bank.transfer(500, &bob, alice.keypair().pubkey(), alice.last_id())
            .unwrap();
        // this has to be stored as zero in the top accounts hashmap ;)
        assert!(bank.accounts.read().unwrap().load(&bob.pubkey()).is_some());
        assert_eq!(bank.get_balance(&bob.pubkey()), 0);
        // double-checks
        assert_eq!(bank.get_balance(&alice.pubkey()), 9_500);
        assert_eq!(bank.get_balance(&charlie.pubkey()), 500);
        assert_eq!(bank.transaction_count(), 3);
        bank.purge(1);

        assert_eq!(bank.get_balance(&bob.pubkey()), 0);
        // double-checks
        assert_eq!(bank.get_balance(&alice.pubkey()), 9_500);
        assert_eq!(bank.get_balance(&charlie.pubkey()), 500);
        assert_eq!(bank.transaction_count(), 3);
        assert_eq!(bank.checkpoint_depth(), 1);

        bank.purge(0);

        // bob should still have 0, alice should have 10_000
        assert_eq!(bank.get_balance(&bob.pubkey()), 0);
        assert!(bank.accounts.read().unwrap().load(&bob.pubkey()).is_none());
        // double-checks
        assert_eq!(bank.get_balance(&alice.pubkey()), 9_500);
        assert_eq!(bank.get_balance(&charlie.pubkey()), 500);
        assert_eq!(bank.transaction_count(), 3);
        assert_eq!(bank.checkpoint_depth(), 0);
    }

    #[test]
    fn test_bank_checkpoint_rollback() {
        let alice = Mint::new(10_000);
        let bank = Bank::new(&alice);
        let bob = Keypair::new();
        let charlie = Keypair::new();

        // bob should have 500
        bank.transfer(500, &alice.keypair(), bob.pubkey(), alice.last_id())
            .unwrap();
        assert_eq!(bank.get_balance(&bob.pubkey()), 500);
        bank.transfer(500, &alice.keypair(), charlie.pubkey(), alice.last_id())
            .unwrap();
        assert_eq!(bank.get_balance(&charlie.pubkey()), 500);
        assert_eq!(bank.checkpoint_depth(), 0);

        bank.checkpoint();
        bank.checkpoint();
        assert_eq!(bank.checkpoint_depth(), 2);
        assert_eq!(bank.get_balance(&bob.pubkey()), 500);
        assert_eq!(bank.get_balance(&charlie.pubkey()), 500);
        assert_eq!(bank.transaction_count(), 2);

        // transfer money back, so bob has zero
        bank.transfer(500, &bob, alice.keypair().pubkey(), alice.last_id())
            .unwrap();
        // this has to be stored as zero in the top accounts hashmap ;)
        assert_eq!(bank.get_balance(&bob.pubkey()), 0);
        assert_eq!(bank.get_balance(&charlie.pubkey()), 500);
        assert_eq!(bank.transaction_count(), 3);
        bank.rollback();

        // bob should have 500 again
        assert_eq!(bank.get_balance(&bob.pubkey()), 500);
        assert_eq!(bank.get_balance(&charlie.pubkey()), 500);
        assert_eq!(bank.transaction_count(), 2);
        assert_eq!(bank.checkpoint_depth(), 1);

        let signature = Signature::default();
        for i in 0..MAX_ENTRY_IDS + 1 {
            let last_id = hash(&serialize(&i).unwrap()); // Unique hash
            bank.register_tick(&last_id);
        }
        assert_eq!(bank.tick_height(), MAX_ENTRY_IDS as u64 + 2);
        assert_eq!(
            bank.reserve_signature_with_last_id_test(&signature, &alice.last_id()),
            Err(BankError::LastIdNotFound)
        );
        bank.rollback();
        assert_eq!(bank.tick_height(), 1);
        assert_eq!(
            bank.reserve_signature_with_last_id_test(&signature, &alice.last_id()),
            Ok(())
        );
        bank.checkpoint();
        assert_eq!(
            bank.reserve_signature_with_last_id_test(&signature, &alice.last_id()),
            Err(BankError::DuplicateSignature)
        );
    }

    #[test]
    #[should_panic]
    fn test_bank_rollback_panic() {
        let alice = Mint::new(10_000);
        let bank = Bank::new(&alice);
        bank.rollback();
    }

}

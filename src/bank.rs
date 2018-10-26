//! The `bank` module tracks client accounts and the progress of smart
//! contracts. It offers a high-level API that signs transactions
//! on behalf of the caller, and a low-level API for when they have
//! already been signed and verified.

use bincode::deserialize;
use bincode::serialize;
use bpf_loader;
use budget_program::BudgetState;
use counter::Counter;
use entry::Entry;
use hash::{hash, Hash};
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
use solana_sdk::account::{Account, KeyedAccount};
use solana_sdk::pubkey::Pubkey;
use std;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::result;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;
use storage_program::StorageProgram;
use system_program::SystemProgram;
use system_transaction::SystemTransaction;
use timing::{duration_as_us, timestamp};
use token_program::TokenProgram;
use tokio::prelude::Future;
use transaction::Transaction;
use vote_program::VoteProgram;
use window::WINDOW_SIZE;

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

    /// Proof of History verification failed.
    LedgerVerificationFailed,
    /// Contract's transaction token balance does not equal the balance after the transaction
    UnbalancedTransaction(u8),
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

pub struct LastIds {
    /// A FIFO queue of `last_id` items, where each item is a set of signatures
    /// that have been processed using that `last_id`. Rejected `last_id`
    /// values are so old that the `last_id` has been pulled out of the queue.

    /// updated whenever an id is registered
    nth: isize,

    /// last id to be registered
    last: Option<Hash>,

    /// Mapping of hashes to signature sets along with timestamp and what nth
    /// was when the id was added. The bank uses this data to
    /// reject transactions with signatures it's seen before and to reject
    /// transactions that are too old (nth is too small)
    sigs: HashMap<Hash, (SignatureStatusMap, u64, isize)>,
}

impl Default for LastIds {
    fn default() -> Self {
        LastIds {
            nth: 0,
            last: None,
            sigs: HashMap::new(),
        }
    }
}

/// The state of all accounts and contracts after processing its entries.
pub struct Bank {
    /// A map of account public keys to the balance in that account.
    pub accounts: RwLock<HashMap<Pubkey, Account>>,

    /// set of accounts which are currently in the pipeline
    account_locks: Mutex<HashSet<Pubkey>>,

    /// FIFO queue of `last_id` items
    last_ids: RwLock<LastIds>,

    /// The number of transactions the bank has processed without error since the
    /// start of the ledger.
    transaction_count: AtomicUsize,

    // The latest finality time for the network
    finality_time: AtomicUsize,

    // Mapping of account ids to Subscriber ids and sinks to notify on userdata update
    account_subscriptions: RwLock<HashMap<Pubkey, HashMap<Pubkey, Sink<Account>>>>,

    // Mapping of signatures to Subscriber ids and sinks to notify on confirmation
    signature_subscriptions: RwLock<HashMap<Signature, HashMap<Pubkey, Sink<RpcSignatureStatus>>>>,

    /// Tracks and updates the leader schedule based on the votes and account stakes
    /// processed by the bank
    pub leader_scheduler: Arc<RwLock<LeaderScheduler>>,

    // The number of ticks that have elapsed since genesis
    tick_height: Mutex<u64>,
}

impl Default for Bank {
    fn default() -> Self {
        Bank {
            accounts: RwLock::new(HashMap::new()),
            account_locks: Mutex::new(HashSet::new()),
            last_ids: RwLock::new(LastIds::default()),
            transaction_count: AtomicUsize::new(0),
            finality_time: AtomicUsize::new(std::usize::MAX),
            account_subscriptions: RwLock::new(HashMap::new()),
            signature_subscriptions: RwLock::new(HashMap::new()),
            leader_scheduler: Arc::new(RwLock::new(LeaderScheduler::default())),
            tick_height: Mutex::new(0),
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
    pub fn new_from_deposit(deposit: &Payment) -> Self {
        let bank = Self::default();
        {
            let mut accounts = bank.accounts.write().unwrap();
            let account = accounts.entry(deposit.to).or_insert_with(Account::default);
            Self::apply_payment(deposit, account);
        }
        bank.add_builtin_programs();
        bank
    }

    /// Create an Bank with only a Mint. Typically used by unit tests.
    pub fn new(mint: &Mint) -> Self {
        let deposit = Payment {
            to: mint.pubkey(),
            tokens: mint.tokens,
        };
        let bank = Self::new_from_deposit(&deposit);
        bank.register_entry_id(&mint.last_id());
        bank
    }

    fn add_builtin_programs(&self) {
        // Preload Bpf Loader account
        let mut accounts = self.accounts.write().unwrap();
        let mut account = accounts
            .entry(bpf_loader::id())
            .or_insert_with(Account::default);
        bpf_loader::populate_account(&mut account);
    }

    /// Commit funds to the given account
    fn apply_payment(payment: &Payment, account: &mut Account) {
        trace!("apply payments {}", payment.tokens);
        account.tokens += payment.tokens;
    }

    /// Return the last entry ID registered.
    pub fn last_id(&self) -> Hash {
        self.last_ids
            .read()
            .unwrap()
            .last
            .expect("no last_id has been set")
    }

    /// Store the given signature. The bank will reject any transaction with the same signature.
    fn reserve_signature(signatures: &mut SignatureStatusMap, signature: &Signature) -> Result<()> {
        if let Some(_result) = signatures.get(signature) {
            return Err(BankError::DuplicateSignature);
        }
        signatures.insert(*signature, Ok(()));
        Ok(())
    }

    /// Forget all signatures. Useful for benchmarking.
    pub fn clear_signatures(&self) {
        for sigs in &mut self.last_ids.write().unwrap().sigs.values_mut() {
            sigs.0.clear();
        }
    }

    /// Check if the age of the entry_id is within the max_age
    /// return false for any entries with an age equal to or above max_age
    fn check_entry_id_age(last_ids: &LastIds, entry_id: Hash, max_age: usize) -> bool {
        let entry = last_ids.sigs.get(&entry_id);

        match entry {
            Some(entry) => ((last_ids.nth - entry.2) as usize) < max_age,
            _ => false,
        }
    }

    fn reserve_signature_with_last_id(
        last_ids: &mut LastIds,
        last_id: &Hash,
        sig: &Signature,
    ) -> Result<()> {
        if let Some(entry) = last_ids.sigs.get_mut(last_id) {
            if ((last_ids.nth - entry.2) as usize) <= MAX_ENTRY_IDS {
                return Self::reserve_signature(&mut entry.0, sig);
            }
        }
        Err(BankError::LastIdNotFound)
    }

    #[cfg(test)]
    fn reserve_signature_with_last_id_test(&self, sig: &Signature, last_id: &Hash) -> Result<()> {
        let mut last_ids = self.last_ids.write().unwrap();
        Self::reserve_signature_with_last_id(&mut last_ids, last_id, sig)
    }

    fn update_signature_status(
        signatures: &mut SignatureStatusMap,
        signature: &Signature,
        result: &Result<()>,
    ) {
        let entry = signatures.entry(*signature).or_insert(Ok(()));
        *entry = result.clone();
    }

    fn update_signature_status_with_last_id(
        last_ids_sigs: &mut HashMap<Hash, (SignatureStatusMap, u64, isize)>,
        signature: &Signature,
        result: &Result<()>,
        last_id: &Hash,
    ) {
        if let Some(entry) = last_ids_sigs.get_mut(last_id) {
            Self::update_signature_status(&mut entry.0, signature, result);
        }
    }

    fn update_transaction_statuses(&self, txs: &[Transaction], res: &[Result<()>]) {
        let mut last_ids = self.last_ids.write().unwrap();
        for (i, tx) in txs.iter().enumerate() {
            Self::update_signature_status_with_last_id(
                &mut last_ids.sigs,
                &tx.signature,
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
                    self.check_signature_subscriptions(&tx.signature, status);
                }
            }
        }
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
            if let Some(entry) = last_ids.sigs.get(id) {
                if ((last_ids.nth - entry.2) as usize) <= MAX_ENTRY_IDS {
                    ret.push((i, entry.1));
                }
            }
        }
        ret
    }

    /// Tell the bank which Entry IDs exist on the ledger. This function
    /// assumes subsequent calls correspond to later entries, and will boot
    /// the oldest ones once its internal cache is full. Once boot, the
    /// bank will reject transactions using that `last_id`.
    pub fn register_entry_id(&self, last_id: &Hash) {
        let mut last_ids = self.last_ids.write().unwrap();

        let last_ids_nth = last_ids.nth;

        // this clean up can be deferred until sigs gets larger
        //  because we verify entry.nth every place we check for validity
        if last_ids.sigs.len() >= MAX_ENTRY_IDS {
            last_ids
                .sigs
                .retain(|_, (_, _, nth)| ((last_ids_nth - *nth) as usize) <= MAX_ENTRY_IDS);
        }

        last_ids
            .sigs
            .insert(*last_id, (HashMap::new(), timestamp(), last_ids_nth));

        last_ids.nth += 1;
        last_ids.last = Some(*last_id);

        inc_new_counter_info!("bank-register_entry_id-registered", 1);
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
            _ => for k in &tx.account_keys {
                account_locks.remove(k);
            },
        }
    }

    fn load_account(
        &self,
        tx: &Transaction,
        accounts: &HashMap<Pubkey, Account>,
        last_ids: &mut LastIds,
        max_age: usize,
        error_counters: &mut ErrorCounters,
    ) -> Result<Vec<Account>> {
        // Copy all the accounts
        if accounts.get(&tx.account_keys[0]).is_none() {
            error_counters.account_not_found += 1;
            Err(BankError::AccountNotFound)
        } else if accounts.get(&tx.account_keys[0]).unwrap().tokens < tx.fee {
            error_counters.insufficient_funds += 1;
            Err(BankError::InsufficientFundsForFee)
        } else {
            if !Self::check_entry_id_age(&last_ids, tx.last_id, max_age) {
                error_counters.last_id_not_found += 1;
                return Err(BankError::LastIdNotFound);
            }

            // There is no way to predict what contract will execute without an error
            // If a fee can pay for execution then the contract will be scheduled
            let err = Self::reserve_signature_with_last_id(last_ids, &tx.last_id, &tx.signature);
            if let Err(BankError::LastIdNotFound) = err {
                error_counters.reserve_last_id += 1;
            } else if let Err(BankError::DuplicateSignature) = err {
                error_counters.duplicate_signature += 1;
            }
            err?;

            let mut called_accounts: Vec<Account> = tx
                .account_keys
                .iter()
                .map(|key| accounts.get(key).cloned().unwrap_or_default())
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

    pub fn verify_transaction(
        instruction_index: usize,
        tx_program_id: &Pubkey,
        pre_program_id: &Pubkey,
        pre_tokens: i64,
        account: &Account,
    ) -> Result<()> {
        // Verify the transaction

        // Make sure that program_id is still the same or this was just assigned by the system call contract
        if *pre_program_id != account.program_id && !SystemProgram::check_id(&tx_program_id) {
            return Err(BankError::ModifiedContractId(instruction_index as u8));
        }
        // For accounts unassigned to the contract, the individual balance of each accounts cannot decrease.
        if *tx_program_id != account.program_id && pre_tokens > account.tokens {
            return Err(BankError::ExternalAccountTokenSpend(
                instruction_index as u8,
            ));
        }
        if account.tokens < 0 {
            return Err(BankError::ResultWithNegativeTokens(instruction_index as u8));
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
        let tx_program_id = tx.program_id(instruction_index);
        // TODO: the runtime should be checking read/write access to memory
        // we are trusting the hard coded contracts not to clobber or allocate
        let pre_total: i64 = program_accounts.iter().map(|a| a.tokens).sum();
        let pre_data: Vec<_> = program_accounts
            .iter_mut()
            .map(|a| (a.program_id, a.tokens))
            .collect();

        // Check account subscriptions before storing data for notifications
        let subscriptions = self.account_subscriptions.read().unwrap();
        let pre_userdata: Vec<_> = tx
            .account_keys
            .iter()
            .enumerate()
            .zip(program_accounts.iter_mut())
            .filter(|((_, pubkey), _)| subscriptions.get(&pubkey).is_some())
            .map(|((i, pubkey), a)| ((i, pubkey), a.userdata.clone()))
            .collect();

        // Call the contract method
        // It's up to the contract to implement its own rules on moving funds
        if SystemProgram::check_id(&tx_program_id) {
            if SystemProgram::process_transaction(&tx, instruction_index, program_accounts).is_err()
            {
                return Err(BankError::ProgramRuntimeError(instruction_index as u8));
            }
        } else if BudgetState::check_id(&tx_program_id) {
            if BudgetState::process_transaction(&tx, instruction_index, program_accounts).is_err() {
                return Err(BankError::ProgramRuntimeError(instruction_index as u8));
            }
        } else if StorageProgram::check_id(&tx_program_id) {
            if StorageProgram::process_transaction(&tx, instruction_index, program_accounts)
                .is_err()
            {
                return Err(BankError::ProgramRuntimeError(instruction_index as u8));
            }
        } else if TokenProgram::check_id(&tx_program_id) {
            if TokenProgram::process_transaction(&tx, instruction_index, program_accounts).is_err()
            {
                return Err(BankError::ProgramRuntimeError(instruction_index as u8));
            }
        } else if VoteProgram::check_id(&tx_program_id) {
            VoteProgram::process_transaction(&tx, instruction_index, program_accounts).is_err();
        } else {
            let mut depth = 0;
            let mut keys = Vec::new();
            let mut accounts = Vec::new();

            let mut program_id = tx.program_ids[instruction_index];
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
                if !program.executable || program.loader_program_id == Pubkey::default() {
                    return Err(BankError::AccountNotFound);
                }

                // add loader to chain
                keys.insert(0, program_id);
                accounts.insert(0, program.clone());

                program_id = program.loader_program_id;
            }

            let mut keyed_accounts: Vec<_> = (&keys)
                .into_iter()
                .zip(accounts.iter_mut())
                .map(|(key, account)| KeyedAccount { key, account })
                .collect();
            let mut keyed_accounts2: Vec<_> = (&tx.instructions[instruction_index].accounts)
                .into_iter()
                .zip(program_accounts.iter_mut())
                .map(|(index, account)| KeyedAccount {
                    key: &tx.account_keys[*index as usize],
                    account,
                }).collect();
            keyed_accounts.append(&mut keyed_accounts2);

            if !native_loader::process_transaction(
                &mut keyed_accounts,
                &tx.instructions[instruction_index].userdata,
            ) {
                return Err(BankError::ProgramRuntimeError(instruction_index as u8));
            }
        }

        // Verify the transaction
        for ((pre_program_id, pre_tokens), post_account) in
            pre_data.iter().zip(program_accounts.iter())
        {
            Self::verify_transaction(
                instruction_index,
                &tx_program_id,
                pre_program_id,
                *pre_tokens,
                post_account,
            )?;
        }
        // Send notifications
        for ((i, pubkey), userdata) in &pre_userdata {
            let account = &program_accounts[*i];
            if userdata != &account.userdata {
                self.check_account_subscriptions(&pubkey, &account);
            }
        }
        // The total sum of all the tokens in all the pages cannot change.
        let post_total: i64 = program_accounts.iter().map(|a| a.tokens).sum();
        if pre_total != post_total {
            Err(BankError::UnbalancedTransaction(instruction_index as u8))
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
                //purge if 0
                if account.tokens == 0 {
                    accounts.remove(&key);
                } else {
                    *accounts.entry(*key).or_insert_with(Account::default) = account.clone();
                    assert_eq!(accounts.get(key).unwrap().tokens, account.tokens);
                }
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
        let results = self.execute_and_commit_transactions(txs, locked_accounts, MAX_ENTRY_IDS / 2);
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
            self.load_accounts(txs, locked_accounts, max_age, &mut error_counters);
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

        self.transaction_count
            .fetch_add(tx_count, Ordering::Relaxed);
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
            let tick_height = {
                let mut tick_height_lock = self.tick_height.lock().unwrap();
                *tick_height_lock += 1;
                *tick_height_lock
            };

            self.leader_scheduler
                .write()
                .unwrap()
                .update_height(tick_height, self);
            self.register_entry_id(&entry.id);
        }

        Ok(())
    }

    /// Process an ordered list of entries, populating a circular buffer "tail"
    /// as we go.
    fn process_entries_tail(
        &self,
        entries: &[Entry],
        tail: &mut Vec<Entry>,
        tail_idx: &mut usize,
    ) -> Result<u64> {
        let mut entry_count = 0;

        for entry in entries {
            if tail.len() > *tail_idx {
                tail[*tail_idx] = entry.clone();
            } else {
                tail.push(entry.clone());
            }
            *tail_idx = (*tail_idx + 1) % WINDOW_SIZE as usize;

            entry_count += 1;
            // TODO: We prepare for implementing voting contract by making the associated
            // process_entries functions aware of the vote-tracking structure inside
            // the leader scheduler. Next we will extract the vote tracking structure
            // out of the leader scheduler, and into the bank, and remove the leader
            // scheduler from these banking functions.
            self.process_entry(entry)?;
        }

        Ok(entry_count)
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
                self.register_entry_id(&entry.id);
                *self.tick_height.lock().unwrap() += 1;
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

    /// Append entry blocks to the ledger, verifying them along the way.
    fn process_blocks<I>(
        &self,
        start_hash: Hash,
        entries: I,
        tail: &mut Vec<Entry>,
        tail_idx: &mut usize,
    ) -> Result<u64>
    where
        I: IntoIterator<Item = Entry>,
    {
        // Ledger verification needs to be parallelized, but we can't pull the whole
        // thing into memory. We therefore chunk it.
        let mut entry_height = *tail_idx as u64;

        for entry in &tail[0..*tail_idx] {
            if entry.is_tick() {
                *self.tick_height.lock().unwrap() += 1;
            }
        }

        let mut id = start_hash;
        for block in &entries.into_iter().chunks(VERIFY_BLOCK_SIZE) {
            let block: Vec<_> = block.collect();
            if !block.verify(&id) {
                warn!("Ledger proof of history failed at entry: {}", entry_height);
                return Err(BankError::LedgerVerificationFailed);
            }
            id = block.last().unwrap().id;
            let entry_count = self.process_entries_tail(&block, tail, tail_idx)?;

            entry_height += entry_count;
        }
        Ok(entry_height)
    }

    /// Process a full ledger.
    pub fn process_ledger<I>(&self, entries: I) -> Result<(u64, u64, Vec<Entry>)>
    where
        I: IntoIterator<Item = Entry>,
    {
        let mut entries = entries.into_iter();

        // The first item in the ledger is required to be an entry with zero num_hashes,
        // which implies its id can be used as the ledger's seed.
        let entry0 = entries.next().expect("invalid ledger: empty");

        // The second item in the ledger is a special transaction where the to and from
        // fields are the same. That entry should be treated as a deposit, not a
        // transfer to oneself.
        let entry1 = entries
            .next()
            .expect("invalid ledger: need at least 2 entries");
        {
            let tx = &entry1.transactions[0];
            assert!(SystemProgram::check_id(tx.program_id(0)), "Invalid ledger");
            let instruction: SystemProgram = deserialize(tx.userdata(0)).unwrap();
            let deposit = if let SystemProgram::Move { tokens } = instruction {
                Some(tokens)
            } else {
                None
            }.expect("invalid ledger, needs to start with a contract");
            {
                let mut accounts = self.accounts.write().unwrap();
                let account = accounts
                    .entry(tx.account_keys[0])
                    .or_insert_with(Account::default);
                account.tokens += deposit;
                trace!("applied genesis payment {:?} => {:?}", deposit, account);
            }
        }
        self.register_entry_id(&entry0.id);
        self.register_entry_id(&entry1.id);
        let entry1_id = entry1.id;

        let mut tail = Vec::with_capacity(WINDOW_SIZE as usize);
        tail.push(entry0);
        tail.push(entry1);
        let mut tail_idx = 2;
        let entry_height = self.process_blocks(entry1_id, entries, &mut tail, &mut tail_idx)?;

        // check if we need to rotate tail
        if tail.len() == WINDOW_SIZE as usize {
            tail.rotate_left(tail_idx)
        }

        Ok((*self.tick_height.lock().unwrap(), entry_height, tail))
    }

    /// Create, sign, and process a Transaction from `keypair` to `to` of
    /// `n` tokens where `last_id` is the last Entry ID observed by the client.
    pub fn transfer(
        &self,
        n: i64,
        keypair: &Keypair,
        to: Pubkey,
        last_id: Hash,
    ) -> Result<Signature> {
        let tx = Transaction::system_new(keypair, to, n, last_id);
        let signature = tx.signature;
        self.process_transaction(&tx).map(|_| signature)
    }

    pub fn read_balance(account: &Account) -> i64 {
        if SystemProgram::check_id(&account.program_id) {
            SystemProgram::get_balance(account)
        } else if BudgetState::check_id(&account.program_id) {
            BudgetState::get_balance(account)
        } else {
            account.tokens
        }
    }
    /// Each contract would need to be able to introspect its own state
    /// this is hard coded to the budget contract language
    pub fn get_balance(&self, pubkey: &Pubkey) -> i64 {
        self.get_account(pubkey)
            .map(|x| Self::read_balance(&x))
            .unwrap_or(0)
    }

    pub fn get_account(&self, pubkey: &Pubkey) -> Option<Account> {
        let accounts = self
            .accounts
            .read()
            .expect("'accounts' read lock in get_balance");
        accounts.get(pubkey).cloned()
    }

    pub fn transaction_count(&self) -> usize {
        self.transaction_count.load(Ordering::Relaxed)
    }

    pub fn get_signature_status(&self, signature: &Signature) -> Result<()> {
        let last_ids = self.last_ids.read().unwrap();
        for (signatures, _, _) in last_ids.sigs.values() {
            if let Some(res) = signatures.get(signature) {
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
            .sigs
            .get(last_id)
            .and_then(|sigs| sigs.0.get(signature).cloned())
    }

    /// Hash the `accounts` HashMap. This represents a validator's interpretation
    ///  of the ledger up to the `last_id`, to be sent back to the leader when voting.
    pub fn hash_internal_state(&self) -> Hash {
        let mut ordered_accounts = BTreeMap::new();
        for (pubkey, account) in self.accounts.read().unwrap().iter() {
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

    pub fn get_current_leader(&self) -> Option<Pubkey> {
        let ls_lock = self.leader_scheduler.read().unwrap();
        let tick_height = self.tick_height.lock().unwrap();
        ls_lock.get_scheduled_leader(*tick_height)
    }

    pub fn get_tick_height(&self) -> u64 {
        *self.tick_height.lock().unwrap()
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
    use budget_program::BudgetState;
    use entry::next_entry;
    use entry::Entry;
    use entry_writer::{self, EntryWriter};
    use hash::hash;
    use jsonrpc_macros::pubsub::{Subscriber, SubscriptionId};
    use ledger;
    use logger;
    use signature::Keypair;
    use signature::{GenKeys, KeypairUtil};
    use std;
    use std::io::{BufReader, Cursor, Seek, SeekFrom};
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
        assert_eq!(bank.get_signature(&t1.last_id, &t1.signature), Some(Ok(())));
        // TODO: Transactions that fail to pay a fee could be dropped silently
        assert_eq!(
            bank.get_signature(&t2.last_id, &t2.signature),
            Some(Err(BankError::AccountInUse))
        );
    }

    #[test]
    fn test_one_tx_two_out_atomic_fail() {
        let mint = Mint::new(1);
        let key1 = Keypair::new().pubkey();
        let key2 = Keypair::new().pubkey();
        let bank = Bank::new(&mint);
        let spend = SystemProgram::Move { tokens: 1 };
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
            &mint.keypair(),
            &[key1, key2],
            mint.last_id(),
            0,
            vec![SystemProgram::id()],
            instructions,
        );
        let res = bank.process_transactions(&vec![t1.clone()]);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0], Err(BankError::ResultWithNegativeTokens(1)));
        assert_eq!(bank.get_balance(&mint.pubkey()), 1);
        assert_eq!(bank.get_balance(&key1), 0);
        assert_eq!(bank.get_balance(&key2), 0);
        assert_eq!(
            bank.get_signature(&t1.last_id, &t1.signature),
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
        assert_eq!(bank.get_signature(&t1.last_id, &t1.signature), Some(Ok(())));
    }

    #[test]
    fn test_negative_tokens() {
        logger::setup();
        let mint = Mint::new(1);
        let pubkey = Keypair::new().pubkey();
        let bank = Bank::new(&mint);
        let res = bank.transfer(-1, &mint.keypair(), pubkey, mint.last_id());
        println!("{:?}", bank.get_account(&pubkey));
        assert_matches!(res, Err(BankError::ResultWithNegativeTokens(0)));
        assert_eq!(bank.transaction_count(), 0);
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
        let signature = tx.signature;
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
        assert_eq!(bank.get_signature_status(&signature), Ok(()));
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
            bank.register_entry_id(&last_id);
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
                bank.register_entry_id(&last_id);
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
        let mint = Mint::new(1);
        let genesis = mint.create_entries();
        let bank = Bank::default();
        bank.process_ledger(genesis).unwrap();
        assert_eq!(bank.get_balance(&mint.pubkey()), 1);
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
        let mint = Mint::new(length as i64 + 1);
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
        let (ledger, dup) = ledger.tee();
        let bank = Bank::default();
        let (tick_height, ledger_height, tail) = bank.process_ledger(ledger).unwrap();
        assert_eq!(bank.get_balance(&pubkey), 1);
        assert_eq!(ledger_height, 4);
        assert_eq!(tick_height, 2);
        assert_eq!(tail.len(), 4);
        assert_eq!(tail, dup.collect_vec());
        let last_entry = &tail[tail.len() - 1];
        // last entry is a tick
        assert_eq!(0, last_entry.transactions.len());
        // tick is registered
        assert_eq!(bank.last_id(), last_entry.id);
    }

    #[test]
    fn test_process_ledger_around_window_size() {
        // TODO: put me back in when Criterion is up
        //        for _ in 0..10 {
        //            let (ledger, _) = create_sample_ledger(WINDOW_SIZE as usize);
        //            let bank = Bank::default();
        //            let (_, _) = bank.process_ledger(ledger).unwrap();
        //        }

        let window_size = 128;
        for entry_count in window_size - 3..window_size + 2 {
            let (ledger, pubkey) = create_sample_ledger(entry_count);
            let bank = Bank::default();
            let (tick_height, ledger_height, tail) = bank.process_ledger(ledger).unwrap();
            assert_eq!(bank.get_balance(&pubkey), 1);
            assert_eq!(ledger_height, entry_count as u64 + 3);
            assert_eq!(tick_height, 2);
            assert!(tail.len() <= WINDOW_SIZE as usize);
            let last_entry = &tail[tail.len() - 1];
            assert_eq!(bank.last_id(), last_entry.id);
        }
    }

    // Write the given entries to a file and then return a file iterator to them.
    fn to_file_iter(entries: impl Iterator<Item = Entry>) -> impl Iterator<Item = Entry> {
        let mut file = Cursor::new(vec![]);
        EntryWriter::write_entries(&mut file, entries).unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();

        let reader = BufReader::new(file);
        entry_writer::read_entries(reader).map(|x| x.unwrap())
    }

    #[test]
    fn test_process_ledger_from_file() {
        let (ledger, pubkey) = create_sample_ledger(1);
        let ledger = to_file_iter(ledger);

        let bank = Bank::default();
        bank.process_ledger(ledger).unwrap();
        assert_eq!(bank.get_balance(&pubkey), 1);
    }

    #[test]
    fn test_process_ledger_from_files() {
        let mint = Mint::new(2);
        let genesis = to_file_iter(mint.create_entries().into_iter());
        let block = to_file_iter(create_sample_block_with_ticks(&mint, 1, 1));

        let bank = Bank::default();
        bank.process_ledger(genesis.chain(block)).unwrap();
        assert_eq!(bank.get_balance(&mint.pubkey()), 1);
    }

    #[test]
    fn test_hash_internal_state() {
        let mint = Mint::new(2_000);
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
            BudgetState::id(),
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
            let expected = format!(r#"{{"jsonrpc":"2.0","method":"accountNotification","params":{{"result":{{"executable":false,"loader_program_id":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],"program_id":[1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],"tokens":1,"userdata":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]}},"subscription":0}}}}"#);
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
        let signature = tx.signature;
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
}

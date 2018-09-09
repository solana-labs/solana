//! The `bank` module tracks client accounts and the progress of smart
//! contracts. It offers a high-level API that signs transactions
//! on behalf of the caller, and a low-level API for when they have
//! already been signed and verified.

use bincode::{deserialize, serialize};
use chrono::prelude::*;
use counter::Counter;
use entry::Entry;
use hash::{hash, Hash};
use itertools::Itertools;
use ledger::Block;
use log::Level;
use mint::Mint;
use payment_plan::{Payment, PaymentPlan, Witness};
use signature::{Keypair, Pubkey, Signature};
use std;
use std::collections::hash_map::Entry::Occupied;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::result;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::RwLock;
use std::time::Instant;
use timing::{duration_as_us, timestamp};
use transaction::{Instruction, Plan, Transaction};
use window::WINDOW_SIZE;

/// The number of most recent `last_id` values that the bank will track the signatures
/// of. Once the bank discards a `last_id`, it will reject any transactions that use
/// that `last_id` in a transaction. Lowering this value reduces memory consumption,
/// but requires clients to update its `last_id` more frequently. Raising the value
/// lengthens the time a client must wait to be certain a missing transaction will
/// not be processed by the network.
pub const MAX_ENTRY_IDS: usize = 1024 * 16;

pub const VERIFY_BLOCK_SIZE: usize = 16;

/// Reasons a transaction might be rejected.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum BankError {
    /// Attempt to debit from `Pubkey`, but no found no record of a prior credit.
    AccountNotFound(Pubkey),

    /// The requested debit from `Pubkey` has the potential to draw the balance
    /// below zero. This can occur when a debit and credit are processed in parallel.
    /// The bank may reject the debit or push it to a future entry.
    InsufficientFunds(Pubkey),

    /// The bank has seen `Signature` before. This can occur under normal operation
    /// when a UDP packet is duplicated, as a user error from a client not updating
    /// its `last_id`, or as a double-spend attack.
    DuplicateSignature(Signature),

    /// The bank has not seen the given `last_id` or the transaction is too old and
    /// the `last_id` has been discarded.
    LastIdNotFound(Hash),

    /// The transaction is invalid and has requested a debit or credit of negative
    /// tokens.
    NegativeTokens,

    /// Proof of History verification failed.
    LedgerVerificationFailed,
    /// Contract's transaction token balance does not equal the balance after the transaction
    UnbalancedTransaction(Signature),
    /// Contract location Pubkey already contains userdata
    ContractAlreadyPending(Pubkey),
}

pub type Result<T> = result::Result<T, BankError>;
/// An Account with userdata that is stored on chain
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Account {
    /// tokens in the account
    pub tokens: i64,
    /// user data
    /// A transaction can write to its userdata
    pub userdata: Vec<u8>,
}
#[derive(Default)]
struct ErrorCounters {
    account_not_found_validator: usize,
    account_not_found_leader: usize,
    account_not_found_vote: usize,
}
/// The state of all accounts and contracts after processing its entries.
pub struct Bank {
    /// A map of account public keys to the balance in that account.
    accounts: RwLock<HashMap<Pubkey, Account>>,

    /// A FIFO queue of `last_id` items, where each item is a set of signatures
    /// that have been processed using that `last_id`. Rejected `last_id`
    /// values are so old that the `last_id` has been pulled out of the queue.
    last_ids: RwLock<VecDeque<Hash>>,

    /// Mapping of hashes to signature sets along with timestamp. The bank uses this data to
    /// reject transactions with signatures its seen before
    last_ids_sigs: RwLock<HashMap<Hash, (HashSet<Signature>, u64)>>,

    /// The number of transactions the bank has processed without error since the
    /// start of the ledger.
    transaction_count: AtomicUsize,

    /// This bool allows us to submit metrics that are specific for leaders or validators
    /// It is set to `true` by fullnode before creating the bank.
    pub is_leader: bool,

    // The latest finality time for the network
    finality_time: AtomicUsize,
}

impl Default for Bank {
    fn default() -> Self {
        Bank {
            accounts: RwLock::new(HashMap::new()),
            last_ids: RwLock::new(VecDeque::new()),
            last_ids_sigs: RwLock::new(HashMap::new()),
            transaction_count: AtomicUsize::new(0),
            is_leader: true,
            finality_time: AtomicUsize::new(std::usize::MAX),
        }
    }
}

impl Bank {
    /// Create a default Bank
    pub fn new_default(is_leader: bool) -> Self {
        let mut bank = Bank::default();
        bank.is_leader = is_leader;
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

    /// Commit funds to the given account
    fn apply_payment(payment: &Payment, account: &mut Account) {
        trace!("apply payments {}", payment.tokens);
        account.tokens += payment.tokens;
    }

    /// Return the last entry ID registered.
    pub fn last_id(&self) -> Hash {
        let last_ids = self.last_ids.read().expect("'last_ids' read lock");
        let last_item = last_ids
            .iter()
            .last()
            .expect("get last item from 'last_ids' list");
        *last_item
    }

    /// Store the given signature. The bank will reject any transaction with the same signature.
    fn reserve_signature(signatures: &mut HashSet<Signature>, signature: &Signature) -> Result<()> {
        if let Some(signature) = signatures.get(signature) {
            return Err(BankError::DuplicateSignature(*signature));
        }
        signatures.insert(*signature);
        Ok(())
    }

    /// Forget all signatures. Useful for benchmarking.
    pub fn clear_signatures(&self) {
        for (_, sigs) in self.last_ids_sigs.write().unwrap().iter_mut() {
            sigs.0.clear();
        }
    }

    fn reserve_signature_with_last_id(&self, signature: &Signature, last_id: &Hash) -> Result<()> {
        if let Some(entry) = self
            .last_ids_sigs
            .write()
            .expect("'last_ids' read lock in reserve_signature_with_last_id")
            .get_mut(last_id)
        {
            return Self::reserve_signature(&mut entry.0, signature);
        }
        Err(BankError::LastIdNotFound(*last_id))
    }

    /// Look through the last_ids and find all the valid ids
    /// This is batched to avoid holding the lock for a significant amount of time
    ///
    /// Return a vec of tuple of (valid index, timestamp)
    /// index is into the passed ids slice to avoid copying hashes
    pub fn count_valid_ids(&self, ids: &[Hash]) -> Vec<(usize, u64)> {
        let last_ids = self.last_ids_sigs.read().unwrap();
        let mut ret = Vec::new();
        for (i, id) in ids.iter().enumerate() {
            if let Some(entry) = last_ids.get(id) {
                ret.push((i, entry.1));
            }
        }
        ret
    }

    /// Tell the bank which Entry IDs exist on the ledger. This function
    /// assumes subsequent calls correspond to later entries, and will boot
    /// the oldest ones once its internal cache is full. Once boot, the
    /// bank will reject transactions using that `last_id`.
    pub fn register_entry_id(&self, last_id: &Hash) {
        let mut last_ids = self
            .last_ids
            .write()
            .expect("'last_ids' write lock in register_entry_id");
        let mut last_ids_sigs = self
            .last_ids_sigs
            .write()
            .expect("last_ids_sigs write lock");
        if last_ids.len() >= MAX_ENTRY_IDS {
            let id = last_ids.pop_front().unwrap();
            last_ids_sigs.remove(&id);
        }
        last_ids_sigs.insert(*last_id, (HashSet::new(), timestamp()));
        last_ids.push_back(*last_id);
    }

    /// Deduct tokens from the source account if it has sufficient funds and the contract isn't
    /// pending
    fn apply_debits_to_budget_payment_plan(
        tx: &Transaction,
        accounts: &mut [Account],
        instruction: &Instruction,
    ) -> Result<()> {
        {
            let tokens = if !accounts[0].userdata.is_empty() {
                0
            } else {
                accounts[0].tokens
            };
            if let Instruction::NewContract(contract) = &instruction {
                if contract.tokens < 0 {
                    return Err(BankError::NegativeTokens);
                }

                if tokens < contract.tokens {
                    return Err(BankError::InsufficientFunds(tx.keys[0]));
                } else {
                    let bal = &mut accounts[0];
                    bal.tokens -= contract.tokens;
                }
            };
        }
        Ok(())
    }

    /// Apply only a transaction's credits.
    /// Note: It is safe to apply credits from multiple transactions in parallel.
    fn apply_credits_to_budget_payment_plan(
        tx: &Transaction,
        accounts: &mut [Account],
        instruction: &Instruction,
    ) -> Result<()> {
        match instruction {
            Instruction::NewContract(contract) => {
                let plan = contract.plan.clone();
                if let Some(payment) = plan.final_payment() {
                    Self::apply_payment(&payment, &mut accounts[1]);
                    Ok(())
                } else if !accounts[1].userdata.is_empty() {
                    Err(BankError::ContractAlreadyPending(tx.keys[1]))
                } else {
                    let mut pending = HashMap::new();
                    pending.insert(tx.signature, plan);
                    //TODO this is a temporary on demand allocation
                    //until system contract requires explicit allocation of memory
                    accounts[1].userdata = serialize(&pending).unwrap();
                    accounts[1].tokens += contract.tokens;
                    Ok(())
                }
            }
            Instruction::ApplyTimestamp(dt) => {
                Self::apply_timestamp(tx.keys[0], *dt, &mut accounts[1]);
                Ok(())
            }
            Instruction::ApplySignature(signature) => {
                Self::apply_signature(tx.keys[0], *signature, accounts);
                Ok(())
            }
            Instruction::NewVote(_vote) => {
                // TODO: record the vote in the stake table...
                trace!("GOT VOTE! last_id={}", tx.last_id);
                Ok(())
            }
        }
    }
    /// Budget DSL contract interface
    /// * tx - the transaction
    /// * accounts[0] - The source of the tokens
    /// * accounts[1] - The contract context.  Once the contract has been completed, the tokens can
    /// be spent from this account .
    pub fn process_transaction_of_budget_instruction(
        tx: &Transaction,
        accounts: &mut [Account],
    ) -> Result<()> {
        let instruction = tx.instruction();
        Self::apply_debits_to_budget_payment_plan(tx, accounts, &instruction)?;
        Self::apply_credits_to_budget_payment_plan(tx, accounts, &instruction)
    }
    //TODO the contract needs to provide a "get_balance" introspection call of the userdata
    pub fn get_balance_of_budget_payment_plan(account: &Account) -> i64 {
        if let Ok(pending) = deserialize(&account.userdata) {
            let pending: HashMap<Signature, Plan> = pending;
            if !pending.is_empty() {
                0
            } else {
                account.tokens
            }
        } else {
            account.tokens
        }
    }

    /// Process a Transaction. If it contains a payment plan that requires a witness
    /// to progress, the payment plan will be stored in the bank.
    pub fn process_transaction(&self, tx: &Transaction) -> Result<()> {
        match self.process_transactions(vec![tx.clone()])[0] {
            Err(ref e) => {
                info!("process_transaction error: {:?}", e);
                Err((*e).clone())
            }
            Ok(_) => Ok(()),
        }
    }

    fn load_account(
        &self,
        tx: &Transaction,
        accounts: &HashMap<Pubkey, Account>,
        error_counters: &mut ErrorCounters,
    ) -> Result<Vec<Account>> {
        // Copy all the accounts
        if accounts.get(&tx.keys[0]).is_none() {
            if !self.is_leader {
                error_counters.account_not_found_validator += 1;
            } else {
                error_counters.account_not_found_leader += 1;
            }
            if let Instruction::NewVote(_vote) = tx.instruction() {
                error_counters.account_not_found_vote += 1;
            }
            Err(BankError::AccountNotFound(*tx.from()))
        } else if accounts.get(&tx.keys[0]).unwrap().tokens < tx.fee {
            Err(BankError::InsufficientFunds(*tx.from()))
        } else {
            let mut called_accounts: Vec<Account> = tx
                .keys
                .iter()
                .map(|key| accounts.get(key).cloned().unwrap_or(Account::default()))
                .collect();
            // There is no way to predict what contract will execute without an error
            // If a fee can pay for execution then the contract will be scheduled
            self.reserve_signature_with_last_id(&tx.signature, &tx.last_id)?;
            called_accounts[0].tokens -= tx.fee;
            Ok(called_accounts)
        }
    }
    fn load_accounts(
        &self,
        txs: &Vec<Transaction>,
        accounts: &HashMap<Pubkey, Account>,
        error_counters: &mut ErrorCounters,
    ) -> Vec<Result<Vec<Account>>> {
        txs.iter()
            .map(|tx| self.load_account(tx, accounts, error_counters))
            .collect()
    }

    pub fn execute_transaction(tx: Transaction, accounts: &mut [Account]) -> Result<Transaction> {
        let pre_total: i64 = accounts.iter().map(|a| a.tokens).sum();

        // TODO next steps is to add hooks to call arbitrary contracts here
        // Call the contract method
        // It's up to the contract to implement its own rules on moving funds
        let e = Self::process_transaction_of_budget_instruction(&tx, accounts);

        // Verify the transaction
        // TODO, At the moment there is only 1 contract, so 1-3 are not checked
        // 1. For accounts assigned to the contract, the total sum of all the tokens in these accounts cannot increase.
        // 2. For accounts unassigned to the contract, the individual balance of each accounts cannot decrease.
        // 3. For accounts unassigned to the contract, the userdata cannot change.

        // 4. The total sum of all the tokens in all the pages cannot change.
        let post_total: i64 = accounts.iter().map(|a| a.tokens).sum();
        if pre_total != post_total {
            Err(BankError::UnbalancedTransaction(tx.signature))
        } else if let Err(err) = e {
            Err(err)
        } else {
            Ok(tx)
        }
    }

    pub fn store_accounts(
        res: &Vec<Result<Transaction>>,
        loaded: &Vec<Result<Vec<Account>>>,
        accounts: &mut HashMap<Pubkey, Account>,
    ) {
        loaded.iter().zip(res.iter()).for_each(|(racc, rtx)| {
            if let (Ok(acc), Ok(tx)) = (racc, rtx) {
                tx.keys.iter().zip(acc.iter()).for_each(|(key, account)| {
                    //purge if 0
                    if account.tokens == 0 {
                        accounts.remove(&key);
                    } else {
                        *accounts.entry(*key).or_insert_with(Account::default) = account.clone();
                        assert_eq!(accounts.get(key).unwrap().tokens, account.tokens);
                    }
                });
            };
        });
    }

    /// Process a batch of transactions.
    #[must_use]
    pub fn process_transactions(&self, txs: Vec<Transaction>) -> Vec<Result<Transaction>> {
        debug!("processing Transactions {}", txs.len());
        // TODO right now a single write lock is held for the duration of processing all the
        // transactions
        // To break this lock each account needs to be locked to prevent concurrent access
        let mut accounts = self.accounts.write().unwrap();
        let txs_len = txs.len();
        let mut error_counters = ErrorCounters::default();
        let now = Instant::now();
        let mut loaded_accounts = self.load_accounts(&txs, &mut accounts, &mut error_counters);
        let load_elapsed = now.elapsed();
        let now = Instant::now();

        let res: Vec<Result<Transaction>> = loaded_accounts
            .iter_mut()
            .zip(txs.into_iter())
            .map(|(acc, tx)| match acc {
                Err(e) => Err(e.clone()),
                Ok(ref mut accounts) => Self::execute_transaction(tx, accounts),
            })
            .collect();
        let execution_elapsed = now.elapsed();
        let now = Instant::now();
        Self::store_accounts(&res, &loaded_accounts, &mut accounts);
        let write_elapsed = now.elapsed();
        debug!(
            "load: {} us execution: {} us write: {} us tx: {}",
            duration_as_us(&load_elapsed),
            duration_as_us(&execution_elapsed),
            duration_as_us(&write_elapsed),
            txs_len
        );
        let mut tx_count = 0;
        let mut err_count = 0;
        for r in &res {
            if r.is_ok() {
                tx_count += 1;
            } else {
                if err_count == 0 {
                    trace!("tx error: {:?}", r);
                }
                err_count += 1;
            }
        }
        if err_count > 0 {
            info!("{} errors of {} txs", err_count, err_count + tx_count);
            if !self.is_leader {
                inc_new_counter_info!("bank-process_transactions_err-validator", err_count);
                inc_new_counter_info!(
                    "bank-appy_debits-account_not_found-validator",
                    error_counters.account_not_found_validator
                );
            } else {
                inc_new_counter_info!("bank-process_transactions_err-leader", err_count);
                inc_new_counter_info!(
                    "bank-appy_debits-account_not_found-leader",
                    error_counters.account_not_found_leader
                );
                inc_new_counter_info!(
                    "bank-appy_debits-vote_account_not_found",
                    error_counters.account_not_found_vote
                );
            }
        }
        self.transaction_count
            .fetch_add(tx_count, Ordering::Relaxed);
        res
    }

    pub fn process_entry(&self, entry: Entry) -> Result<()> {
        if !entry.transactions.is_empty() {
            for result in self.process_transactions(entry.transactions) {
                result?;
            }
        }
        if !entry.has_more {
            self.register_entry_id(&entry.id);
        }
        Ok(())
    }

    /// Process an ordered list of entries, populating a circular buffer "tail"
    ///   as we go.
    fn process_entries_tail(
        &self,
        entries: Vec<Entry>,
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
            self.process_entry(entry)?;
        }

        Ok(entry_count)
    }

    /// Process an ordered list of entries.
    pub fn process_entries(&self, entries: Vec<Entry>) -> Result<()> {
        for entry in entries {
            self.process_entry(entry)?;
        }
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
        let mut entry_count = *tail_idx as u64;
        let mut id = start_hash;
        for block in &entries.into_iter().chunks(VERIFY_BLOCK_SIZE) {
            let block: Vec<_> = block.collect();
            if !block.verify(&id) {
                warn!("Ledger proof of history failed at entry: {}", entry_count);
                return Err(BankError::LedgerVerificationFailed);
            }
            id = block.last().unwrap().id;
            entry_count += self.process_entries_tail(block, tail, tail_idx)?;
        }
        Ok(entry_count)
    }

    /// Process a full ledger.
    pub fn process_ledger<I>(&self, entries: I) -> Result<(u64, Vec<Entry>)>
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
            let instruction = tx.instruction();
            let deposit = if let Instruction::NewContract(contract) = instruction {
                contract.plan.final_payment()
            } else {
                None
            }.expect("invalid ledger, needs to start with a contract");
            {
                let mut accounts = self.accounts.write().unwrap();
                let entry = accounts.entry(tx.keys[0]).or_insert_with(Account::default);
                Self::apply_payment(&deposit, entry);
                trace!("applied genesis payment {:?} {:?}", deposit, entry);
            }
        }
        self.register_entry_id(&entry0.id);
        self.register_entry_id(&entry1.id);
        let entry1_id = entry1.id;

        let mut tail = Vec::with_capacity(WINDOW_SIZE as usize);
        tail.push(entry0);
        tail.push(entry1);
        let mut tail_idx = 2;
        let entry_count = self.process_blocks(entry1_id, entries, &mut tail, &mut tail_idx)?;

        // check f we need to rotate tail
        if tail.len() == WINDOW_SIZE as usize {
            tail.rotate_left(tail_idx)
        }

        Ok((entry_count, tail))
    }

    /// Process a Witness Signature. Any payment plans waiting on this signature
    /// will progress one step.
    fn apply_signature(from: Pubkey, signature: Signature, account: &mut [Account]) {
        let mut pending: HashMap<Signature, Plan> =
            deserialize(&account[1].userdata).unwrap_or(HashMap::new());
        if let Occupied(mut e) = pending.entry(signature) {
            e.get_mut().apply_witness(&Witness::Signature, &from);
            if let Some(payment) = e.get().final_payment() {
                //move the tokens back to the from account
                account[0].tokens += payment.tokens;
                account[1].tokens -= payment.tokens;
                e.remove_entry();
            }
        };
        //TODO this allocation needs to be changed once the runtime only allows for explitly
        //allocated memory
        account[1].userdata = if pending.is_empty() {
            vec![]
        } else {
            serialize(&pending).unwrap()
        };
    }

    /// Process a Witness Timestamp. Any payment plans waiting on this timestamp
    /// will progress one step.
    fn apply_timestamp(from: Pubkey, dt: DateTime<Utc>, account: &mut Account) {
        let mut pending: HashMap<Signature, Plan> =
            deserialize(&account.userdata).unwrap_or(HashMap::new());
        // Check to see if any timelocked transactions can be completed.
        let mut completed = vec![];

        // Hold 'pending' write lock until the end of this function. Otherwise another thread can
        // double-spend if it enters before the modified plan is removed from 'pending'.
        for (key, plan) in pending.iter_mut() {
            plan.apply_witness(&Witness::Timestamp(dt), &from);
            if let Some(_payment) = plan.final_payment() {
                completed.push(key.clone());
            }
        }

        for key in completed {
            pending.remove(&key);
        }
        //TODO this allocation needs to be changed once the runtime only allows for explitly
        //allocated memory
        account.userdata = if pending.is_empty() {
            vec![]
        } else {
            serialize(&pending).unwrap()
        };
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
        let tx = Transaction::new(keypair, to, n, last_id);
        let signature = tx.signature;
        self.process_transaction(&tx).map(|_| signature)
    }

    /// Create, sign, and process a postdated Transaction from `keypair`
    /// to `to` of `n` tokens on `dt` where `last_id` is the last Entry ID
    /// observed by the client.
    pub fn transfer_on_date(
        &self,
        n: i64,
        keypair: &Keypair,
        to: Pubkey,
        dt: DateTime<Utc>,
        last_id: Hash,
    ) -> Result<Signature> {
        let tx = Transaction::new_on_date(keypair, to, dt, n, last_id);
        let signature = tx.signature;
        self.process_transaction(&tx).map(|_| signature)
    }

    pub fn get_balance(&self, pubkey: &Pubkey) -> i64 {
        self.get_account(pubkey)
            .map(|x| Self::get_balance_of_budget_payment_plan(&x))
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

    pub fn has_signature(&self, signature: &Signature) -> bool {
        let last_ids_sigs = self
            .last_ids_sigs
            .read()
            .expect("'last_ids_sigs' read lock");
        for (_hash, signatures) in last_ids_sigs.iter() {
            if signatures.0.contains(signature) {
                return true;
            }
        }
        false
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::serialize;
    use entry::next_entry;
    use entry::Entry;
    use entry_writer::{self, EntryWriter};
    use hash::hash;
    use ledger;
    use packet::BLOB_DATA_SIZE;
    use signature::{GenKeys, KeypairUtil};
    use std;
    use std::io::{BufReader, Cursor, Seek, SeekFrom};
    use std::mem::size_of;

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
    fn test_negative_tokens() {
        let mint = Mint::new(1);
        let pubkey = Keypair::new().pubkey();
        let bank = Bank::new(&mint);
        assert_eq!(
            bank.transfer(-1, &mint.keypair(), pubkey, mint.last_id()),
            Err(BankError::NegativeTokens)
        );
        assert_eq!(bank.transaction_count(), 0);
    }

    // TODO: This test verifies potentially undesirable behavior
    // See github issue 1157 (https://github.com/solana-labs/solana/issues/1157)
    #[test]
    fn test_detect_failed_duplicate_transactions_issue_1157() {
        let mint = Mint::new(1);
        let bank = Bank::new(&mint);

        let tx = Transaction::new(&mint.keypair(), mint.keypair().pubkey(), -1, mint.last_id());
        let signature = tx.signature;
        assert!(!bank.has_signature(&signature));
        assert_eq!(
            bank.process_transaction(&tx),
            Err(BankError::NegativeTokens)
        );
        assert!(bank.has_signature(&signature));
    }

    #[test]
    fn test_account_not_found() {
        let mint = Mint::new(1);
        let bank = Bank::new(&mint);
        let keypair = Keypair::new();
        assert_eq!(
            bank.transfer(1, &keypair, mint.pubkey(), mint.last_id()),
            Err(BankError::AccountNotFound(keypair.pubkey()))
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
        assert_eq!(
            bank.transfer(10_001, &mint.keypair(), pubkey, mint.last_id()),
            Err(BankError::InsufficientFunds(mint.pubkey()))
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
    fn test_transfer_on_date() {
        let mint = Mint::new(2);
        let bank = Bank::new(&mint);
        let pubkey = Keypair::new().pubkey();
        let dt = Utc::now();
        bank.transfer_on_date(1, &mint.keypair(), pubkey, dt, mint.last_id())
            .unwrap();

        // Mint's balance will be 1 because 1 of the tokens is locked up
        assert_eq!(bank.get_balance(&mint.pubkey()), 1);

        // tx count is 1, because debits were applied.
        assert_eq!(bank.transaction_count(), 1);

        // pubkey's balance will be 0 because the funds have not been
        // sent.
        assert_eq!(bank.get_balance(&pubkey), 0);

        // Now, acknowledge the time in the condition occurred and
        // that pubkey's funds are now available.
        let tx = Transaction::new_timestamp(&mint.keypair(), pubkey, dt, bank.last_id());
        let res = bank.process_transaction(&tx);
        assert!(res.is_ok());
        assert_eq!(bank.get_balance(&pubkey), 1);

        // tx count is 2
        assert_eq!(bank.transaction_count(), 2);

        // try to replay the timestamp contract
        bank.register_entry_id(&hash(bank.last_id().as_ref()));
        let tx = Transaction::new_timestamp(&mint.keypair(), pubkey, dt, bank.last_id());
        let res = bank.process_transaction(&tx);
        assert!(res.is_ok());

        assert_eq!(bank.get_balance(&pubkey), 1);
    }

    #[test]
    fn test_cancel_transfer() {
        // mint needs to have a balance to modify the external contract
        let mint = Mint::new(2);
        let bank = Bank::new(&mint);
        let pubkey = Keypair::new().pubkey();
        let dt = Utc::now();
        let signature = bank
            .transfer_on_date(1, &mint.keypair(), pubkey, dt, mint.last_id())
            .unwrap();

        // Assert the debit counts as a transaction.
        assert_eq!(bank.transaction_count(), 1);

        // Mint's balance will be 1 because 1 of the tokens is locked up.
        assert_eq!(bank.get_balance(&mint.pubkey()), 1);

        // pubkey's balance will be 0 because the funds are locked up
        assert_eq!(bank.get_balance(&pubkey), 0);

        // Now, cancel the transaction. Mint gets her funds back, pubkey never sees them.
        let tx = Transaction::new_signature(&mint.keypair(), pubkey, signature, bank.last_id());
        let res = bank.process_transaction(&tx);
        assert!(res.is_ok());
        assert_eq!(bank.get_balance(&pubkey), 0);
        assert_eq!(bank.get_balance(&mint.pubkey()), 2);

        // Assert cancel counts as a tx
        assert_eq!(bank.transaction_count(), 2);

        // try to replay the signature contract
        bank.register_entry_id(&hash(bank.last_id().as_ref()));
        let tx = Transaction::new_signature(&mint.keypair(), pubkey, signature, bank.last_id());
        let res = bank.process_transaction(&tx); //<-- attack! try to get budget dsl to pay out with another signature
        assert!(res.is_ok());
        // balance is is still 2 for the mint
        assert_eq!(bank.get_balance(&mint.pubkey()), 2);
        // balance is is still 0 for the contract
        assert_eq!(bank.get_balance(&pubkey), 0);
    }

    #[test]
    fn test_duplicate_transaction_signature() {
        let mint = Mint::new(1);
        let bank = Bank::new(&mint);
        let signature = Signature::default();
        assert!(
            bank.reserve_signature_with_last_id(&signature, &mint.last_id())
                .is_ok()
        );
        assert_eq!(
            bank.reserve_signature_with_last_id(&signature, &mint.last_id()),
            Err(BankError::DuplicateSignature(signature))
        );
    }

    #[test]
    fn test_clear_signatures() {
        let mint = Mint::new(1);
        let bank = Bank::new(&mint);
        let signature = Signature::default();
        bank.reserve_signature_with_last_id(&signature, &mint.last_id())
            .unwrap();
        bank.clear_signatures();
        assert!(
            bank.reserve_signature_with_last_id(&signature, &mint.last_id())
                .is_ok()
        );
    }

    #[test]
    fn test_has_signature() {
        let mint = Mint::new(1);
        let bank = Bank::new(&mint);
        let signature = Signature::default();
        bank.reserve_signature_with_last_id(&signature, &mint.last_id())
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
            bank.reserve_signature_with_last_id(&signature, &mint.last_id()),
            Err(BankError::LastIdNotFound(mint.last_id()))
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
            })
            .collect();
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
        let tx0 = Transaction::new(&mint.keypair(), keypair.pubkey(), 2, mint.last_id());
        let tx1 = Transaction::new(&keypair, mint.pubkey(), 1, mint.last_id());
        let txs = vec![tx0, tx1];
        let results = bank.process_transactions(txs);
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
        let tx = Transaction::new(&mint.keypair(), keypair.pubkey(), 1, entry.id);

        // First, ensure the TX is rejected because of the unregistered last ID
        assert_eq!(
            bank.process_transaction(&tx),
            Err(BankError::LastIdNotFound(entry.id))
        );

        // Now ensure the TX is accepted despite pointing to the ID of an empty entry.
        bank.process_entries(vec![entry]).unwrap();
        assert!(bank.process_transaction(&tx).is_ok());
    }

    #[test]
    fn test_process_genesis() {
        let mint = Mint::new(1);
        let genesis = mint.create_entries();
        let bank = Bank::default();
        bank.process_ledger(genesis).unwrap();
        assert_eq!(bank.get_balance(&mint.pubkey()), 1);
    }

    fn create_sample_block_with_next_entries(
        mint: &Mint,
        length: usize,
    ) -> impl Iterator<Item = Entry> {
        let keypair = Keypair::new();
        let hash = mint.last_id();
        let mut txs = Vec::with_capacity(length);
        for i in 0..length {
            txs.push(Transaction::new(
                &mint.keypair(),
                keypair.pubkey(),
                i as i64,
                hash,
            ));
        }
        let entries = ledger::next_entries(&hash, 0, txs);
        entries.into_iter()
    }

    fn create_sample_block_with_next_entries_using_keypairs(
        mint: &Mint,
        keypairs: &[Keypair],
    ) -> impl Iterator<Item = Entry> {
        let hash = mint.last_id();
        let transactions: Vec<_> = keypairs
            .iter()
            .map(|keypair| Transaction::new(&mint.keypair(), keypair.pubkey(), 1, hash))
            .collect();
        let entries = ledger::next_entries(&hash, 0, transactions);
        entries.into_iter()
    }

    fn create_sample_block(mint: &Mint, length: usize) -> impl Iterator<Item = Entry> {
        let mut entries = Vec::with_capacity(length);
        let mut hash = mint.last_id();
        let mut num_hashes = 0;
        for _ in 0..length {
            let keypair = Keypair::new();
            let tx = Transaction::new(&mint.keypair(), keypair.pubkey(), 1, hash);
            let entry = Entry::new_mut(&mut hash, &mut num_hashes, vec![tx], false);
            entries.push(entry);
        }
        entries.into_iter()
    }

    fn create_sample_ledger_with_next_entries(
        length: usize,
    ) -> (impl Iterator<Item = Entry>, Pubkey) {
        let mint = Mint::new((length * length) as i64);
        let genesis = mint.create_entries();
        let block = create_sample_block_with_next_entries(&mint, length);
        (genesis.into_iter().chain(block), mint.pubkey())
    }

    fn create_sample_ledger(length: usize) -> (impl Iterator<Item = Entry>, Pubkey) {
        let mint = Mint::new(1 + length as i64);
        let genesis = mint.create_entries();
        let block = create_sample_block(&mint, length);
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
    fn test_process_ledger() {
        let (ledger, pubkey) = create_sample_ledger(1);
        let (ledger, dup) = ledger.tee();
        let bank = Bank::default();
        let (ledger_height, tail) = bank.process_ledger(ledger).unwrap();
        assert_eq!(bank.get_balance(&pubkey), 1);
        assert_eq!(ledger_height, 3);
        assert_eq!(tail.len(), 3);
        assert_eq!(tail, dup.collect_vec());
        let last_entry = &tail[tail.len() - 1];
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

        let window_size = WINDOW_SIZE as usize;
        for entry_count in window_size - 3..window_size + 2 {
            let (ledger, pubkey) = create_sample_ledger(entry_count);
            let bank = Bank::default();
            let (ledger_height, tail) = bank.process_ledger(ledger).unwrap();
            assert_eq!(bank.get_balance(&pubkey), 1);
            assert_eq!(ledger_height, entry_count as u64 + 2);
            assert!(tail.len() <= window_size);
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
        let block = to_file_iter(create_sample_block(&mint, 1));

        let bank = Bank::default();
        bank.process_ledger(genesis.chain(block)).unwrap();
        assert_eq!(bank.get_balance(&mint.pubkey()), 1);
    }

    #[test]
    fn test_process_ledger_has_more_cross_block() {
        // size_of<Transaction> is quite large for serialized size, so
        // use 2 * verify_block_size to ensure we get enough txes to cross that
        // block boundary with has_more set
        let num_txs = (2 * VERIFY_BLOCK_SIZE) * BLOB_DATA_SIZE / size_of::<Transaction>();
        let (ledger, _pubkey) = create_sample_ledger_with_next_entries(num_txs);
        let bank = Bank::default();
        assert!(bank.process_ledger(ledger).is_ok());
    }
    #[test]
    fn test_new_default() {
        let def_bank = Bank::default();
        assert!(def_bank.is_leader);
        let leader_bank = Bank::new_default(true);
        assert!(leader_bank.is_leader);
        let validator_bank = Bank::new_default(false);
        assert!(!validator_bank.is_leader);
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
}

//! The `bank` module tracks client accounts and the progress of smart
//! contracts. It offers a high-level API that signs transactions
//! on behalf of the caller, and a low-level API for when they have
//! already been signed and verified.

use bincode::deserialize;
use bincode::serialize;
use budget_program::BudgetState;
use counter::Counter;
use dynamic_program::{DynamicProgram, KeyedAccount};
use entry::Entry;
use hash::{hash, Hash};
use itertools::Itertools;
use ledger::Block;
use log::Level;
use mint::Mint;
use payment_plan::Payment;
use signature::{Keypair, Pubkey, Signature};
use std;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::result;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::RwLock;
use std::time::Instant;
use storage_program::StorageProgram;
use system_program::SystemProgram;
use system_transaction::SystemTransaction;
use tictactoe_program::TicTacToeProgram;
use timing::{duration_as_us, timestamp};
use transaction::Transaction;
use window::WINDOW_SIZE;

/// An Account with userdata that is stored on chain
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Account {
    /// tokens in the account
    pub tokens: i64,
    /// user data
    /// A transaction can write to its userdata
    pub userdata: Vec<u8>,
    /// contract id this contract belongs to
    pub program_id: Pubkey,
}
impl Account {
    pub fn new(tokens: i64, space: usize, program_id: Pubkey) -> Account {
        Account {
            tokens,
            userdata: vec![0u8; space],
            program_id,
        }
    }
}

impl Default for Account {
    fn default() -> Self {
        Account {
            tokens: 0,
            userdata: vec![],
            program_id: SystemProgram::id(),
        }
    }
}
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

    /// The from `Pubkey` does not have sufficient balance to pay the fee to schedule the transaction
    InsufficientFundsForFee(Pubkey),

    /// The bank has seen `Signature` before. This can occur under normal operation
    /// when a UDP packet is duplicated, as a user error from a client not updating
    /// its `last_id`, or as a double-spend attack.
    DuplicateSignature(Signature),

    /// The bank has not seen the given `last_id` or the transaction is too old and
    /// the `last_id` has been discarded.
    LastIdNotFound(Hash),

    /// Proof of History verification failed.
    LedgerVerificationFailed,
    /// Contract's transaction token balance does not equal the balance after the transaction
    UnbalancedTransaction(Signature),
    /// Contract's transactions resulted in an account with a negative balance
    /// The difference from InsufficientFundsForFee is that the transaction was executed by the
    /// contract
    ResultWithNegativeTokens(Signature),

    /// Contract id is unknown
    UnknownContractId(Pubkey),

    /// Contract modified an accounts contract id
    ModifiedContractId(Signature),

    /// Contract spent the tokens of an account that doesn't belong to it
    ExternalAccountTokenSpend(Signature),

    /// The program returned an error
    ProgramRuntimeError,
}

pub type Result<T> = result::Result<T, BankError>;

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
    last_ids_sigs: RwLock<HashMap<Hash, (HashMap<Signature, Result<()>>, u64)>>,

    /// The number of transactions the bank has processed without error since the
    /// start of the ledger.
    transaction_count: AtomicUsize,

    /// This bool allows us to submit metrics that are specific for leaders or validators
    /// It is set to `true` by fullnode before creating the bank.
    pub is_leader: bool,

    // The latest finality time for the network
    finality_time: AtomicUsize,

    // loaded contracts hashed by program_id
    loaded_contracts: RwLock<HashMap<Pubkey, DynamicProgram>>,
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
            loaded_contracts: RwLock::new(HashMap::new()),
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
    fn reserve_signature(
        signatures: &mut HashMap<Signature, Result<()>>,
        signature: &Signature,
    ) -> Result<()> {
        if let Some(_result) = signatures.get(signature) {
            return Err(BankError::DuplicateSignature(*signature));
        }
        signatures.insert(*signature, Ok(()));
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

    fn update_signature_status(
        signatures: &mut HashMap<Signature, Result<()>>,
        signature: &Signature,
        result: &Result<()>,
    ) {
        let entry = signatures.entry(*signature).or_insert(Ok(()));
        *entry = result.clone();
    }

    fn update_signature_status_with_last_id(
        &self,
        signature: &Signature,
        result: &Result<()>,
        last_id: &Hash,
    ) {
        if let Some(entry) = self.last_ids_sigs.write().unwrap().get_mut(last_id) {
            Self::update_signature_status(&mut entry.0, signature, result);
        }
    }

    fn update_transaction_statuses(&self, txs: &[Transaction], res: &[Result<()>]) {
        for (i, tx) in txs.iter().enumerate() {
            self.update_signature_status_with_last_id(&tx.signature, &res[i], &tx.last_id);
        }
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
        last_ids_sigs.insert(*last_id, (HashMap::new(), timestamp()));
        last_ids.push_back(*last_id);
    }

    /// Process a Transaction. This is used for unit tests and simply calls the vector Bank::process_transactions method.
    pub fn process_transaction(&self, tx: &Transaction) -> Result<()> {
        match self.process_transactions(&[tx.clone()])[0] {
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
            if BudgetState::check_id(&tx.program_id) {
                use instruction::Instruction;
                if let Some(Instruction::NewVote(_vote)) = tx.instruction() {
                    error_counters.account_not_found_vote += 1;
                }
            }
            Err(BankError::AccountNotFound(*tx.from()))
        } else if accounts.get(&tx.keys[0]).unwrap().tokens < tx.fee {
            Err(BankError::InsufficientFundsForFee(*tx.from()))
        } else {
            let mut called_accounts: Vec<Account> = tx
                .keys
                .iter()
                .map(|key| accounts.get(key).cloned().unwrap_or_default())
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
        txs: &[Transaction],
        accounts: &HashMap<Pubkey, Account>,
        error_counters: &mut ErrorCounters,
    ) -> Vec<Result<Vec<Account>>> {
        txs.iter()
            .map(|tx| self.load_account(tx, accounts, error_counters))
            .collect()
    }

    pub fn verify_transaction(
        tx: &Transaction,
        pre_program_id: &Pubkey,
        pre_tokens: i64,
        account: &Account,
    ) -> Result<()> {
        // Verify the transaction
        // make sure that program_id is still the same or this was just assigned by the system call contract
        if !((*pre_program_id == account.program_id)
            || (SystemProgram::check_id(&tx.program_id)
                && SystemProgram::check_id(&pre_program_id)))
        {
            //TODO, this maybe redundant bpf should be able to guarantee this property
            return Err(BankError::ModifiedContractId(tx.signature));
        }
        // For accounts unassigned to the contract, the individual balance of each accounts cannot decrease.
        if tx.program_id != account.program_id && pre_tokens > account.tokens {
            return Err(BankError::ExternalAccountTokenSpend(tx.signature));
        }
        if account.tokens < 0 {
            return Err(BankError::ResultWithNegativeTokens(tx.signature));
        }
        Ok(())
    }

    fn loaded_contract(&self, tx: &Transaction, accounts: &mut [Account]) -> bool {
        let loaded_contracts = self.loaded_contracts.write().unwrap();
        match loaded_contracts.get(&tx.program_id) {
            Some(dc) => {
                let mut infos: Vec<_> = (&tx.keys)
                    .into_iter()
                    .zip(accounts)
                    .map(|(key, account)| KeyedAccount { key, account })
                    .collect();

                dc.call(&mut infos, &tx.userdata);
                true
            }
            None => false,
        }
    }

    /// Execute a transaction.
    /// This method calls the contract's process_transaction method and verifies that the result of
    /// the contract does not violate the bank's accounting rules.
    /// The accounts are committed back to the bank only if this function returns Ok(_).
    fn execute_transaction(&self, tx: &Transaction, accounts: &mut [Account]) -> Result<()> {
        let pre_total: i64 = accounts.iter().map(|a| a.tokens).sum();
        let pre_data: Vec<_> = accounts
            .iter_mut()
            .map(|a| (a.program_id, a.tokens))
            .collect();

        // Call the contract method
        // It's up to the contract to implement its own rules on moving funds
        if SystemProgram::check_id(&tx.program_id) {
            SystemProgram::process_transaction(&tx, accounts, &self.loaded_contracts)
        } else if BudgetState::check_id(&tx.program_id) {
            // TODO: the runtime should be checking read/write access to memory
            // we are trusting the hard coded contracts not to clobber or allocate
            if BudgetState::process_transaction(&tx, accounts).is_err() {
                return Err(BankError::ProgramRuntimeError);
            }
        } else if StorageProgram::check_id(&tx.program_id) {
            StorageProgram::process_transaction(&tx, accounts)
        } else if TicTacToeProgram::check_id(&tx.program_id) {
            if TicTacToeProgram::process_transaction(&tx, accounts).is_err() {
                return Err(BankError::ProgramRuntimeError);
            }
        } else if self.loaded_contract(&tx, accounts) {
        } else {
            return Err(BankError::UnknownContractId(tx.program_id));
        }
        // Verify the transaction
        for ((pre_program_id, pre_tokens), post_account) in pre_data.iter().zip(accounts.iter()) {
            Self::verify_transaction(&tx, pre_program_id, *pre_tokens, post_account)?;
        }
        // The total sum of all the tokens in all the pages cannot change.
        let post_total: i64 = accounts.iter().map(|a| a.tokens).sum();
        if pre_total != post_total {
            Err(BankError::UnbalancedTransaction(tx.signature))
        } else {
            Ok(())
        }
    }

    pub fn store_accounts(
        txs: &[Transaction],
        res: &[Result<()>],
        loaded: &[Result<Vec<Account>>],
        accounts: &mut HashMap<Pubkey, Account>,
    ) {
        for (i, racc) in loaded.iter().enumerate() {
            if res[i].is_err() || racc.is_err() {
                continue;
            }

            let tx = &txs[i];
            let acc = racc.as_ref().unwrap();
            for (key, account) in tx.keys.iter().zip(acc.iter()) {
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

    /// Process a batch of transactions.
    #[must_use]
    pub fn process_transactions(&self, txs: &[Transaction]) -> Vec<Result<()>> {
        debug!("processing transactions: {}", txs.len());
        // TODO right now a single write lock is held for the duration of processing all the
        // transactions
        // To break this lock each account needs to be locked to prevent concurrent access
        let mut accounts = self.accounts.write().unwrap();
        let txs_len = txs.len();
        let mut error_counters = ErrorCounters::default();
        let now = Instant::now();
        let mut loaded_accounts = self.load_accounts(&txs, &accounts, &mut error_counters);
        let load_elapsed = now.elapsed();
        let now = Instant::now();

        let res: Vec<_> = loaded_accounts
            .iter_mut()
            .zip(txs.iter())
            .map(|(acc, tx)| match acc {
                Err(e) => Err(e.clone()),
                Ok(ref mut accounts) => self.execute_transaction(tx, accounts),
            }).collect();
        let execution_elapsed = now.elapsed();
        let now = Instant::now();
        Self::store_accounts(&txs, &res, &loaded_accounts, &mut accounts);
        self.update_transaction_statuses(&txs, &res);
        let write_elapsed = now.elapsed();
        debug!(
            "load: {}us execution: {}us write: {}us txs_len={}",
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
                    debug!("tx error: {:?}", r);
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
        let cur_tx_count = self.transaction_count.load(Ordering::Relaxed);
        if ((cur_tx_count + tx_count) & !(262_144 - 1)) > cur_tx_count & !(262_144 - 1) {
            info!("accounts.len: {}", accounts.len());
        }
        self.transaction_count
            .fetch_add(tx_count, Ordering::Relaxed);
        res
    }

    pub fn process_entry(&self, entry: &Entry) -> Result<()> {
        if !entry.transactions.is_empty() {
            for result in self.process_transactions(&entry.transactions) {
                result?;
            }
        }
        self.register_entry_id(&entry.id);
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
            self.process_entry(&entry)?;
        }

        Ok(entry_count)
    }

    /// Process an ordered list of entries.
    pub fn process_entries(&self, entries: &[Entry]) -> Result<()> {
        for entry in entries {
            self.process_entry(&entry)?;
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
            assert!(SystemProgram::check_id(&tx.program_id), "Invalid ledger");
            let instruction: SystemProgram = deserialize(&tx.userdata).unwrap();
            let deposit = if let SystemProgram::Move { tokens } = instruction {
                Some(tokens)
            } else {
                None
            }.expect("invalid ledger, needs to start with a contract");
            {
                let mut accounts = self.accounts.write().unwrap();
                let account = accounts.entry(tx.keys[0]).or_insert_with(Account::default);
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
        let entry_count = self.process_blocks(entry1_id, entries, &mut tail, &mut tail_idx)?;

        // check f we need to rotate tail
        if tail.len() == WINDOW_SIZE as usize {
            tail.rotate_left(tail_idx)
        }

        Ok((entry_count, tail))
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

    pub fn has_signature(&self, signature: &Signature) -> bool {
        let last_ids_sigs = self
            .last_ids_sigs
            .read()
            .expect("'last_ids_sigs' read lock");
        for (_hash, signatures) in last_ids_sigs.iter() {
            if signatures.0.contains_key(signature) {
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
    use logger;
    use signature::{GenKeys, KeypairUtil};
    use std;
    use std::io::{BufReader, Cursor, Seek, SeekFrom};

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
        logger::setup();
        let mint = Mint::new(1);
        let pubkey = Keypair::new().pubkey();
        let bank = Bank::new(&mint);
        let res = bank.transfer(-1, &mint.keypair(), pubkey, mint.last_id());
        println!("{:?}", bank.get_account(&pubkey));
        assert_matches!(res, Err(BankError::ResultWithNegativeTokens(_)));
        assert_eq!(bank.transaction_count(), 0);
    }

    // TODO: This test verifies potentially undesirable behavior
    // See github issue 1157 (https://github.com/solana-labs/solana/issues/1157)
    #[test]
    fn test_detect_failed_duplicate_transactions_issue_1157() {
        let mint = Mint::new(1);
        let bank = Bank::new(&mint);
        let dest = Keypair::new();

        // source with 0 contract context
        let tx = Transaction::system_new(&mint.keypair(), dest.pubkey(), 2, mint.last_id());
        let signature = tx.signature;
        assert!(!bank.has_signature(&signature));
        let res = bank.process_transaction(&tx);
        // This is the potentially wrong behavior
        // result failed, but signature is registered
        assert!(!res.is_ok());
        assert!(bank.has_signature(&signature));
        // sanity check that tokens didn't move
        assert_eq!(bank.get_balance(&dest.pubkey()), 0);
        assert_eq!(bank.get_balance(&mint.pubkey()), 1);
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
        assert_matches!(
            bank.transfer(10_001, &mint.keypair(), pubkey, mint.last_id()),
            Err(BankError::ResultWithNegativeTokens(_))
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
            Err(BankError::LastIdNotFound(entry.id))
        );

        // Now ensure the TX is accepted despite pointing to the ID of an empty entry.
        bank.process_entries(&[entry]).unwrap();
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

    fn create_sample_block_with_next_entries_using_keypairs(
        mint: &Mint,
        keypairs: &[Keypair],
    ) -> impl Iterator<Item = Entry> {
        let hash = mint.last_id();
        let transactions: Vec<_> = keypairs
            .iter()
            .map(|keypair| Transaction::system_new(&mint.keypair(), keypair.pubkey(), 1, hash))
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
            let tx = Transaction::system_new(&mint.keypair(), keypair.pubkey(), 1, hash);
            let entry = Entry::new_mut(&mut hash, &mut num_hashes, vec![tx]);
            entries.push(entry);
        }
        entries.into_iter()
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

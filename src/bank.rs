//! The `bank` module tracks client balances and the progress of smart
//! contracts. It offers a high-level API that signs transactions
//! on behalf of the caller, and a low-level API for when they have
//! already been signed and verified.

extern crate libc;

use bincode::{deserialize, serialize};
use chrono::prelude::*;
use entry::Entry;
use hash::Hash;
use itertools::Itertools;
use ledger::Block;
use mint::Mint;
use page_table::{self, Call, Page, PageTable};
use payment_plan::{Payment, PaymentPlan, Witness};
use signature::{KeyPair, PublicKey, Signature};
use std::collections::hash_map::Entry::Occupied;
use std::collections::{HashMap, VecDeque};
use std::result;
use std::sync::{Arc, RwLock};
use std::time::Instant;
use streamer::WINDOW_SIZE;
use timing::{duration_as_us, timestamp};
use transaction::{Instruction, Plan, Transaction};

/// The number of most recent `last_id` values that the bank will track the signatures
/// of. Once the bank discards a `last_id`, it will reject any transactions that use
/// that `last_id` in a transaction. Lowering this value reduces memory consumption,
/// but requires clients to update its `last_id` more frequently. Raising the value
/// lengthens the time a client must wait to be certain a missing transaction will
/// not be processed by the network.
pub const MAX_ENTRY_IDS: usize = 1024 * 16;

pub const VERIFY_BLOCK_SIZE: usize = 16;

pub const BANK_PROCESS_TRANSACTION_METHOD: u8 = 129;

/// Reasons a transaction might be rejected.
#[derive(Debug, PartialEq, Eq)]
pub enum BankError {
    /// The requested debit from `PublicKey` has the potential to draw the balance
    /// below zero. This can occur when a debit and credit are processed in parallel.
    /// The bank may reject the debit or push it to a future entry.
    InsufficientFunds(PublicKey),

    /// The bank has not seen the given `last_id` or the transaction is too old and
    /// the `last_id` has been discarded.
    LastIdNotFound(Hash),

    /// The transaction is invalid and has requested a debit or credit of negative
    /// tokens.
    NegativeTokens,

    /// Proof of History verification failed.
    LedgerVerificationFailed,

    /// Account is concurrently processing
    AccountInUse,
}

pub type Result<T> = result::Result<T, BankError>;

/// The state of all accounts and contracts after processing its entries.
pub struct Bank {
    /// The page table
    page_table: Arc<PageTable>,
    ctx: RwLock<page_table::Context>,
    //TODO(anatoly): this functionality should be handled by the ledger
    //Ledger context in a call is
    //(PoH Count, PoH Hash)
    //Ledger should implement a O(1) `lookup(PoH Count) -> PoH Hash`
    //which shoudl filter invalid Calls
    last_ids: RwLock<VecDeque<Hash>>,
    /// Mapping of hashes to timestamp. The bank uses this data to
    /// reject transactions with old last_id hashes
    last_ids_set: RwLock<HashMap<Hash, u64>>,
}

impl Default for Bank {
    fn default() -> Self {
        Bank {
            page_table: Arc::new(PageTable::default()),
            ctx: RwLock::new(page_table::Context::default()),
            last_ids: RwLock::new(VecDeque::new()),
            last_ids_set: RwLock::new(HashMap::new()),
        }
    }
}

impl Bank {
    /// Create an Bank using a deposit.
    pub fn new_with_page_table(page_table: Arc<PageTable>) -> Self {
        Bank {
            page_table,
            ctx: RwLock::new(page_table::Context::default()),
            last_ids: RwLock::new(VecDeque::new()),
            last_ids_set: RwLock::new(HashMap::new()),
        }
    }
    /// Create an Bank using a deposit.
    pub fn new_from_deposit(deposit: &Payment) -> Self {
        let pt = Arc::new(PageTable::default());
        let bank = Self::new_with_page_table(pt);
        bank.force_deposit(deposit.to, deposit.tokens);
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

    /// Get the underlying PageTable
    /// this can be used to create multiple banks that process transactions in parallel
    pub fn page_table(&self) -> &Arc<PageTable> {
        &self.page_table
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

    /// Look through the last_ids and find all the valid ids
    /// This is batched to avoid holding the lock for a significant amount of time
    ///
    /// Return a vec of tuple of (valid index, timestamp)
    /// index is into the passed ids slice to avoid copying hashes
    pub fn count_valid_ids(&self, ids: &[Hash]) -> Vec<(usize, u64)> {
        let last_ids = self.last_ids_set.read().unwrap();
        let mut ret = Vec::new();
        for (i, id) in ids.iter().enumerate() {
            if let Some(entry) = last_ids.get(id) {
                ret.push((i, *entry));
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
        let mut last_ids_set = self.last_ids_set.write().expect("last_ids_set write lock");
        if last_ids.len() >= MAX_ENTRY_IDS {
            let id = last_ids.pop_front().unwrap();
            last_ids_set.remove(&id);
        }
        last_ids_set.insert(*last_id, timestamp());
        last_ids.push_back(*last_id);
    }

    /// Deduct tokens from the 'from' address the account has sufficient
    /// funds and isn't a duplicate.
    fn apply_debits(tx: &Call, instruction: &Instruction, bals: &mut [Page]) -> Result<()> {
        // check the spendable amount of tokens in the contract
        let spendable = Self::default_contract_get_balance(&bals[0]);
        if let Instruction::NewContract(contract) = &instruction {
            if contract.tokens < 0 {
                return Err(BankError::NegativeTokens);
            }
            if spendable < contract.tokens {
                return Err(BankError::InsufficientFunds(tx.data.keys[0]));
            } else {
                bals[0].balance -= contract.tokens;
            }
        }
        Ok(())
    }
    fn force_deposit(&self, to: PublicKey, tokens: i64) {
        self.page_table
            .force_deposit(to, page_table::DEFAULT_CONTRACT, tokens);
    }
    /// Deposit Payment to the Page balance
    fn apply_payment(payment: &Payment, page: &mut Page) {
        page.balance += payment.tokens;
    }

    /// Apply only a transaction's credits.
    fn apply_credits(tx: &Call, instruction: &Instruction, balances: &mut [Page]) {
        match &instruction {
            Instruction::NewContract(contract) => {
                let plan = contract.plan.clone();
                if let Some(payment) = plan.final_payment() {
                    Self::apply_payment(&payment, &mut balances[1]);
                } else {
                    let mut pending: HashMap<Signature, Plan> =
                        deserialize(&balances[1].memory).unwrap_or_default();
                    pending.insert(tx.proofs[0], plan);
                    //TODO(anatoly): this wont work in the future, the user will have to alloate first
                    //and call serealize_into
                    balances[1].memory = serialize(&pending).expect("serealize pending hashmap");
                    //The sol is stored in the contract, but the contract shouldn't allow it to
                    //be spendable
                    balances[1].balance += contract.tokens;
                }
            }
            Instruction::ApplyTimestamp(dt) => {
                if let Ok(mut pending) = deserialize(&balances[1].memory) {
                    //TODO(anatoly): capture error into the contract's data
                    let _ = Self::apply_timestamp(&balances[0], &mut pending, *dt);
                    //TODO(anatoly): this wont work in the future, the user will have to alloate first
                    //and call serealize_into
                    balances[1].memory = if pending.is_empty() {
                        //free up the memory when the contract is done
                        vec![]
                    } else {
                        serialize(&pending).expect("serealize pending hashmap")
                    };
                }
            }
            Instruction::ApplySignature(tx_sig) => {
                if let Ok(mut pending) = deserialize(&balances[1].memory) {
                    //TODO(anatoly): capture error into the contract's data
                    let _ = Self::apply_signature(balances, &mut pending, *tx_sig);
                    //TODO(anatoly): this wont work in the future, the user will have to alloate first
                    //and call serealize_into
                    balances[1].memory = if pending.is_empty() {
                        //free up the memory when the contract is done
                        vec![]
                    } else {
                        serialize(&pending).expect("serealize pending hashmap")
                    };
                }
            }
            Instruction::NewVote(_vote) => {
                trace!("GOT VOTE! last_id={:?}", &tx.data.last_hash.as_ref()[..8]);
                // TODO: record the vote in the stake table...
            }
        }
    }

    /// Process a Transaction. If it contains a payment plan that requires a witness
    /// to progress, the payment plan will be stored in the bank.
    pub fn default_contract_129_process_transaction(tx: &Call, pages: &mut [Page]) {
        let instruction = deserialize(&tx.data.user_data).expect("instruction deserialize");
        if Self::apply_debits(tx, &instruction, pages).is_ok() {
            Self::apply_credits(tx, &instruction, pages);
        }
    }

    /// Only used for testing
    pub fn process_transaction(&self, txs: &Transaction) -> Result<Transaction> {
        let rv = self.process_transactions(vec![txs.clone()]);
        match rv[0] {
            Err(BankError::AccountInUse) => Err(BankError::AccountInUse),
            Err(BankError::InsufficientFunds(key)) => Err(BankError::InsufficientFunds(key)),
            Err(BankError::LastIdNotFound(id)) => Err(BankError::LastIdNotFound(id)),

            Ok(ref t) => Ok(t.clone()),
            _ => {
                assert!(false, "unexpected return value from process_transactions");
                Err(BankError::AccountInUse)
            }
        }
    }
    /// Process a batch of transactions.
    #[must_use]
    pub fn process_calls(&self, txs: Vec<Call>) -> Vec<Result<Call>> {
        debug!("processing Transactions {}", txs.len());
        let mut ctx = self.ctx.write().unwrap();

        let txs_len = txs.len();
        let now = Instant::now();
        {
            let last_ids_set = self.last_ids_set.read().unwrap();
            for (i, t) in txs.iter().enumerate() {
                ctx.valid_ledger[i] = last_ids_set.get(&t.data.last_hash).is_some();
            }
        }
        self.page_table.acquire_validate_find(&txs, &mut ctx);
        self.page_table.allocate_keys_with_ctx(&txs, &mut ctx);
        self.page_table.load_pages_with_ctx(&txs, &mut ctx);

        let loaded = now.elapsed();
        let now = Instant::now();
        PageTable::execute_with_ctx(&txs, &mut ctx);
        let executed = now.elapsed();
        let now = Instant::now();
        self.page_table.commit_release_with_ctx(&txs, &ctx);

        debug!(
            "loaded: {} us executed: {:?} us commit: {:?} us tx: {}",
            duration_as_us(&loaded),
            duration_as_us(&executed),
            duration_as_us(&now.elapsed()),
            txs_len
        );
        let res: Vec<_> = txs.into_iter()
            .enumerate()
            .map(|(i, t)| {
                if !ctx.valid_ledger[i] {
                    Err(BankError::LastIdNotFound(t.data.last_hash))
                } else if !ctx.lock[i] {
                    Err(BankError::AccountInUse)
                } else if !ctx.checked[i] || !ctx.commit[i] {
                    Err(BankError::InsufficientFunds(t.data.keys[0]))
                } else {
                    Ok(t)
                }
            })
            .collect();

        let mut tx_count = 0;
        let mut err_count = 0;
        for r in &res {
            if r.is_ok() {
                tx_count += 1;
            } else {
                if err_count == 0 {
                    info!("tx error: {:?}", r);
                }
                err_count += 1;
            }
        }
        if err_count > 0 {
            info!("{} errors of {} txs", err_count, err_count + tx_count);
        }
        res
    }
    pub fn process_transactions(&self, txs: Vec<Transaction>) -> Vec<Result<Transaction>> {
        self.process_calls(txs.into_iter().map(|t| t.call).collect())
            .into_iter()
            .map(|v| v.map(|call| Transaction { call }))
            .collect()
    }

    fn process_entry(&self, entry: Entry) -> Result<()> {
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
    pub fn process_entries(&self, entries: Vec<Entry>) -> Result<u64> {
        let mut entry_count = 0;
        for entry in entries {
            entry_count += 1;
            self.process_entry(entry)?;
        }
        Ok(entry_count)
    }

    /// Append entry blocks to the ledger, verifying them along the way.
    fn process_blocks<I>(
        &self,
        entries: I,
        tail: &mut Vec<Entry>,
        tail_idx: &mut usize,
    ) -> Result<u64>
    where
        I: IntoIterator<Item = Entry>,
    {
        // Ledger verification needs to be parallelized, but we can't pull the whole
        // thing into memory. We therefore chunk it.
        let mut entry_count = 0;
        for block in &entries.into_iter().chunks(VERIFY_BLOCK_SIZE) {
            let block: Vec<_> = block.collect();
            if !block.verify(&self.last_id()) {
                warn!("Ledger proof of history failed at entry: {}", entry_count);
                return Err(BankError::LedgerVerificationFailed);
            }
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

            self.force_deposit(deposit.to, deposit.tokens);
        }
        self.register_entry_id(&entry0.id);
        self.register_entry_id(&entry1.id);

        let mut tail = Vec::with_capacity(WINDOW_SIZE as usize);
        tail.push(entry0);
        tail.push(entry1);
        let mut tail_idx = 2;
        let entry_count = 2 + self.process_blocks(entries, &mut tail, &mut tail_idx)?;

        // check f we need to rotate tail
        if tail.len() == WINDOW_SIZE as usize {
            tail.rotate_left(tail_idx)
        }

        Ok((entry_count, tail))
    }
    /// Process a Witness Signature. Any payment plans waiting on this signature
    /// will progress one step.
    fn apply_signature(
        balances: &mut [Page],
        pending: &mut HashMap<Signature, Plan>,
        tx_sig: Signature,
    ) -> Result<()> {
        if let Occupied(mut e) = pending.entry(tx_sig) {
            e.get_mut()
                .apply_witness(&Witness::Signature, &balances[0].owner);
            if let Some(payment) = e.get().final_payment() {
                //return the tokens back to the source
                balances[0].balance += payment.tokens;
                balances[1].balance -= payment.tokens;
                e.remove_entry();
            }
        };
        Ok(())
    }

    /// Process a Witness Timestamp. Any payment plans waiting on this timestamp
    /// will progress one step.
    fn apply_timestamp(
        page: &Page,
        pending: &mut HashMap<Signature, Plan>,
        dt: DateTime<Utc>,
    ) -> Result<()> {
        // Check to see if any timelocked transactions can be completed.
        let mut completed = vec![];

        // Hold 'pending' write lock until the end of this function. Otherwise another thread can
        // double-spend if it enters before the modified plan is removed from 'pending'.
        for (key, plan) in pending.iter_mut() {
            plan.apply_witness(&Witness::Timestamp(dt), &page.owner);
            if let Some(_payment) = plan.final_payment() {
                completed.push(key.clone());
            }
        }

        for key in completed {
            pending.remove(&key);
        }

        Ok(())
    }

    pub fn transfer_timestamp(
        &self,
        keypair: &KeyPair,
        to: PublicKey,
        dt: DateTime<Utc>,
        last_id: Hash,
        version: u64,
    ) -> Result<Signature> {
        let tx = Transaction::new_timestamp(keypair, to, dt, last_id, version);
        let sig = tx.call.proofs[0];
        self.process_transaction(&tx).map(|_| sig)
    }
    pub fn transfer_signature(
        &self,
        keypair: &KeyPair,
        to: PublicKey,
        sig: Signature,
        last_id: Hash,
        version: u64,
    ) -> Result<Signature> {
        let tx = Transaction::new_signature(keypair, to, sig, last_id, version);
        let sig = tx.call.proofs[0];
        self.process_transaction(&tx).map(|_| sig)
    }

    /// Create, sign, and process a Transaction from `keypair` to `to` of
    /// `n` tokens where `last_id` is the last Entry ID observed by the client.
    pub fn transfer(
        &self,
        n: i64,
        keypair: &KeyPair,
        to: PublicKey,
        last_id: Hash,
        version: u64,
    ) -> Result<Signature> {
        let tx = Transaction::new(keypair, to, n, last_id, version);
        let sig = tx.call.proofs[0];
        self.process_transaction(&tx).map(|_| sig)
    }

    /// Create, sign, and process a postdated Transaction from `keypair`
    /// to `to` of `n` tokens on `dt` where `last_id` is the last Entry ID
    /// observed by the client.
    pub fn transfer_on_date(
        &self,
        n: i64,
        keypair: &KeyPair,
        to: PublicKey,
        dt: DateTime<Utc>,
        last_id: Hash,
        version: u64,
    ) -> Result<Signature> {
        let tx = Transaction::new_on_date(keypair, to, dt, n, last_id, version);
        let sig = tx.call.proofs[0];
        self.process_transaction(&tx).map(|_| sig)
    }
    //calculate how much is left in the contract
    pub fn default_contract_get_balance(page: &Page) -> i64 {
        let contract: Option<HashMap<Signature, Plan>> = deserialize(&page.memory).ok();
        let spendable = if let Some(pending) = contract {
            pending
                .values()
                .flat_map(|plan| plan.final_payment().map(|p| p.tokens))
                .sum()
        } else {
            page.balance
        };

        assert!(spendable <= page.balance);
        spendable
    }
    pub fn get_balance(&self, pubkey: &PublicKey) -> i64 {
        self.page_table.get_balance(pubkey).unwrap_or(0)
    }

    pub fn get_version(&self, pubkey: &PublicKey) -> (u64, Signature) {
        self.page_table.get_version(pubkey).unwrap_or_default()
    }

    pub fn transaction_count(&self) -> usize {
        self.page_table.transaction_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use entry::next_entry;
    use entry::Entry;
    use entry_writer::{self, EntryWriter};
    use logger;
    use signature::KeyPairUtil;
    use std::io::{BufReader, Cursor, Seek, SeekFrom};
    use hash::hash;

    #[test]
    fn test_two_payments_to_one_party() {
        let mint = Mint::new(10_000);
        let pubkey = KeyPair::new().pubkey();
        let bank = Bank::new(&mint);
        assert_eq!(bank.last_id(), mint.last_id());

        bank.transfer(1_000, &mint.keypair(), pubkey, mint.last_id(), 0)
            .unwrap();
        assert_eq!(bank.get_balance(&pubkey), 1_000);

        bank.transfer(500, &mint.keypair(), pubkey, mint.last_id(), 1)
            .unwrap();
        assert_eq!(bank.get_balance(&pubkey), 1_500);
        assert_eq!(bank.transaction_count(), 2);
    }

    #[test]
    fn test_negative_tokens() {
        let mint = Mint::new(1);
        let pubkey = KeyPair::new().pubkey();
        let bank = Bank::new(&mint);
        //NOTE(anatoly): page table will allow any transaction that can pay the fee to move forward
        //but will not accept an overdraft, or creation of tokens
        assert_matches!(
            bank.transfer(-1, &mint.keypair(), pubkey, mint.last_id(), 0),
            Ok(_)
        );
        assert_eq!(bank.transaction_count(), 1);
        assert_eq!(bank.get_balance(&mint.pubkey()), 1);
        assert_eq!(bank.get_balance(&pubkey), 0);
    }

    #[test]
    fn test_account_with_no_funds() {
        let mint = Mint::new(1);
        let bank = Bank::new(&mint);
        let keypair = KeyPair::new();
        assert_eq!(
            bank.transfer(1, &keypair, mint.pubkey(), mint.last_id(), 0),
            //TODO(anatoly): page table treats this as a validate error
            Err(BankError::InsufficientFunds(keypair.pubkey()))
        );
        assert_eq!(bank.transaction_count(), 0);
    }

    #[test]
    fn test_overdraft() {
        let mint = Mint::new(11_000);
        let bank = Bank::new(&mint);
        let pubkey = KeyPair::new().pubkey();
        bank.transfer(1_000, &mint.keypair(), pubkey, mint.last_id(), 0)
            .unwrap();
        assert_eq!(bank.transaction_count(), 1);
        //NOTE(anatoly): page table will allow any transaction that can pay the fee to move forward
        //but will not accept an overdraft
        assert_matches!(
            bank.transfer(10_001, &mint.keypair(), pubkey, mint.last_id(), 1),
            Ok(_)
        );
        assert_eq!(bank.transaction_count(), 2);

        let mint_pubkey = mint.keypair().pubkey();
        assert_eq!(bank.get_balance(&mint_pubkey), 10_000);
        assert_eq!(bank.get_balance(&pubkey), 1_000);
    }

    #[test]
    fn test_transfer_to_newb() {
        let mint = Mint::new(10_000);
        let bank = Bank::new(&mint);
        let pubkey = KeyPair::new().pubkey();
        bank.transfer(500, &mint.keypair(), pubkey, mint.last_id(), 0)
            .unwrap();
        assert_eq!(bank.get_balance(&pubkey), 500);
    }

    #[test]
    fn test_transfer_on_date() {
        logger::setup();
        let mint = Mint::new(1);
        let bank = Bank::new(&mint);
        let pubkey = KeyPair::new().pubkey();
        let dt = Utc::now();
        bank.transfer_on_date(1, &mint.keypair(), pubkey, dt, mint.last_id(), 0)
            .unwrap();

        // Mint's balance will be zero because all funds are locked up.
        assert_eq!(bank.get_balance(&mint.pubkey()), 0);

        // tx count is 1, because debits were applied.
        assert_eq!(bank.transaction_count(), 1);

        // pubkey's balance will be None because the funds have not been
        // sent.
        assert_eq!(bank.get_balance(&pubkey), 0);

        // Now, acknowledge the time in the condition occurred and
        // that pubkey's funds are now available.
        bank.transfer_timestamp(&mint.keypair(), pubkey, dt, mint.last_id(), 1)
            .unwrap();
        assert_eq!(bank.get_balance(&pubkey), 1);

        // PageTable counts all transactions
        assert_eq!(bank.transaction_count(), 2);

        bank.transfer_timestamp(&mint.keypair(), pubkey, dt, mint.last_id(), 2)
            .unwrap(); // <-- Attack! Attempt to process completed transaction.
        assert_ne!(bank.get_balance(&pubkey), 2);
    }

    #[test]
    fn test_cancel_transfer() {
        let mint = Mint::new(1);
        let bank = Bank::new(&mint);
        let pubkey = KeyPair::new().pubkey();
        let dt = Utc::now();
        let sig = bank.transfer_on_date(1, &mint.keypair(), pubkey, dt, mint.last_id(), 0)
            .unwrap();

        // Assert the debit counts as a transaction.
        assert_eq!(bank.transaction_count(), 1);

        // Mint's balance will be zero because all funds are locked up.
        assert_eq!(bank.get_balance(&mint.pubkey()), 0);

        // pubkey's balance will be None because the funds have not been
        // sent.
        assert_eq!(bank.get_balance(&pubkey), 0);

        // Now, cancel the trancaction. Mint gets her funds back, pubkey never sees them.
        bank.transfer_signature(&mint.keypair(), pubkey, sig, mint.last_id(), 1)
            .unwrap();
        assert_eq!(bank.get_balance(&mint.pubkey()), 1);
        assert_eq!(bank.get_balance(&pubkey), 0);

        // Assert each transaction is registered
        assert_eq!(bank.transaction_count(), 2);

        bank.transfer_signature(&mint.keypair(), pubkey, sig, mint.last_id(), 2)
            .unwrap(); // <-- Attack! Attempt to cancel completed transaction.
        assert_ne!(bank.get_balance(&mint.pubkey()), 2);
    }

    #[test]
    fn test_reject_old_last_id() {
        let mint = Mint::new(1);
        let bank = Bank::new(&mint);
        for i in 0..MAX_ENTRY_IDS {
            let last_id = hash(&serialize(&i).unwrap()); // Unique hash
            bank.register_entry_id(&last_id);
        }
        let tx0 = Transaction::new(&mint.keypair(), mint.keypair().pubkey(), 1, mint.last_id(), 0);
        // Assert we're no longer able to use the oldest entry ID.
        assert_eq!(
            bank.process_transaction(&tx0),
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
        let keypair = KeyPair::new();
        let tx0 = Transaction::new(&mint.keypair(), keypair.pubkey(), 2, mint.last_id(), 0);
        let tx1 = Transaction::new(&keypair, mint.pubkey(), 1, mint.last_id(), 0);
        let txs = vec![tx0, tx1];
        let results = bank.process_transactions(txs);
        assert!(results[0].is_ok());
        //TODO(anatoly): tx1 is rejected because it overlaps in memory with tx0
        assert_eq!(results[1], Err(BankError::AccountInUse));

        // Assert bad transactions aren't counted.
        assert_eq!(bank.transaction_count(), 1);
    }

    #[test]
    fn test_process_empty_entry_is_registered() {
        let mint = Mint::new(1);
        let bank = Bank::new(&mint);
        let keypair = KeyPair::new();
        let entry = next_entry(&mint.last_id(), 1, vec![]);
        let tx = Transaction::new(&mint.keypair(), keypair.pubkey(), 1, entry.id, 0);

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

    fn create_sample_block(mint: &Mint, length: usize) -> impl Iterator<Item = Entry> {
        let mut entries = Vec::with_capacity(length);
        let mut hash = mint.last_id();
        let mut cur_hashes = 0;
        for v in 0..length {
            let keypair = KeyPair::new();
            let tx = Transaction::new(&mint.keypair(), keypair.pubkey(), 1, hash, v as u64);
            let entry = Entry::new_mut(&mut hash, &mut cur_hashes, vec![tx], false);
            entries.push(entry);
        }
        entries.into_iter()
    }
    fn create_sample_ledger(length: usize) -> (impl Iterator<Item = Entry>, PublicKey) {
        let mint = Mint::new(1 + length as i64);
        let genesis = mint.create_entries();
        let block = create_sample_block(&mint, length);
        (genesis.into_iter().chain(block), mint.pubkey())
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

}

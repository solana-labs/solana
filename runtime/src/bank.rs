//! The `bank` module tracks client accounts and the progress of on-chain
//! programs. It offers a high-level API that signs transactions
//! on behalf of the caller, and a low-level API for when they have
//! already been signed and verified.
use crate::accounts::Accounts;
use crate::accounts_db::{
    ErrorCounters, InstructionAccounts, InstructionCredits, InstructionLoaders,
};
use crate::accounts_index::Fork;
use crate::blockhash_queue::BlockhashQueue;
use crate::epoch_schedule::EpochSchedule;
use crate::locked_accounts_results::LockedAccountsResults;
use crate::message_processor::{MessageProcessor, ProcessInstruction};
use crate::serde_utils::{
    deserialize_atomicbool, deserialize_atomicusize, serialize_atomicbool, serialize_atomicusize,
};
use crate::stakes::Stakes;
use crate::status_cache::StatusCache;
use crate::storage_utils;
use crate::storage_utils::StorageAccounts;
use bincode::{deserialize_from, serialize, serialize_into, serialized_size};
use log::*;
use serde::{Deserialize, Serialize};
use solana_metrics::{
    datapoint_info, inc_new_counter_debug, inc_new_counter_error, inc_new_counter_info,
};
use solana_sdk::account::Account;
use solana_sdk::fee_calculator::FeeCalculator;
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::hash::{extend_and_hash, hashv, Hash};
use solana_sdk::inflation::Inflation;
use solana_sdk::native_loader;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signature};
use solana_sdk::syscall::current;
use solana_sdk::syscall::fees::{self, Fees};
use solana_sdk::syscall::slot_hashes::{self, SlotHashes};
use solana_sdk::syscall::tick_height::{self, TickHeight};
use solana_sdk::system_transaction;
use solana_sdk::timing::{duration_as_ms, duration_as_ns, duration_as_us, MAX_RECENT_BLOCKHASHES};
use solana_sdk::transaction::{Result, Transaction, TransactionError};
use std::cmp;
use std::collections::HashMap;
use std::fmt;
use std::io::{BufReader, Cursor, Read};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock, RwLockReadGuard};
use std::time::Instant;

pub const SECONDS_PER_YEAR: f64 = (365.0 * 24.0 * 60.0 * 60.0);

type BankStatusCache = StatusCache<Result<()>>;

#[derive(Default)]
pub struct BankRc {
    /// where all the Accounts are stored
    accounts: Arc<Accounts>,

    /// Previous checkpoint of this bank
    parent: RwLock<Option<Arc<Bank>>>,
}

impl BankRc {
    pub fn new(account_paths: Option<String>) -> Self {
        let accounts = Accounts::new(account_paths);
        BankRc {
            accounts: Arc::new(accounts),
            parent: RwLock::new(None),
        }
    }

    pub fn update_from_stream<R: Read>(
        &self,
        mut stream: &mut BufReader<R>,
    ) -> std::result::Result<(), std::io::Error> {
        let _len: usize = deserialize_from(&mut stream)
            .map_err(|_| BankRc::get_io_error("len deserialize error"))?;
        self.accounts.update_from_stream(stream)
    }

    fn get_io_error(error: &str) -> std::io::Error {
        warn!("BankRc error: {:?}", error);
        std::io::Error::new(std::io::ErrorKind::Other, error)
    }
}

impl Serialize for BankRc {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        use serde::ser::Error;
        let len = serialized_size(&*self.accounts.accounts_db).unwrap();
        let mut buf = vec![0u8; len as usize];
        let mut wr = Cursor::new(&mut buf[..]);
        serialize_into(&mut wr, &*self.accounts.accounts_db).map_err(Error::custom)?;
        let len = wr.position() as usize;
        serializer.serialize_bytes(&wr.into_inner()[..len])
    }
}

#[derive(Default)]
pub struct StatusCacheRc {
    /// where all the Accounts are stored
    /// A cache of signature statuses
    status_cache: Arc<RwLock<BankStatusCache>>,
}

impl Serialize for StatusCacheRc {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        use serde::ser::Error;
        let len = serialized_size(&*self.status_cache).unwrap();
        let mut buf = vec![0u8; len as usize];
        let mut wr = Cursor::new(&mut buf[..]);
        {
            let mut status_cache = self.status_cache.write().unwrap();
            serialize_into(&mut wr, &*status_cache).map_err(Error::custom)?;
            status_cache.merge_caches();
        }
        let len = wr.position() as usize;
        serializer.serialize_bytes(&wr.into_inner()[..len])
    }
}

struct StatusCacheRcVisitor;

impl<'a> serde::de::Visitor<'a> for StatusCacheRcVisitor {
    type Value = StatusCacheRc;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("Expecting StatusCacheRc")
    }

    #[allow(clippy::mutex_atomic)]
    fn visit_bytes<E>(self, data: &[u8]) -> std::result::Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        use serde::de::Error;
        let mut rd = Cursor::new(&data[..]);
        let status_cache: BankStatusCache = deserialize_from(&mut rd).map_err(Error::custom)?;
        Ok(StatusCacheRc {
            status_cache: Arc::new(RwLock::new(status_cache)),
        })
    }
}

impl<'de> Deserialize<'de> for StatusCacheRc {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: ::serde::Deserializer<'de>,
    {
        deserializer.deserialize_bytes(StatusCacheRcVisitor)
    }
}

impl StatusCacheRc {
    pub fn append(&self, status_cache_rc: &StatusCacheRc) {
        let sc = status_cache_rc.status_cache.write().unwrap();
        self.status_cache.write().unwrap().append(&sc);
    }
}

/// Manager for the state of all accounts and programs after processing its entries.
#[derive(Default, Deserialize, Serialize)]
pub struct Bank {
    /// References to accounts, parent and signature status
    #[serde(skip)]
    pub rc: BankRc,

    #[serde(skip)]
    pub src: StatusCacheRc,

    /// FIFO queue of `recent_blockhash` items
    blockhash_queue: RwLock<BlockhashQueue>,

    /// The set of parents including this bank
    pub ancestors: HashMap<u64, usize>,

    /// Hash of this Bank's state. Only meaningful after freezing.
    hash: RwLock<Hash>,

    /// Hash of this Bank's parent's state
    parent_hash: Hash,

    /// The number of transactions processed without error
    #[serde(serialize_with = "serialize_atomicusize")]
    #[serde(deserialize_with = "deserialize_atomicusize")]
    transaction_count: AtomicUsize, // TODO: Use AtomicU64 if/when available

    /// Bank tick height
    #[serde(serialize_with = "serialize_atomicusize")]
    #[serde(deserialize_with = "deserialize_atomicusize")]
    tick_height: AtomicUsize, // TODO: Use AtomicU64 if/when available

    /// The number of signatures from valid transactions in this slot
    #[serde(serialize_with = "serialize_atomicusize")]
    #[serde(deserialize_with = "deserialize_atomicusize")]
    signature_count: AtomicUsize,

    /// Total capitalization, used to calculate inflation
    #[serde(serialize_with = "serialize_atomicusize")]
    #[serde(deserialize_with = "deserialize_atomicusize")]
    capitalization: AtomicUsize, // TODO: Use AtomicU64 if/when available

    // Bank max_tick_height
    max_tick_height: u64,

    /// The number of ticks in each slot.
    ticks_per_slot: u64,

    /// The number of slots per year, used for inflation
    slots_per_year: f64,

    /// Bank fork (i.e. slot, i.e. block)
    slot: u64,

    /// Bank height in term of banks
    bank_height: u64,

    /// The pubkey to send transactions fees to.
    collector_id: Pubkey,

    /// Fees that have been collected
    #[serde(serialize_with = "serialize_atomicusize")]
    #[serde(deserialize_with = "deserialize_atomicusize")]
    collector_fees: AtomicUsize, // TODO: Use AtomicU64 if/when available

    /// Latest transaction fees for transactions processed by this bank
    fee_calculator: FeeCalculator,

    /// initialized from genesis
    epoch_schedule: EpochSchedule,

    /// inflation specs
    inflation: Inflation,

    /// cache of vote_account and stake_account state for this fork
    stakes: RwLock<Stakes>,

    /// cache of validator and replicator storage accounts for this fork
    storage_accounts: RwLock<StorageAccounts>,

    /// staked nodes on epoch boundaries, saved off when a bank.slot() is at
    ///   a leader schedule calculation boundary
    epoch_stakes: HashMap<u64, Stakes>,

    /// A boolean reflecting whether any entries were recorded into the PoH
    /// stream for the slot == self.slot
    #[serde(serialize_with = "serialize_atomicbool")]
    #[serde(deserialize_with = "deserialize_atomicbool")]
    is_delta: AtomicBool,

    /// The Message processor
    message_processor: MessageProcessor,
}

impl Default for BlockhashQueue {
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
        bank.ancestors.insert(bank.slot(), 0);
        bank.rc.accounts = Arc::new(Accounts::new(paths));
        bank.process_genesis_block(genesis_block);
        // genesis needs stakes for all epochs up to the epoch implied by
        //  slot = 0 and genesis configuration
        {
            let stakes = bank.stakes.read().unwrap();
            for i in 0..=bank.get_stakers_epoch(bank.slot) {
                bank.epoch_stakes.insert(i, stakes.clone());
            }
        }
        bank.update_current();
        bank
    }

    /// Create a new bank that points to an immutable checkpoint of another bank.
    pub fn new_from_parent(parent: &Arc<Bank>, collector_id: &Pubkey, slot: u64) -> Self {
        Self::default().init_from_parent(parent, collector_id, slot)
    }

    /// Create a new bank that points to an immutable checkpoint of another bank.
    pub fn init_from_parent(
        mut self,
        parent: &Arc<Bank>,
        collector_id: &Pubkey,
        slot: u64,
    ) -> Self {
        parent.freeze();
        assert_ne!(slot, parent.slot());

        // TODO: clean this up, soo much special-case copying...
        self.ticks_per_slot = parent.ticks_per_slot;
        self.slots_per_year = parent.slots_per_year;
        self.epoch_schedule = parent.epoch_schedule;

        self.slot = slot;
        self.max_tick_height = (self.slot + 1) * self.ticks_per_slot - 1;

        self.blockhash_queue = RwLock::new(parent.blockhash_queue.read().unwrap().clone());
        self.src.status_cache = parent.src.status_cache.clone();
        self.bank_height = parent.bank_height + 1;
        self.fee_calculator =
            FeeCalculator::new_derived(&parent.fee_calculator, parent.signature_count());

        self.capitalization
            .store(parent.capitalization() as usize, Ordering::Relaxed);
        self.inflation = parent.inflation.clone();

        self.transaction_count
            .store(parent.transaction_count() as usize, Ordering::Relaxed);
        self.stakes = RwLock::new(parent.stakes.read().unwrap().clone_with_epoch(self.epoch()));
        self.storage_accounts = RwLock::new(parent.storage_accounts.read().unwrap().clone());

        self.tick_height.store(
            parent.tick_height.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );

        datapoint_info!(
            "bank-new_from_parent-heights",
            ("slot_height", slot, i64),
            ("bank_height", self.bank_height, i64)
        );

        self.rc.parent = RwLock::new(Some(parent.clone()));
        self.parent_hash = parent.hash();
        self.collector_id = *collector_id;

        self.rc.accounts = Arc::new(Accounts::new_from_parent(&parent.rc.accounts));

        self.epoch_stakes = {
            let mut epoch_stakes = parent.epoch_stakes.clone();
            let epoch = self.get_stakers_epoch(self.slot);
            // update epoch_vote_states cache
            //  if my parent didn't populate for this epoch, we've
            //  crossed a boundary
            if epoch_stakes.get(&epoch).is_none() {
                epoch_stakes.insert(epoch, self.stakes.read().unwrap().clone());
            }
            epoch_stakes
        };
        self.ancestors.insert(self.slot(), 0);
        self.parents().iter().enumerate().for_each(|(i, p)| {
            self.ancestors.insert(p.slot(), i + 1);
        });

        self.update_rewards(parent.epoch(), parent.last_blockhash());
        self.update_current();
        self.update_fees();
        self
    }

    pub fn collector_id(&self) -> &Pubkey {
        &self.collector_id
    }

    pub fn create_with_genesis(
        genesis_block: &GenesisBlock,
        account_paths: Option<String>,
        status_cache_rc: &StatusCacheRc,
    ) -> Self {
        let mut bank = Self::default();
        bank.set_bank_rc(&BankRc::new(account_paths), &status_cache_rc);
        bank.process_genesis_block(genesis_block);
        bank.ancestors.insert(0, 0);
        bank
    }

    pub fn slot(&self) -> u64 {
        self.slot
    }

    pub fn epoch(&self) -> u64 {
        self.epoch_schedule.get_epoch(self.slot)
    }

    pub fn freeze_lock(&self) -> RwLockReadGuard<Hash> {
        self.hash.read().unwrap()
    }

    pub fn hash(&self) -> Hash {
        *self.hash.read().unwrap()
    }

    pub fn is_frozen(&self) -> bool {
        *self.hash.read().unwrap() != Hash::default()
    }

    fn update_current(&self) {
        self.store_account(
            &current::id(),
            &current::create_account(
                1,
                self.slot,
                self.epoch_schedule.get_epoch(self.slot),
                self.epoch_schedule.get_stakers_epoch(self.slot),
            ),
        );
    }

    fn update_slot_hashes(&self) {
        let mut account = self
            .get_account(&slot_hashes::id())
            .unwrap_or_else(|| slot_hashes::create_account(1));

        let mut slot_hashes = SlotHashes::from(&account).unwrap();
        slot_hashes.add(self.slot(), self.hash());
        slot_hashes.to(&mut account).unwrap();

        self.store_account(&slot_hashes::id(), &account);
    }

    fn update_fees(&self) {
        let mut account = self
            .get_account(&fees::id())
            .unwrap_or_else(|| fees::create_account(1));

        let mut fees = Fees::from(&account).unwrap();
        fees.fee_calculator = self.fee_calculator.clone();
        fees.to(&mut account).unwrap();

        self.store_account(&fees::id(), &account);
    }

    fn update_tick_height(&self) {
        let mut account = self
            .get_account(&tick_height::id())
            .unwrap_or_else(|| tick_height::create_account(1));

        TickHeight::to(self.tick_height(), &mut account).unwrap();

        self.store_account(&tick_height::id(), &account);
    }

    // update reward for previous epoch
    fn update_rewards(&mut self, epoch: u64, blockhash: Hash) {
        if epoch == self.epoch() {
            return;
        }
        // if I'm the first Bank in an epoch, count, claim, disburse rewards from Inflation

        // TODO: on-chain wallclock?
        //  years_elapsed =         slots_elapsed                             /     slots/year
        let year = (self.epoch_schedule.get_last_slot_in_epoch(epoch)) as f64 / self.slots_per_year;

        // period: time that has passed as a fraction of a year, basically the length of
        //  an epoch as a fraction of a year
        //  years_elapsed =   slots_elapsed                                   /  slots/year
        let period = self.epoch_schedule.get_slots_in_epoch(epoch) as f64 / self.slots_per_year;

        // validators
        {
            let validator_rewards =
                (self.inflation.validator(year) * self.capitalization() as f64 * period) as u64;

            // claim points and create a pool
            let mining_pool = self
                .stakes
                .write()
                .unwrap()
                .create_mining_pool(epoch, validator_rewards);

            self.store_account(
                &Pubkey::new(
                    hashv(&[
                        blockhash.as_ref(),
                        "StakeMiningPool".as_ref(),
                        &serialize(&epoch).unwrap(),
                    ])
                    .as_ref(),
                ),
                &mining_pool,
            );

            self.capitalization
                .fetch_add(validator_rewards as usize, Ordering::Relaxed);
        }
    }

    fn set_hash(&self) -> bool {
        let mut hash = self.hash.write().unwrap();
        if *hash == Hash::default() {
            let collector_fees = self.collector_fees.load(Ordering::Relaxed) as u64;
            if collector_fees != 0 {
                self.deposit(&self.collector_id, collector_fees);
            }
            //  freeze is a one-way trip, idempotent
            *hash = self.hash_internal_state();
            true
        } else {
            false
        }
    }

    pub fn freeze(&self) {
        if self.set_hash() {
            self.update_slot_hashes();
        }
    }

    pub fn epoch_schedule(&self) -> &EpochSchedule {
        &self.epoch_schedule
    }

    /// squash the parent's state up into this Bank,
    ///   this Bank becomes a root
    pub fn squash(&self) {
        self.freeze();

        let parents = self.parents();
        *self.rc.parent.write().unwrap() = None;

        let squash_accounts_start = Instant::now();
        for p in parents.iter().rev() {
            // root forks cannot be purged
            self.rc.accounts.add_root(p.slot());
        }
        let squash_accounts_ms = duration_as_ms(&squash_accounts_start.elapsed());

        let squash_cache_start = Instant::now();
        parents
            .iter()
            .for_each(|p| self.src.status_cache.write().unwrap().add_root(p.slot()));
        let squash_cache_ms = duration_as_ms(&squash_cache_start.elapsed());

        datapoint_info!(
            "locktower-observed",
            ("squash_accounts_ms", squash_accounts_ms, i64),
            ("squash_cache_ms", squash_cache_ms, i64)
        );
    }

    /// Return the more recent checkpoint of this bank instance.
    pub fn parent(&self) -> Option<Arc<Bank>> {
        self.rc.parent.read().unwrap().clone()
    }

    fn process_genesis_block(&mut self, genesis_block: &GenesisBlock) {
        // Bootstrap leader collects fees until `new_from_parent` is called.
        self.fee_calculator = genesis_block.fee_calculator.clone();
        self.update_fees();

        for (pubkey, account) in genesis_block.accounts.iter() {
            self.store_account(pubkey, account);
            self.capitalization
                .fetch_add(account.lamports as usize, Ordering::Relaxed);
        }
        // highest staked node is the first collector
        self.collector_id = self
            .stakes
            .read()
            .unwrap()
            .highest_staked_node()
            .unwrap_or_default();

        self.blockhash_queue
            .write()
            .unwrap()
            .genesis_hash(&genesis_block.hash(), &self.fee_calculator);

        self.ticks_per_slot = genesis_block.ticks_per_slot;
        self.max_tick_height = (self.slot + 1) * self.ticks_per_slot - 1;
        //   ticks/year     =      seconds/year ...
        self.slots_per_year = SECONDS_PER_YEAR
        //  * (ns/s)/(ns/tick) / ticks/slot = 1/s/1/tick = ticks/s
            *(1_000_000_000.0 / duration_as_ns(&genesis_block.poh_config.target_tick_duration) as f64)
        //  / ticks/slot
            / self.ticks_per_slot as f64;

        // make bank 0 votable
        self.is_delta.store(true, Ordering::Relaxed);

        self.epoch_schedule = EpochSchedule::new(
            genesis_block.slots_per_epoch,
            genesis_block.stakers_slot_offset,
            genesis_block.epoch_warmup,
        );

        self.inflation = genesis_block.inflation.clone();

        // Add native programs mandatory for the MessageProcessor to function
        self.register_native_instruction_processor(
            "solana_system_program",
            &solana_sdk::system_program::id(),
        );

        // Add additional native programs specified in the genesis block
        for (name, program_id) in &genesis_block.native_instruction_processors {
            self.register_native_instruction_processor(name, program_id);
        }
    }

    pub fn register_native_instruction_processor(&self, name: &str, program_id: &Pubkey) {
        debug!("Adding native program {} under {:?}", name, program_id);
        let account = native_loader::create_loadable_account(name);
        self.store_account(program_id, &account);
    }

    /// Return the last block hash registered.
    pub fn last_blockhash(&self) -> Hash {
        self.blockhash_queue.read().unwrap().last_hash()
    }

    pub fn last_blockhash_with_fee_calculator(&self) -> (Hash, FeeCalculator) {
        let blockhash_queue = self.blockhash_queue.read().unwrap();
        let last_hash = blockhash_queue.last_hash();
        (
            last_hash,
            blockhash_queue
                .get_fee_calculator(&last_hash)
                .unwrap()
                .clone(),
        )
    }

    pub fn confirmed_last_blockhash(&self) -> (Hash, FeeCalculator) {
        const NUM_BLOCKHASH_CONFIRMATIONS: usize = 3;

        let parents = self.parents();
        if parents.is_empty() {
            self.last_blockhash_with_fee_calculator()
        } else {
            let index = cmp::min(NUM_BLOCKHASH_CONFIRMATIONS, parents.len() - 1);
            parents[index].last_blockhash_with_fee_calculator()
        }
    }

    /// Forget all signatures. Useful for benchmarking.
    pub fn clear_signatures(&self) {
        self.src.status_cache.write().unwrap().clear_signatures();
    }

    pub fn can_commit(result: &Result<()>) -> bool {
        match result {
            Ok(_) => true,
            Err(TransactionError::InstructionError(_, _)) => true,
            Err(_) => false,
        }
    }

    fn update_transaction_statuses(&self, txs: &[Transaction], res: &[Result<()>]) {
        let mut status_cache = self.src.status_cache.write().unwrap();
        for (i, tx) in txs.iter().enumerate() {
            if Self::can_commit(&res[i]) && !tx.signatures.is_empty() {
                status_cache.insert(
                    &tx.message().recent_blockhash,
                    &tx.signatures[0],
                    self.slot(),
                    res[i].clone(),
                );
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
        slots_and_stakes.sort_by(|a, b| b.0.cmp(&a.0));

        let max_slot = self.slot();
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
            self.tick_height.fetch_add(1, Ordering::Relaxed);
            self.tick_height.load(Ordering::Relaxed) as u64
        };
        inc_new_counter_debug!("bank-register_tick-registered", 1);

        self.update_tick_height();

        // Register a new block hash if at the last tick in the slot
        if current_tick_height % self.ticks_per_slot == self.ticks_per_slot - 1 {
            self.blockhash_queue
                .write()
                .unwrap()
                .register_hash(hash, &self.fee_calculator);
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

    pub fn lock_accounts<'a, 'b>(
        &'a self,
        txs: &'b [Transaction],
    ) -> LockedAccountsResults<'a, 'b> {
        if self.is_frozen() {
            warn!("=========== FIXME: lock_accounts() working on a frozen bank! ================");
        }
        // TODO: put this assert back in
        // assert!(!self.is_frozen());
        let results = self.rc.accounts.lock_accounts(txs);
        LockedAccountsResults::new(results, &self, txs)
    }

    pub fn unlock_accounts(&self, locked_accounts_results: &mut LockedAccountsResults) {
        if locked_accounts_results.needs_unlock {
            locked_accounts_results.needs_unlock = false;
            self.rc.accounts.unlock_accounts(
                locked_accounts_results.transactions(),
                locked_accounts_results.locked_accounts_results(),
            )
        }
    }

    fn load_accounts(
        &self,
        txs: &[Transaction],
        results: Vec<Result<()>>,
        error_counters: &mut ErrorCounters,
    ) -> Vec<Result<(InstructionAccounts, InstructionLoaders, InstructionCredits)>> {
        self.rc.accounts.load_accounts(
            &self.ancestors,
            txs,
            results,
            &self.blockhash_queue.read().unwrap(),
            error_counters,
        )
    }
    fn check_refs(
        &self,
        txs: &[Transaction],
        lock_results: &[Result<()>],
        error_counters: &mut ErrorCounters,
    ) -> Vec<Result<()>> {
        txs.iter()
            .zip(lock_results)
            .map(|(tx, lock_res)| {
                if lock_res.is_ok() && !tx.verify_refs() {
                    error_counters.invalid_account_index += 1;
                    Err(TransactionError::InvalidAccountIndex)
                } else {
                    lock_res.clone()
                }
            })
            .collect()
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
                if lock_res.is_ok()
                    && !hash_queue.check_hash_age(&tx.message().recent_blockhash, max_age)
                {
                    error_counters.reserve_blockhash += 1;
                    Err(TransactionError::BlockhashNotFound)
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
        let rcache = self.src.status_cache.read().unwrap();
        txs.iter()
            .zip(lock_results.into_iter())
            .map(|(tx, lock_res)| {
                if tx.signatures.is_empty() {
                    return lock_res;
                }
                if lock_res.is_ok()
                    && rcache
                        .get_signature_status(
                            &tx.signatures[0],
                            &tx.message().recent_blockhash,
                            &self.ancestors,
                        )
                        .is_some()
                {
                    error_counters.duplicate_signature += 1;
                    Err(TransactionError::DuplicateSignature)
                } else {
                    lock_res
                }
            })
            .collect()
    }

    pub fn check_transactions(
        &self,
        txs: &[Transaction],
        lock_results: &[Result<()>],
        max_age: usize,
        mut error_counters: &mut ErrorCounters,
    ) -> Vec<Result<()>> {
        let refs_results = self.check_refs(txs, lock_results, &mut error_counters);
        let age_results = self.check_age(txs, refs_results, max_age, &mut error_counters);
        self.check_signatures(txs, age_results, &mut error_counters)
    }

    fn update_error_counters(error_counters: &ErrorCounters) {
        if 0 != error_counters.blockhash_not_found {
            inc_new_counter_error!(
                "bank-process_transactions-error-blockhash_not_found",
                error_counters.blockhash_not_found,
                0,
                1000
            );
        }
        if 0 != error_counters.invalid_account_index {
            inc_new_counter_error!(
                "bank-process_transactions-error-invalid_account_index",
                error_counters.invalid_account_index,
                0,
                1000
            );
        }
        if 0 != error_counters.reserve_blockhash {
            inc_new_counter_error!(
                "bank-process_transactions-error-reserve_blockhash",
                error_counters.reserve_blockhash,
                0,
                1000
            );
        }
        if 0 != error_counters.duplicate_signature {
            inc_new_counter_error!(
                "bank-process_transactions-error-duplicate_signature",
                error_counters.duplicate_signature,
                0,
                1000
            );
        }
        if 0 != error_counters.invalid_account_for_fee {
            inc_new_counter_error!(
                "bank-process_transactions-error-invalid_account_for_fee",
                error_counters.invalid_account_for_fee,
                0,
                1000
            );
        }
        if 0 != error_counters.insufficient_funds {
            inc_new_counter_error!(
                "bank-process_transactions-error-insufficient_funds",
                error_counters.insufficient_funds,
                0,
                1000
            );
        }
        if 0 != error_counters.account_loaded_twice {
            inc_new_counter_error!(
                "bank-process_transactions-account_loaded_twice",
                error_counters.account_loaded_twice,
                0,
                1000
            );
        }
    }

    #[allow(clippy::type_complexity)]
    pub fn load_and_execute_transactions(
        &self,
        txs: &[Transaction],
        lock_results: &LockedAccountsResults,
        max_age: usize,
    ) -> (
        Vec<Result<(InstructionAccounts, InstructionLoaders, InstructionCredits)>>,
        Vec<Result<()>>,
    ) {
        debug!("processing transactions: {}", txs.len());
        let mut error_counters = ErrorCounters::default();
        let now = Instant::now();
        let sig_results = self.check_transactions(
            txs,
            lock_results.locked_accounts_results(),
            max_age,
            &mut error_counters,
        );
        let mut loaded_accounts = self.load_accounts(txs, sig_results, &mut error_counters);

        let load_elapsed = now.elapsed();
        let now = Instant::now();
        let mut signature_count = 0;
        let executed: Vec<Result<()>> = loaded_accounts
            .iter_mut()
            .zip(txs.iter())
            .map(|(accs, tx)| match accs {
                Err(e) => Err(e.clone()),
                Ok((ref mut accounts, ref mut loaders, ref mut credits)) => {
                    signature_count += tx.message().header.num_required_signatures as usize;
                    self.message_processor
                        .process_message(tx.message(), loaders, accounts, credits)
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
                    debug!("tx error: {:?} {:?}", r, tx);
                }
                err_count += 1;
            }
        }
        if err_count > 0 {
            debug!("{} errors of {} txs", err_count, err_count + tx_count);
            inc_new_counter_error!(
                "bank-process_transactions-account_not_found",
                error_counters.account_not_found,
                0,
                1000
            );
            inc_new_counter_error!("bank-process_transactions-error_count", err_count, 0, 1000);
        }

        self.increment_transaction_count(tx_count);
        self.increment_signature_count(signature_count);

        inc_new_counter_info!("bank-process_transactions-txs", tx_count, 0, 1000);
        inc_new_counter_info!("bank-process_transactions-sigs", signature_count, 0, 1000);
        Self::update_error_counters(&error_counters);
        (loaded_accounts, executed)
    }

    fn filter_program_errors_and_collect_fee(
        &self,
        txs: &[Transaction],
        executed: &[Result<()>],
    ) -> Vec<Result<()>> {
        let hash_queue = self.blockhash_queue.read().unwrap();
        let mut fees = 0;
        let results = txs
            .iter()
            .zip(executed.iter())
            .map(|(tx, res)| {
                let fee_calculator = hash_queue
                    .get_fee_calculator(&tx.message().recent_blockhash)
                    .ok_or(TransactionError::BlockhashNotFound)?;
                let fee = fee_calculator.calculate_fee(tx.message());

                let message = tx.message();
                match *res {
                    Err(TransactionError::InstructionError(_, _)) => {
                        // credit the transaction fee even in case of InstructionError
                        // necessary to withdraw from account[0] here because previous
                        // work of doing so (in accounts.load()) is ignored by store_account()
                        self.withdraw(&message.account_keys[0], fee)?;
                        fees += fee;
                        Ok(())
                    }
                    Ok(()) => {
                        fees += fee;
                        Ok(())
                    }
                    _ => res.clone(),
                }
            })
            .collect();

        self.collector_fees
            .fetch_add(fees as usize, Ordering::Relaxed);
        results
    }

    pub fn commit_transactions(
        &self,
        txs: &[Transaction],
        loaded_accounts: &[Result<(InstructionAccounts, InstructionLoaders, InstructionCredits)>],
        executed: &[Result<()>],
    ) -> Vec<Result<()>> {
        if self.is_frozen() {
            warn!("=========== FIXME: commit_transactions() working on a frozen bank! ================");
        }

        if executed.iter().any(|res| Self::can_commit(res)) {
            self.is_delta.store(true, Ordering::Relaxed);
        }

        // TODO: put this assert back in
        // assert!(!self.is_frozen());
        let now = Instant::now();
        self.rc
            .accounts
            .store_accounts(self.slot(), txs, executed, loaded_accounts);

        self.update_cached_accounts(txs, executed, loaded_accounts);

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
        lock_results: &LockedAccountsResults,
        max_age: usize,
    ) -> Vec<Result<()>> {
        let (loaded_accounts, executed) =
            self.load_and_execute_transactions(txs, lock_results, max_age);

        self.commit_transactions(txs, &loaded_accounts, &executed)
    }

    #[must_use]
    pub fn process_transactions(&self, txs: &[Transaction]) -> Vec<Result<()>> {
        let lock_results = self.lock_accounts(txs);
        self.load_execute_and_commit_transactions(txs, &lock_results, MAX_RECENT_BLOCKHASHES)
    }

    /// Create, sign, and process a Transaction from `keypair` to `to` of
    /// `n` lamports where `blockhash` is the last Entry ID observed by the client.
    pub fn transfer(&self, n: u64, keypair: &Keypair, to: &Pubkey) -> Result<Signature> {
        let blockhash = self.last_blockhash();
        let tx = system_transaction::create_user_account(keypair, to, n, blockhash);
        let signature = tx.signatures[0];
        self.process_transaction(&tx).map(|_| signature)
    }

    pub fn read_balance(account: &Account) -> u64 {
        account.lamports
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

    pub fn store_account(&self, pubkey: &Pubkey, account: &Account) {
        self.rc.accounts.store_slow(self.slot(), pubkey, account);

        if Stakes::is_stake(account) {
            self.stakes.write().unwrap().store(pubkey, account);
        } else if storage_utils::is_storage(account) {
            self.storage_accounts
                .write()
                .unwrap()
                .store(pubkey, account);
        }
    }

    pub fn withdraw(&self, pubkey: &Pubkey, lamports: u64) -> Result<()> {
        match self.get_account(pubkey) {
            Some(mut account) => {
                if lamports > account.lamports {
                    return Err(TransactionError::InsufficientFundsForFee);
                }

                account.lamports -= lamports;
                self.store_account(pubkey, &account);

                Ok(())
            }
            None => Err(TransactionError::AccountNotFound),
        }
    }

    pub fn deposit(&self, pubkey: &Pubkey, lamports: u64) {
        let mut account = self.get_account(pubkey).unwrap_or_default();
        account.lamports += lamports;
        self.store_account(pubkey, &account);
    }

    pub fn accounts(&self) -> Arc<Accounts> {
        self.rc.accounts.clone()
    }

    pub fn set_bank_rc(&mut self, bank_rc: &BankRc, status_cache_rc: &StatusCacheRc) {
        self.rc.accounts = bank_rc.accounts.clone();
        self.src.status_cache = status_cache_rc.status_cache.clone()
    }

    pub fn set_parent(&mut self, parent: &Arc<Bank>) {
        self.rc.parent = RwLock::new(Some(parent.clone()));
    }

    pub fn get_account(&self, pubkey: &Pubkey) -> Option<Account> {
        self.rc
            .accounts
            .load_slow(&self.ancestors, pubkey)
            .map(|(account, _)| account)
    }

    pub fn get_program_accounts_modified_since_parent(
        &self,
        program_id: &Pubkey,
    ) -> Vec<(Pubkey, Account)> {
        self.rc.accounts.load_by_program(self.slot(), program_id)
    }

    pub fn get_account_modified_since_parent(&self, pubkey: &Pubkey) -> Option<(Account, Fork)> {
        let just_self: HashMap<u64, usize> = vec![(self.slot(), 0)].into_iter().collect();
        self.rc.accounts.load_slow(&just_self, pubkey)
    }

    pub fn transaction_count(&self) -> u64 {
        self.transaction_count.load(Ordering::Relaxed) as u64
    }

    fn increment_transaction_count(&self, tx_count: usize) {
        self.transaction_count
            .fetch_add(tx_count, Ordering::Relaxed);
    }

    pub fn signature_count(&self) -> usize {
        self.signature_count.load(Ordering::Relaxed)
    }

    fn increment_signature_count(&self, signature_count: usize) {
        self.signature_count
            .fetch_add(signature_count, Ordering::Relaxed);
    }

    pub fn get_signature_confirmation_status(
        &self,
        signature: &Signature,
    ) -> Option<(usize, Result<()>)> {
        let rcache = self.src.status_cache.read().unwrap();
        rcache.get_signature_status_slow(signature, &self.ancestors)
    }

    pub fn get_signature_status(&self, signature: &Signature) -> Option<Result<()>> {
        self.get_signature_confirmation_status(signature)
            .map(|v| v.1)
    }

    pub fn has_signature(&self, signature: &Signature) -> bool {
        self.get_signature_confirmation_status(signature).is_some()
    }

    /// Hash the `accounts` HashMap. This represents a validator's interpretation
    ///  of the delta of the ledger since the last vote and up to now
    fn hash_internal_state(&self) -> Hash {
        // If there are no accounts, return the same hash as we did before
        // checkpointing.
        if let Some(accounts_delta_hash) = self.rc.accounts.hash_internal_state(self.slot()) {
            extend_and_hash(&self.parent_hash, &serialize(&accounts_delta_hash).unwrap())
        } else {
            self.parent_hash
        }
    }

    /// Return the number of ticks per slot
    pub fn ticks_per_slot(&self) -> u64 {
        self.ticks_per_slot
    }

    /// Return the number of ticks since genesis.
    pub fn tick_height(&self) -> u64 {
        // tick_height is using an AtomicUSize because AtomicU64 is not yet a stable API.
        // Until we can switch to AtomicU64, fail if usize is not the same as u64
        assert_eq!(std::usize::MAX, 0xFFFF_FFFF_FFFF_FFFF);
        self.tick_height.load(Ordering::Relaxed) as u64
    }

    /// Return the total capititalization of the Bank
    pub fn capitalization(&self) -> u64 {
        // capitalization is using an AtomicUSize because AtomicU64 is not yet a stable API.
        // Until we can switch to AtomicU64, fail if usize is not the same as u64
        assert_eq!(std::usize::MAX, 0xFFFF_FFFF_FFFF_FFFF);
        self.capitalization.load(Ordering::Relaxed) as u64
    }

    /// Return this bank's max_tick_height
    pub fn max_tick_height(&self) -> u64 {
        self.max_tick_height
    }

    /// Return the number of slots per epoch for the given epoch
    pub fn get_slots_in_epoch(&self, epoch: u64) -> u64 {
        self.epoch_schedule.get_slots_in_epoch(epoch)
    }

    /// returns the epoch for which this bank's stakers_slot_offset and slot would
    ///  need to cache stakers
    pub fn get_stakers_epoch(&self, slot: u64) -> u64 {
        self.epoch_schedule.get_stakers_epoch(slot)
    }

    /// a bank-level cache of vote accounts
    fn update_cached_accounts(
        &self,
        txs: &[Transaction],
        res: &[Result<()>],
        loaded: &[Result<(InstructionAccounts, InstructionLoaders, InstructionCredits)>],
    ) {
        for (i, raccs) in loaded.iter().enumerate() {
            if res[i].is_err() || raccs.is_err() {
                continue;
            }

            let message = &txs[i].message();
            let acc = raccs.as_ref().unwrap();

            for (pubkey, account) in
                message
                    .account_keys
                    .iter()
                    .zip(acc.0.iter())
                    .filter(|(_, account)| {
                        (Stakes::is_stake(account)) || storage_utils::is_storage(account)
                    })
            {
                if Stakes::is_stake(account) {
                    self.stakes.write().unwrap().store(pubkey, account);
                } else if storage_utils::is_storage(account) {
                    self.storage_accounts
                        .write()
                        .unwrap()
                        .store(pubkey, account);
                }
            }
        }
    }

    pub fn storage_accounts(&self) -> StorageAccounts {
        self.storage_accounts.read().unwrap().clone()
    }

    /// current vote accounts for this bank along with the stake
    ///   attributed to each account
    pub fn vote_accounts(&self) -> HashMap<Pubkey, (u64, Account)> {
        self.stakes.read().unwrap().vote_accounts().clone()
    }

    /// vote accounts for the specific epoch along with the stake
    ///   attributed to each account
    pub fn epoch_vote_accounts(&self, epoch: u64) -> Option<&HashMap<Pubkey, (u64, Account)>> {
        self.epoch_stakes.get(&epoch).map(Stakes::vote_accounts)
    }

    /// given a slot, return the epoch and offset into the epoch this slot falls
    /// e.g. with a fixed number for slots_per_epoch, the calculation is simply:
    ///
    ///  ( slot/slots_per_epoch, slot % slots_per_epoch )
    ///
    pub fn get_epoch_and_slot_index(&self, slot: u64) -> (u64, u64) {
        self.epoch_schedule.get_epoch_and_slot_index(slot)
    }

    pub fn is_votable(&self) -> bool {
        let max_tick_height = (self.slot + 1) * self.ticks_per_slot - 1;
        self.is_delta.load(Ordering::Relaxed) && self.tick_height() == max_tick_height
    }

    /// Add an instruction processor to intercept instructions before the dynamic loader.
    pub fn add_instruction_processor(
        &mut self,
        program_id: Pubkey,
        process_instruction: ProcessInstruction,
    ) {
        self.message_processor
            .add_instruction_processor(program_id, process_instruction);

        // Register a bogus executable account, which will be loaded and ignored.
        self.register_native_instruction_processor("", &program_id);
    }

    pub fn compare_bank(&self, dbank: &Bank) {
        assert_eq!(self.slot, dbank.slot);
        assert_eq!(self.collector_id, dbank.collector_id);
        assert_eq!(self.epoch_schedule, dbank.epoch_schedule);
        assert_eq!(self.ticks_per_slot, dbank.ticks_per_slot);
        assert_eq!(self.parent_hash, dbank.parent_hash);
        assert_eq!(
            self.tick_height.load(Ordering::Relaxed),
            dbank.tick_height.load(Ordering::Relaxed)
        );
        assert_eq!(
            self.is_delta.load(Ordering::Relaxed),
            dbank.is_delta.load(Ordering::Relaxed)
        );

        let st = self.stakes.read().unwrap();
        let dst = dbank.stakes.read().unwrap();
        assert_eq!(*st, *dst);

        let bh = self.hash.read().unwrap();
        let dbh = dbank.hash.read().unwrap();
        assert_eq!(*bh, *dbh);

        let bhq = self.blockhash_queue.read().unwrap();
        let dbhq = dbank.blockhash_queue.read().unwrap();
        assert_eq!(*bhq, *dbhq);

        let sc = self.src.status_cache.read().unwrap();
        let dsc = dbank.src.status_cache.read().unwrap();
        assert_eq!(*sc, *dsc);
        assert_eq!(
            self.rc.accounts.hash_internal_state(self.slot),
            dbank.rc.accounts.hash_internal_state(dbank.slot)
        );
    }
}

impl Drop for Bank {
    fn drop(&mut self) {
        // For root forks this is a noop
        self.rc.accounts.purge_fork(self.slot());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::epoch_schedule::MINIMUM_SLOTS_PER_EPOCH;
    use crate::genesis_utils::{
        create_genesis_block_with_leader, GenesisBlockInfo, BOOTSTRAP_LEADER_LAMPORTS,
    };
    use bincode::{deserialize_from, serialize_into, serialized_size};
    use solana_sdk::genesis_block::create_genesis_block;
    use solana_sdk::hash;
    use solana_sdk::instruction::InstructionError;
    use solana_sdk::poh_config::PohConfig;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_instruction;
    use solana_sdk::system_transaction;
    use solana_sdk::timing::DEFAULT_TICKS_PER_SLOT;
    use solana_vote_api::vote_instruction;
    use solana_vote_api::vote_state::{VoteState, MAX_LOCKOUT_HISTORY};
    use std::io::Cursor;
    use std::time::Duration;

    #[test]
    fn test_bank_new() {
        let dummy_leader_pubkey = Pubkey::new_rand();
        let dummy_leader_lamports = BOOTSTRAP_LEADER_LAMPORTS;
        let mint_lamports = 10_000;
        let GenesisBlockInfo {
            genesis_block,
            mint_keypair,
            voting_keypair,
            ..
        } = create_genesis_block_with_leader(
            mint_lamports,
            &dummy_leader_pubkey,
            dummy_leader_lamports,
        );
        let bank = Bank::new(&genesis_block);
        assert_eq!(bank.get_balance(&mint_keypair.pubkey()), mint_lamports);
        assert_eq!(
            bank.get_balance(&voting_keypair.pubkey()),
            dummy_leader_lamports /* 1 token goes to the vote account associated with dummy_leader_lamports */
        );
    }

    #[test]
    fn test_bank_capitalization() {
        let bank = Arc::new(Bank::new(&GenesisBlock {
            accounts: vec![(Pubkey::default(), Account::new(42, 0, &Pubkey::default()),); 42],
            ..GenesisBlock::default()
        }));
        assert_eq!(bank.capitalization(), 42 * 42);
        let bank1 = Bank::new_from_parent(&bank, &Pubkey::default(), 1);
        assert_eq!(bank1.capitalization(), 42 * 42);
    }

    #[test]
    fn test_bank_update_rewards() {
        // create a bank that ticks really slowly...
        let bank = Arc::new(Bank::new(&GenesisBlock {
            accounts: vec![
                (
                    Pubkey::default(),
                    Account::new(1_000_000_000, 0, &Pubkey::default()),
                );
                42
            ],
            // set it up so the first epoch is a full year long
            poh_config: PohConfig {
                target_tick_duration: Duration::from_secs(
                    SECONDS_PER_YEAR as u64
                        / MINIMUM_SLOTS_PER_EPOCH as u64
                        / DEFAULT_TICKS_PER_SLOT,
                ),
                hashes_per_tick: None,
            },

            ..GenesisBlock::default()
        }));
        assert_eq!(bank.capitalization(), 42 * 1_000_000_000);

        let ((vote_id, mut vote_account), stake) =
            crate::stakes::tests::create_staked_node_accounts(1_0000);

        // set up stakes and vote accounts
        bank.store_account(&stake.0, &stake.1);

        // generate some rewards
        let mut vote_state = VoteState::from(&vote_account).unwrap();
        for i in 0..MAX_LOCKOUT_HISTORY + 42 {
            vote_state.process_slot_vote_unchecked(i as u64);
            vote_state.to(&mut vote_account).unwrap();
            bank.store_account(&vote_id, &vote_account);
        }
        bank.store_account(&vote_id, &vote_account);

        // put a child bank in epoch 1, which calls update_rewards()...
        let bank1 = Bank::new_from_parent(
            &bank,
            &Pubkey::default(),
            bank.get_slots_in_epoch(bank.epoch()) + 1,
        );
        // verify that there's inflation
        assert_ne!(bank1.capitalization(), bank.capitalization());
        // verify the inflation is in rewards pools
        let inflation = bank1.capitalization() - bank.capitalization();

        let validator_rewards: u64 = bank1
            .stakes
            .read()
            .unwrap()
            .mining_pools()
            .map(|(_key, account)| account.lamports)
            .sum();

        assert_eq!(validator_rewards, inflation);
    }

    #[test]
    fn test_two_payments_to_one_party() {
        let (genesis_block, mint_keypair) = create_genesis_block(10_000);
        let pubkey = Pubkey::new_rand();
        let bank = Bank::new(&genesis_block);
        assert_eq!(bank.last_blockhash(), genesis_block.hash());

        bank.transfer(1_000, &mint_keypair, &pubkey).unwrap();
        assert_eq!(bank.get_balance(&pubkey), 1_000);

        bank.transfer(500, &mint_keypair, &pubkey).unwrap();
        assert_eq!(bank.get_balance(&pubkey), 1_500);
        assert_eq!(bank.transaction_count(), 2);
    }

    #[test]
    fn test_one_source_two_tx_one_batch() {
        let (genesis_block, mint_keypair) = create_genesis_block(1);
        let key1 = Pubkey::new_rand();
        let key2 = Pubkey::new_rand();
        let bank = Bank::new(&genesis_block);
        assert_eq!(bank.last_blockhash(), genesis_block.hash());

        let t1 = system_transaction::transfer(&mint_keypair, &key1, 1, genesis_block.hash());
        let t2 = system_transaction::transfer(&mint_keypair, &key2, 1, genesis_block.hash());
        let res = bank.process_transactions(&vec![t1.clone(), t2.clone()]);
        assert_eq!(res.len(), 2);
        assert_eq!(res[0], Ok(()));
        assert_eq!(res[1], Err(TransactionError::AccountInUse));
        assert_eq!(bank.get_balance(&mint_keypair.pubkey()), 0);
        assert_eq!(bank.get_balance(&key1), 1);
        assert_eq!(bank.get_balance(&key2), 0);
        assert_eq!(bank.get_signature_status(&t1.signatures[0]), Some(Ok(())));
        // TODO: Transactions that fail to pay a fee could be dropped silently.
        // Non-instruction errors don't get logged in the signature cache
        assert_eq!(bank.get_signature_status(&t2.signatures[0]), None);
    }

    #[test]
    fn test_one_tx_two_out_atomic_fail() {
        let (genesis_block, mint_keypair) = create_genesis_block(1);
        let key1 = Pubkey::new_rand();
        let key2 = Pubkey::new_rand();
        let bank = Bank::new(&genesis_block);
        let instructions =
            system_instruction::transfer_many(&mint_keypair.pubkey(), &[(key1, 1), (key2, 1)]);
        let tx = Transaction::new_signed_instructions(
            &[&mint_keypair],
            instructions,
            genesis_block.hash(),
        );
        assert_eq!(
            bank.process_transaction(&tx).unwrap_err(),
            TransactionError::InstructionError(
                1,
                InstructionError::new_result_with_negative_lamports(),
            )
        );
        assert_eq!(bank.get_balance(&mint_keypair.pubkey()), 1);
        assert_eq!(bank.get_balance(&key1), 0);
        assert_eq!(bank.get_balance(&key2), 0);
    }

    #[test]
    fn test_one_tx_two_out_atomic_pass() {
        let (genesis_block, mint_keypair) = create_genesis_block(2);
        let key1 = Pubkey::new_rand();
        let key2 = Pubkey::new_rand();
        let bank = Bank::new(&genesis_block);
        let instructions =
            system_instruction::transfer_many(&mint_keypair.pubkey(), &[(key1, 1), (key2, 1)]);
        let tx = Transaction::new_signed_instructions(
            &[&mint_keypair],
            instructions,
            genesis_block.hash(),
        );
        bank.process_transaction(&tx).unwrap();
        assert_eq!(bank.get_balance(&mint_keypair.pubkey()), 0);
        assert_eq!(bank.get_balance(&key1), 1);
        assert_eq!(bank.get_balance(&key2), 1);
    }

    // This test demonstrates that fees are paid even when a program fails.
    #[test]
    fn test_detect_failed_duplicate_transactions() {
        let (mut genesis_block, mint_keypair) = create_genesis_block(2);
        genesis_block.fee_calculator.lamports_per_signature = 1;
        let bank = Bank::new(&genesis_block);

        let dest = Keypair::new();

        // source with 0 program context
        let tx = system_transaction::create_user_account(
            &mint_keypair,
            &dest.pubkey(),
            2,
            genesis_block.hash(),
        );
        let signature = tx.signatures[0];
        assert!(!bank.has_signature(&signature));

        assert_eq!(
            bank.process_transaction(&tx),
            Err(TransactionError::InstructionError(
                0,
                InstructionError::new_result_with_negative_lamports(),
            ))
        );

        // The lamports didn't move, but the from address paid the transaction fee.
        assert_eq!(bank.get_balance(&dest.pubkey()), 0);

        // This should be the original balance minus the transaction fee.
        assert_eq!(bank.get_balance(&mint_keypair.pubkey()), 1);
    }

    #[test]
    fn test_account_not_found() {
        let (genesis_block, mint_keypair) = create_genesis_block(0);
        let bank = Bank::new(&genesis_block);
        let keypair = Keypair::new();
        assert_eq!(
            bank.transfer(1, &keypair, &mint_keypair.pubkey()),
            Err(TransactionError::AccountNotFound)
        );
        assert_eq!(bank.transaction_count(), 0);
    }

    #[test]
    fn test_insufficient_funds() {
        let (genesis_block, mint_keypair) = create_genesis_block(11_000);
        let bank = Bank::new(&genesis_block);
        let pubkey = Pubkey::new_rand();
        bank.transfer(1_000, &mint_keypair, &pubkey).unwrap();
        assert_eq!(bank.transaction_count(), 1);
        assert_eq!(bank.get_balance(&pubkey), 1_000);
        assert_eq!(
            bank.transfer(10_001, &mint_keypair, &pubkey),
            Err(TransactionError::InstructionError(
                0,
                InstructionError::new_result_with_negative_lamports(),
            ))
        );
        assert_eq!(bank.transaction_count(), 1);

        let mint_pubkey = mint_keypair.pubkey();
        assert_eq!(bank.get_balance(&mint_pubkey), 10_000);
        assert_eq!(bank.get_balance(&pubkey), 1_000);
    }

    #[test]
    fn test_transfer_to_newb() {
        let (genesis_block, mint_keypair) = create_genesis_block(10_000);
        let bank = Bank::new(&genesis_block);
        let pubkey = Pubkey::new_rand();
        bank.transfer(500, &mint_keypair, &pubkey).unwrap();
        assert_eq!(bank.get_balance(&pubkey), 500);
    }

    #[test]
    fn test_bank_deposit() {
        let (genesis_block, _mint_keypair) = create_genesis_block(100);
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
        let (genesis_block, _mint_keypair) = create_genesis_block(100);
        let bank = Bank::new(&genesis_block);

        // Test no account
        let key = Keypair::new();
        assert_eq!(
            bank.withdraw(&key.pubkey(), 10),
            Err(TransactionError::AccountNotFound)
        );

        bank.deposit(&key.pubkey(), 3);
        assert_eq!(bank.get_balance(&key.pubkey()), 3);

        // Low balance
        assert_eq!(
            bank.withdraw(&key.pubkey(), 10),
            Err(TransactionError::InsufficientFundsForFee)
        );

        // Enough balance
        assert_eq!(bank.withdraw(&key.pubkey(), 2), Ok(()));
        assert_eq!(bank.get_balance(&key.pubkey()), 1);
    }

    fn goto_end_of_slot(bank: &mut Bank) {
        let mut tick_hash = bank.last_blockhash();
        loop {
            tick_hash = extend_and_hash(&tick_hash, &[42]);
            bank.register_tick(&tick_hash);
            if tick_hash == bank.last_blockhash() {
                bank.freeze();
                return;
            }
        }
    }

    #[test]
    fn test_bank_tx_fee() {
        let leader = Pubkey::new_rand();
        let GenesisBlockInfo {
            mut genesis_block,
            mint_keypair,
            ..
        } = create_genesis_block_with_leader(100, &leader, 3);
        genesis_block.fee_calculator.lamports_per_signature = 3;
        let mut bank = Bank::new(&genesis_block);

        let key = Keypair::new();
        let tx =
            system_transaction::transfer(&mint_keypair, &key.pubkey(), 2, bank.last_blockhash());

        let initial_balance = bank.get_balance(&leader);
        assert_eq!(bank.process_transaction(&tx), Ok(()));
        assert_eq!(bank.get_balance(&key.pubkey()), 2);
        assert_eq!(bank.get_balance(&mint_keypair.pubkey()), 100 - 5);

        assert_eq!(bank.get_balance(&leader), initial_balance);
        goto_end_of_slot(&mut bank);
        assert_eq!(bank.signature_count(), 1);
        assert_eq!(bank.get_balance(&leader), initial_balance + 3); // Leader collects fee after the bank is frozen

        // Verify that an InstructionError collects fees, too
        let mut bank = Bank::new_from_parent(&Arc::new(bank), &leader, 1);
        let mut tx =
            system_transaction::transfer(&mint_keypair, &key.pubkey(), 1, bank.last_blockhash());
        // Create a bogus instruction to system_program to cause an instruction error
        tx.message.instructions[0].data[0] = 40;

        bank.process_transaction(&tx)
            .expect_err("instruction error");
        assert_eq!(bank.get_balance(&key.pubkey()), 2); // no change
        assert_eq!(bank.get_balance(&mint_keypair.pubkey()), 100 - 5 - 3); // mint_keypair still pays a fee
        goto_end_of_slot(&mut bank);
        assert_eq!(bank.signature_count(), 1);

        // Profit! 2 transaction signatures processed at 3 lamports each
        assert_eq!(bank.get_balance(&leader), initial_balance + 6);
    }

    #[test]
    fn test_bank_blockhash_fee_schedule() {
        //solana_logger::setup();

        let leader = Pubkey::new_rand();
        let GenesisBlockInfo {
            mut genesis_block,
            mint_keypair,
            ..
        } = create_genesis_block_with_leader(1_000_000, &leader, 3);
        genesis_block.fee_calculator.target_lamports_per_signature = 1000;
        genesis_block.fee_calculator.target_signatures_per_slot = 1;

        let mut bank = Bank::new(&genesis_block);
        goto_end_of_slot(&mut bank);
        let (cheap_blockhash, cheap_fee_calculator) = bank.last_blockhash_with_fee_calculator();
        assert_eq!(cheap_fee_calculator.lamports_per_signature, 0);

        let mut bank = Bank::new_from_parent(&Arc::new(bank), &leader, 1);
        goto_end_of_slot(&mut bank);
        let (expensive_blockhash, expensive_fee_calculator) =
            bank.last_blockhash_with_fee_calculator();
        assert!(
            cheap_fee_calculator.lamports_per_signature
                < expensive_fee_calculator.lamports_per_signature
        );

        let bank = Bank::new_from_parent(&Arc::new(bank), &leader, 2);

        // Send a transfer using cheap_blockhash
        let key = Keypair::new();
        let initial_mint_balance = bank.get_balance(&mint_keypair.pubkey());
        let tx = system_transaction::transfer(&mint_keypair, &key.pubkey(), 1, cheap_blockhash);
        assert_eq!(bank.process_transaction(&tx), Ok(()));
        assert_eq!(bank.get_balance(&key.pubkey()), 1);
        assert_eq!(
            bank.get_balance(&mint_keypair.pubkey()),
            initial_mint_balance - 1 - cheap_fee_calculator.lamports_per_signature
        );

        // Send a transfer using expensive_blockhash
        let key = Keypair::new();
        let initial_mint_balance = bank.get_balance(&mint_keypair.pubkey());
        let tx = system_transaction::transfer(&mint_keypair, &key.pubkey(), 1, expensive_blockhash);
        assert_eq!(bank.process_transaction(&tx), Ok(()));
        assert_eq!(bank.get_balance(&key.pubkey()), 1);
        assert_eq!(
            bank.get_balance(&mint_keypair.pubkey()),
            initial_mint_balance - 1 - expensive_fee_calculator.lamports_per_signature
        );
    }

    #[test]
    fn test_filter_program_errors_and_collect_fee() {
        let leader = Pubkey::new_rand();
        let GenesisBlockInfo {
            mut genesis_block,
            mint_keypair,
            ..
        } = create_genesis_block_with_leader(100, &leader, 3);
        genesis_block.fee_calculator.lamports_per_signature = 2;
        let bank = Bank::new(&genesis_block);

        let key = Keypair::new();
        let tx1 =
            system_transaction::transfer(&mint_keypair, &key.pubkey(), 2, genesis_block.hash());
        let tx2 =
            system_transaction::transfer(&mint_keypair, &key.pubkey(), 5, genesis_block.hash());

        let results = vec![
            Ok(()),
            Err(TransactionError::InstructionError(
                1,
                InstructionError::new_result_with_negative_lamports(),
            )),
        ];

        let initial_balance = bank.get_balance(&leader);
        let results = bank.filter_program_errors_and_collect_fee(&vec![tx1, tx2], &results);
        bank.freeze();
        assert_eq!(bank.get_balance(&leader), initial_balance + 2 + 2);
        assert_eq!(results[0], Ok(()));
        assert_eq!(results[1], Ok(()));
    }

    #[test]
    fn test_debits_before_credits() {
        let (genesis_block, mint_keypair) = create_genesis_block(2);
        let bank = Bank::new(&genesis_block);
        let keypair = Keypair::new();
        let tx0 = system_transaction::create_user_account(
            &mint_keypair,
            &keypair.pubkey(),
            2,
            genesis_block.hash(),
        );
        let tx1 = system_transaction::create_user_account(
            &keypair,
            &mint_keypair.pubkey(),
            1,
            genesis_block.hash(),
        );
        let txs = vec![tx0, tx1];
        let results = bank.process_transactions(&txs);
        assert!(results[1].is_err());

        // Assert bad transactions aren't counted.
        assert_eq!(bank.transaction_count(), 1);
    }

    #[test]
    fn test_need_credit_only_accounts() {
        let (genesis_block, mint_keypair) = create_genesis_block(10);
        let bank = Bank::new(&genesis_block);
        let payer0 = Keypair::new();
        let payer1 = Keypair::new();
        let recipient = Pubkey::new_rand();
        // Fund additional payers
        bank.transfer(3, &mint_keypair, &payer0.pubkey()).unwrap();
        bank.transfer(3, &mint_keypair, &payer1.pubkey()).unwrap();
        let tx0 = system_transaction::transfer(&mint_keypair, &recipient, 1, genesis_block.hash());
        let tx1 = system_transaction::transfer(&payer0, &recipient, 1, genesis_block.hash());
        let tx2 = system_transaction::transfer(&payer1, &recipient, 1, genesis_block.hash());
        let txs = vec![tx0, tx1, tx2];
        let results = bank.process_transactions(&txs);

        // If multiple transactions attempt to deposit into the same account, only the first will
        // succeed, even though such atomic adds are safe. A System Transfer `To` account should be
        // given credit-only handling

        assert_eq!(results[0], Ok(()));
        assert_eq!(results[1], Err(TransactionError::AccountInUse));
        assert_eq!(results[2], Err(TransactionError::AccountInUse));

        // After credit-only account handling is implemented, the following checks should pass instead:
        // assert_eq!(results[0], Ok(()));
        // assert_eq!(results[1], Ok(()));
        // assert_eq!(results[2], Ok(()));
    }

    #[test]
    fn test_interleaving_locks() {
        let (genesis_block, mint_keypair) = create_genesis_block(3);
        let bank = Bank::new(&genesis_block);
        let alice = Keypair::new();
        let bob = Keypair::new();

        let tx1 = system_transaction::create_user_account(
            &mint_keypair,
            &alice.pubkey(),
            1,
            genesis_block.hash(),
        );
        let pay_alice = vec![tx1];

        let lock_result = bank.lock_accounts(&pay_alice);
        let results_alice = bank.load_execute_and_commit_transactions(
            &pay_alice,
            &lock_result,
            MAX_RECENT_BLOCKHASHES,
        );
        assert_eq!(results_alice[0], Ok(()));

        // try executing an interleaved transfer twice
        assert_eq!(
            bank.transfer(1, &mint_keypair, &bob.pubkey()),
            Err(TransactionError::AccountInUse)
        );
        // the second time should fail as well
        // this verifies that `unlock_accounts` doesn't unlock `AccountInUse` accounts
        assert_eq!(
            bank.transfer(1, &mint_keypair, &bob.pubkey()),
            Err(TransactionError::AccountInUse)
        );

        drop(lock_result);

        assert!(bank.transfer(2, &mint_keypair, &bob.pubkey()).is_ok());
    }

    #[test]
    fn test_credit_only_relaxed_locks() {
        use solana_sdk::message::{Message, MessageHeader};

        let (genesis_block, mint_keypair) = create_genesis_block(3);
        let bank = Bank::new(&genesis_block);
        let key0 = Keypair::new();
        let key1 = Pubkey::new_rand();
        let key2 = Pubkey::new_rand();

        let message = Message {
            header: MessageHeader {
                num_required_signatures: 1,
                num_credit_only_signed_accounts: 0,
                num_credit_only_unsigned_accounts: 1,
            },
            account_keys: vec![key0.pubkey(), key1, key2],
            recent_blockhash: Hash::default(),
            instructions: vec![],
        };
        let tx0 = Transaction::new(&[&key0], message, genesis_block.hash());
        let txs = vec![tx0];

        let lock_result = bank.lock_accounts(&txs);
        assert_eq!(lock_result.locked_accounts_results(), &vec![Ok(())]);

        // Try executing a tx loading a credit-only account from tx0
        assert!(bank.transfer(1, &mint_keypair, &key2).is_ok());
        // Try executing a tx loading a account locked as credit-debit in tx0
        assert_eq!(
            bank.transfer(1, &mint_keypair, &key1),
            Err(TransactionError::AccountInUse)
        );
    }

    #[test]
    fn test_bank_invalid_account_index() {
        let (genesis_block, mint_keypair) = create_genesis_block(1);
        let keypair = Keypair::new();
        let bank = Bank::new(&genesis_block);

        let tx =
            system_transaction::transfer(&mint_keypair, &keypair.pubkey(), 1, genesis_block.hash());

        let mut tx_invalid_program_index = tx.clone();
        tx_invalid_program_index.message.instructions[0].program_ids_index = 42;
        assert_eq!(
            bank.process_transaction(&tx_invalid_program_index),
            Err(TransactionError::InvalidAccountIndex)
        );

        let mut tx_invalid_account_index = tx.clone();
        tx_invalid_account_index.message.instructions[0].accounts[0] = 42;
        assert_eq!(
            bank.process_transaction(&tx_invalid_account_index),
            Err(TransactionError::InvalidAccountIndex)
        );
    }

    #[test]
    fn test_bank_pay_to_self() {
        let (genesis_block, mint_keypair) = create_genesis_block(1);
        let key1 = Keypair::new();
        let bank = Bank::new(&genesis_block);

        bank.transfer(1, &mint_keypair, &key1.pubkey()).unwrap();
        assert_eq!(bank.get_balance(&key1.pubkey()), 1);
        let tx = system_transaction::transfer(&key1, &key1.pubkey(), 1, genesis_block.hash());
        let res = bank.process_transactions(&vec![tx.clone()]);
        assert_eq!(res.len(), 1);
        assert_eq!(bank.get_balance(&key1.pubkey()), 1);

        // TODO: Why do we convert errors to Oks?
        //res[0].clone().unwrap_err();

        bank.get_signature_status(&tx.signatures[0])
            .unwrap()
            .unwrap_err();
    }

    fn new_from_parent(parent: &Arc<Bank>) -> Bank {
        Bank::new_from_parent(parent, &Pubkey::default(), parent.slot() + 1)
    }

    /// Verify that the parent's vector is computed correctly
    #[test]
    fn test_bank_parents() {
        let (genesis_block, _) = create_genesis_block(1);
        let parent = Arc::new(Bank::new(&genesis_block));

        let bank = new_from_parent(&parent);
        assert!(Arc::ptr_eq(&bank.parents()[0], &parent));
    }

    /// Verifies that last ids and status cache are correctly referenced from parent
    #[test]
    fn test_bank_parent_duplicate_signature() {
        let (genesis_block, mint_keypair) = create_genesis_block(2);
        let key1 = Keypair::new();
        let parent = Arc::new(Bank::new(&genesis_block));

        let tx =
            system_transaction::transfer(&mint_keypair, &key1.pubkey(), 1, genesis_block.hash());
        assert_eq!(parent.process_transaction(&tx), Ok(()));
        let bank = new_from_parent(&parent);
        assert_eq!(
            bank.process_transaction(&tx),
            Err(TransactionError::DuplicateSignature)
        );
    }

    /// Verifies that last ids and accounts are correctly referenced from parent
    #[test]
    fn test_bank_parent_account_spend() {
        let (genesis_block, mint_keypair) = create_genesis_block(2);
        let key1 = Keypair::new();
        let key2 = Keypair::new();
        let parent = Arc::new(Bank::new(&genesis_block));

        let tx =
            system_transaction::transfer(&mint_keypair, &key1.pubkey(), 1, genesis_block.hash());
        assert_eq!(parent.process_transaction(&tx), Ok(()));
        let bank = new_from_parent(&parent);
        let tx = system_transaction::transfer(&key1, &key2.pubkey(), 1, genesis_block.hash());
        assert_eq!(bank.process_transaction(&tx), Ok(()));
        assert_eq!(parent.get_signature_status(&tx.signatures[0]), None);
    }

    #[test]
    fn test_bank_hash_internal_state() {
        let (genesis_block, mint_keypair) = create_genesis_block(2_000);
        let bank0 = Bank::new(&genesis_block);
        let bank1 = Bank::new(&genesis_block);
        let initial_state = bank0.hash_internal_state();
        assert_eq!(bank1.hash_internal_state(), initial_state);

        let pubkey = Pubkey::new_rand();
        bank0.transfer(1_000, &mint_keypair, &pubkey).unwrap();
        assert_ne!(bank0.hash_internal_state(), initial_state);
        bank1.transfer(1_000, &mint_keypair, &pubkey).unwrap();
        assert_eq!(bank0.hash_internal_state(), bank1.hash_internal_state());

        // Checkpointing should not change its state
        let bank2 = new_from_parent(&Arc::new(bank1));
        assert_eq!(bank0.hash_internal_state(), bank2.hash_internal_state());
    }

    #[test]
    fn test_hash_internal_state_genesis() {
        let bank0 = Bank::new(&create_genesis_block(10).0);
        let bank1 = Bank::new(&create_genesis_block(20).0);
        assert_ne!(bank0.hash_internal_state(), bank1.hash_internal_state());
    }

    #[test]
    fn test_bank_hash_internal_state_squash() {
        let collector_id = Pubkey::default();
        let bank0 = Arc::new(Bank::new(&create_genesis_block(10).0));
        let hash0 = bank0.hash_internal_state();
        // save hash0 because new_from_parent
        // updates syscall entries

        let bank1 = Bank::new_from_parent(&bank0, &collector_id, 1);

        // no delta in bank1, hashes match
        assert_eq!(hash0, bank1.hash_internal_state());

        // remove parent
        bank1.squash();
        assert!(bank1.parents().is_empty());

        // hash should still match,
        //  can't use hash_internal_state() after a freeze()...
        assert_eq!(hash0, bank1.hash());
    }

    /// Verifies that last ids and accounts are correctly referenced from parent
    #[test]
    fn test_bank_squash() {
        solana_logger::setup();
        let (genesis_block, mint_keypair) = create_genesis_block(2);
        let key1 = Keypair::new();
        let key2 = Keypair::new();
        let parent = Arc::new(Bank::new(&genesis_block));

        let tx_transfer_mint_to_1 =
            system_transaction::transfer(&mint_keypair, &key1.pubkey(), 1, genesis_block.hash());
        trace!("parent process tx ");
        assert_eq!(parent.process_transaction(&tx_transfer_mint_to_1), Ok(()));
        trace!("done parent process tx ");
        assert_eq!(parent.transaction_count(), 1);
        assert_eq!(
            parent.get_signature_status(&tx_transfer_mint_to_1.signatures[0]),
            Some(Ok(()))
        );

        trace!("new form parent");
        let bank = new_from_parent(&parent);
        trace!("done new form parent");
        assert_eq!(
            bank.get_signature_status(&tx_transfer_mint_to_1.signatures[0]),
            Some(Ok(()))
        );

        assert_eq!(bank.transaction_count(), parent.transaction_count());
        let tx_transfer_1_to_2 =
            system_transaction::transfer(&key1, &key2.pubkey(), 1, genesis_block.hash());
        assert_eq!(bank.process_transaction(&tx_transfer_1_to_2), Ok(()));
        assert_eq!(bank.transaction_count(), 2);
        assert_eq!(parent.transaction_count(), 1);
        assert_eq!(
            parent.get_signature_status(&tx_transfer_1_to_2.signatures[0]),
            None
        );

        for _ in 0..3 {
            // first time these should match what happened above, assert that parents are ok
            assert_eq!(bank.get_balance(&key1.pubkey()), 0);
            assert_eq!(bank.get_account(&key1.pubkey()), None);
            assert_eq!(bank.get_balance(&key2.pubkey()), 1);
            trace!("start");
            assert_eq!(
                bank.get_signature_status(&tx_transfer_mint_to_1.signatures[0]),
                Some(Ok(()))
            );
            assert_eq!(
                bank.get_signature_status(&tx_transfer_1_to_2.signatures[0]),
                Some(Ok(()))
            );

            // works iteration 0, no-ops on iteration 1 and 2
            trace!("SQUASH");
            bank.squash();

            assert_eq!(parent.transaction_count(), 1);
            assert_eq!(bank.transaction_count(), 2);
        }
    }

    #[test]
    fn test_bank_get_account_in_parent_after_squash() {
        let (genesis_block, mint_keypair) = create_genesis_block(500);
        let parent = Arc::new(Bank::new(&genesis_block));

        let key1 = Keypair::new();

        parent.transfer(1, &mint_keypair, &key1.pubkey()).unwrap();
        assert_eq!(parent.get_balance(&key1.pubkey()), 1);
        let bank = new_from_parent(&parent);
        bank.squash();
        assert_eq!(parent.get_balance(&key1.pubkey()), 1);
    }

    #[test]
    fn test_bank_get_account_in_parent_after_squash2() {
        solana_logger::setup();
        let (genesis_block, mint_keypair) = create_genesis_block(500);
        let bank0 = Arc::new(Bank::new(&genesis_block));

        let key1 = Keypair::new();

        bank0.transfer(1, &mint_keypair, &key1.pubkey()).unwrap();
        assert_eq!(bank0.get_balance(&key1.pubkey()), 1);

        let bank1 = Arc::new(Bank::new_from_parent(&bank0, &Pubkey::default(), 1));
        bank1.transfer(3, &mint_keypair, &key1.pubkey()).unwrap();
        let bank2 = Arc::new(Bank::new_from_parent(&bank0, &Pubkey::default(), 2));
        bank2.transfer(2, &mint_keypair, &key1.pubkey()).unwrap();
        let bank3 = Arc::new(Bank::new_from_parent(&bank1, &Pubkey::default(), 3));
        bank1.squash();

        assert_eq!(bank0.get_balance(&key1.pubkey()), 1);
        assert_eq!(bank3.get_balance(&key1.pubkey()), 4);
        assert_eq!(bank2.get_balance(&key1.pubkey()), 3);
        bank3.squash();
        assert_eq!(bank1.get_balance(&key1.pubkey()), 4);

        let bank4 = Arc::new(Bank::new_from_parent(&bank3, &Pubkey::default(), 4));
        bank4.transfer(4, &mint_keypair, &key1.pubkey()).unwrap();
        assert_eq!(bank4.get_balance(&key1.pubkey()), 8);
        assert_eq!(bank3.get_balance(&key1.pubkey()), 4);
        bank4.squash();
        let bank5 = Arc::new(Bank::new_from_parent(&bank4, &Pubkey::default(), 5));
        bank5.squash();
        let bank6 = Arc::new(Bank::new_from_parent(&bank5, &Pubkey::default(), 6));
        bank6.squash();

        // This picks up the values from 4 which is the highest root:
        // TODO: if we need to access rooted banks older than this,
        // need to fix the lookup.
        assert_eq!(bank3.get_balance(&key1.pubkey()), 8);
        assert_eq!(bank2.get_balance(&key1.pubkey()), 8);

        assert_eq!(bank4.get_balance(&key1.pubkey()), 8);
    }

    #[test]
    fn test_bank_epoch_vote_accounts() {
        let leader_pubkey = Pubkey::new_rand();
        let leader_lamports = 3;
        let mut genesis_block =
            create_genesis_block_with_leader(5, &leader_pubkey, leader_lamports).genesis_block;

        // set this up weird, forces future generation, odd mod(), etc.
        //  this says: "vote_accounts for epoch X should be generated at slot index 3 in epoch X-2...
        const SLOTS_PER_EPOCH: u64 = MINIMUM_SLOTS_PER_EPOCH as u64;
        const STAKERS_SLOT_OFFSET: u64 = SLOTS_PER_EPOCH * 3 - 3;
        genesis_block.slots_per_epoch = SLOTS_PER_EPOCH;
        genesis_block.stakers_slot_offset = STAKERS_SLOT_OFFSET;
        genesis_block.epoch_warmup = false; // allows me to do the normal division stuff below

        let parent = Arc::new(Bank::new(&genesis_block));

        let vote_accounts0: Option<HashMap<_, _>> = parent.epoch_vote_accounts(0).map(|accounts| {
            accounts
                .iter()
                .filter_map(|(pubkey, (_, account))| {
                    if let Ok(vote_state) = VoteState::deserialize(&account.data) {
                        if vote_state.node_pubkey == leader_pubkey {
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
            &leader_pubkey,
            SLOTS_PER_EPOCH - (STAKERS_SLOT_OFFSET % SLOTS_PER_EPOCH),
        );

        assert!(child.epoch_vote_accounts(i).is_some());

        // child crosses epoch boundary but isn't the first slot in the epoch
        let child = Bank::new_from_parent(
            &parent,
            &leader_pubkey,
            SLOTS_PER_EPOCH - (STAKERS_SLOT_OFFSET % SLOTS_PER_EPOCH) + 1,
        );
        assert!(child.epoch_vote_accounts(i).is_some());
    }

    #[test]
    fn test_zero_signatures() {
        solana_logger::setup();
        let (genesis_block, mint_keypair) = create_genesis_block(500);
        let mut bank = Bank::new(&genesis_block);
        bank.fee_calculator.lamports_per_signature = 2;
        let key = Keypair::new();

        let mut transfer_instruction =
            system_instruction::transfer(&mint_keypair.pubkey(), &key.pubkey(), 0);
        transfer_instruction.accounts[0].is_signer = false;

        let tx = Transaction::new_signed_instructions(
            &Vec::<&Keypair>::new(),
            vec![transfer_instruction],
            bank.last_blockhash(),
        );

        assert_eq!(bank.process_transaction(&tx), Ok(()));
        assert_eq!(bank.get_balance(&key.pubkey()), 0);
    }

    #[test]
    fn test_bank_get_slots_in_epoch() {
        let (genesis_block, _) = create_genesis_block(500);

        let bank = Bank::new(&genesis_block);

        assert_eq!(bank.get_slots_in_epoch(0), MINIMUM_SLOTS_PER_EPOCH as u64);
        assert_eq!(
            bank.get_slots_in_epoch(2),
            (MINIMUM_SLOTS_PER_EPOCH * 4) as u64
        );
        assert_eq!(bank.get_slots_in_epoch(5000), genesis_block.slots_per_epoch);
    }

    #[test]
    fn test_is_delta_true() {
        let (genesis_block, mint_keypair) = create_genesis_block(500);
        let bank = Arc::new(Bank::new(&genesis_block));
        let key1 = Keypair::new();
        let tx_transfer_mint_to_1 =
            system_transaction::transfer(&mint_keypair, &key1.pubkey(), 1, genesis_block.hash());
        assert_eq!(bank.process_transaction(&tx_transfer_mint_to_1), Ok(()));
        assert_eq!(bank.is_delta.load(Ordering::Relaxed), true);

        let bank1 = new_from_parent(&bank);
        assert_eq!(bank1.is_delta.load(Ordering::Relaxed), false);
        assert_eq!(bank1.hash_internal_state(), bank.hash());
        // ticks don't make a bank into a delta
        bank1.register_tick(&Hash::default());
        assert_eq!(bank1.is_delta.load(Ordering::Relaxed), false);
        assert_eq!(bank1.hash_internal_state(), bank.hash());
    }

    #[test]
    fn test_is_votable() {
        // test normal case
        let (genesis_block, mint_keypair) = create_genesis_block(500);
        let bank = Arc::new(Bank::new(&genesis_block));
        let key1 = Keypair::new();
        assert_eq!(bank.is_votable(), false);

        // Set is_delta to true
        let tx_transfer_mint_to_1 =
            system_transaction::transfer(&mint_keypair, &key1.pubkey(), 1, genesis_block.hash());
        assert_eq!(bank.process_transaction(&tx_transfer_mint_to_1), Ok(()));
        assert_eq!(bank.is_votable(), false);

        // Register enough ticks to hit max tick height
        for i in 0..genesis_block.ticks_per_slot - 1 {
            bank.register_tick(&hash::hash(format!("hello world {}", i).as_bytes()));
        }

        assert_eq!(bank.is_votable(), true);

        // test empty bank with ticks
        let (genesis_block, _mint_keypair) = create_genesis_block(500);
        // make an empty bank at slot 1
        let bank = new_from_parent(&Arc::new(Bank::new(&genesis_block)));
        assert_eq!(bank.is_votable(), false);

        // Register enough ticks to hit max tick height
        for i in 0..genesis_block.ticks_per_slot - 1 {
            bank.register_tick(&hash::hash(format!("hello world {}", i).as_bytes()));
        }
        // empty banks aren't votable even at max tick height
        assert_eq!(bank.is_votable(), false);
    }

    #[test]
    fn test_bank_inherit_tx_count() {
        let (genesis_block, mint_keypair) = create_genesis_block(500);
        let bank0 = Arc::new(Bank::new(&genesis_block));

        // Bank 1
        let bank1 = Arc::new(new_from_parent(&bank0));
        // Bank 2
        let bank2 = new_from_parent(&bank0);

        // transfer a token
        assert_eq!(
            bank1.process_transaction(&system_transaction::transfer(
                &mint_keypair,
                &Keypair::new().pubkey(),
                1,
                genesis_block.hash(),
            )),
            Ok(())
        );

        assert_eq!(bank0.transaction_count(), 0);
        assert_eq!(bank2.transaction_count(), 0);
        assert_eq!(bank1.transaction_count(), 1);

        bank1.squash();

        assert_eq!(bank0.transaction_count(), 0);
        assert_eq!(bank2.transaction_count(), 0);
        assert_eq!(bank1.transaction_count(), 1);

        let bank6 = new_from_parent(&bank1);
        assert_eq!(bank1.transaction_count(), 1);
        assert_eq!(bank6.transaction_count(), 1);

        bank6.squash();
        assert_eq!(bank6.transaction_count(), 1);
    }

    #[test]
    fn test_bank_inherit_fee_calculator() {
        let (mut genesis_block, _mint_keypair) = create_genesis_block(500);
        genesis_block.fee_calculator.target_lamports_per_signature = 123;
        assert_eq!(genesis_block.fee_calculator.target_signatures_per_slot, 0);
        let bank0 = Arc::new(Bank::new(&genesis_block));
        let bank1 = Arc::new(new_from_parent(&bank0));
        assert_eq!(
            bank0.fee_calculator.target_lamports_per_signature,
            bank1.fee_calculator.lamports_per_signature
        );
    }

    #[test]
    fn test_bank_vote_accounts() {
        let GenesisBlockInfo {
            genesis_block,
            mint_keypair,
            ..
        } = create_genesis_block_with_leader(500, &Pubkey::new_rand(), 1);
        let bank = Arc::new(Bank::new(&genesis_block));

        let vote_accounts = bank.vote_accounts();
        assert_eq!(vote_accounts.len(), 1); // bootstrap leader has
                                            // to have a vote account

        let vote_keypair = Keypair::new();
        let instructions = vote_instruction::create_account(
            &mint_keypair.pubkey(),
            &vote_keypair.pubkey(),
            &mint_keypair.pubkey(),
            0,
            10,
        );

        let transaction = Transaction::new_signed_instructions(
            &[&mint_keypair],
            instructions,
            bank.last_blockhash(),
        );

        bank.process_transaction(&transaction).unwrap();

        let vote_accounts = bank.vote_accounts();

        assert_eq!(vote_accounts.len(), 2);

        assert!(vote_accounts.get(&vote_keypair.pubkey()).is_some());

        assert!(bank.withdraw(&vote_keypair.pubkey(), 10).is_ok());

        let vote_accounts = bank.vote_accounts();

        assert_eq!(vote_accounts.len(), 1);
    }

    #[test]
    fn test_bank_0_votable() {
        let (genesis_block, _) = create_genesis_block(500);
        let bank = Arc::new(Bank::new(&genesis_block));
        //set tick height to max
        let max_tick_height = ((bank.slot + 1) * bank.ticks_per_slot - 1) as usize;
        bank.tick_height.store(max_tick_height, Ordering::Relaxed);
        assert!(bank.is_votable());
    }

    #[test]
    fn test_bank_fees_account() {
        let (mut genesis_block, _) = create_genesis_block(500);
        genesis_block.fee_calculator.lamports_per_signature = 12345;
        let bank = Arc::new(Bank::new(&genesis_block));

        let fees_account = bank.get_account(&fees::id()).unwrap();
        let fees = Fees::from(&fees_account).unwrap();
        assert_eq!(
            bank.fee_calculator.lamports_per_signature,
            fees.fee_calculator.lamports_per_signature
        );
        assert_eq!(fees.fee_calculator.lamports_per_signature, 12345);
    }

    #[test]
    fn test_bank_tick_height_account() {
        let (genesis_block, _) = create_genesis_block(1);
        let bank = Bank::new(&genesis_block);

        for i in 0..10 {
            bank.register_tick(&hash::hash(format!("hashing {}", i).as_bytes()));
        }

        let tick_account = bank.get_account(&tick_height::id()).unwrap();
        let tick_height = TickHeight::from(&tick_account).unwrap();
        assert_eq!(bank.tick_height(), tick_height);
        assert_eq!(tick_height, 10);
    }

    #[test]
    fn test_is_delta_with_no_committables() {
        let (genesis_block, mint_keypair) = create_genesis_block(8000);
        let bank = Bank::new(&genesis_block);
        bank.is_delta.store(false, Ordering::Relaxed);

        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();
        let fail_tx = system_transaction::create_user_account(
            &keypair1,
            &keypair2.pubkey(),
            1,
            bank.last_blockhash(),
        );

        // Should fail with TransactionError::AccountNotFound, which means
        // the account which this tx operated on will not be committed. Thus
        // the bank is_delta should still be false
        assert_eq!(
            bank.process_transaction(&fail_tx),
            Err(TransactionError::AccountNotFound)
        );

        // Check the bank is_delta is still false
        assert!(!bank.is_delta.load(Ordering::Relaxed));

        // Should fail with InstructionError, but InstructionErrors are committable,
        // so is_delta should be true
        assert_eq!(
            bank.transfer(10_001, &mint_keypair, &Pubkey::new_rand()),
            Err(TransactionError::InstructionError(
                0,
                InstructionError::new_result_with_negative_lamports(),
            ))
        );

        assert!(bank.is_delta.load(Ordering::Relaxed));
    }

    #[test]
    fn test_bank_serialize() {
        let (genesis_block, _) = create_genesis_block(500);
        let bank0 = Arc::new(Bank::new(&genesis_block));
        let bank = new_from_parent(&bank0);

        // Test new account
        let key = Keypair::new();
        bank.deposit(&key.pubkey(), 10);
        assert_eq!(bank.get_balance(&key.pubkey()), 10);

        let len = serialized_size(&bank).unwrap() + serialized_size(&bank.rc).unwrap();
        let mut buf = vec![0u8; len as usize];
        let mut writer = Cursor::new(&mut buf[..]);
        serialize_into(&mut writer, &bank).unwrap();
        serialize_into(&mut writer, &bank.rc).unwrap();

        let mut rdr = Cursor::new(&buf[..]);
        let mut dbank: Bank = deserialize_from(&mut rdr).unwrap();
        let mut reader = BufReader::new(&buf[rdr.position() as usize..]);
        dbank.set_bank_rc(
            &BankRc::new(Some(bank0.accounts().paths.clone())),
            &StatusCacheRc::default(),
        );
        assert!(dbank.rc.update_from_stream(&mut reader).is_ok());
        assert_eq!(dbank.get_balance(&key.pubkey()), 10);
        bank.compare_bank(&dbank);
    }
}

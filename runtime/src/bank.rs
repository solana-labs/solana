//! The `bank` module tracks client accounts and the progress of on-chain
//! programs. It offers a high-level API that signs transactions
//! on behalf of the caller, and a low-level API for when they have
//! already been signed and verified.
use crate::transaction_utils::OrderedIterator;
use crate::{
    accounts::{Accounts, TransactionLoadResult},
    accounts_db::{AccountStorageEntry, AccountsDBSerialize, AppendVecId, ErrorCounters},
    accounts_index::Fork,
    blockhash_queue::BlockhashQueue,
    epoch_schedule::EpochSchedule,
    message_processor::{MessageProcessor, ProcessInstruction},
    rent_collector::RentCollector,
    serde_utils::{
        deserialize_atomicbool, deserialize_atomicusize, serialize_atomicbool,
        serialize_atomicusize,
    },
    stakes::Stakes,
    status_cache::{SlotDelta, StatusCache},
    storage_utils,
    storage_utils::StorageAccounts,
    transaction_batch::TransactionBatch,
};
use bincode::{deserialize_from, serialize_into};
use byteorder::{ByteOrder, LittleEndian};
use itertools::Itertools;
use log::*;
use serde::{Deserialize, Serialize};
use solana_measure::measure::Measure;
use solana_metrics::{
    datapoint_info, inc_new_counter_debug, inc_new_counter_error, inc_new_counter_info,
};
use solana_sdk::{
    account::Account,
    clock::{get_segment_from_slot, Epoch, Slot, MAX_RECENT_BLOCKHASHES},
    fee_calculator::FeeCalculator,
    genesis_block::GenesisBlock,
    hash::{hashv, Hash},
    inflation::Inflation,
    native_loader,
    pubkey::Pubkey,
    signature::{Keypair, Signature},
    system_transaction,
    sysvar::{
        clock, fees, rent, rewards,
        slot_hashes::{self, SlotHashes},
        stake_history,
    },
    timing::duration_as_ns,
    transaction::{Result, Transaction, TransactionError},
};
use std::collections::HashMap;
use std::io::{BufReader, Cursor, Error as IOError, Read};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock, RwLockReadGuard};

pub const SECONDS_PER_YEAR: f64 = (365.25 * 24.0 * 60.0 * 60.0);

type BankStatusCache = StatusCache<Result<()>>;

#[derive(Default)]
pub struct BankRc {
    /// where all the Accounts are stored
    accounts: Arc<Accounts>,

    /// Previous checkpoint of this bank
    parent: RwLock<Option<Arc<Bank>>>,

    /// Current slot
    slot: u64,
}

impl BankRc {
    pub fn new(account_paths: String, id: AppendVecId, slot: u64) -> Self {
        let accounts = Accounts::new(Some(account_paths));
        accounts
            .accounts_db
            .next_id
            .store(id as usize, Ordering::Relaxed);
        BankRc {
            accounts: Arc::new(accounts),
            parent: RwLock::new(None),
            slot,
        }
    }

    pub fn accounts_from_stream<R: Read, P: AsRef<Path>>(
        &self,
        mut stream: &mut BufReader<R>,
        local_paths: String,
        append_vecs_path: P,
    ) -> std::result::Result<(), IOError> {
        let _len: usize =
            deserialize_from(&mut stream).map_err(|e| BankRc::get_io_error(&e.to_string()))?;
        self.accounts
            .accounts_from_stream(stream, local_paths, append_vecs_path)?;

        Ok(())
    }

    pub fn get_storage_entries(&self) -> Vec<Arc<AccountStorageEntry>> {
        self.accounts.accounts_db.get_storage_entries()
    }

    fn get_io_error(error: &str) -> IOError {
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
        let mut wr = Cursor::new(Vec::new());
        let accounts_db_serialize =
            AccountsDBSerialize::new(&*self.accounts.accounts_db, self.slot);
        serialize_into(&mut wr, &accounts_db_serialize).map_err(Error::custom)?;
        let len = wr.position() as usize;
        serializer.serialize_bytes(&wr.into_inner()[..len])
    }
}

#[derive(Default)]
pub struct StatusCacheRc {
    /// where all the Accounts are stored
    /// A cache of signature statuses
    pub status_cache: Arc<RwLock<BankStatusCache>>,
}

impl StatusCacheRc {
    pub fn slot_deltas(&self, slots: &[Slot]) -> Vec<SlotDelta<Result<()>>> {
        let sc = self.status_cache.read().unwrap();
        sc.slot_deltas(slots)
    }

    pub fn roots(&self) -> Vec<u64> {
        self.status_cache
            .read()
            .unwrap()
            .roots()
            .iter()
            .cloned()
            .sorted()
            .collect()
    }

    pub fn append(&self, slot_deltas: &[SlotDelta<Result<()>>]) {
        let mut sc = self.status_cache.write().unwrap();
        sc.append(slot_deltas);
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

    /// The number of slots per Storage segment
    slots_per_segment: u64,

    /// Bank slot (i.e. block)
    slot: Slot,

    /// Bank epoch
    epoch: Epoch,

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

    /// latest rent collector, knows the epoch
    rent_collector: RentCollector,

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
            for epoch in 0..=bank.get_stakers_epoch(bank.slot) {
                bank.epoch_stakes.insert(epoch, stakes.clone());
            }
            bank.update_stake_history(None);
        }
        bank.update_clock();
        bank
    }

    /// Create a new bank that points to an immutable checkpoint of another bank.
    pub fn new_from_parent(parent: &Arc<Bank>, collector_id: &Pubkey, slot: u64) -> Self {
        parent.freeze();
        assert_ne!(slot, parent.slot());

        let rc = BankRc {
            accounts: Arc::new(Accounts::new_from_parent(
                &parent.rc.accounts,
                slot,
                parent.slot(),
            )),
            parent: RwLock::new(Some(parent.clone())),
            slot,
        };
        let src = StatusCacheRc {
            status_cache: parent.src.status_cache.clone(),
        };
        let epoch_schedule = parent.epoch_schedule;
        let epoch = epoch_schedule.get_epoch(slot);

        let mut new = Bank {
            rc,
            src,
            slot,
            epoch,
            blockhash_queue: RwLock::new(parent.blockhash_queue.read().unwrap().clone()),

            // TODO: clean this up, soo much special-case copying...
            ticks_per_slot: parent.ticks_per_slot,
            slots_per_segment: parent.slots_per_segment,
            slots_per_year: parent.slots_per_year,
            epoch_schedule,
            rent_collector: parent.rent_collector.clone_with_epoch(epoch),
            max_tick_height: (slot + 1) * parent.ticks_per_slot - 1,
            bank_height: parent.bank_height + 1,
            fee_calculator: FeeCalculator::new_derived(
                &parent.fee_calculator,
                parent.signature_count(),
            ),
            capitalization: AtomicUsize::new(parent.capitalization() as usize),
            inflation: parent.inflation,
            transaction_count: AtomicUsize::new(parent.transaction_count() as usize),
            stakes: RwLock::new(parent.stakes.read().unwrap().clone_with_epoch(epoch)),
            epoch_stakes: parent.epoch_stakes.clone(),
            storage_accounts: RwLock::new(parent.storage_accounts.read().unwrap().clone()),
            parent_hash: parent.hash(),
            collector_id: *collector_id,
            collector_fees: AtomicUsize::new(0),
            ancestors: HashMap::new(),
            hash: RwLock::new(Hash::default()),
            is_delta: AtomicBool::new(false),
            tick_height: AtomicUsize::new(parent.tick_height.load(Ordering::Relaxed)),
            signature_count: AtomicUsize::new(0),
            message_processor: MessageProcessor::default(),
        };

        datapoint_info!(
            "bank-new_from_parent-heights",
            ("slot_height", slot, i64),
            ("bank_height", new.bank_height, i64)
        );

        let stakers_epoch = epoch_schedule.get_stakers_epoch(slot);
        // update epoch_stakes cache
        //  if my parent didn't populate for this staker's epoch, we've
        //  crossed a boundary
        if new.epoch_stakes.get(&stakers_epoch).is_none() {
            new.epoch_stakes
                .insert(stakers_epoch, new.stakes.read().unwrap().clone());
        }

        new.ancestors.insert(new.slot(), 0);
        new.parents().iter().enumerate().for_each(|(i, p)| {
            new.ancestors.insert(p.slot(), i + 1);
        });

        new.update_rewards(parent.epoch());
        new.update_stake_history(Some(parent.epoch()));
        new.update_clock();
        new.update_fees();
        new.update_rent();
        new
    }

    pub fn collector_id(&self) -> &Pubkey {
        &self.collector_id
    }

    pub fn create_with_genesis(
        genesis_block: &GenesisBlock,
        account_paths: String,
        status_cache_rc: &StatusCacheRc,
        id: AppendVecId,
    ) -> Self {
        let mut bank = Self::default();
        bank.set_bank_rc(
            &BankRc::new(account_paths, id, bank.slot()),
            &status_cache_rc,
        );
        bank.process_genesis_block(genesis_block);
        bank.ancestors.insert(0, 0);
        bank
    }

    pub fn slot(&self) -> u64 {
        self.slot
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
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

    pub fn status_cache_ancestors(&self) -> Vec<u64> {
        let mut roots = self.src.status_cache.read().unwrap().roots().clone();
        let min = roots.iter().min().cloned().unwrap_or(0);
        for ancestor in self.ancestors.keys() {
            if *ancestor >= min {
                roots.insert(*ancestor);
            }
        }

        let mut ancestors: Vec<_> = roots.into_iter().collect();
        ancestors.sort();
        ancestors
    }

    fn update_clock(&self) {
        self.store_account(
            &clock::id(),
            &clock::new_account(
                1,
                self.slot,
                get_segment_from_slot(self.slot, self.slots_per_segment),
                self.epoch_schedule.get_epoch(self.slot),
                self.epoch_schedule.get_stakers_epoch(self.slot),
            ),
        );
    }

    fn update_slot_hashes(&self) {
        let mut account = self
            .get_account(&slot_hashes::id())
            .unwrap_or_else(|| slot_hashes::create_account(1, &[]));

        let mut slot_hashes = SlotHashes::from_account(&account).unwrap();
        slot_hashes.add(self.slot(), self.hash());
        slot_hashes.to_account(&mut account).unwrap();

        self.store_account(&slot_hashes::id(), &account);
    }

    fn update_fees(&self) {
        self.store_account(&fees::id(), &fees::create_account(1, &self.fee_calculator));
    }

    fn update_rent(&self) {
        self.store_account(
            &rent::id(),
            &rent::create_account(1, &self.rent_collector.rent_calculator),
        );
    }

    fn update_stake_history(&self, epoch: Option<Epoch>) {
        if epoch == Some(self.epoch()) {
            return;
        }
        // if I'm the first Bank in an epoch, ensure stake_history is updated
        self.store_account(
            &stake_history::id(),
            &stake_history::create_account(1, self.stakes.read().unwrap().history()),
        );
    }

    // update reward for previous epoch
    fn update_rewards(&mut self, epoch: Epoch) {
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

        let validator_rewards =
            self.inflation.validator(year) * self.capitalization() as f64 * period;

        let validator_points = self.stakes.write().unwrap().claim_points();

        let storage_rewards = self.inflation.storage(year) * self.capitalization() as f64 * period;

        let storage_points = self.storage_accounts.write().unwrap().claim_points();

        let (validator_point_value, storage_point_value) = self.check_point_values(
            validator_rewards / validator_points as f64,
            storage_rewards / storage_points as f64,
        );
        self.store_account(
            &rewards::id(),
            &rewards::create_account(1, validator_point_value, storage_point_value),
        );

        self.capitalization.fetch_add(
            (validator_rewards + storage_rewards) as usize,
            Ordering::Relaxed,
        );
    }

    // If the point values are not `normal`, bring them back into range and
    // set them to the last value or 0.
    fn check_point_values(
        &self,
        mut validator_point_value: f64,
        mut storage_point_value: f64,
    ) -> (f64, f64) {
        let rewards = rewards::Rewards::from_account(
            &self
                .get_account(&rewards::id())
                .unwrap_or_else(|| rewards::create_account(1, 0.0, 0.0)),
        )
        .unwrap_or_else(Default::default);
        if !validator_point_value.is_normal() {
            validator_point_value = rewards.validator_point_value;
        }
        if !storage_point_value.is_normal() {
            storage_point_value = rewards.storage_point_value
        }
        (validator_point_value, storage_point_value)
    }

    fn collect_fees(&self) {
        let collector_fees = self.collector_fees.load(Ordering::Relaxed) as u64;

        if collector_fees != 0 {
            let (unburned, burned) = self.fee_calculator.burn(collector_fees);
            // burn a portion of fees
            self.deposit(&self.collector_id, unburned);
            self.capitalization
                .fetch_sub(burned as usize, Ordering::Relaxed);
        }
    }

    fn set_hash(&self) -> bool {
        let mut hash = self.hash.write().unwrap();

        if *hash == Hash::default() {
            // finish up any deferred changes to account state
            self.commit_credits();
            self.collect_fees();

            // freeze is a one-way trip, idempotent
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

        //this bank and all its parents are now on the rooted path
        let mut roots = vec![self.slot()];
        roots.append(&mut self.parents().iter().map(|p| p.slot()).collect());
        *self.rc.parent.write().unwrap() = None;

        let mut squash_accounts_time = Measure::start("squash_accounts_time");
        for slot in roots.iter().rev() {
            // root forks cannot be purged
            self.rc.accounts.add_root(*slot);
        }
        squash_accounts_time.stop();

        let mut squash_cache_time = Measure::start("squash_cache_time");
        roots
            .iter()
            .for_each(|slot| self.src.status_cache.write().unwrap().add_root(*slot));
        squash_cache_time.stop();

        datapoint_info!(
            "tower-observed",
            ("squash_accounts_ms", squash_accounts_time.as_ms(), i64),
            ("squash_cache_ms", squash_cache_time.as_ms(), i64)
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
        for (pubkey, account) in genesis_block.rewards_pools.iter() {
            self.store_account(pubkey, account);
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
        self.slots_per_segment = genesis_block.slots_per_segment;
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

        self.inflation = genesis_block.inflation;

        let rent_calculator = genesis_block.rent_calculator;
        self.rent_collector = RentCollector::new(
            self.epoch,
            &self.epoch_schedule,
            self.slots_per_year,
            &rent_calculator,
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
            let index = NUM_BLOCKHASH_CONFIRMATIONS.min(parents.len() - 1);
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

    fn update_transaction_statuses(
        &self,
        txs: &[Transaction],
        iteration_order: Option<&[usize]>,
        res: &[Result<()>],
    ) {
        let mut status_cache = self.src.status_cache.write().unwrap();
        for (i, tx) in OrderedIterator::new(txs, iteration_order).enumerate() {
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
        let current_tick_height = self.tick_height.load(Ordering::Relaxed) as u64;
        let should_register_hash = {
            if self.ticks_per_slot() != 1 || self.slot() != 0 {
                let lock = {
                    if (current_tick_height + 1) % self.ticks_per_slot == self.ticks_per_slot - 1 {
                        Some(self.blockhash_queue.write().unwrap())
                    } else {
                        None
                    }
                };
                self.tick_height.fetch_add(1, Ordering::Relaxed);
                inc_new_counter_debug!("bank-register_tick-registered", 1);
                lock
            } else if current_tick_height % self.ticks_per_slot == self.ticks_per_slot - 1 {
                // Register a new block hash if at the last tick in the slot
                Some(self.blockhash_queue.write().unwrap())
            } else {
                None
            }
        };

        if let Some(mut w_blockhash_queue) = should_register_hash {
            w_blockhash_queue.register_hash(hash, &self.fee_calculator);
        }
    }

    /// Process a Transaction. This is used for unit tests and simply calls the vector
    /// Bank::process_transactions method, and commits credit-only credits.
    pub fn process_transaction(&self, tx: &Transaction) -> Result<()> {
        let txs = vec![tx.clone()];
        self.process_transactions(&txs)[0].clone()?;
        // Call this instead of commit_credits(), so that the credit-only locks hashmap on this
        // bank isn't deleted
        self.rc
            .accounts
            .commit_credits_unsafe(&self.ancestors, self.slot());
        tx.signatures
            .get(0)
            .map_or(Ok(()), |sig| self.get_signature_status(sig).unwrap())
    }

    pub fn prepare_batch<'a, 'b>(
        &'a self,
        txs: &'b [Transaction],
        iteration_order: Option<Vec<usize>>,
    ) -> TransactionBatch<'a, 'b> {
        if self.is_frozen() {
            warn!("=========== FIXME: lock_accounts() working on a frozen bank! ================");
        }
        // TODO: put this assert back in
        // assert!(!self.is_frozen());
        let results = self
            .rc
            .accounts
            .lock_accounts(txs, iteration_order.as_ref().map(|v| v.as_slice()));
        TransactionBatch::new(results, &self, txs, iteration_order)
    }

    pub fn unlock_accounts(&self, batch: &mut TransactionBatch) {
        if batch.needs_unlock {
            batch.needs_unlock = false;
            self.rc.accounts.unlock_accounts(
                batch.transactions(),
                batch.iteration_order(),
                batch.lock_results(),
            )
        }
    }

    fn load_accounts(
        &self,
        txs: &[Transaction],
        iteration_order: Option<&[usize]>,
        results: Vec<Result<()>>,
        error_counters: &mut ErrorCounters,
    ) -> Vec<Result<TransactionLoadResult>> {
        self.rc.accounts.load_accounts(
            &self.ancestors,
            txs,
            iteration_order,
            results,
            &self.blockhash_queue.read().unwrap(),
            error_counters,
            &self.rent_collector,
        )
    }
    fn check_refs(
        &self,
        txs: &[Transaction],
        iteration_order: Option<&[usize]>,
        lock_results: &[Result<()>],
        error_counters: &mut ErrorCounters,
    ) -> Vec<Result<()>> {
        OrderedIterator::new(txs, iteration_order)
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
        iteration_order: Option<&[usize]>,
        lock_results: Vec<Result<()>>,
        max_age: usize,
        error_counters: &mut ErrorCounters,
    ) -> Vec<Result<()>> {
        let hash_queue = self.blockhash_queue.read().unwrap();
        OrderedIterator::new(txs, iteration_order)
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
        iteration_order: Option<&[usize]>,
        lock_results: Vec<Result<()>>,
        error_counters: &mut ErrorCounters,
    ) -> Vec<Result<()>> {
        let rcache = self.src.status_cache.read().unwrap();
        OrderedIterator::new(txs, iteration_order)
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

    pub fn check_hash_age(&self, hash: &Hash, max_age: usize) -> bool {
        self.blockhash_queue
            .read()
            .unwrap()
            .check_hash_age(hash, max_age)
    }

    pub fn check_transactions(
        &self,
        txs: &[Transaction],
        iteration_order: Option<&[usize]>,
        lock_results: &[Result<()>],
        max_age: usize,
        mut error_counters: &mut ErrorCounters,
    ) -> Vec<Result<()>> {
        let refs_results = self.check_refs(txs, iteration_order, lock_results, &mut error_counters);
        let age_results = self.check_age(
            txs,
            iteration_order,
            refs_results,
            max_age,
            &mut error_counters,
        );
        self.check_signatures(txs, iteration_order, age_results, &mut error_counters)
    }

    fn update_error_counters(error_counters: &ErrorCounters) {
        if 0 != error_counters.blockhash_not_found {
            inc_new_counter_error!(
                "bank-process_transactions-error-blockhash_not_found",
                error_counters.blockhash_not_found
            );
        }
        if 0 != error_counters.invalid_account_index {
            inc_new_counter_error!(
                "bank-process_transactions-error-invalid_account_index",
                error_counters.invalid_account_index
            );
        }
        if 0 != error_counters.reserve_blockhash {
            inc_new_counter_error!(
                "bank-process_transactions-error-reserve_blockhash",
                error_counters.reserve_blockhash
            );
        }
        if 0 != error_counters.duplicate_signature {
            inc_new_counter_error!(
                "bank-process_transactions-error-duplicate_signature",
                error_counters.duplicate_signature
            );
        }
        if 0 != error_counters.invalid_account_for_fee {
            inc_new_counter_error!(
                "bank-process_transactions-error-invalid_account_for_fee",
                error_counters.invalid_account_for_fee
            );
        }
        if 0 != error_counters.insufficient_funds {
            inc_new_counter_error!(
                "bank-process_transactions-error-insufficient_funds",
                error_counters.insufficient_funds
            );
        }
        if 0 != error_counters.account_loaded_twice {
            inc_new_counter_error!(
                "bank-process_transactions-account_loaded_twice",
                error_counters.account_loaded_twice
            );
        }
    }

    #[allow(clippy::type_complexity)]
    pub fn load_and_execute_transactions(
        &self,
        batch: &TransactionBatch,
        max_age: usize,
    ) -> (
        Vec<Result<TransactionLoadResult>>,
        Vec<Result<()>>,
        Vec<usize>,
        usize,
        usize,
    ) {
        let txs = batch.transactions();
        debug!("processing transactions: {}", txs.len());
        inc_new_counter_info!("bank-process_transactions", txs.len());
        let mut error_counters = ErrorCounters::default();
        let mut load_time = Measure::start("accounts_load");

        let retryable_txs: Vec<_> =
            OrderedIterator::new(batch.lock_results(), batch.iteration_order())
                .enumerate()
                .filter_map(|(index, res)| match res {
                    Err(TransactionError::AccountInUse) => Some(index),
                    Ok(_) => None,
                    Err(_) => None,
                })
                .collect();

        let sig_results = self.check_transactions(
            txs,
            batch.iteration_order(),
            batch.lock_results(),
            max_age,
            &mut error_counters,
        );
        let mut loaded_accounts = self.load_accounts(
            txs,
            batch.iteration_order(),
            sig_results,
            &mut error_counters,
        );
        load_time.stop();

        let mut execution_time = Measure::start("execution_time");
        let mut signature_count = 0;
        let executed: Vec<Result<()>> = loaded_accounts
            .iter_mut()
            .zip(OrderedIterator::new(txs, batch.iteration_order()))
            .map(|(accs, tx)| match accs {
                Err(e) => Err(e.clone()),
                Ok((ref mut accounts, ref mut loaders, ref mut credits, ref mut _rents)) => {
                    signature_count += tx.message().header.num_required_signatures as usize;
                    self.message_processor
                        .process_message(tx.message(), loaders, accounts, credits)
                }
            })
            .collect();

        execution_time.stop();

        debug!(
            "load: {}us execute: {}us txs_len={}",
            load_time.as_us(),
            execution_time.as_us(),
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
                error_counters.account_not_found
            );
            inc_new_counter_error!("bank-process_transactions-error_count", err_count);
        }

        Self::update_error_counters(&error_counters);
        (
            loaded_accounts,
            executed,
            retryable_txs,
            tx_count,
            signature_count,
        )
    }

    fn filter_program_errors_and_collect_fee(
        &self,
        txs: &[Transaction],
        iteration_order: Option<&[usize]>,
        executed: &[Result<()>],
    ) -> Vec<Result<()>> {
        let hash_queue = self.blockhash_queue.read().unwrap();
        let mut fees = 0;
        let results = OrderedIterator::new(txs, iteration_order)
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
        iteration_order: Option<&[usize]>,
        loaded_accounts: &mut [Result<TransactionLoadResult>],
        executed: &[Result<()>],
        tx_count: usize,
        signature_count: usize,
    ) -> Vec<Result<()>> {
        if self.is_frozen() {
            warn!("=========== FIXME: commit_transactions() working on a frozen bank! ================");
        }

        self.increment_transaction_count(tx_count);
        self.increment_signature_count(signature_count);

        inc_new_counter_info!("bank-process_transactions-txs", tx_count);
        inc_new_counter_info!("bank-process_transactions-sigs", signature_count);

        if executed.iter().any(|res| Self::can_commit(res)) {
            self.is_delta.store(true, Ordering::Relaxed);
        }

        // TODO: put this assert back in
        // assert!(!self.is_frozen());
        let mut write_time = Measure::start("write_time");
        self.rc.accounts.store_accounts(
            self.slot(),
            txs,
            iteration_order,
            executed,
            loaded_accounts,
        );

        self.update_cached_accounts(txs, iteration_order, executed, loaded_accounts);

        // once committed there is no way to unroll
        write_time.stop();
        debug!("store: {}us txs_len={}", write_time.as_us(), txs.len(),);
        self.update_transaction_statuses(txs, iteration_order, &executed);
        self.filter_program_errors_and_collect_fee(txs, iteration_order, executed)
    }

    /// Process a batch of transactions.
    #[must_use]
    pub fn load_execute_and_commit_transactions(
        &self,
        batch: &TransactionBatch,
        max_age: usize,
    ) -> Vec<Result<()>> {
        let (mut loaded_accounts, executed, _, tx_count, signature_count) =
            self.load_and_execute_transactions(batch, max_age);

        self.commit_transactions(
            batch.transactions(),
            batch.iteration_order(),
            &mut loaded_accounts,
            &executed,
            tx_count,
            signature_count,
        )
    }

    #[must_use]
    pub fn process_transactions(&self, txs: &[Transaction]) -> Vec<Result<()>> {
        let batch = self.prepare_batch(txs, None);
        self.load_execute_and_commit_transactions(&batch, MAX_RECENT_BLOCKHASHES)
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

    pub fn get_program_accounts(&self, program_id: &Pubkey) -> Vec<(Pubkey, Account)> {
        self.rc
            .accounts
            .load_by_program(&self.ancestors, program_id)
    }

    pub fn get_program_accounts_modified_since_parent(
        &self,
        program_id: &Pubkey,
    ) -> Vec<(Pubkey, Account)> {
        self.rc
            .accounts
            .load_by_program_fork(self.slot(), program_id)
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
            let mut signature_count_buf = [0u8; 8];
            LittleEndian::write_u64(&mut signature_count_buf[..], self.signature_count() as u64);

            hashv(&[
                &self.parent_hash.as_ref(),
                &accounts_delta_hash.as_ref(),
                &signature_count_buf,
            ])
        } else {
            self.parent_hash
        }
    }

    /// Recalculate the hash_internal_state from the account stores. Would be used to verify a
    /// snaphsot.
    pub fn verify_hash_internal_state(&self) -> bool {
        self.rc
            .accounts
            .verify_hash_internal_state(self.slot(), &self.ancestors)
    }

    /// Return the number of ticks per slot
    pub fn ticks_per_slot(&self) -> u64 {
        self.ticks_per_slot
    }

    /// Return the number of slots per segment
    pub fn slots_per_segment(&self) -> u64 {
        self.slots_per_segment
    }

    /// Return the number of ticks since genesis.
    pub fn tick_height(&self) -> u64 {
        // tick_height is using an AtomicUSize because AtomicU64 is not yet a stable API.
        // Until we can switch to AtomicU64, fail if usize is not the same as u64
        assert_eq!(std::usize::MAX, 0xFFFF_FFFF_FFFF_FFFF);
        self.tick_height.load(Ordering::Relaxed) as u64
    }

    /// Return the inflation parameters of the Bank
    pub fn inflation(&self) -> Inflation {
        self.inflation
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
    pub fn get_slots_in_epoch(&self, epoch: Epoch) -> u64 {
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
        iteration_order: Option<&[usize]>,
        res: &[Result<()>],
        loaded: &[Result<TransactionLoadResult>],
    ) {
        for (i, (raccs, tx)) in loaded
            .iter()
            .zip(OrderedIterator::new(txs, iteration_order))
            .enumerate()
        {
            if res[i].is_err() || raccs.is_err() {
                continue;
            }

            let message = &tx.message();
            let acc = raccs.as_ref().unwrap();

            for (pubkey, account) in
                message
                    .account_keys
                    .iter()
                    .zip(acc.0.iter())
                    .filter(|(_key, account)| {
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
    pub fn epoch_vote_accounts(&self, epoch: Epoch) -> Option<&HashMap<Pubkey, (u64, Account)>> {
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
        self.is_delta.load(Ordering::Relaxed) && self.tick_height() == self.max_tick_height
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

        // TODO: Uncomment once status cache serialization is done
        let sc = self.src.status_cache.read().unwrap();
        let dsc = dbank.src.status_cache.read().unwrap();
        assert_eq!(*sc, *dsc);
        assert_eq!(
            self.rc.accounts.hash_internal_state(self.slot),
            dbank.rc.accounts.hash_internal_state(dbank.slot)
        );
    }

    fn commit_credits(&self) {
        self.rc
            .accounts
            .commit_credits(&self.ancestors, self.slot());
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
    use crate::accounts_db::get_temp_accounts_paths;
    use crate::accounts_db::tests::copy_append_vecs;
    use crate::epoch_schedule::MINIMUM_SLOTS_PER_EPOCH;
    use crate::genesis_utils::{
        create_genesis_block_with_leader, GenesisBlockInfo, BOOTSTRAP_LEADER_LAMPORTS,
    };
    use crate::status_cache::MAX_CACHE_ENTRIES;
    use bincode::{deserialize_from, serialize_into, serialized_size};
    use solana_sdk::clock::DEFAULT_TICKS_PER_SLOT;
    use solana_sdk::genesis_block::create_genesis_block;
    use solana_sdk::hash;
    use solana_sdk::instruction::InstructionError;
    use solana_sdk::poh_config::PohConfig;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_instruction;
    use solana_sdk::system_transaction;
    use solana_sdk::sysvar::{fees::Fees, rewards::Rewards};
    use solana_stake_api::stake_state::Stake;
    use solana_vote_api::vote_instruction;
    use solana_vote_api::vote_state::{VoteInit, VoteState, MAX_LOCKOUT_HISTORY};
    use std::io::Cursor;
    use std::time::Duration;
    use tempfile::TempDir;

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

        let ((validator_id, validator_account), (replicator_id, replicator_account)) =
            crate::storage_utils::tests::create_storage_accounts_with_credits(100);

        // set up stakes,vote, and storage accounts
        bank.store_account(&stake.0, &stake.1);
        bank.store_account(&validator_id, &validator_account);
        bank.store_account(&replicator_id, &replicator_account);

        // generate some rewards
        let mut vote_state = VoteState::from(&vote_account).unwrap();
        for i in 0..MAX_LOCKOUT_HISTORY + 42 {
            vote_state.process_slot_vote_unchecked(i as u64);
            vote_state.to(&mut vote_account).unwrap();
            bank.store_account(&vote_id, &vote_account);
        }
        bank.store_account(&vote_id, &vote_account);

        let validator_points = bank.stakes.read().unwrap().points();
        let storage_points = bank.storage_accounts.read().unwrap().points();

        // put a child bank in epoch 1, which calls update_rewards()...
        let bank1 = Bank::new_from_parent(
            &bank,
            &Pubkey::default(),
            bank.get_slots_in_epoch(bank.epoch()) + 1,
        );
        // verify that there's inflation
        assert_ne!(bank1.capitalization(), bank.capitalization());

        // verify the inflation is represented in validator_points *
        let inflation = bank1.capitalization() - bank.capitalization();

        let rewards = bank1
            .get_account(&rewards::id())
            .map(|account| Rewards::from_account(&account).unwrap())
            .unwrap();

        assert!(
            ((rewards.validator_point_value * validator_points as f64
                + rewards.storage_point_value * storage_points as f64)
                - inflation as f64)
                .abs()
                < 1.0 // rounding, truncating
        );
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
        bank.commit_credits();

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
        solana_logger::setup();
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
            tick_hash = hashv(&[&tick_hash.as_ref(), &[42]]);
            bank.register_tick(&tick_hash);
            if tick_hash == bank.last_blockhash() {
                bank.freeze();
                return;
            }
        }
    }

    #[test]
    fn test_bank_tx_fee() {
        let arbitrary_transfer_amount = 42;
        let mint = arbitrary_transfer_amount * 100;
        let leader = Pubkey::new_rand();
        let GenesisBlockInfo {
            mut genesis_block,
            mint_keypair,
            ..
        } = create_genesis_block_with_leader(mint, &leader, 3);
        genesis_block.fee_calculator.lamports_per_signature = 4; // something divisible by 2

        let expected_fee_paid = genesis_block.fee_calculator.lamports_per_signature;
        let (expected_fee_collected, expected_fee_burned) =
            genesis_block.fee_calculator.burn(expected_fee_paid);

        let mut bank = Bank::new(&genesis_block);

        let capitalization = bank.capitalization();

        let key = Keypair::new();
        let tx = system_transaction::transfer(
            &mint_keypair,
            &key.pubkey(),
            arbitrary_transfer_amount,
            bank.last_blockhash(),
        );

        let initial_balance = bank.get_balance(&leader);
        assert_eq!(bank.process_transaction(&tx), Ok(()));
        assert_eq!(bank.get_balance(&key.pubkey()), arbitrary_transfer_amount);
        assert_eq!(
            bank.get_balance(&mint_keypair.pubkey()),
            mint - arbitrary_transfer_amount - expected_fee_paid
        );

        assert_eq!(bank.get_balance(&leader), initial_balance);
        goto_end_of_slot(&mut bank);
        assert_eq!(bank.signature_count(), 1);
        assert_eq!(
            bank.get_balance(&leader),
            initial_balance + expected_fee_collected
        ); // Leader collects fee after the bank is frozen

        // verify capitalization
        assert_eq!(capitalization - expected_fee_burned, bank.capitalization());

        // Verify that an InstructionError collects fees, too
        let mut bank = Bank::new_from_parent(&Arc::new(bank), &leader, 1);
        let mut tx =
            system_transaction::transfer(&mint_keypair, &key.pubkey(), 1, bank.last_blockhash());
        // Create a bogus instruction to system_program to cause an instruction error
        tx.message.instructions[0].data[0] = 40;

        bank.process_transaction(&tx)
            .expect_err("instruction error");
        assert_eq!(bank.get_balance(&key.pubkey()), arbitrary_transfer_amount); // no change
        assert_eq!(
            bank.get_balance(&mint_keypair.pubkey()),
            mint - arbitrary_transfer_amount - 2 * expected_fee_paid
        ); // mint_keypair still pays a fee
        goto_end_of_slot(&mut bank);
        assert_eq!(bank.signature_count(), 1);

        // Profit! 2 transaction signatures processed at 3 lamports each
        assert_eq!(
            bank.get_balance(&leader),
            initial_balance + 2 * expected_fee_collected
        );
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

        let results = bank.filter_program_errors_and_collect_fee(&vec![tx1, tx2], None, &results);
        bank.freeze();
        assert_eq!(
            bank.get_balance(&leader),
            initial_balance
                + bank
                    .fee_calculator
                    .burn(bank.fee_calculator.lamports_per_signature * 2)
                    .0
        );
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
    fn test_credit_only_accounts() {
        let (genesis_block, mint_keypair) = create_genesis_block(100);
        let bank = Bank::new(&genesis_block);
        let payer0 = Keypair::new();
        let payer1 = Keypair::new();
        let recipient = Keypair::new();
        // Fund additional payers
        bank.transfer(3, &mint_keypair, &payer0.pubkey()).unwrap();
        bank.transfer(3, &mint_keypair, &payer1.pubkey()).unwrap();
        let tx0 = system_transaction::transfer(
            &mint_keypair,
            &recipient.pubkey(),
            1,
            genesis_block.hash(),
        );
        let tx1 =
            system_transaction::transfer(&payer0, &recipient.pubkey(), 1, genesis_block.hash());
        let tx2 =
            system_transaction::transfer(&payer1, &recipient.pubkey(), 1, genesis_block.hash());
        let txs = vec![tx0, tx1, tx2];
        let results = bank.process_transactions(&txs);
        bank.rc
            .accounts
            .commit_credits_unsafe(&bank.ancestors, bank.slot());

        // If multiple transactions attempt to deposit into the same account, they should succeed,
        // since System Transfer `To` accounts are given credit-only handling
        assert_eq!(results[0], Ok(()));
        assert_eq!(results[1], Ok(()));
        assert_eq!(results[2], Ok(()));
        assert_eq!(bank.get_balance(&recipient.pubkey()), 3);

        let tx0 = system_transaction::transfer(
            &mint_keypair,
            &recipient.pubkey(),
            2,
            genesis_block.hash(),
        );
        let tx1 =
            system_transaction::transfer(&recipient, &payer0.pubkey(), 1, genesis_block.hash());
        let txs = vec![tx0, tx1];
        let results = bank.process_transactions(&txs);
        bank.rc
            .accounts
            .commit_credits_unsafe(&bank.ancestors, bank.slot());
        // However, an account may not be locked as credit-only and credit-debit at the same time.
        assert_eq!(results[0], Ok(()));
        assert_eq!(results[1], Err(TransactionError::AccountInUse));
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

        let lock_result = bank.prepare_batch(&pay_alice, None);
        let results_alice =
            bank.load_execute_and_commit_transactions(&lock_result, MAX_RECENT_BLOCKHASHES);
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

        let (genesis_block, _) = create_genesis_block(3);
        let bank = Bank::new(&genesis_block);
        let key0 = Keypair::new();
        let key1 = Keypair::new();
        let key2 = Keypair::new();
        let key3 = Pubkey::new_rand();

        let message = Message {
            header: MessageHeader {
                num_required_signatures: 1,
                num_credit_only_signed_accounts: 0,
                num_credit_only_unsigned_accounts: 1,
            },
            account_keys: vec![key0.pubkey(), key3],
            recent_blockhash: Hash::default(),
            instructions: vec![],
        };
        let tx = Transaction::new(&[&key0], message, genesis_block.hash());
        let txs = vec![tx];

        let batch0 = bank.prepare_batch(&txs, None);
        assert!(batch0.lock_results()[0].is_ok());

        // Try locking accounts, locking a previously credit-only account as credit-debit
        // should fail
        let message = Message {
            header: MessageHeader {
                num_required_signatures: 1,
                num_credit_only_signed_accounts: 0,
                num_credit_only_unsigned_accounts: 0,
            },
            account_keys: vec![key1.pubkey(), key3],
            recent_blockhash: Hash::default(),
            instructions: vec![],
        };
        let tx = Transaction::new(&[&key1], message, genesis_block.hash());
        let txs = vec![tx];

        let batch1 = bank.prepare_batch(&txs, None);
        assert!(batch1.lock_results()[0].is_err());

        // Try locking a previously credit-only account a 2nd time; should succeed
        let message = Message {
            header: MessageHeader {
                num_required_signatures: 1,
                num_credit_only_signed_accounts: 0,
                num_credit_only_unsigned_accounts: 1,
            },
            account_keys: vec![key2.pubkey(), key3],
            recent_blockhash: Hash::default(),
            instructions: vec![],
        };
        let tx = Transaction::new(&[&key2], message, genesis_block.hash());
        let txs = vec![tx];

        let batch2 = bank.prepare_batch(&txs, None);
        assert!(batch2.lock_results()[0].is_ok());
    }

    #[test]
    fn test_bank_invalid_account_index() {
        let (genesis_block, mint_keypair) = create_genesis_block(1);
        let keypair = Keypair::new();
        let bank = Bank::new(&genesis_block);

        let tx =
            system_transaction::transfer(&mint_keypair, &keypair.pubkey(), 1, genesis_block.hash());

        let mut tx_invalid_program_index = tx.clone();
        tx_invalid_program_index.message.instructions[0].program_id_index = 42;
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
        let _res = bank.process_transaction(&tx);

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

        let pubkey2 = Pubkey::new_rand();
        info!("transfer 2 {}", pubkey2);
        bank2.transfer(10, &mint_keypair, &pubkey2).unwrap();
        assert!(bank2.verify_hash_internal_state());
    }

    #[test]
    fn test_bank_hash_internal_state_verify() {
        solana_logger::setup();
        let (genesis_block, mint_keypair) = create_genesis_block(2_000);
        let bank0 = Bank::new(&genesis_block);

        let pubkey = Pubkey::new_rand();
        info!("transfer 0 {} mint: {}", pubkey, mint_keypair.pubkey());
        bank0.transfer(1_000, &mint_keypair, &pubkey).unwrap();

        let bank0_state = bank0.hash_internal_state();
        // Checkpointing should not change its state
        let bank2 = new_from_parent(&Arc::new(bank0));
        assert_eq!(bank0_state, bank2.hash_internal_state());

        let pubkey2 = Pubkey::new_rand();
        info!("transfer 2 {}", pubkey2);
        bank2.transfer(10, &mint_keypair, &pubkey2).unwrap();
        assert!(bank2.verify_hash_internal_state());
    }

    // Test that two bank forks with the same accounts should not hash to the same value.
    #[test]
    fn test_bank_hash_internal_state_same_account_different_fork() {
        solana_logger::setup();
        let (genesis_block, mint_keypair) = create_genesis_block(2_000);
        let bank0 = Arc::new(Bank::new(&genesis_block));
        let initial_state = bank0.hash_internal_state();
        let bank1 = Bank::new_from_parent(&bank0.clone(), &Pubkey::default(), 1);
        assert_eq!(bank1.hash_internal_state(), initial_state);

        info!("transfer bank1");
        let pubkey = Pubkey::new_rand();
        bank1.transfer(1_000, &mint_keypair, &pubkey).unwrap();
        assert_ne!(bank1.hash_internal_state(), initial_state);

        info!("transfer bank2");
        // bank2 should not hash the same as bank1
        let bank2 = Bank::new_from_parent(&bank0, &Pubkey::default(), 2);
        bank2.transfer(1_000, &mint_keypair, &pubkey).unwrap();
        assert_ne!(bank2.hash_internal_state(), initial_state);
        assert_ne!(bank1.hash_internal_state(), bank2.hash_internal_state());
    }

    #[test]
    fn test_hash_internal_state_genesis() {
        let bank0 = Bank::new(&create_genesis_block(10).0);
        let bank1 = Bank::new(&create_genesis_block(20).0);
        assert_ne!(bank0.hash_internal_state(), bank1.hash_internal_state());
    }

    // See that the order of two transfers does not affect the result
    // of hash_internal_state
    #[test]
    fn test_hash_internal_state_order() {
        let (genesis_block, mint_keypair) = create_genesis_block(100);
        let bank0 = Bank::new(&genesis_block);
        let bank1 = Bank::new(&genesis_block);
        assert_eq!(bank0.hash_internal_state(), bank1.hash_internal_state());
        let key0 = Pubkey::new_rand();
        let key1 = Pubkey::new_rand();
        bank0.transfer(10, &mint_keypair, &key0).unwrap();
        bank0.transfer(20, &mint_keypair, &key1).unwrap();

        bank1.transfer(20, &mint_keypair, &key1).unwrap();
        bank1.transfer(10, &mint_keypair, &key0).unwrap();

        assert_eq!(bank0.hash_internal_state(), bank1.hash_internal_state());
    }

    #[test]
    fn test_hash_internal_state_error() {
        solana_logger::setup();
        let (genesis_block, mint_keypair) = create_genesis_block(100);
        let bank = Bank::new(&genesis_block);
        let key0 = Pubkey::new_rand();
        bank.transfer(10, &mint_keypair, &key0).unwrap();
        let orig = bank.hash_internal_state();

        // Transfer will error but still take a fee
        assert!(bank.transfer(1000, &mint_keypair, &key0).is_err());
        assert_ne!(orig, bank.hash_internal_state());

        let orig = bank.hash_internal_state();
        let empty_keypair = Keypair::new();
        assert!(bank.transfer(1000, &empty_keypair, &key0).is_err());
        assert_eq!(orig, bank.hash_internal_state());
    }

    #[test]
    fn test_bank_hash_internal_state_squash() {
        let collector_id = Pubkey::default();
        let bank0 = Arc::new(Bank::new(&create_genesis_block(10).0));
        let hash0 = bank0.hash_internal_state();
        // save hash0 because new_from_parent
        // updates sysvar entries

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

        trace!("new from parent");
        let bank = new_from_parent(&parent);
        trace!("done new from parent");
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

        // This picks up the values from 1 which is the highest root:
        // TODO: if we need to access rooted banks older than this,
        // need to fix the lookup.
        assert_eq!(bank0.get_balance(&key1.pubkey()), 4);
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
        let mut leader_vote_stake: Vec<_> = parent
            .epoch_vote_accounts(0)
            .map(|accounts| {
                accounts
                    .iter()
                    .filter_map(|(pubkey, (stake, account))| {
                        if let Ok(vote_state) = VoteState::deserialize(&account.data) {
                            if vote_state.node_pubkey == leader_pubkey {
                                Some((*pubkey, *stake))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .unwrap();
        assert_eq!(leader_vote_stake.len(), 1);
        let (leader_vote_account, leader_stake) = leader_vote_stake.pop().unwrap();
        assert!(leader_stake > 0);

        let leader_stake = Stake {
            stake: leader_lamports,
            activation_epoch: std::u64::MAX, // bootstrap
            ..Stake::default()
        };

        let mut epoch = 1;
        loop {
            if epoch > STAKERS_SLOT_OFFSET / SLOTS_PER_EPOCH {
                break;
            }
            let vote_accounts = parent.epoch_vote_accounts(epoch);
            assert!(vote_accounts.is_some());

            // epoch_stakes are a snapshot at the stakers_slot_offset boundary
            //   in the prior epoch (0 in this case)
            assert_eq!(
                leader_stake.stake(0, None),
                vote_accounts.unwrap().get(&leader_vote_account).unwrap().0
            );

            epoch += 1;
        }

        // child crosses epoch boundary and is the first slot in the epoch
        let child = Bank::new_from_parent(
            &parent,
            &leader_pubkey,
            SLOTS_PER_EPOCH - (STAKERS_SLOT_OFFSET % SLOTS_PER_EPOCH),
        );

        assert!(child.epoch_vote_accounts(epoch).is_some());
        assert_eq!(
            leader_stake.stake(child.epoch(), None),
            child
                .epoch_vote_accounts(epoch)
                .unwrap()
                .get(&leader_vote_account)
                .unwrap()
                .0
        );

        // child crosses epoch boundary but isn't the first slot in the epoch, still
        //  makes an epoch stakes snapshot at 1
        let child = Bank::new_from_parent(
            &parent,
            &leader_pubkey,
            SLOTS_PER_EPOCH - (STAKERS_SLOT_OFFSET % SLOTS_PER_EPOCH) + 1,
        );
        assert!(child.epoch_vote_accounts(epoch).is_some());
        assert_eq!(
            leader_stake.stake(child.epoch(), None),
            child
                .epoch_vote_accounts(epoch)
                .unwrap()
                .get(&leader_vote_account)
                .unwrap()
                .0
        );
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

        let bank0 = Arc::new(Bank::new(&genesis_block));
        let bank1 = Arc::new(new_from_parent(&bank0));
        assert_eq!(
            bank0.fee_calculator.target_lamports_per_signature / 2,
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
            &VoteInit {
                node_pubkey: mint_keypair.pubkey(),
                authorized_voter: vote_keypair.pubkey(),
                authorized_withdrawer: vote_keypair.pubkey(),
                commission: 0,
            },
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
        let fees = Fees::from_account(&fees_account).unwrap();
        assert_eq!(
            bank.fee_calculator.lamports_per_signature,
            fees.fee_calculator.lamports_per_signature
        );
        assert_eq!(fees.fee_calculator.lamports_per_signature, 12345);
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

        // Create a new set of directories for this bank's accounts
        let (_accounts_dir, dbank_paths) = get_temp_accounts_paths(4).unwrap();
        dbank.set_bank_rc(
            &BankRc::new(dbank_paths.clone(), 0, dbank.slot()),
            &StatusCacheRc::default(),
        );
        // Create a directory to simulate AppendVecs unpackaged from a snapshot tar
        let copied_accounts = TempDir::new().unwrap();
        copy_append_vecs(&bank.rc.accounts.accounts_db, copied_accounts.path()).unwrap();
        dbank
            .rc
            .accounts_from_stream(&mut reader, dbank_paths, copied_accounts.path())
            .unwrap();
        assert_eq!(dbank.get_balance(&key.pubkey()), 10);
        bank.compare_bank(&dbank);
    }

    #[test]
    fn test_check_point_values() {
        let (genesis_block, _) = create_genesis_block(500);
        let bank = Arc::new(Bank::new(&genesis_block));

        // check that point values are 0 if no previous value was known and current values are not normal
        assert_eq!(
            bank.check_point_values(std::f64::INFINITY, std::f64::NAN),
            (0.0, 0.0)
        );

        bank.store_account(&rewards::id(), &rewards::create_account(1, 1.0, 1.0));
        // check that point values are the previous value if current values are not normal
        assert_eq!(
            bank.check_point_values(std::f64::INFINITY, std::f64::NAN),
            (1.0, 1.0)
        );
    }

    #[test]
    fn test_bank_get_program_accounts() {
        let (genesis_block, _mint_keypair) = create_genesis_block(500);
        let parent = Arc::new(Bank::new(&genesis_block));

        let bank0 = Arc::new(new_from_parent(&parent));

        let pubkey0 = Pubkey::new_rand();
        let program_id = Pubkey::new(&[2; 32]);
        let account0 = Account::new(1, 0, &program_id);
        bank0.store_account(&pubkey0, &account0);

        assert_eq!(
            bank0.get_program_accounts_modified_since_parent(&program_id),
            vec![(pubkey0, account0.clone())]
        );

        let bank1 = Arc::new(new_from_parent(&bank0));
        bank1.squash();
        assert_eq!(
            bank0.get_program_accounts(&program_id),
            vec![(pubkey0, account0.clone())]
        );
        assert_eq!(
            bank1.get_program_accounts(&program_id),
            vec![(pubkey0, account0.clone())]
        );
        assert_eq!(
            bank1.get_program_accounts_modified_since_parent(&program_id),
            vec![]
        );

        let bank2 = Arc::new(new_from_parent(&bank1));
        let pubkey1 = Pubkey::new_rand();
        let account1 = Account::new(3, 0, &program_id);
        bank2.store_account(&pubkey1, &account1);
        // Accounts with 0 lamports should be filtered out by Accounts::load_by_program()
        let pubkey2 = Pubkey::new_rand();
        let account2 = Account::new(0, 0, &program_id);
        bank2.store_account(&pubkey2, &account2);

        let bank3 = Arc::new(new_from_parent(&bank2));
        bank3.squash();
        assert_eq!(bank1.get_program_accounts(&program_id).len(), 2);
        assert_eq!(bank3.get_program_accounts(&program_id).len(), 2);
    }

    #[test]
    fn test_status_cache_ancestors() {
        let (genesis_block, _mint_keypair) = create_genesis_block(500);
        let parent = Arc::new(Bank::new(&genesis_block));
        let bank1 = Arc::new(new_from_parent(&parent));
        let mut bank = bank1;
        for _ in 0..MAX_CACHE_ENTRIES * 2 {
            bank = Arc::new(new_from_parent(&bank));
            bank.squash();
        }

        let bank = new_from_parent(&bank);
        assert_eq!(
            bank.status_cache_ancestors(),
            (bank.slot() - MAX_CACHE_ENTRIES as u64..=bank.slot()).collect::<Vec<_>>()
        );
    }
}

//! The `bank` module tracks client accounts and the progress of on-chain
//! programs. It offers a high-level API that signs transactions
//! on behalf of the caller, and a low-level API for when they have
//! already been signed and verified.
use crate::{
    bank::{BankRc, EnteredEpochCallback, StatusCacheRc},
    blockhash_queue::BlockhashQueue,
    message_processor::MessageProcessor,
    rent_collector::RentCollector,
    serde_utils::{
        deserialize_atomicbool, deserialize_atomicu64, serialize_atomicbool, serialize_atomicu64,
    },
    stakes::Stakes,
    storage_utils::StorageAccounts,
};
use serde::{Deserialize, Serialize};
use solana_sdk::{
    clock::{Epoch, Slot, UnixTimestamp},
    epoch_schedule::EpochSchedule,
    fee_calculator::{FeeCalculator, FeeRateGovernor},
    hard_forks::HardForks,
    hash::Hash,
    inflation::Inflation,
    pubkey::Pubkey,
};
use std::{
    collections::HashMap,
    sync::atomic::{AtomicBool, AtomicU64},
    sync::{Arc, RwLock},
};

/// Manager for the state of all accounts and programs after processing its entries.
#[derive(Deserialize, Serialize)]
pub struct Bank1_0 {
    /// References to accounts, parent and signature status
    #[serde(skip)]
    pub rc: BankRc,

    #[serde(skip)]
    pub src: StatusCacheRc,

    /// FIFO queue of `recent_blockhash` items
    pub blockhash_queue: RwLock<BlockhashQueue>,

    /// The set of parents including this bank
    pub ancestors: HashMap<Slot, usize>,

    /// Hash of this Bank's state. Only meaningful after freezing.
    pub hash: RwLock<Hash>,

    /// Hash of this Bank's parent's state
    pub parent_hash: Hash,

    /// parent's slot
    pub parent_slot: Slot,

    /// slots to hard fork at
    pub hard_forks: Arc<RwLock<HardForks>>,

    /// The number of transactions processed without error
    #[serde(serialize_with = "serialize_atomicu64")]
    #[serde(deserialize_with = "deserialize_atomicu64")]
    pub transaction_count: AtomicU64,

    /// Bank tick height
    #[serde(serialize_with = "serialize_atomicu64")]
    #[serde(deserialize_with = "deserialize_atomicu64")]
    pub tick_height: AtomicU64,

    /// The number of signatures from valid transactions in this slot
    #[serde(serialize_with = "serialize_atomicu64")]
    #[serde(deserialize_with = "deserialize_atomicu64")]
    pub signature_count: AtomicU64,

    /// Total capitalization, used to calculate inflation
    #[serde(serialize_with = "serialize_atomicu64")]
    #[serde(deserialize_with = "deserialize_atomicu64")]
    pub capitalization: AtomicU64,

    // Bank max_tick_height
    pub max_tick_height: u64,

    /// The number of hashes in each tick. None value means hashing is disabled.
    pub hashes_per_tick: Option<u64>,

    /// The number of ticks in each slot.
    pub ticks_per_slot: u64,

    /// length of a slot in ns
    pub ns_per_slot: u128,

    /// genesis time, used for computed clock
    pub genesis_creation_time: UnixTimestamp,

    /// The number of slots per year, used for inflation
    pub slots_per_year: f64,

    /// The number of slots per Storage segment
    pub slots_per_segment: u64,

    /// Bank slot (i.e. block)
    pub slot: Slot,

    /// Bank epoch
    pub epoch: Epoch,

    /// Bank block_height
    pub block_height: u64,

    /// The pubkey to send transactions fees to.
    pub collector_id: Pubkey,

    /// Fees that have been collected
    #[serde(serialize_with = "serialize_atomicu64")]
    #[serde(deserialize_with = "deserialize_atomicu64")]
    pub collector_fees: AtomicU64,

    /// Latest transaction fees for transactions processed by this bank
    pub fee_calculator: FeeCalculator,

    /// Track cluster signature throughput and adjust fee rate
    pub fee_rate_governor: FeeRateGovernor,

    /// Rent that have been collected
    #[serde(serialize_with = "serialize_atomicu64")]
    #[serde(deserialize_with = "deserialize_atomicu64")]
    pub collected_rent: AtomicU64,

    /// latest rent collector, knows the epoch
    pub rent_collector: RentCollector,

    /// initialized from genesis
    pub epoch_schedule: EpochSchedule,

    /// inflation specs
    pub inflation: Arc<RwLock<Inflation>>,

    /// cache of vote_account and stake_account state for this fork
    pub stakes: RwLock<Stakes>,

    /// cache of validator and archiver storage accounts for this fork
    pub storage_accounts: RwLock<StorageAccounts>,

    /// staked nodes on epoch boundaries, saved off when a bank.slot() is at
    ///   a leader schedule calculation boundary
    pub epoch_stakes: HashMap<Epoch, Stakes>,

    /// A boolean reflecting whether any entries were recorded into the PoH
    /// stream for the slot == self.slot
    #[serde(serialize_with = "serialize_atomicbool")]
    #[serde(deserialize_with = "deserialize_atomicbool")]
    pub is_delta: AtomicBool,

    /// The Message processor
    pub message_processor: MessageProcessor,

    /// Callback to be notified when a bank enters a new Epoch
    /// (used to adjust cluster features over time)
    #[serde(skip)]
    pub entered_epoch_callback: Arc<RwLock<Option<EnteredEpochCallback>>>,

    /// Last time when the cluster info vote listener has synced with this bank
    #[serde(skip)]
    pub last_vote_sync: AtomicU64,

    /// Rewards that were paid out immediately after this bank was created
    #[serde(skip)]
    pub rewards: Option<Vec<(Pubkey, i64)>>,
}

//! Provides information about the network's clock which is made up of ticks, slots, etc...

// The default tick rate that the cluster attempts to achieve.  Note that the actual tick
// rate at any given time should be expected to drift
pub const DEFAULT_TICKS_PER_SECOND: u64 = 160;

pub const MS_PER_TICK: u64 = 1000 / DEFAULT_TICKS_PER_SECOND;

// At 160 ticks/s, 64 ticks per slot implies that leader rotation and voting will happen
// every 400 ms. A fast voting cadence ensures faster finality and convergence
pub const DEFAULT_TICKS_PER_SLOT: u64 = 64;

// GCP n1-standard hardware and also a xeon e5-2520 v4 are about this rate of hashes/s
pub const DEFAULT_HASHES_PER_SECOND: u64 = 2_000_000;

// 1 Dev Epoch = 400 ms * 8192 ~= 55 minutes
pub const DEFAULT_DEV_SLOTS_PER_EPOCH: u64 = 8192;

pub const SECONDS_PER_DAY: u64 = 24 * 60 * 60;
pub const TICKS_PER_DAY: u64 = DEFAULT_TICKS_PER_SECOND * SECONDS_PER_DAY;

// 1 Epoch ~= 2 days
pub const DEFAULT_SLOTS_PER_EPOCH: u64 = 2 * TICKS_PER_DAY / DEFAULT_TICKS_PER_SLOT;

// 4 times longer than the max_lockout to allow enough time for PoRep (128 slots)
pub const DEFAULT_SLOTS_PER_TURN: u64 = 32 * 4;

// leader schedule is governed by this
pub const NUM_CONSECUTIVE_LEADER_SLOTS: u64 = 4;

pub const DEFAULT_MS_PER_SLOT: u64 = 1_000 * DEFAULT_TICKS_PER_SLOT / DEFAULT_TICKS_PER_SECOND;

/// The time window of recent block hash values that the bank will track the signatures
/// of over. Once the bank discards a block hash, it will reject any transactions that use
/// that `recent_blockhash` in a transaction. Lowering this value reduces memory consumption,
/// but requires clients to update its `recent_blockhash` more frequently. Raising the value
/// lengthens the time a client must wait to be certain a missing transaction will
/// not be processed by the network.
pub const MAX_HASH_AGE_IN_SECONDS: usize = 120;

// Number of maximum recent blockhashes (one blockhash per slot)
pub const MAX_RECENT_BLOCKHASHES: usize =
    MAX_HASH_AGE_IN_SECONDS * DEFAULT_TICKS_PER_SECOND as usize / DEFAULT_TICKS_PER_SLOT as usize;

// The maximum age of a blockhash that will be accepted by the leader
pub const MAX_PROCESSING_AGE: usize = MAX_RECENT_BLOCKHASHES / 2;

/// This is maximum time consumed in forwarding a transaction from one node to next, before
/// it can be processed in the target node
pub const MAX_TRANSACTION_FORWARDING_DELAY_GPU: usize = 2;

/// More delay is expected if CUDA is not enabled (as signature verification takes longer)
pub const MAX_TRANSACTION_FORWARDING_DELAY: usize = 6;

/// Slot is a unit of time given to a leader for encoding,
///  is some some number of Ticks long.
pub type Slot = u64;

/// Epoch is a unit of time a given leader schedule is honored,
///  some number of Slots.
pub type Epoch = u64;

/// SlotIndex is an index to the slots of a epoch
pub type SlotIndex = u64;

/// SlotCount is the number of slots in a epoch
pub type SlotCount = u64;

/// UnixTimestamp is an approximate measure of real-world time,
/// expressed as Unix time (ie. seconds since the Unix epoch)
pub type UnixTimestamp = i64;

/// Clock represents network time.  Members of Clock start from 0 upon
///  network boot.  The best way to map Clock to wallclock time is to use
///  current Slot, as Epochs vary in duration (they start short and grow
///  as the network progresses).
///
#[repr(C)]
#[derive(Serialize, Deserialize, Debug, Default, PartialEq)]
pub struct Clock {
    /// the current network/bank Slot
    pub slot: Slot,
    /// unused
    pub unused: u64,
    /// the bank Epoch
    pub epoch: Epoch,
    /// the future Epoch for which the leader schedule has
    ///  most recently been calculated
    pub leader_schedule_epoch: Epoch,
    /// computed from genesis creation time and network time
    ///  in slots, drifts!
    pub unix_timestamp: UnixTimestamp,
}

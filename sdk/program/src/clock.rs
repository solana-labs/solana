//! Information about the network's clock, ticks, slots, etc.
//!
//! Time in Solana is marked primarily by _slots_, which occur approximately every
//! 400 milliseconds, and are numbered sequentially. For every slot, a leader is
//! chosen from the validator set, and that leader is expected to produce a new
//! block, though sometimes leaders may fail to do so. Blocks can be identified
//! by their slot number, and some slots do not contain a block.
//!
//! An approximation of the passage of real-world time can be calculated by
//! multiplying a number of slots by [`DEFAULT_MS_PER_SLOT`], which is a constant target
//! time for the network to produce slots. Note though that this method suffers
//! a variable amount of drift, as the network does not produce slots at exactly
//! the target rate, and the greater number of slots being calculated for, the
//! greater the drift. Epochs cannot be used this way as they contain variable
//! numbers of slots.
//!
//! The network's current view of the real-world time can always be accessed via
//! [`Clock::unix_timestamp`], which is produced by an [oracle derived from the
//! validator set][oracle].
//!
//! [oracle]: https://docs.solana.com/implemented-proposals/validator-timestamp-oracle

use solana_sdk_macro::CloneZeroed;

/// The default tick rate that the cluster attempts to achieve (160 per second).
///
/// Note that the actual tick rate at any given time should be expected to drift.
pub const DEFAULT_TICKS_PER_SECOND: u64 = 160;

#[cfg(test)]
static_assertions::const_assert_eq!(MS_PER_TICK, 6);

/// The number of milliseconds per tick (6).
pub const MS_PER_TICK: u64 = 1000 / DEFAULT_TICKS_PER_SECOND;

#[deprecated(since = "1.15.0", note = "Please use DEFAULT_MS_PER_SLOT instead")]
/// The expected duration of a slot (400 milliseconds).
pub const SLOT_MS: u64 = DEFAULT_MS_PER_SLOT;

// At 160 ticks/s, 64 ticks per slot implies that leader rotation and voting will happen
// every 400 ms. A fast voting cadence ensures faster finality and convergence
pub const DEFAULT_TICKS_PER_SLOT: u64 = 64;

// GCP n1-standard hardware and also a xeon e5-2520 v4 are about this rate of hashes/s
pub const DEFAULT_HASHES_PER_SECOND: u64 = 2_000_000;

// Empirical sampling of mainnet validator hash rate showed the following stake
// percentages can exceed the designated hash rates as of July 2023:
// 97.6%
pub const UPDATED_HASHES_PER_SECOND_2: u64 = 2_800_000;
// 96.2%
pub const UPDATED_HASHES_PER_SECOND_3: u64 = 4_400_000;
// 96.2%
pub const UPDATED_HASHES_PER_SECOND_4: u64 = 7_600_000;
// 96.2%
pub const UPDATED_HASHES_PER_SECOND_5: u64 = 9_200_000;
// 96.2%
pub const UPDATED_HASHES_PER_SECOND_6: u64 = 10_000_000;

#[cfg(test)]
static_assertions::const_assert_eq!(DEFAULT_HASHES_PER_TICK, 12_500);
pub const DEFAULT_HASHES_PER_TICK: u64 = DEFAULT_HASHES_PER_SECOND / DEFAULT_TICKS_PER_SECOND;

#[cfg(test)]
static_assertions::const_assert_eq!(UPDATED_HASHES_PER_TICK2, 17_500);
pub const UPDATED_HASHES_PER_TICK2: u64 = UPDATED_HASHES_PER_SECOND_2 / DEFAULT_TICKS_PER_SECOND;

#[cfg(test)]
static_assertions::const_assert_eq!(UPDATED_HASHES_PER_TICK3, 27_500);
pub const UPDATED_HASHES_PER_TICK3: u64 = UPDATED_HASHES_PER_SECOND_3 / DEFAULT_TICKS_PER_SECOND;

#[cfg(test)]
static_assertions::const_assert_eq!(UPDATED_HASHES_PER_TICK4, 47_500);
pub const UPDATED_HASHES_PER_TICK4: u64 = UPDATED_HASHES_PER_SECOND_4 / DEFAULT_TICKS_PER_SECOND;

#[cfg(test)]
static_assertions::const_assert_eq!(UPDATED_HASHES_PER_TICK5, 57_500);
pub const UPDATED_HASHES_PER_TICK5: u64 = UPDATED_HASHES_PER_SECOND_5 / DEFAULT_TICKS_PER_SECOND;

#[cfg(test)]
static_assertions::const_assert_eq!(UPDATED_HASHES_PER_TICK6, 62_500);
pub const UPDATED_HASHES_PER_TICK6: u64 = UPDATED_HASHES_PER_SECOND_6 / DEFAULT_TICKS_PER_SECOND;

// 1 Dev Epoch = 400 ms * 8192 ~= 55 minutes
pub const DEFAULT_DEV_SLOTS_PER_EPOCH: u64 = 8192;

#[cfg(test)]
static_assertions::const_assert_eq!(SECONDS_PER_DAY, 86_400);
pub const SECONDS_PER_DAY: u64 = 24 * 60 * 60;

#[cfg(test)]
static_assertions::const_assert_eq!(TICKS_PER_DAY, 13_824_000);
pub const TICKS_PER_DAY: u64 = DEFAULT_TICKS_PER_SECOND * SECONDS_PER_DAY;

#[cfg(test)]
static_assertions::const_assert_eq!(DEFAULT_SLOTS_PER_EPOCH, 432_000);

/// The number of slots per epoch after initial network warmup.
///
/// 1 Epoch ~= 2 days.
pub const DEFAULT_SLOTS_PER_EPOCH: u64 = 2 * TICKS_PER_DAY / DEFAULT_TICKS_PER_SLOT;

// leader schedule is governed by this
pub const NUM_CONSECUTIVE_LEADER_SLOTS: u64 = 4;

#[cfg(test)]
static_assertions::const_assert_eq!(DEFAULT_MS_PER_SLOT, 400);
/// The expected duration of a slot (400 milliseconds).
pub const DEFAULT_MS_PER_SLOT: u64 = 1_000 * DEFAULT_TICKS_PER_SLOT / DEFAULT_TICKS_PER_SECOND;
pub const DEFAULT_S_PER_SLOT: f64 = DEFAULT_TICKS_PER_SLOT as f64 / DEFAULT_TICKS_PER_SECOND as f64;

/// The time window of recent block hash values over which the bank will track
/// signatures.
///
/// Once the bank discards a block hash, it will reject any transactions that
/// use that `recent_blockhash` in a transaction. Lowering this value reduces
/// memory consumption, but requires a client to update its `recent_blockhash`
/// more frequently. Raising the value lengthens the time a client must wait to
/// be certain a missing transaction will not be processed by the network.
pub const MAX_HASH_AGE_IN_SECONDS: usize = 120;

#[cfg(test)]
static_assertions::const_assert_eq!(MAX_RECENT_BLOCKHASHES, 300);
// Number of maximum recent blockhashes (one blockhash per non-skipped slot)
pub const MAX_RECENT_BLOCKHASHES: usize =
    MAX_HASH_AGE_IN_SECONDS * DEFAULT_TICKS_PER_SECOND as usize / DEFAULT_TICKS_PER_SLOT as usize;

#[cfg(test)]
static_assertions::const_assert_eq!(MAX_PROCESSING_AGE, 150);
// The maximum age of a blockhash that will be accepted by the leader
pub const MAX_PROCESSING_AGE: usize = MAX_RECENT_BLOCKHASHES / 2;

/// This is maximum time consumed in forwarding a transaction from one node to next, before
/// it can be processed in the target node
pub const MAX_TRANSACTION_FORWARDING_DELAY_GPU: usize = 2;

/// More delay is expected if CUDA is not enabled (as signature verification takes longer)
pub const MAX_TRANSACTION_FORWARDING_DELAY: usize = 6;

/// Transaction forwarding, which leader to forward to and how long to hold
pub const FORWARD_TRANSACTIONS_TO_LEADER_AT_SLOT_OFFSET: u64 = 2;
pub const HOLD_TRANSACTIONS_SLOT_OFFSET: u64 = 20;

/// The unit of time given to a leader for encoding a block.
///
/// It is some some number of _ticks_ long.
pub type Slot = u64;

/// Uniquely distinguishes every version of a slot.
///
/// The `BankId` is unique even if the slot number of two different slots is the
/// same. This can happen in the case of e.g. duplicate slots.
pub type BankId = u64;

/// The unit of time a given leader schedule is honored.
///
/// It lasts for some number of [`Slot`]s.
pub type Epoch = u64;

pub const GENESIS_EPOCH: Epoch = 0;
// must be sync with Account::rent_epoch::default()
pub const INITIAL_RENT_EPOCH: Epoch = 0;

/// An index to the slots of a epoch.
pub type SlotIndex = u64;

/// The number of slots in a epoch.
pub type SlotCount = u64;

/// An approximate measure of real-world time.
///
/// Expressed as Unix time (i.e. seconds since the Unix epoch).
pub type UnixTimestamp = i64;

/// A representation of network time.
///
/// All members of `Clock` start from 0 upon network boot.
#[repr(C)]
#[derive(Serialize, Deserialize, Debug, CloneZeroed, Default, PartialEq, Eq)]
pub struct Clock {
    /// The current `Slot`.
    pub slot: Slot,
    /// The timestamp of the first `Slot` in this `Epoch`.
    pub epoch_start_timestamp: UnixTimestamp,
    /// The current `Epoch`.
    pub epoch: Epoch,
    /// The future `Epoch` for which the leader schedule has
    /// most recently been calculated.
    pub leader_schedule_epoch: Epoch,
    /// The approximate real world time of the current slot.
    ///
    /// This value was originally computed from genesis creation time and
    /// network time in slots, incurring a lot of drift. Following activation of
    /// the [`timestamp_correction` and `timestamp_bounding`][tsc] features it
    /// is calculated using a [validator timestamp oracle][oracle].
    ///
    /// [tsc]: https://docs.solana.com/implemented-proposals/bank-timestamp-correction
    /// [oracle]: https://docs.solana.com/implemented-proposals/validator-timestamp-oracle
    pub unix_timestamp: UnixTimestamp,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clone() {
        let clock = Clock {
            slot: 1,
            epoch_start_timestamp: 2,
            epoch: 3,
            leader_schedule_epoch: 4,
            unix_timestamp: 5,
        };
        let cloned_clock = clock.clone();
        assert_eq!(cloned_clock, clock);
    }
}

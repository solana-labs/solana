//! Provides information about the network's clock which is made up of ticks, slots, segments, etc...

// The default tick rate that the cluster attempts to achieve.  Note that the actual tick
// rate at any given time should be expected to drift
pub const DEFAULT_TICKS_PER_SECOND: u64 = 160;

// At 160 ticks/s, 64 ticks per slot implies that leader rotation and voting will happen
// every 400 ms. A fast voting cadence ensures faster finality and convergence
pub const DEFAULT_TICKS_PER_SLOT: u64 = 64;

// GCP n1-standard hardware and also a xeon e5-2520 v4 are about this rate of hashes/s
pub const DEFAULT_HASHES_PER_SECOND: u64 = 2_000_000;

// 1 Dev Epoch = 400 ms * 8192 ~= 55 minutes
pub const DEFAULT_DEV_SLOTS_PER_EPOCH: u64 = 8192;

pub const SECONDS_PER_DAY: u64 = 24 * 60 * 60;
pub const SECONDS_PER_WEEK: u64 = 7 * SECONDS_PER_DAY;
pub const SECONDS_PER_FORTNIGHT: u64 = 2 * SECONDS_PER_WEEK;
pub const TICKS_PER_FORTNIGHT: u64 = DEFAULT_TICKS_PER_SECOND * SECONDS_PER_FORTNIGHT;

// 1 Epoch ~= 2 weeks
pub const DEFAULT_SLOTS_PER_EPOCH: u64 = TICKS_PER_FORTNIGHT / DEFAULT_TICKS_PER_SLOT;

// Storage segment configuration
pub const DEFAULT_SLOTS_PER_SEGMENT: u64 = 1024;

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

/// Converts a slot to a storage segment. Does not indicate that a segment is complete.
pub fn get_segment_from_slot(rooted_slot: Slot, slots_per_segment: u64) -> Segment {
    ((rooted_slot + (slots_per_segment - 1)) / slots_per_segment)
}

/// Given a slot returns the latest complete segment, if no segment could possibly be complete
/// for a given slot it returns `None` (i.e if `slot < slots_per_segment`)
pub fn get_complete_segment_from_slot(
    rooted_slot: Slot,
    slots_per_segment: u64,
) -> Option<Segment> {
    let completed_segment = rooted_slot / slots_per_segment;
    if rooted_slot < slots_per_segment {
        None
    } else {
        Some(completed_segment)
    }
}

/// Slot is a unit of time given to a leader for encoding,
///  is some some number of Ticks long.
pub type Slot = u64;

/// A segment is some number of slots stored by archivers
pub type Segment = u64;

/// Epoch is a unit of time a given leader schedule is honored,
///  some number of Slots.
pub type Epoch = u64;

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
    /// the current Segment, used for archiver rounds
    pub segment: Segment,
    /// the bank Epoch
    pub epoch: Epoch,
    /// the future Epoch for which the leader schedule has
    ///  most recently been calculated
    pub leader_schedule_epoch: Epoch,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get_segments(slot: Slot, slots_per_segment: u64) -> (Segment, Segment) {
        (
            get_segment_from_slot(slot, slots_per_segment),
            get_complete_segment_from_slot(slot, slots_per_segment).unwrap(),
        )
    }

    #[test]
    fn test_complete_segment_impossible() {
        // slot < slots_per_segment so there can be no complete segments
        assert_eq!(get_complete_segment_from_slot(5, 10), None);
    }

    #[test]
    fn test_segment_conversion() {
        let (current, complete) = get_segments(2048, 1024);
        assert_eq!(current, complete);
        let (current, complete) = get_segments(2049, 1024);
        assert!(complete < current);
    }
}

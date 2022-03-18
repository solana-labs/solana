use {
    solana_metrics::{datapoint::PohTimingPoint, datapoint_info},
    solana_sdk::clock::Slot,
    std::collections::HashMap,
};

/// A PohTimestamp records timing of the events during the processing of a slot
/// the validator
#[derive(Debug)]
pub struct SlotPohTimestamp {
    /// Slot start time from poh
    pub start_time: u64,
    /// Slot end time from poh
    pub end_time: u64,
    /// Last shred received time from block producer
    pub last_shred_recv_time: u64,
}

/// Default value (all zeros)
impl Default for SlotPohTimestamp {
    fn default() -> Self {
        Self {
            start_time: 0,
            end_time: 0,
            last_shred_recv_time: 0,
        }
    }
}

impl SlotPohTimestamp {
    /// Return true if the timing points of all event are received.
    pub fn is_complete(&self) -> bool {
        self.start_time != 0 && self.end_time != 0 && self.last_shred_recv_time != 0
    }

    /// Update timing point
    pub fn update(&mut self, timing_point: PohTimingPoint) {
        match timing_point {
            PohTimingPoint::PohSlotStart(ts) => self.start_time = ts,
            PohTimingPoint::PohSlotEnd(ts) => self.end_time = ts,
            PohTimingPoint::FullShredReceived(ts) => self.last_shred_recv_time = ts,
        }
    }

    /// Return the time difference between last shred received and slot start
    pub fn shred_recv_slot_start_time_diff(&self) -> u64 {
        self.last_shred_recv_time - self.start_time
    }

    /// Return the time difference between last shred received and slot end
    pub fn shred_recv_slot_end_time_diff(&self) -> u64 {
        self.last_shred_recv_time - self.end_time
    }
}

/// A PohTimingReporter manages and reports the timing of events for incoming
/// slots
pub struct PohTimingReporter {
    /// Storage map of SlotPohTimestamp per slot
    slot_timestamps: HashMap<Slot, SlotPohTimestamp>,
}

impl Default for PohTimingReporter {
    fn default() -> Self {
        Self::new()
    }
}

impl PohTimingReporter {
    /// Return a PohTimingReporter instance
    pub fn new() -> Self {
        Self {
            slot_timestamps: HashMap::new(),
        }
    }

    /// Check if PohTiming is complete for a slot
    pub fn is_complete(&self, slot: Slot) -> bool {
        if let Some(slot_timestamp) = self.slot_timestamps.get(&slot) {
            slot_timestamp.is_complete()
        } else {
            false
        }
    }

    /// Report PohTiming for a slot
    pub fn report(&self, slot: Slot, slot_timestamp: &SlotPohTimestamp) {
        datapoint_info!(
            "poh_slot_timing",
            ("slot", slot as i64, i64),
            ("start_time", slot_timestamp.start_time as i64, i64),
            ("end_time", slot_timestamp.end_time as i64, i64),
            (
                "last_shred_recv_time",
                slot_timestamp.last_shred_recv_time as i64,
                i64
            ),
            (
                "last_shred_recv_slot_start_time_diff",
                slot_timestamp.shred_recv_slot_start_time_diff() as i64,
                i64
            ),
            (
                "last_shred_recv_slot_end_time_diff",
                slot_timestamp.shred_recv_slot_end_time_diff() as i64,
                i64
            ),
        );
    }

    /// Process incoming PohTimingPoint from the channel
    pub fn process(&mut self, slot: Slot, t: PohTimingPoint) {
        let slot_timestamp = self
            .slot_timestamps
            .entry(slot)
            .or_insert_with(SlotPohTimestamp::default);

        slot_timestamp.update(t);

        if let Some(slot_timestamp) = self.slot_timestamps.get(&slot) {
            if slot_timestamp.is_complete() {
                self.report(slot, slot_timestamp);
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    /// Test poh_timing_reporter
    fn test_poh_timing_reporter() {
        // create a reporter
        let mut reporter = PohTimingReporter::default();

        // process all relevant PohTimingPoints for a slot
        reporter.process(42, PohTimingPoint::PohSlotStart(100));
        reporter.process(42, PohTimingPoint::PohSlotEnd(200));
        reporter.process(42, PohTimingPoint::FullShredReceived(150));

        // assert that the PohTiming is complete
        assert!(reporter.is_complete(42));
    }
}

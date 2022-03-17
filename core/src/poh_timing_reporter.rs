use {solana_metrics::datapoint_info, solana_sdk::clock::Slot, std::collections::HashMap};

/// SlotPohTimestamp
#[derive(Debug, Default)]
pub struct SlotPohTimestamp {
    /// slot start time from poh
    pub start_time: u64,
    /// slot end time from poh
    pub end_time: u64,
    /// last shred received time from block producer
    pub last_shred_recv_time: u64,
}

impl SlotPohTimestamp {
    pub fn is_complete(&self) -> bool {
        self.start_time != 0 && self.end_time != 0 && self.last_shred_recv_time != 0
    }

    pub fn update(&mut self, s: &str, ts: u64) {
        if s == "start_time" {
            self.start_time = ts;
        } else if s == "end_time" {
            self.end_time = ts;
        } else if s == "last_shred_recv_time" {
            self.last_shred_recv_time = ts;
        }
    }

    pub fn shred_recv_slot_start_time_diff(&self) -> u64 {
        self.last_shred_recv_time - self.start_time
    }

    pub fn shred_recv_slot_end_time_diff(&self) -> u64 {
        self.last_shred_recv_time - self.end_time
    }
}

/// Collect and report PohTiming
pub struct PohTimingReporter {
    slot_timestamps: HashMap<Slot, SlotPohTimestamp>,
}

impl PohTimingReporter {
    pub fn new() -> Self {
        Self {
            slot_timestamps: HashMap::new(),
        }
    }

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

    pub fn process(&mut self, slot: Slot, name: &str, ts: u64) {
        if !self.slot_timestamps.contains_key(&slot) {
            self.slot_timestamps.insert(slot, Default::default());
        }
        if let Some(slot_timestamp) = self.slot_timestamps.get_mut(&slot) {
            slot_timestamp.update(name, ts);
        }

        if let Some(slot_timestamp) = self.slot_timestamps.get(&slot) {
            if slot_timestamp.is_complete() {
                self.report(slot, slot_timestamp);
            }
        }
    }
}

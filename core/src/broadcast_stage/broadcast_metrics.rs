use super::*;

pub(crate) trait BroadcastShredStats {
    fn update(&mut self, new_stats: &Self);
    fn report_stats(&mut self, slot: Slot, slot_start: Instant);
}
#[derive(Clone)]
pub(crate) struct BroadcastShredBatchInfo {
    pub(crate) slot: Slot,
    pub(crate) num_expected_batches: Option<usize>,
    pub(crate) slot_start_ts: Instant,
}

#[derive(Default, Clone)]
pub(crate) struct ProcessShredsStats {
    // Per-slot elapsed time
    pub(crate) shredding_elapsed: u64,
    pub(crate) receive_elapsed: u64,
}
impl ProcessShredsStats {
    pub(crate) fn update(&mut self, new_stats: &ProcessShredsStats) {
        self.shredding_elapsed += new_stats.shredding_elapsed;
        self.receive_elapsed += new_stats.receive_elapsed;
    }
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }
}

#[derive(Default, Clone)]
pub(crate) struct TransmitShredsStats {
    pub(crate) transmit_elapsed: u64,
    pub(crate) send_mmsg_elapsed: u64,
    pub(crate) get_peers_elapsed: u64,
    pub(crate) num_shreds: usize,
    pub(crate) num_transmitted_batches: usize,
    // Filled in when the last batch of shreds is received,
    // signals how many batches of shreds to expect
    pub(crate) num_expected_batches: Option<usize>,
}
impl BroadcastShredStats for TransmitShredsStats {
    fn update(&mut self, new_stats: &TransmitShredsStats) {
        self.transmit_elapsed += new_stats.transmit_elapsed;
        self.send_mmsg_elapsed += new_stats.send_mmsg_elapsed;
        self.get_peers_elapsed += new_stats.get_peers_elapsed;
        self.num_shreds += new_stats.num_shreds;
    }
    fn report_stats(&mut self, slot: Slot, slot_start: Instant) {
        datapoint_info!(
            "broadcast-transmit-shreds-stats",
            ("slot", slot as i64, i64),
            (
                "end_to_end_elapsed",
                // `slot_start` signals when the first batch of shreds was
                // received, used to measure duration of broadcast
                slot_start.elapsed().as_micros() as i64,
                i64
            ),
            ("transmit_elapsed", self.transmit_elapsed as i64, i64),
            ("send_mmsg_elapsed", self.send_mmsg_elapsed as i64, i64),
            ("get_peers_elapsed", self.get_peers_elapsed as i64, i64),
            ("num_shreds", self.num_shreds as i64, i64),
        );
    }
}

#[derive(Default, Clone)]
pub(crate) struct InsertShredsStats {
    insert_shreds_elapsed: u64,
    num_shreds: usize,
    num_inserted_batches: usize,
    // Filled in when the last batch of shreds is received,
    // signals how many batches of shreds to expect
    num_expected_batches: Option<usize>,
}
impl BroadcastShredStats for InsertShredsStats {
    fn update(&mut self, new_stats: &InsertShredsStats) {
        self.insert_shreds_elapsed += new_stats.insert_shreds_elapsed;
        self.num_shreds += new_stats.num_shreds;
    }
    fn report_stats(&mut self, slot: Slot, slot_start: Instant) {
        datapoint_info!(
            "broadcast-insert-shreds-stats",
            ("slot", slot as i64, i64),
            (
                "end_to_end_elapsed",
                // `slot_start` signals when the first batch of shreds was
                // received, used to measure duration of broadcast
                slot_start.elapsed().as_micros() as i64,
                i64
            ),
            (
                "insert_shreds_elapsed",
                self.insert_shreds_elapsed as i64,
                i64
            ),
            ("num_shreds", self.num_shreds as i64, i64),
        );
    }
}

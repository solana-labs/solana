use super::*;

pub(crate) trait BroadcastStats {
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
}

impl BroadcastStats for TransmitShredsStats {
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
    pub(crate) insert_shreds_elapsed: u64,
    pub(crate) num_shreds: usize,
}
impl BroadcastStats for InsertShredsStats {
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

// Tracks metrics of type `T` acrosss multiple threads
#[derive(Default)]
pub(crate) struct BatchCounter<T: BroadcastStats + Default> {
    // The number of batches processed across all threads so far
    num_batches: usize,
    // Filled in when the last batch of shreds is received,
    // signals how many batches of shreds to expect
    num_expected_batches: Option<usize>,
    broadcast_shred_stats: T,
}

#[derive(Default)]
pub(crate) struct SlotBroadcastStats<T: BroadcastStats + Default>(HashMap<Slot, BatchCounter<T>>);

impl<T: BroadcastStats + Default> SlotBroadcastStats<T> {
    pub(crate) fn update(&mut self, new_stats: &T, batch_info: &Option<BroadcastShredBatchInfo>) {
        if let Some(batch_info) = batch_info {
            let mut should_delete = false;
            {
                let slot_batch_counter = self.0.entry(batch_info.slot).or_default();
                slot_batch_counter.broadcast_shred_stats.update(new_stats);
                // Only count the ones where `broadcast_shred_batch_info`.is_some(), because
                // there could potentially be other `retransmit` slots inserted into the
                // transmit pipeline (signaled by ReplayStage) that are not created by the
                // main shredding/broadcast pipeline
                slot_batch_counter.num_batches += 1;
                if let Some(num_expected_batches) = batch_info.num_expected_batches {
                    slot_batch_counter.num_expected_batches = Some(num_expected_batches);
                }
                if let Some(num_expected_batches) = slot_batch_counter.num_expected_batches {
                    if slot_batch_counter.num_batches == num_expected_batches {
                        slot_batch_counter
                            .broadcast_shred_stats
                            .report_stats(batch_info.slot, batch_info.slot_start_ts);
                    }
                    should_delete = true;
                }
            }
            if should_delete {
                self.0
                    .remove(&batch_info.slot)
                    .expect("delete should be successful");
            }
        }
    }
}

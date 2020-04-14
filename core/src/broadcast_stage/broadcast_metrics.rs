use super::*;

#[derive(Clone)]
pub struct BroadcastShredBatchInfo {
    pub slot: Slot,
    pub num_expected_batches: Option<usize>,
    pub slot_start_ts: Instant,
}

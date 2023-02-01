//! Records the maximum observed slot index.

use {
    super::{CompletedDataSetHandler, SlotEntries},
    solana_rpc::max_slots::MaxSlots,
    std::sync::{atomic::Ordering, Arc},
};

pub struct MaxSlot {
    stats: Arc<MaxSlots>,
}

impl MaxSlot {
    pub fn handler(stats: Arc<MaxSlots>) -> CompletedDataSetHandler {
        let me = Self { stats };
        Box::new(move |entries| me.handle(entries))
    }

    fn handle(&self, SlotEntries { slot, .. }: SlotEntries) {
        self.stats.shred_insert.fetch_max(slot, Ordering::Relaxed);
    }
}

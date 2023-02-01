use std::sync::atomic::AtomicU64;

#[derive(Default)]
pub struct MaxSlots {
    pub retransmit: AtomicU64,

    /// Maximum observed `solana_core::Slot` index, seen during the shred insertion operation.
    // TODO Should this be renamed to `max_shred_insert_slot`?
    pub shred_insert: AtomicU64,
}

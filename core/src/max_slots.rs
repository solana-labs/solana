use std::sync::atomic::AtomicU64;

#[derive(Default)]
pub struct MaxSlots {
    pub retransmit: AtomicU64,
    pub shred_insert: AtomicU64,
}

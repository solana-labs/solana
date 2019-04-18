//! The `timing` module provides std::time utility functions.
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

pub const NUM_TICKS_PER_SECOND: u64 = 10;

// At 10 ticks/s, 8 ticks per slot implies that leader rotation and voting will happen
// every 800 ms. A fast voting cadence ensures faster finality and convergence
pub const DEFAULT_TICKS_PER_SLOT: u64 = 8;

// 1 Epoch = 800 * 4096 ms ~= 55 minutes
pub const DEFAULT_SLOTS_PER_EPOCH: u64 = 4096;

pub const NUM_CONSECUTIVE_LEADER_SLOTS: u64 = 8;

/// The time window of recent block hash values that the bank will track the signatures
/// of over. Once the bank discards a block hash, it will reject any transactions that use
/// that `recent_blockhash` in a transaction. Lowering this value reduces memory consumption,
/// but requires clients to update its `recent_blockhash` more frequently. Raising the value
/// lengthens the time a client must wait to be certain a missing transaction will
/// not be processed by the network.
pub const MAX_HASH_AGE_IN_SECONDS: usize = 120;

// This must be <= MAX_HASH_AGE_IN_SECONDS, otherwise there's risk for DuplicateSignature errors
pub const MAX_RECENT_BLOCKHASHES: usize = MAX_HASH_AGE_IN_SECONDS;

pub fn duration_as_ns(d: &Duration) -> u64 {
    d.as_secs() * 1_000_000_000 + u64::from(d.subsec_nanos())
}

pub fn duration_as_us(d: &Duration) -> u64 {
    (d.as_secs() * 1000 * 1000) + (u64::from(d.subsec_nanos()) / 1_000)
}

pub fn duration_as_ms(d: &Duration) -> u64 {
    (d.as_secs() * 1000) + (u64::from(d.subsec_nanos()) / 1_000_000)
}

pub fn duration_as_s(d: &Duration) -> f32 {
    d.as_secs() as f32 + (d.subsec_nanos() as f32 / 1_000_000_000.0)
}

pub fn timestamp() -> u64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("create timestamp in timing");
    duration_as_ms(&now)
}

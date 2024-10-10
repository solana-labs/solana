#[deprecated(
    since = "2.1.0",
    note = "Use solana-clock and solana-epoch-schedule crates instead."
)]
pub use {
    solana_clock::{Epoch, Slot, DEFAULT_SLOTS_PER_EPOCH},
    solana_epoch_schedule::*,
};

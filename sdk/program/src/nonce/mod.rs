//! Durable transaction nonces.

pub mod state;
pub use state::State;

pub const NONCED_TX_MARKER_IX_INDEX: u8 = 0;

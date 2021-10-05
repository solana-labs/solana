//! Helper types and functions for handling and dealing with snapshot hashes.
use solana_sdk::{clock::Slot, hash::Hash};

type SlotHash = (Slot, Hash);
pub type SnapshotHash = SlotHash;

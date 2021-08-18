mod cache;
mod pending_changes;

pub use cache::*;
pub use pending_changes::*;

use solana_sdk::clock::Epoch;

/// The number of epochs required to warm up a newly activated address map. If
/// an address map is activated in some slot in epoch X, it can be used to map
/// addresses from the start of epoch X + 2.
pub const ACTIVATION_WARMUP: Epoch = 2;

/// The number of epochs required to deactivate an address map. If an address
/// map is deactivated in some slot in epoch X, it can no longer be used to map
/// addresses from the start of epoch X + 2.
pub const DEACTIVATION_COOLDOWN: Epoch = 2;

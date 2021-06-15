//! config for staking
//!  carries variables that the stake program cares about
use serde_derive::{Deserialize, Serialize};

// stake config ID
crate::declare_id!("StakeConfig11111111111111111111111111111111");

// means that no more than RATE of current effective stake may be added or subtracted per
//  epoch
pub const DEFAULT_WARMUP_COOLDOWN_RATE: f64 = 0.25;
pub const DEFAULT_SLASH_PENALTY: u8 = ((5 * std::u8::MAX as usize) / 100) as u8;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
pub struct Config {
    /// how much stake we can activate/deactivate per-epoch as a fraction of currently effective stake
    pub warmup_cooldown_rate: f64,
    /// percentage of stake lost when slash, expressed as a portion of std::u8::MAX
    pub slash_penalty: u8,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            warmup_cooldown_rate: DEFAULT_WARMUP_COOLDOWN_RATE,
            slash_penalty: DEFAULT_SLASH_PENALTY,
        }
    }
}

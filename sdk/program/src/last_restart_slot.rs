//! Information about the last restart slot (hard fork).

use solana_sdk_macro::CloneZeroed;

pub type Slot = u64;

#[repr(C)]
#[derive(Serialize, Deserialize, Debug, CloneZeroed, PartialEq, Eq)]
pub struct LastRestartSlot {
    /// The last restart `Slot`.
    pub last_restart_slot: Slot,
}

impl Default for LastRestartSlot {
    fn default() -> Self {
        Self {
            last_restart_slot: 0,
        }
    }
}

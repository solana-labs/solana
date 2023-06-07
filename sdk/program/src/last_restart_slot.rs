//! Information about the last restart slot (hard fork).

use {crate::clock::Slot, solana_sdk_macro::CloneZeroed};

#[repr(C)]
#[derive(Serialize, Deserialize, Debug, CloneZeroed, PartialEq, Eq, Default)]
pub struct LastRestartSlot {
    /// The last restart `Slot`.
    pub last_restart_slot: Slot,
}

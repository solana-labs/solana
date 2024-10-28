//! Information about the last restart slot (hard fork).
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

use solana_sdk_macro::CloneZeroed;

#[repr(C)]
#[cfg_attr(
    feature = "serde",
    derive(serde_derive::Deserialize, serde_derive::Serialize)
)]
#[derive(Debug, CloneZeroed, PartialEq, Eq, Default)]
pub struct LastRestartSlot {
    /// The last restart `Slot`.
    pub last_restart_slot: u64,
}

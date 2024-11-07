pub use solana_sdk_ids::sysvar::last_restart_slot::{check_id, id, ID};
use {crate::LastRestartSlot, solana_sysvar_id::impl_sysvar_id};

impl_sysvar_id!(LastRestartSlot);

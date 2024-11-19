#[deprecated(since = "2.1.0", note = "Use `solana-sysvar-id` crate instead")]
pub use solana_sysvar_id::{declare_deprecated_sysvar_id, declare_sysvar_id, SysvarId};
#[deprecated(since = "2.2.0", note = "Use `solana-sysvar` crate instead")]
#[allow(deprecated)]
pub use {
    solana_sdk_ids::sysvar::{check_id, id, ID},
    solana_sysvar::{
        clock, epoch_rewards, epoch_schedule, fees, instructions, is_sysvar_id, last_restart_slot,
        recent_blockhashes, rent, rewards, slot_hashes, slot_history, stake_history, Sysvar,
        ALL_IDS,
    },
};

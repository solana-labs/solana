pub use solana_sdk_ids::sysvar::epoch_rewards::{check_id, id, ID};
use {crate::EpochRewards, solana_sysvar_id::impl_sysvar_id};

impl_sysvar_id!(EpochRewards);

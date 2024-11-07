pub use solana_sdk_ids::sysvar::epoch_schedule::{check_id, id, ID};
use {crate::EpochSchedule, solana_sysvar_id::impl_sysvar_id};

impl_sysvar_id!(EpochSchedule);

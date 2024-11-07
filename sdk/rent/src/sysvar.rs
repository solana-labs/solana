pub use solana_sdk_ids::sysvar::rent::{check_id, id, ID};
use {crate::Rent, solana_sysvar_id::impl_sysvar_id};

impl_sysvar_id!(Rent);

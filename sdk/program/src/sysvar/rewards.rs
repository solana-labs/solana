//! This sysvar is deprecated and unused.
pub use solana_sdk_ids::sysvar::rewards::{check_id, id, ID};
use {crate::sysvar::Sysvar, solana_sysvar_id::impl_sysvar_id};

impl_sysvar_id!(Rewards);

#[repr(C)]
#[derive(Serialize, Deserialize, Debug, Default, PartialEq)]
pub struct Rewards {
    pub validator_point_value: f64,
    pub unused: f64,
}
impl Rewards {
    pub fn new(validator_point_value: f64) -> Self {
        Self {
            validator_point_value,
            unused: 0.0,
        }
    }
}

impl Sysvar for Rewards {}

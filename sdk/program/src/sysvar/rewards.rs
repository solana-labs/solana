//! This sysvar is deprecated and unused.

use crate::sysvar::Sysvar;

crate::declare_sysvar_id!("SysvarRewards111111111111111111111111111111", Rewards);

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

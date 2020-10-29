//! This account contains the current cluster fees
//!
use crate::{fee_calculator::FeeCalculator, sysvar::Sysvar};

crate::declare_sysvar_id!("SysvarFees111111111111111111111111111111111", Fees);

#[repr(C)]
#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Fees {
    pub fee_calculator: FeeCalculator,
}
impl Fees {
    pub fn new(fee_calculator: &FeeCalculator) -> Self {
        Self {
            fee_calculator: fee_calculator.clone(),
        }
    }
}

impl Sysvar for Fees {}

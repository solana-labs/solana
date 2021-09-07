//! This account contains the current cluster fees
//!
#![allow(deprecated)]

use crate::{
    fee_calculator::FeeCalculator, impl_sysvar_get, program_error::ProgramError, sysvar::Sysvar,
};

crate::declare_deprecated_sysvar_id!("SysvarFees111111111111111111111111111111111", Fees);

#[deprecated(
    since = "1.8.0",
    note = "Please do not use, will no longer be available in the future"
)]
#[repr(C)]
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Fees {
    pub fee_calculator: FeeCalculator,
}
impl Fees {
    pub fn new(fee_calculator: &FeeCalculator) -> Self {
        #[allow(deprecated)]
        Self {
            fee_calculator: fee_calculator.clone(),
        }
    }
}

impl Sysvar for Fees {
    impl_sysvar_get!(sol_get_fees_sysvar);
}

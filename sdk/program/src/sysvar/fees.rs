//! This account contains the current cluster fees
//!
use crate::{
    fee_calculator::FeeCalculator, impl_sysvar_get, program_error::ProgramError, sysvar::Sysvar,
};

crate::declare_sysvar_id!("SysvarFees111111111111111111111111111111111", Fees);

#[repr(C)]
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
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

impl Sysvar for Fees {
    impl_sysvar_get!(sol_get_fees_sysvar);
}

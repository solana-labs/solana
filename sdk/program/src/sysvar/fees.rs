//! This account contains the current cluster fees
//!
#![allow(deprecated)]

use {
    crate::{
        clone_zeroed, copy_field, fee_calculator::FeeCalculator, impl_sysvar_get,
        program_error::ProgramError, sysvar::Sysvar,
    },
    std::mem::MaybeUninit,
};

crate::declare_deprecated_sysvar_id!("SysvarFees111111111111111111111111111111111", Fees);

#[deprecated(
    since = "1.9.0",
    note = "Please do not use, will no longer be available in the future"
)]
#[repr(C)]
#[derive(Serialize, Deserialize, Debug, Default, PartialEq, Eq)]
pub struct Fees {
    pub fee_calculator: FeeCalculator,
}

impl Clone for Fees {
    fn clone(&self) -> Self {
        clone_zeroed(|cloned: &mut MaybeUninit<Self>| {
            let ptr = cloned.as_mut_ptr();
            unsafe {
                copy_field!(ptr, self, fee_calculator);
            }
        })
    }
}

impl Fees {
    pub fn new(fee_calculator: &FeeCalculator) -> Self {
        #[allow(deprecated)]
        Self {
            fee_calculator: *fee_calculator,
        }
    }
}

impl Sysvar for Fees {
    impl_sysvar_get!(sol_get_fees_sysvar);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clone() {
        let fees = Fees {
            fee_calculator: FeeCalculator {
                lamports_per_signature: 1,
            },
        };
        let cloned_fees = fees.clone();
        assert_eq!(cloned_fees, fees);
    }
}

//! Current cluster fees.
//!
//! The _fees sysvar_ provides access to the [`Fees`] type, which contains the
//! current [`FeeCalculator`].
//!
//! [`Fees`] implements [`Sysvar::get`] and can be loaded efficiently without
//! passing the sysvar account ID to the program.
//!
//! This sysvar is deprecated and will not be available in the future.
//! Transaction fees should be determined with the [`getFeeForMessage`] RPC
//! method. For additional context see the [Comprehensive Compute Fees
//! proposal][ccf].
//!
//! [`getFeeForMessage`]: https://solana.com/docs/rpc/http/getfeeformessage
//! [ccf]: https://docs.solanalabs.com/proposals/comprehensive-compute-fees
//!
//! See also the Solana [documentation on the fees sysvar][sdoc].
//!
//! [sdoc]: https://docs.solanalabs.com/runtime/sysvars#fees

#![allow(deprecated)]

#[cfg(feature = "bincode")]
use crate::{impl_sysvar_get, Sysvar};
#[cfg(feature = "serde")]
use serde_derive::{Deserialize, Serialize};
pub use solana_sdk_ids::sysvar::fees::{check_id, id, ID};
use {
    solana_fee_calculator::FeeCalculator, solana_sdk_macro::CloneZeroed,
    solana_sysvar_id::impl_deprecated_sysvar_id,
};

impl_deprecated_sysvar_id!(Fees);

/// Transaction fees.
#[deprecated(
    since = "1.9.0",
    note = "Please do not use, will no longer be available in the future"
)]
#[repr(C)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[derive(Debug, CloneZeroed, Default, PartialEq, Eq)]
pub struct Fees {
    pub fee_calculator: FeeCalculator,
}

impl Fees {
    pub fn new(fee_calculator: &FeeCalculator) -> Self {
        #[allow(deprecated)]
        Self {
            fee_calculator: *fee_calculator,
        }
    }
}

#[cfg(feature = "bincode")]
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

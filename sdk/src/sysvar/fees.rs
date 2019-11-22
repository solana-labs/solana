//! This account contains the current cluster fees
//!
use crate::{account::Account, fee_calculator::FeeCalculator, sysvar::Sysvar};

crate::declare_sysvar_id!("SysvarFees111111111111111111111111111111111", Fees);

#[repr(C)]
#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Fees {
    pub fee_calculator: FeeCalculator,
}

impl Sysvar for Fees {}

pub fn create_account(lamports: u64, fee_calculator: &FeeCalculator) -> Account {
    Fees {
        fee_calculator: fee_calculator.clone(),
    }
    .create_account(lamports)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fees_create_account() {
        let lamports = 42;
        let account = create_account(lamports, &FeeCalculator::default());
        let fees = Fees::from_account(&account).unwrap();
        assert_eq!(fees.fee_calculator, FeeCalculator::default());
    }
}

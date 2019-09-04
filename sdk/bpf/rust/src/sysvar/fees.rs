//! This account contains the current cluster fees
//!
use crate::account::Account;
use crate::fee_calculator::FeeCalculator;
use crate::sysvar;
use bincode::serialized_size;

///  fees account pubkey
const ID: [u8; 32] = [
    6, 167, 213, 23, 24, 226, 90, 141, 131, 80, 60, 37, 26, 122, 240, 113, 38, 253, 114, 0, 223,
    111, 196, 237, 82, 106, 156, 144, 0, 0, 0, 0,
];

crate::solana_name_id!(ID, "SysvarFees111111111111111111111111111111111");

#[repr(C)]
#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Fees {
    pub fee_calculator: FeeCalculator,
}

impl Fees {
    pub fn from(account: &Account) -> Option<Self> {
        account.deserialize_data().ok()
    }
    pub fn to(&self, account: &mut Account) -> Option<()> {
        account.serialize_data(self).ok()
    }

    pub fn size_of() -> usize {
        serialized_size(&Fees::default()).unwrap() as usize
    }
}

pub fn create_account(lamports: u64, fee_calculator: &FeeCalculator) -> Account {
    Account::new_data(
        lamports,
        &Fees {
            fee_calculator: fee_calculator.clone(),
        },
        &sysvar::id(),
    )
    .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fees_create_account() {
        let lamports = 42;
        let account = create_account(lamports, &FeeCalculator::default());
        let fees = Fees::from(&account).unwrap();
        assert_eq!(fees.fee_calculator, FeeCalculator::default());
    }
}

//! This account contains the current cluster fees
//!
use crate::account::Account;
use crate::fee_calculator::FeeCalculator;
use crate::syscall;
use bincode::serialized_size;

///  fees account pubkey
const ID: [u8; 32] = [
    6, 167, 211, 138, 69, 218, 104, 33, 3, 92, 89, 173, 16, 89, 109, 253, 49, 97, 98, 165, 87, 222,
    119, 112, 253, 90, 76, 184, 0, 0, 0, 0,
];

crate::solana_name_id!(ID, "Sysca11Fees11111111111111111111111111111111");

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
        &syscall::id(),
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

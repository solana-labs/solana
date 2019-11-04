//! This account contains the current cluster rent
//!
pub use crate::rent::Rent;

use crate::{
    account::{Account, KeyedAccount},
    instruction::InstructionError,
    sysvar::Sysvar,
};

///  rent account pubkey
const ID: [u8; 32] = [
    6, 167, 213, 23, 25, 44, 92, 81, 33, 140, 201, 76, 61, 74, 241, 127, 88, 218, 238, 8, 155, 161,
    253, 68, 227, 219, 217, 138, 0, 0, 0, 0,
];

crate::solana_sysvar_id!(ID, "SysvarRent111111111111111111111111111111111", Rent);

impl Sysvar for Rent {}

pub fn create_account(lamports: u64, rent: &Rent) -> Account {
    rent.create_account(lamports)
}

pub fn verify_rent_exemption(
    account: &KeyedAccount,
    rent_sysvar_account: &KeyedAccount,
) -> Result<(), InstructionError> {
    let rent = Rent::from_keyed_account(rent_sysvar_account)?;
    if !rent.is_exempt(account.account.lamports, account.account.data.len()) {
        Err(InstructionError::InsufficientFunds)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rent_create_account() {
        let lamports = 42;
        let account = create_account(lamports, &Rent::default());
        let rent = Rent::from_account(&account).unwrap();
        assert_eq!(rent, Rent::default());
    }
}

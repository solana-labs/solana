//! This account contains the current cluster rent
//!
pub use crate::rent::Rent;

use crate::{
    account::{Account, KeyedAccount},
    instruction::InstructionError,
    sysvar::Sysvar,
};

crate::declare_sysvar_id!("SysvarRent111111111111111111111111111111111", Rent);

impl Sysvar for Rent {}

pub fn create_account(lamports: u64, rent: &Rent) -> Account {
    rent.create_account(lamports)
}

pub fn verify_rent_exemption(
    keyed_account: &KeyedAccount,
    rent_sysvar_account: &KeyedAccount,
) -> Result<(), InstructionError> {
    let rent = Rent::from_keyed_account(rent_sysvar_account)?;
    if !rent.is_exempt(
        keyed_account.account.lamports,
        keyed_account.account.data.len(),
    ) {
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

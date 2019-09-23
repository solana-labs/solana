use crate::account::KeyedAccount;
use crate::instruction::InstructionError;
use crate::sysvar::rent;

pub fn verify_rent_exemption(
    account: &KeyedAccount,
    rent_sysvar_account: &KeyedAccount,
) -> Result<(), InstructionError> {
    if !rent::from_keyed_account(rent_sysvar_account)?
        .rent_calculator
        .is_exempt(account.account.lamports, account.account.data.len())
    {
        Err(InstructionError::InsufficientFunds)
    } else {
        Ok(())
    }
}

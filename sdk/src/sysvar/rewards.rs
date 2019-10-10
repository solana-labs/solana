//! This account contains the current cluster rewards point values
//!
use crate::account::Account;
use crate::account_info::AccountInfo;
use crate::sysvar;
use bincode::serialized_size;

///  account pubkey
const ID: [u8; 32] = [
    6, 167, 213, 23, 25, 44, 97, 55, 206, 224, 146, 217, 182, 146, 62, 225, 204, 214, 25, 3, 250,
    130, 184, 161, 97, 145, 87, 141, 128, 0, 0, 0,
];

crate::solana_sysvar_id!(ID, "SysvarRewards111111111111111111111111111111");

#[repr(C)]
#[derive(Serialize, Deserialize, Debug, Default, PartialEq)]
pub struct Rewards {
    pub validator_point_value: f64,
    pub storage_point_value: f64,
}

impl Rewards {
    pub fn from_account(account: &Account) -> Option<Self> {
        account.deserialize_data().ok()
    }
    pub fn to_account(&self, account: &mut Account) -> Option<()> {
        account.serialize_data(self).ok()
    }
    pub fn from_account_info(account: &AccountInfo) -> Option<Self> {
        account.deserialize_data().ok()
    }
    pub fn to_account_info(&self, account: &mut AccountInfo) -> Option<()> {
        account.serialize_data(self).ok()
    }
    pub fn size_of() -> usize {
        serialized_size(&Self::default()).unwrap() as usize
    }
}

pub fn create_account(
    lamports: u64,
    validator_point_value: f64,
    storage_point_value: f64,
) -> Account {
    Account::new_data(
        lamports,
        &Rewards {
            validator_point_value,
            storage_point_value,
        },
        &sysvar::id(),
    )
    .unwrap()
}

use crate::account::KeyedAccount;
use crate::instruction::InstructionError;
pub fn from_keyed_account(account: &KeyedAccount) -> Result<Rewards, InstructionError> {
    if !check_id(account.unsigned_key()) {
        return Err(InstructionError::InvalidArgument);
    }
    Rewards::from_account(account.account).ok_or(InstructionError::InvalidAccountData)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_account() {
        let account = create_account(1, 0.0, 0.0);
        let rewards = Rewards::from_account(&account).unwrap();
        assert_eq!(rewards, Rewards::default());
    }
}

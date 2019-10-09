//! This account contains the current cluster rent
//!
use crate::{
    account::{Account, KeyedAccount},
    account_info::AccountInfo,
    epoch_schedule::EpochSchedule,
    instruction::InstructionError,
    sysvar,
};

///  epoch_schedule account pubkey
const ID: [u8; 32] = [
    6, 167, 213, 23, 24, 220, 63, 238, 2, 211, 228, 127, 1, 0, 248, 176, 84, 247, 148, 46, 96, 89,
    30, 63, 80, 135, 25, 168, 5, 0, 0, 0,
];

crate::solana_sysvar_id!(ID, "SysvarEpochSchedu1e111111111111111111111111");

impl EpochSchedule {
    pub fn deserialize(account: &Account) -> Result<Self, bincode::Error> {
        account.deserialize_data()
    }
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
    pub fn from_keyed_account(account: &KeyedAccount) -> Result<EpochSchedule, InstructionError> {
        if !check_id(account.unsigned_key()) {
            return Err(InstructionError::InvalidArgument);
        }
        EpochSchedule::from_account(account.account).ok_or(InstructionError::InvalidArgument)
    }
}

pub fn create_account(lamports: u64, epoch_schedule: &EpochSchedule) -> Account {
    Account::new_data(lamports, epoch_schedule, &sysvar::id()).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_account() {
        let account = create_account(42, &EpochSchedule::default());
        let epoch_schedule = EpochSchedule::from_account(&account).unwrap();
        assert_eq!(epoch_schedule, EpochSchedule::default());
    }
}

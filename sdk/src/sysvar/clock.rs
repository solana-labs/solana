//! This account contains the clock slot, epoch, and stakers_epoch
//!

use crate::account::Account;
use crate::account_info::AccountInfo;
use crate::clock::{Epoch, Segment, Slot};
use crate::sysvar;
use bincode::serialized_size;

const ID: [u8; 32] = [
    6, 167, 213, 23, 24, 199, 116, 201, 40, 86, 99, 152, 105, 29, 94, 182, 139, 94, 184, 163, 155,
    75, 109, 92, 115, 85, 91, 33, 0, 0, 0, 0,
];

crate::solana_name_id!(ID, "SysvarC1ock11111111111111111111111111111111");

#[repr(C)]
#[derive(Serialize, Deserialize, Debug, Default, PartialEq)]
pub struct Clock {
    pub slot: Slot,
    pub segment: Segment,
    pub epoch: Epoch,
    pub stakers_epoch: Epoch,
}

impl Clock {
    pub fn size_of() -> usize {
        serialized_size(&Self::default()).unwrap() as usize
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
}

pub fn new_account(
    lamports: u64,
    slot: Slot,
    segment: Segment,
    epoch: Epoch,
    stakers_epoch: Epoch,
) -> Account {
    Account::new_data(
        lamports,
        &Clock {
            slot,
            segment,
            epoch,
            stakers_epoch,
        },
        &sysvar::id(),
    )
    .unwrap()
}

use crate::account::KeyedAccount;
use crate::instruction::InstructionError;

pub fn from_keyed_account(account: &KeyedAccount) -> Result<Clock, InstructionError> {
    if !check_id(account.unsigned_key()) {
        return Err(InstructionError::InvalidArgument);
    }
    Clock::from_account(account.account).ok_or(InstructionError::InvalidArgument)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let account = new_account(1, 0, 0, 0, 0);
        let clock = Clock::from_account(&account).unwrap();
        assert_eq!(clock, Clock::default());
    }
}

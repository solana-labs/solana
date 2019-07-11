//! This account contains the current slot, epoch, and stakers_epoch
//!
use crate::account::Account;
use crate::syscall;
use bincode::serialized_size;

pub use crate::timing::{Epoch, Slot};

crate::solana_name_id!(ID, "Sysca11Current11111111111111111111111111111");

const ID: [u8; 32] = [
    6, 167, 211, 138, 69, 218, 14, 184, 34, 50, 188, 33, 201, 49, 63, 13, 15, 193, 33, 132, 208,
    238, 129, 224, 101, 67, 14, 11, 160, 0, 0, 0,
];

#[repr(C)]
#[derive(Serialize, Deserialize, Debug, Default, PartialEq)]
pub struct Current {
    pub slot: Slot,
    pub segment: Segment,
    pub epoch: Epoch,
    pub stakers_epoch: Epoch,
}

impl Current {
    pub fn from(account: &Account) -> Option<Self> {
        account.deserialize_data().ok()
    }
    pub fn to(&self, account: &mut Account) -> Option<()> {
        account.serialize_data(self).ok()
    }

    pub fn size_of() -> usize {
        serialized_size(&Self::default()).unwrap() as usize
    }
}

pub fn create_account(
    lamports: u64,
    slot: Slot,
    segment: Segment,
    epoch: Epoch,
    stakers_epoch: Epoch,
) -> Account {
    Account::new_data(
        lamports,
        &Current {
            slot,
            segment,
            epoch,
            stakers_epoch,
        },
        &syscall::id(),
    )
    .unwrap()
}

use crate::account::KeyedAccount;
use crate::instruction::InstructionError;
use crate::timing::Segment;

pub fn from_keyed_account(account: &KeyedAccount) -> Result<Current, InstructionError> {
    if !check_id(account.unsigned_key()) {
        return Err(InstructionError::InvalidArgument);
    }
    Current::from(account.account).ok_or(InstructionError::InvalidArgument)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_account() {
        let account = create_account(1, 0, 0, 0, 0);
        let current = Current::from(&account).unwrap();
        assert_eq!(current, Current::default());
    }
}

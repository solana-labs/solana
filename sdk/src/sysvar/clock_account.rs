//! Serialize clock support for Account
//!
use crate::account::Account;
use crate::sysvar;
use crate::sysvar::clock::{check_id, Clock};
use bincode::serialized_size;

pub use crate::clock::{Epoch, Slot};

impl Clock {
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

pub fn new(
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
use crate::clock::Segment;
use crate::instruction::InstructionError;

pub fn from_keyed_account(account: &KeyedAccount) -> Result<Clock, InstructionError> {
    if !check_id(account.unsigned_key()) {
        return Err(InstructionError::InvalidArgument);
    }
    Clock::from(account.account).ok_or(InstructionError::InvalidArgument)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_account() {
        let account = new(1, 0, 0, 0, 0);
        let clock = Clock::from(&account).unwrap();
        assert_eq!(clock, Clock::default());
    }
}

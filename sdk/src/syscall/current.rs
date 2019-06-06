//! This account contains the current slot, epoch, and stakers_epoch
//!
use crate::account::Account;
use crate::syscall;
use bincode::serialized_size;

crate::solana_name_id!(ID, "Sysca11Current11111111111111111111111111111");

const ID: [u8; 32] = [
    6, 167, 211, 138, 69, 218, 14, 184, 34, 50, 188, 33, 201, 49, 63, 13, 15, 193, 33, 132, 208,
    238, 129, 224, 101, 67, 14, 11, 160, 0, 0, 0,
];

#[repr(C)]
#[derive(Serialize, Deserialize, Debug, Default, PartialEq)]
pub struct Current {
    slot: u64,
    block: u64,
    epoch: u64,
    stakers_epoch: u64,
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

pub fn create_account(lamports: u64) -> Account {
    Account::new(lamports, Current::size_of(), &syscall::id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_account() {
        let account = create_account(1);
        let current = Current::from(&account).unwrap();
        assert_eq!(current, Current::default());
    }
}

//! This account contains the current cluster rewards point values
//!
use crate::account::Account;
use crate::syscall;
use bincode::serialized_size;

///  account pubkey
const ID: [u8; 32] = [
    6, 167, 211, 138, 69, 219, 174, 221, 84, 28, 161, 202, 169, 28, 9, 210, 255, 70, 57, 99, 48,
    156, 150, 32, 59, 104, 53, 117, 192, 0, 0, 0,
];

crate::solana_name_id!(ID, "Sysca11Rewards11111111111111111111111111111");

#[repr(C)]
#[derive(Serialize, Deserialize, Debug, Default, PartialEq)]
pub struct Rewards {
    pub validator_point_value: f64,
    pub replicator_point_value: f64,
}

impl Rewards {
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
    validator_point_value: f64,
    replicator_point_value: f64,
) -> Account {
    Account::new_data(
        lamports,
        &Rewards {
            validator_point_value,
            replicator_point_value,
        },
        &syscall::id(),
    )
    .unwrap()
}

use crate::account::KeyedAccount;
use crate::instruction::InstructionError;
pub fn from_keyed_account(account: &KeyedAccount) -> Result<Rewards, InstructionError> {
    if !check_id(account.unsigned_key()) {
        dbg!(account.unsigned_key());
        return Err(InstructionError::InvalidArgument);
    }
    Rewards::from(account.account).ok_or(InstructionError::InvalidAccountData)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_account() {
        let account = create_account(1, 0.0, 0.0);
        let rewards = Rewards::from(&account).unwrap();
        assert_eq!(rewards, Rewards::default());
    }
}

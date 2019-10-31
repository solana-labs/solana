//! named accounts for synthesized data accounts for bank state, etc.
//!
use crate::{
    account::{Account, KeyedAccount},
    account_info::AccountInfo,
    instruction::InstructionError,
    pubkey::Pubkey,
};

pub mod clock;
pub mod epoch_schedule;
pub mod fees;
pub mod recent_blockhashes;
pub mod rent;
pub mod rewards;
pub mod slot_hashes;
pub mod stake_history;

pub fn is_sysvar_id(id: &Pubkey) -> bool {
    clock::check_id(id)
        || epoch_schedule::check_id(id)
        || fees::check_id(id)
        || recent_blockhashes::check_id(id)
        || rent::check_id(id)
        || rewards::check_id(id)
        || slot_hashes::check_id(id)
        || stake_history::check_id(id)
}

#[macro_export]
macro_rules! solana_sysvar_id(
    ($id:ident, $name:expr, $type:ty) => (
        $crate::solana_name_id!($id, $name);

        impl $crate::sysvar::SysvarId for $type {
            fn check_id(pubkey: &$crate::pubkey::Pubkey) -> bool {
                check_id(pubkey)
            }
        }

        #[cfg(test)]
        #[test]
        fn test_sysvar_id() {
            if !$crate::sysvar::is_sysvar_id(&id()) {
                panic!("sysvar::is_sysvar_id() doesn't know about {}", $name);
            }
        }
    )
);

/// "Sysvar1111111111111111111111111111111111111"
///   owner pubkey for sysvar accounts
const ID: [u8; 32] = [
    6, 167, 213, 23, 24, 117, 247, 41, 199, 61, 147, 64, 143, 33, 97, 32, 6, 126, 216, 140, 118,
    224, 140, 40, 127, 193, 148, 96, 0, 0, 0, 0,
];

crate::solana_name_id!(ID, "Sysvar1111111111111111111111111111111111111");

pub trait SysvarId {
    fn check_id(pubkey: &Pubkey) -> bool;
}

// utilities for moving into and out of Accounts
pub trait Sysvar:
    SysvarId + Default + Sized + serde::Serialize + serde::de::DeserializeOwned
{
    fn biggest() -> Self {
        Self::default()
    }
    fn size_of() -> usize {
        bincode::serialized_size(&Self::biggest()).unwrap() as usize
    }
    fn from_account(account: &Account) -> Option<Self> {
        bincode::deserialize(&account.data).ok()
    }
    fn to_account(&self, account: &mut Account) -> Option<()> {
        bincode::serialize_into(&mut account.data[..], self).ok()
    }
    fn from_account_info(account: &AccountInfo) -> Option<Self> {
        bincode::deserialize(&account.data).ok()
    }
    fn to_account_info(&self, account: &mut AccountInfo) -> Option<()> {
        bincode::serialize_into(&mut account.data[..], self).ok()
    }
    fn from_keyed_account(account: &KeyedAccount) -> Result<Self, InstructionError> {
        if !Self::check_id(account.unsigned_key()) {
            return Err(InstructionError::InvalidArgument);
        }
        Self::from_account(account.account).ok_or(InstructionError::InvalidArgument)
    }
    fn create_account(&self, lamports: u64) -> Account {
        let mut account = Account::new(lamports, Self::size_of(), &id());
        self.to_account(&mut account).unwrap();
        account
    }
}

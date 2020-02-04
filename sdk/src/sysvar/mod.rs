//! named accounts for synthesized data accounts for bank state, etc.
//!
use crate::{
    account::{Account, KeyedAccount},
    account_info::AccountInfo,
    instruction::InstructionError,
    program_error::ProgramError,
    pubkey::Pubkey,
};

pub mod clock;
pub mod epoch_schedule;
pub mod fees;
pub mod recent_blockhashes;
pub mod rent;
pub mod rewards;
pub mod slot_hashes;
pub mod slot_history;
pub mod stake_history;

pub fn is_sysvar_id(id: &Pubkey) -> bool {
    clock::check_id(id)
        || epoch_schedule::check_id(id)
        || fees::check_id(id)
        || recent_blockhashes::check_id(id)
        || rent::check_id(id)
        || rewards::check_id(id)
        || slot_hashes::check_id(id)
        || slot_history::check_id(id)
        || stake_history::check_id(id)
}

#[macro_export]
macro_rules! declare_sysvar_id(
    ($name:expr, $type:ty) => (
        $crate::declare_id!($name);

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

// owner pubkey for sysvar accounts
crate::declare_id!("Sysvar1111111111111111111111111111111111111");

pub trait SysvarId {
    fn check_id(pubkey: &Pubkey) -> bool;
}

// utilities for moving into and out of Accounts
pub trait Sysvar:
    SysvarId + Default + Sized + serde::Serialize + serde::de::DeserializeOwned
{
    fn size_of() -> usize {
        bincode::serialized_size(&Self::default()).unwrap() as usize
    }
    fn from_account(account: &Account) -> Option<Self> {
        bincode::deserialize(&account.data).ok()
    }
    fn to_account(&self, account: &mut Account) -> Option<()> {
        bincode::serialize_into(&mut account.data[..], self).ok()
    }
    fn from_account_info(account_info: &AccountInfo) -> Result<Self, ProgramError> {
        bincode::deserialize(&account_info.data.borrow()).map_err(|_| ProgramError::InvalidArgument)
    }
    fn to_account_info(&self, account_info: &mut AccountInfo) -> Option<()> {
        bincode::serialize_into(&mut account_info.data.borrow_mut()[..], self).ok()
    }
    fn from_keyed_account(keyed_account: &KeyedAccount) -> Result<Self, InstructionError> {
        if !Self::check_id(keyed_account.unsigned_key()) {
            return Err(InstructionError::InvalidArgument);
        }
        Self::from_account(&*keyed_account.try_account_ref()?)
            .ok_or(InstructionError::InvalidArgument)
    }
    fn create_account(&self, lamports: u64) -> Account {
        let data_len = Self::size_of().max(bincode::serialized_size(self).unwrap() as usize);
        let mut account = Account::new(lamports, data_len, &id());
        self.to_account(&mut account).unwrap();
        account
    }
}

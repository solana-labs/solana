//! named accounts for synthesized data accounts for bank state, etc.
//!
//! this account carries the Bank's most recent blockhashes for some N parents
//!
pub use crate::slot_hashes::{SlotHash, SlotHashes};
use crate::{account::Account, account_info::AccountInfo, sysvar};
use bincode::serialized_size;

const ID: [u8; 32] = [
    6, 167, 213, 23, 25, 47, 10, 175, 198, 242, 101, 227, 251, 119, 204, 122, 218, 130, 197, 41,
    208, 190, 59, 19, 110, 45, 0, 85, 32, 0, 0, 0,
];

crate::solana_sysvar_id!(ID, "SysvarS1otHashes111111111111111111111111111");

pub const MAX_SLOT_HASHES: usize = 512; // 512 slots to get your vote in

impl SlotHashes {
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
        serialized_size(&SlotHashes::new(&[SlotHash::default(); MAX_SLOT_HASHES])).unwrap() as usize
    }
}

pub fn create_account(lamports: u64, slot_hashes: &[SlotHash]) -> Account {
    let mut account = Account::new(lamports, SlotHashes::size_of(), &sysvar::id());
    SlotHashes::new(slot_hashes)
        .to_account(&mut account)
        .unwrap();
    account
}

use crate::account::KeyedAccount;
use crate::instruction::InstructionError;
pub fn from_keyed_account(account: &KeyedAccount) -> Result<SlotHashes, InstructionError> {
    if !check_id(account.unsigned_key()) {
        return Err(InstructionError::InvalidArgument);
    }
    SlotHashes::from_account(account.account).ok_or(InstructionError::InvalidArgument)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_account() {
        let lamports = 42;
        let account = create_account(lamports, &[]);
        assert_eq!(account.data.len(), SlotHashes::size_of());
        let slot_hashes = SlotHashes::from_account(&account);
        assert_eq!(slot_hashes, Some(SlotHashes::default()));
    }
}

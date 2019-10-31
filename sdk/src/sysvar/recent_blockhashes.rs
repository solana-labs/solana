use crate::{account::Account, account_info::AccountInfo, hash::Hash, sysvar};
use bincode::serialized_size;
use std::ops::Deref;

pub const MAX_ENTRIES: usize = 32;
const ID: [u8; 32] = [
    0x06, 0xa7, 0xd5, 0x17, 0x19, 0x2c, 0x56, 0x8e, 0xe0, 0x8a, 0x84, 0x5f, 0x73, 0xd2, 0x97, 0x88,
    0xcf, 0x03, 0x5c, 0x31, 0x45, 0xb2, 0x1a, 0xb3, 0x44, 0xd8, 0x06, 0x2e, 0xa9, 0x40, 0x00, 0x00,
];

crate::solana_sysvar_id!(ID, "SysvarRecentB1ockHashes11111111111111111111");

#[repr(C)]
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct RecentBlockhashes(Vec<Hash>);

impl Default for RecentBlockhashes {
    fn default() -> Self {
        Self(Vec::with_capacity(MAX_ENTRIES))
    }
}

impl RecentBlockhashes {
    pub fn from_account(account: &Account) -> Option<Self> {
        account.deserialize_data().ok()
    }
    pub fn to_account(&self, account: &mut Account) -> Option<()> {
        account.serialize_data(self).unwrap();
        Some(())
    }
    pub fn from_account_info(account: &AccountInfo) -> Option<Self> {
        account.deserialize_data().ok()
    }
    pub fn to_account_info(&self, account: &mut AccountInfo) -> Option<()> {
        account.serialize_data(self).ok()
    }
    pub fn size_of() -> usize {
        serialized_size(&RecentBlockhashes(vec![Hash::default(); MAX_ENTRIES])).unwrap() as usize
    }
}

impl Deref for RecentBlockhashes {
    type Target = Vec<Hash>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub fn create_account(lamports: u64, recent_blockhashes: Vec<Hash>) -> Account {
    let mut account = Account::new(lamports, RecentBlockhashes::size_of(), &sysvar::id());
    let recent_blockhashes = RecentBlockhashes(recent_blockhashes);
    recent_blockhashes.to_account(&mut account).unwrap();
    account
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::Hash;

    #[test]
    fn test_create_account_empty() {
        let account = create_account(42, vec![]);
        let recent_blockhashes = RecentBlockhashes::from_account(&account).unwrap();
        assert_eq!(recent_blockhashes, RecentBlockhashes::default());
    }

    #[test]
    fn test_create_account_full() {
        let account = create_account(42, vec![Hash::default(); MAX_ENTRIES]);
        let recent_blockhashes = RecentBlockhashes::from_account(&account).unwrap();
        assert_eq!(recent_blockhashes.len(), MAX_ENTRIES);
    }

    #[test]
    #[should_panic]
    fn test_create_account_too_big() {
        let account = create_account(42, vec![Hash::default(); MAX_ENTRIES + 1]);
        RecentBlockhashes::from_account(&account).unwrap();
    }
}

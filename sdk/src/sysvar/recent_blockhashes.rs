use crate::{account::Account, hash::Hash, sysvar::Sysvar};
use std::collections::BinaryHeap;
use std::iter::FromIterator;
use std::ops::Deref;

const MAX_ENTRIES: usize = 32;
const ID: [u8; 32] = [
    0x06, 0xa7, 0xd5, 0x17, 0x19, 0x2c, 0x56, 0x8e, 0xe0, 0x8a, 0x84, 0x5f, 0x73, 0xd2, 0x97, 0x88,
    0xcf, 0x03, 0x5c, 0x31, 0x45, 0xb2, 0x1a, 0xb3, 0x44, 0xd8, 0x06, 0x2e, 0xa9, 0x40, 0x00, 0x00,
];

crate::solana_sysvar_id!(
    ID,
    "SysvarRecentB1ockHashes11111111111111111111",
    RecentBlockhashes
);

#[repr(C)]
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct RecentBlockhashes(Vec<Hash>);

impl Default for RecentBlockhashes {
    fn default() -> Self {
        Self(Vec::with_capacity(MAX_ENTRIES))
    }
}

impl<'a> FromIterator<&'a Hash> for RecentBlockhashes {
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = &'a Hash>,
    {
        let mut new = Self::default();
        for i in iter {
            new.0.push(*i)
        }
        new
    }
}

impl Sysvar for RecentBlockhashes {
    fn biggest() -> Self {
        RecentBlockhashes(vec![Hash::default(); MAX_ENTRIES])
    }
}

impl Deref for RecentBlockhashes {
    type Target = Vec<Hash>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub fn create_account(lamports: u64) -> Account {
    RecentBlockhashes::default().create_account(lamports)
}

pub fn update_account<'a, I>(account: &mut Account, recent_blockhash_iter: I) -> Option<()>
where
    I: IntoIterator<Item = (u64, &'a Hash)>,
{
    let sorted = BinaryHeap::from_iter(recent_blockhash_iter);
    let recent_blockhash_iter = sorted.into_iter().take(MAX_ENTRIES).map(|(_, hash)| hash);
    let recent_blockhashes = RecentBlockhashes::from_iter(recent_blockhash_iter);
    recent_blockhashes.to_account(account)
}

pub fn create_account_with_data<'a, I>(lamports: u64, recent_blockhash_iter: I) -> Account
where
    I: IntoIterator<Item = (u64, &'a Hash)>,
{
    let mut account = create_account(lamports);
    update_account(&mut account, recent_blockhash_iter).unwrap();
    account
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::Hash;

    #[test]
    fn test_create_account_empty() {
        let account = create_account_with_data(42, vec![].into_iter());
        let recent_blockhashes = RecentBlockhashes::from_account(&account).unwrap();
        assert_eq!(recent_blockhashes, RecentBlockhashes::default());
    }

    #[test]
    fn test_create_account_full() {
        let def_hash = Hash::default();
        let account =
            create_account_with_data(42, vec![(0u64, &def_hash); MAX_ENTRIES].into_iter());
        let recent_blockhashes = RecentBlockhashes::from_account(&account).unwrap();
        assert_eq!(recent_blockhashes.len(), MAX_ENTRIES);
    }

    #[test]
    fn test_create_account_truncate() {
        let def_hash = Hash::default();
        let account =
            create_account_with_data(42, vec![(0u64, &def_hash); MAX_ENTRIES + 1].into_iter());
        let recent_blockhashes = RecentBlockhashes::from_account(&account).unwrap();
        assert_eq!(recent_blockhashes.len(), MAX_ENTRIES);
    }
}

//! named accounts for synthesized data accounts for bank state, etc.
//!
//! this account carries the Bank's most recent blockhashes for some N parents
//!
use crate::account::Account;
use crate::hash::Hash;
use crate::sysvar;
use bincode::serialized_size;
use std::ops::Deref;

pub use crate::timing::Slot;

const ID: [u8; 32] = [
    6, 167, 213, 23, 25, 44, 97, 55, 206, 224, 146, 217, 182, 146, 62, 225, 204, 214, 25, 3, 250,
    130, 184, 161, 97, 145, 87, 141, 128, 0, 0, 0,
];

crate::solana_name_id!(ID, "SysvarRewards111111111111111111111111111111");

pub const MAX_SLOT_HASHES: usize = 512; // 512 slots to get your vote in

#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct SlotHashes {
    // non-pub to keep control of size
    inner: Vec<(Slot, Hash)>,
}

impl SlotHashes {
    pub fn from(account: &Account) -> Option<Self> {
        account.deserialize_data().ok()
    }
    pub fn to(&self, account: &mut Account) -> Option<()> {
        account.serialize_data(self).ok()
    }

    pub fn size_of() -> usize {
        serialized_size(&SlotHashes {
            inner: vec![(0, Hash::default()); MAX_SLOT_HASHES],
        })
        .unwrap() as usize
    }
    pub fn add(&mut self, slot: Slot, hash: Hash) {
        self.inner.insert(0, (slot, hash));
        self.inner.truncate(MAX_SLOT_HASHES);
    }
    pub fn new(slot_hashes: &[(Slot, Hash)]) -> Self {
        Self {
            inner: slot_hashes.to_vec(),
        }
    }
}

impl Deref for SlotHashes {
    type Target = Vec<(u64, Hash)>;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

pub fn create_account(lamports: u64, slot_hashes: &[(Slot, Hash)]) -> Account {
    let mut account = Account::new(lamports, SlotHashes::size_of(), &sysvar::id());
    SlotHashes::new(slot_hashes).to(&mut account).unwrap();
    account
}

use crate::account::KeyedAccount;
use crate::instruction::InstructionError;
pub fn from_keyed_account(account: &KeyedAccount) -> Result<SlotHashes, InstructionError> {
    if !check_id(account.unsigned_key()) {
        return Err(InstructionError::InvalidArgument);
    }
    SlotHashes::from(account.account).ok_or(InstructionError::InvalidArgument)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::hash;

    #[test]
    fn test_create_account() {
        let lamports = 42;
        let account = create_account(lamports, &[]);
        assert_eq!(account.data.len(), SlotHashes::size_of());
        let slot_hashes = SlotHashes::from(&account);
        assert_eq!(slot_hashes, Some(SlotHashes { inner: vec![] }));
        let mut slot_hashes = slot_hashes.unwrap();
        for i in 0..MAX_SLOT_HASHES + 1 {
            slot_hashes.add(
                i as u64,
                hash(&[(i >> 24) as u8, (i >> 16) as u8, (i >> 8) as u8, i as u8]),
            );
        }
        for i in 0..MAX_SLOT_HASHES {
            assert_eq!(slot_hashes[i].0, (MAX_SLOT_HASHES - i) as u64);
        }

        assert_eq!(slot_hashes.len(), MAX_SLOT_HASHES);
    }
}

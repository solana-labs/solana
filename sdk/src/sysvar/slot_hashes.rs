//! named accounts for synthesized data accounts for bank state, etc.
//!
//! this account carries the Bank's most recent blockhashes for some N parents
//!
use crate::account::Account;
use crate::hash::Hash;
use crate::sysvar;
use bincode::serialized_size;
use std::ops::Deref;

pub use crate::clock::Slot;

const ID: [u8; 32] = [
    6, 167, 213, 23, 25, 47, 10, 175, 198, 242, 101, 227, 251, 119, 204, 122, 218, 130, 197, 41,
    208, 190, 59, 19, 110, 45, 0, 85, 32, 0, 0, 0,
];

crate::solana_name_id!(ID, "SysvarS1otHashes111111111111111111111111111");

pub const MAX_SLOT_HASHES: usize = 512; // 512 slots to get your vote in

#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct SlotHashes(Vec<(Slot, Hash)>);

impl SlotHashes {
    pub fn from(account: &Account) -> Option<Self> {
        account.deserialize_data().ok()
    }
    pub fn to(&self, account: &mut Account) -> Option<()> {
        account.serialize_data(self).ok()
    }

    pub fn size_of() -> usize {
        serialized_size(&SlotHashes(vec![(0, Hash::default()); MAX_SLOT_HASHES])).unwrap() as usize
    }
    pub fn add(&mut self, slot: Slot, hash: Hash) {
        match self.binary_search_by(|probe| slot.cmp(&probe.0)) {
            Ok(index) => (self.0)[index] = (slot, hash),
            Err(index) => (self.0).insert(index, (slot, hash)),
        }
        (self.0).truncate(MAX_SLOT_HASHES);
    }
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub fn get(&self, slot: &Slot) -> Option<&Hash> {
        self.binary_search_by(|probe| slot.cmp(&probe.0))
            .ok()
            .map(|index| &self[index].1)
    }
    pub fn new(slot_hashes: &[(Slot, Hash)]) -> Self {
        Self(slot_hashes.to_vec())
    }
}

impl Deref for SlotHashes {
    type Target = Vec<(u64, Hash)>;
    fn deref(&self) -> &Self::Target {
        &self.0
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
        assert_eq!(slot_hashes, Some(SlotHashes(vec![])));
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

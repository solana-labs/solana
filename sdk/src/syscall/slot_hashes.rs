//! named accounts for synthesized data accounts for bank state, etc.
//!
//! this account carries the Bank's most recent blockhashes for some N parents
//!
use crate::account::Account;
use crate::account_utils::State;
use crate::hash::Hash;
use crate::pubkey::Pubkey;
use crate::syscall;
use bincode::serialized_size;
use std::ops::Deref;

/// "Sysca11SlotHashes11111111111111111111111111"
///  slot hashes account pubkey
const ID: [u8; 32] = [
    6, 167, 211, 138, 69, 219, 186, 157, 48, 170, 46, 66, 2, 146, 193, 59, 39, 59, 245, 188, 30,
    60, 130, 78, 86, 27, 113, 191, 208, 0, 0, 0,
];

pub fn id() -> Pubkey {
    Pubkey::new(&ID)
}

pub fn check_id(pubkey: &Pubkey) -> bool {
    pubkey.as_ref() == ID
}

pub const MAX_SLOT_HASHES: usize = 512; // 512 slots to get your vote in

#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct SlotHashes {
    // non-pub to keep control of size
    inner: Vec<(u64, Hash)>,
}

impl SlotHashes {
    pub fn from(account: &Account) -> Option<Self> {
        account.state().ok()
    }
    pub fn to(&self, account: &mut Account) -> Option<()> {
        account.set_state(self).ok()
    }

    pub fn size_of() -> usize {
        serialized_size(&SlotHashes {
            inner: vec![(0, Hash::default()); MAX_SLOT_HASHES],
        })
        .unwrap() as usize
    }
    pub fn add(&mut self, slot: u64, hash: Hash) {
        self.inner.insert(0, (slot, hash));
        self.inner.truncate(MAX_SLOT_HASHES);
    }
}

impl Deref for SlotHashes {
    type Target = Vec<(u64, Hash)>;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

pub fn create_account(lamports: u64) -> Account {
    Account::new(lamports, SlotHashes::size_of(), &syscall::id())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::hash;

    #[test]
    fn test_create_account() {
        let lamports = 42;
        let account = create_account(lamports);
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

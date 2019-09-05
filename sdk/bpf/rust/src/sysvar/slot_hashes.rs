//! named accounts for synthesized data accounts for bank state, etc.
//!
//! this account carries the Bank's most recent blockhashes for some N parents
//!
use crate::account::Account;
use crate::hash::Hash;
use bincode::serialized_size;
use std::ops::Deref;

pub use crate::timing::Slot;

const ID: [u8; 32] = [
    6, 167, 213, 23, 25, 47, 10, 175, 198, 242, 101, 227, 251, 119, 204, 122, 218, 130, 197, 41,
    208, 190, 59, 19, 110, 45, 0, 85, 32, 0, 0, 0,
];

crate::solana_name_id!(ID, "SysvarS1otHashes111111111111111111111111111");

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
}

impl Deref for SlotHashes {
    type Target = Vec<(u64, Hash)>;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

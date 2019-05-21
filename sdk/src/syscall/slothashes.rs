//! named accounts for synthesized data accounts for bank state, etc.
//!
//! this account carries the Bank's most recent blockhashes for some N parents
//!
use crate::account::Account;
use crate::hash::Hash;
use crate::pubkey::Pubkey;

/// "Sysca11SlotHashes11111111111111111111111111"
///  slot hashes account pubkey
const SYSCALL_SLOT_HASHES_ID: [u8; 32] = [
    6, 167, 211, 138, 69, 219, 186, 157, 48, 170, 46, 66, 2, 146, 193, 59, 39, 59, 245, 188, 30,
    60, 130, 78, 86, 27, 113, 191, 208, 0, 0, 0,
];

pub fn id() -> Pubkey {
    Pubkey::new(&SYSCALL_SLOT_HASHES_ID)
}

use crate::account_utils::State;
use std::ops::{Deref, DerefMut};
#[derive(Serialize, Deserialize)]
pub struct SlotHashes(Vec<(u64, Hash)>);

impl SlotHashes {
    pub fn from(account: &Account) -> Option<Self> {
        account.state().ok()
    }
}
impl Deref for SlotHashes {
    type Target = Vec<(u64, Hash)>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for SlotHashes {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_syscall_ids() {
        let ids = [("Sysca11S1otHashes11111111111111111111111111", id())];
        // to get the bytes above:
        //        ids.iter().for_each(|(name, _)| {
        //            dbg!((name, bs58::decode(name).into_vec().unwrap()));
        //        });
        assert!(ids.iter().all(|(name, id)| *name == id.to_string()));
    }
}

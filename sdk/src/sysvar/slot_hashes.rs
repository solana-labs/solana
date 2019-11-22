//! named accounts for synthesized data accounts for bank state, etc.
//!
//! this account carries the Bank's most recent blockhashes for some N parents
//!
pub use crate::slot_hashes::{SlotHash, SlotHashes};
use crate::{account::Account, sysvar::Sysvar};

crate::declare_sysvar_id!("SysvarS1otHashes111111111111111111111111111", SlotHashes);

pub const MAX_SLOT_HASHES: usize = 512; // 512 slots to get your vote in

impl Sysvar for SlotHashes {
    fn biggest() -> Self {
        // override
        SlotHashes::new(&[SlotHash::default(); MAX_SLOT_HASHES])
    }
}

pub fn create_account(lamports: u64, slot_hashes: &[SlotHash]) -> Account {
    SlotHashes::new(slot_hashes).create_account(lamports)
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

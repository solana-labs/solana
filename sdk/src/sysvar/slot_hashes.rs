//! named accounts for synthesized data accounts for bank state, etc.
//!
//! this account carries the Bank's most recent blockhashes for some N parents
//!
pub use crate::slot_hashes::{Slot, SlotHash, SlotHashes, MAX_SLOT_HASHES};
use crate::{account::Account, hash::Hash, sysvar::Sysvar};

crate::declare_sysvar_id!("SysvarS1otHashes111111111111111111111111111", SlotHashes);

impl Sysvar for SlotHashes {
    fn biggest() -> Self {
        // override
        (0..MAX_SLOT_HASHES)
            .map(|slot| (slot as Slot, Hash::default()))
            .collect::<Self>()
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

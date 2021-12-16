//! named accounts for synthesized data accounts for bank state, etc.
//!
//! this account carries the Bank's most recent bank hashes for some N parents
//!
pub use crate::slot_hashes::SlotHashes;
use crate::{account_info::AccountInfo, program_error::ProgramError, sysvar::Sysvar};

crate::declare_sysvar_id!("SysvarS1otHashes111111111111111111111111111", SlotHashes);

impl Sysvar for SlotHashes {
    // override
    fn size_of() -> usize {
        // hard-coded so that we don't have to construct an empty
        20_488 // golden, update if MAX_ENTRIES changes
    }
    fn from_account_info(_account_info: &AccountInfo) -> Result<Self, ProgramError> {
        // This sysvar is too large to bincode::deserialize in-program
        Err(ProgramError::UnsupportedSysvar)
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{clock::Slot, hash::Hash, slot_hashes::MAX_ENTRIES},
    };

    #[test]
    fn test_size_of() {
        assert_eq!(
            SlotHashes::size_of(),
            bincode::serialized_size(
                &(0..MAX_ENTRIES)
                    .map(|slot| (slot as Slot, Hash::default()))
                    .collect::<SlotHashes>()
            )
            .unwrap() as usize
        );
    }
}

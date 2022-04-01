//! named accounts for synthesized data accounts for bank state, etc.
//!
//! this account carries a bitvector of slots present over the past
//! epoch
//!
use crate::sysvar::Sysvar;
pub use crate::{
    account_info::AccountInfo, program_error::ProgramError, slot_history::SlotHistory,
};

crate::declare_sysvar_id!("SysvarS1otHistory11111111111111111111111111", SlotHistory);

impl Sysvar for SlotHistory {
    // override
    fn size_of() -> usize {
        // hard-coded so that we don't have to construct an empty
        131_097 // golden, update if MAX_ENTRIES changes
    }
    fn from_account_info(_account_info: &AccountInfo) -> Result<Self, ProgramError> {
        // This sysvar is too large to bincode::deserialize in-program
        Err(ProgramError::UnsupportedSysvar)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_size_of() {
        assert_eq!(
            SlotHistory::size_of(),
            bincode::serialized_size(&SlotHistory::default()).unwrap() as usize
        );
    }
}

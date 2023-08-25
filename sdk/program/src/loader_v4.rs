//! The v3 built-in loader program.
//!
//! This is the loader of the program runtime v2.

use crate::pubkey::Pubkey;

crate::declare_id!("LoaderV411111111111111111111111111111111111");

/// Cooldown before a program can be un-/redeployed again
pub const DEPLOYMENT_COOLDOWN_IN_SLOTS: u64 = 750;

/// LoaderV4 account states
#[repr(C)]
#[derive(Default, Debug, Serialize, Deserialize, PartialEq, Eq, Clone, Copy, AbiExample)]
pub struct LoaderV4State {
    /// Authority address, zero means it is finalized
    pub authority_address: Pubkey,
    /// Slot that the program was last initialized, deployed or retracted in.
    pub slot: u64,
    /// True if the program is ready to be executed, false if it is retracted for maintainance.
    pub is_deployed: bool,
    /// Padding, always zero
    pub padding: [u8; 7],
    // The raw program data follows this serialized structure in the
    // account's data.
}

impl LoaderV4State {
    /// Size of a serialized program account.
    pub const fn program_data_offset() -> usize {
        std::mem::size_of::<Self>()
    }

    /// Returns true if program is finalized.
    pub fn is_finalized(&self) -> bool {
        self.authority_address == Pubkey::default()
    }
}

#[cfg(test)]
mod tests {
    use {super::*, memoffset::offset_of};

    #[test]
    fn test_state_layout() {
        assert_eq!(std::mem::size_of::<LoaderV4State>(), 0x30);
        assert_eq!(LoaderV4State::program_data_offset(), 0x30);
        assert_eq!(offset_of!(LoaderV4State, authority_address), 0x00);
        assert_eq!(offset_of!(LoaderV4State, slot), 0x20);
        assert_eq!(offset_of!(LoaderV4State, is_deployed), 0x28);
        assert_eq!(offset_of!(LoaderV4State, padding), 0x29);
    }
}

//! The v4 built-in loader program.
//!
//! This is the loader of the program runtime v2.

use crate::pubkey::Pubkey;

crate::declare_id!("LoaderV411111111111111111111111111111111111");

/// Cooldown before a program can be un-/redeployed again
pub const DEPLOYMENT_COOLDOWN_IN_SLOTS: u64 = 750;

#[repr(u64)]
#[derive(Debug, PartialEq, Eq, Clone, Copy, AbiExample)]
pub enum LoaderV4Status {
    /// Program is in maintanance
    Retracted,
    /// Program is ready to be executed
    Deployed,
    /// Same as `Deployed`, but can not be retracted anymore
    Finalized,
}

/// LoaderV4 account states
#[repr(C)]
#[derive(Debug, PartialEq, Eq, Clone, Copy, AbiExample)]
pub struct LoaderV4State {
    /// Slot in which the program was last deployed, retracted or initialized.
    pub slot: u64,
    /// Address of signer which can send program management instructions.
    pub authority_address: Pubkey,
    /// Deployment status.
    pub status: LoaderV4Status,
    // The raw program data follows this serialized structure in the
    // account's data.
}

impl LoaderV4State {
    /// Size of a serialized program account.
    pub const fn program_data_offset() -> usize {
        std::mem::size_of::<Self>()
    }
}

#[cfg(test)]
mod tests {
    use {super::*, memoffset::offset_of};

    #[test]
    fn test_layout() {
        assert_eq!(offset_of!(LoaderV4State, slot), 0x00);
        assert_eq!(offset_of!(LoaderV4State, authority_address), 0x08);
        assert_eq!(offset_of!(LoaderV4State, status), 0x28);
        assert_eq!(LoaderV4State::program_data_offset(), 0x30);
    }
}

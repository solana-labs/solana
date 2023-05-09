//! The v3 built-in loader program.
//!
//! This is the loader of the program runtime v2.

use crate::pubkey::Pubkey;

crate::declare_id!("LoaderV311111111111111111111111111111111111");

/// Cooldown before a program can be un-/redeployed again
pub const DEPLOYMENT_COOLDOWN_IN_SLOTS: u64 = 750;

/// LoaderV3 account states
#[repr(C)]
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone, Copy, AbiExample)]
pub struct LoaderV3State {
    /// Slot that the program was last initialized, deployed or retracted in.
    pub slot: u64,
    /// True if the program is ready to be executed, false if it is retracted for maintainance.
    pub is_deployed: bool,
    /// Authority address, `None` means it is finalized.
    pub authority_address: Option<Pubkey>,
    // The raw program data follows this serialized structure in the
    // account's data.
}

impl LoaderV3State {
    /// Size of a serialized program account.
    pub const fn program_data_offset() -> usize {
        std::mem::size_of::<Self>()
    }
}

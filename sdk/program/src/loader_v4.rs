//! The v4 built-in loader program.
//!
//! This is the loader of the program runtime v2.

use crate::pubkey::Pubkey;

crate::declare_id!("LoaderV411111111111111111111111111111111111");

/// Cooldown before a program can be un-/redeployed again
pub const DEPLOYMENT_COOLDOWN_IN_SLOTS: u64 = 750;

/// LoaderV4 account states
#[repr(C)]
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone, Copy, AbiExample)]
pub struct LoaderV4State {
    /// Slot that the program was last initialized, deployed or retracted in.
    pub slot: u64,
    /// True if the program is ready to be executed, false if it is retracted for maintainance.
    pub is_deployed: bool,
    /// Authority address, `None` means it is finalized.
    pub authority_address: Option<Pubkey>,
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
    fn test_state_layout() {
        assert_eq!(std::mem::size_of::<LoaderV4State>(), 0x30);
        assert_eq!(LoaderV4State::program_data_offset(), 0x30);
        assert_eq!(offset_of!(LoaderV4State, slot), 0x00);
        assert_eq!(offset_of!(LoaderV4State, is_deployed), 0x08);
        assert_eq!(offset_of!(LoaderV4State, authority_address), 0x09);
    }

    #[test]
    fn test_transmute() {
        // Ensure transmute behaves correctly on complex types.
        let mut state = LoaderV4State {
            slot: 0,
            is_deployed: true,
            authority_address: Some(super::id()),
        };

        let raw: &mut [u8; std::mem::size_of::<LoaderV4State>()];
        unsafe {
            raw = std::mem::transmute(&mut state);
        }

        // Layout assertion
        assert_eq!(
            &raw[0x09..0x2a],
            &[
                0x01, // Option discriminant
                0x05, 0x12, 0xb4, 0x11, 0x51, 0x51, 0xe3, 0x7a, // 0x08
                0xad, 0x0a, 0x8b, 0xc5, 0xd3, 0x88, 0x2e, 0x7b, // 0x10
                0x7f, 0xda, 0x4c, 0xf3, 0xd2, 0xc0, 0x28, 0xc8, // 0x18
                0xcf, 0x83, 0x36, 0x18, 0x00, 0x00, 0x00, 0x00, // 0x20
            ]
        );

        // Ensure that changing the option disciminant properly
        // sanitizes the previously set value.
        state.authority_address = None;
        assert_eq!(
            &raw[0x09..0x2a],
            &[
                0x00, // Option discriminant
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0x08
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0x10
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0x18
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 0x20
            ]
        );
    }
}

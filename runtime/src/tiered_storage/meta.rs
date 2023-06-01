//! The account meta and related structs for the tiered storage.

use modular_bitfield::prelude::*;

/// The struct that handles the account meta flags.
#[allow(dead_code)]
#[bitfield(bits = 32)]
#[repr(C)]
#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
pub(crate) struct AccountMetaFlags {
    /// whether the account meta has rent epoch
    #[allow(dead_code)]
    has_rent_epoch: bool,
    /// whether the account meta has account hash
    #[allow(dead_code)]
    has_account_hash: bool,
    /// whether the account meta has write version
    #[allow(dead_code)]
    has_write_version: bool,
    /// the reserved bits.
    #[allow(dead_code)]
    reserved: B29,
}

#[cfg(test)]
pub mod tests {
    use crate::tiered_storage::meta::*;

    #[test]
    fn test_account_meta_flags_new() {
        let flags = AccountMetaFlags::new();

        assert_eq!(flags.has_rent_epoch(), false);
        assert_eq!(flags.has_account_hash(), false);
        assert_eq!(flags.has_write_version(), false);
        assert_eq!(flags.reserved() as u32, 0u32);

        assert_eq!(
            std::mem::size_of::<AccountMetaFlags>(),
            std::mem::size_of::<u32>()
        );
    }

    fn verify_flags_serialization(flags: &AccountMetaFlags) {
        assert_eq!(AccountMetaFlags::from_bytes(flags.into_bytes()), *flags);
    }

    #[test]
    fn test_account_meta_flags_set() {
        let mut flags = AccountMetaFlags::new();

        flags.set_has_rent_epoch(true);

        assert_eq!(flags.has_rent_epoch(), true);
        assert_eq!(flags.has_account_hash(), false);
        assert_eq!(flags.has_write_version(), false);
        verify_flags_serialization(&flags);

        flags.set_has_account_hash(true);

        assert_eq!(flags.has_rent_epoch(), true);
        assert_eq!(flags.has_account_hash(), true);
        assert_eq!(flags.has_write_version(), false);
        verify_flags_serialization(&flags);

        flags.set_has_write_version(true);

        assert_eq!(flags.has_rent_epoch(), true);
        assert_eq!(flags.has_account_hash(), true);
        assert_eq!(flags.has_write_version(), true);
        verify_flags_serialization(&flags);

        // make sure the reserved bits are untouched.
        assert_eq!(flags.reserved() as u32, 0u32);
    }
}

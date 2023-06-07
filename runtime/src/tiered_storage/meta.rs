#![allow(dead_code)]
//! The account meta and related structs for the tiered storage.

use modular_bitfield::prelude::*;

/// The struct that handles the account meta flags.
#[bitfield(bits = 32)]
#[repr(C)]
#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
pub struct AccountMetaFlags {
    /// whether the account meta has rent epoch
    pub has_rent_epoch: bool,
    /// whether the account meta has account hash
    pub has_account_hash: bool,
    /// whether the account meta has write version
    pub has_write_version: bool,
    /// the reserved bits.
    reserved: B29,
}

#[cfg(test)]
pub mod tests {
    use crate::tiered_storage::meta::*;

    #[test]
    fn test_account_meta_flags_new() {
        let flags = AccountMetaFlags::new();

        assert!(!flags.has_rent_epoch());
        assert!(!flags.has_account_hash());
        assert!(!flags.has_write_version());
        assert_eq!(flags.reserved(), 0u32);

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

        assert!(flags.has_rent_epoch());
        assert!(!flags.has_account_hash());
        assert!(!flags.has_write_version());
        verify_flags_serialization(&flags);

        flags.set_has_account_hash(true);

        assert!(flags.has_rent_epoch());
        assert!(flags.has_account_hash());
        assert!(!flags.has_write_version());
        verify_flags_serialization(&flags);

        flags.set_has_write_version(true);

        assert!(flags.has_rent_epoch());
        assert!(flags.has_account_hash());
        assert!(flags.has_write_version());
        verify_flags_serialization(&flags);

        // make sure the reserved bits are untouched.
        assert_eq!(flags.reserved(), 0u32);
    }
}

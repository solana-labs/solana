#![allow(dead_code)]
//! The account meta and related structs for the tiered storage.

use {
    crate::account_storage::meta::StoredMetaWriteVersion,
    modular_bitfield::prelude::*,
    solana_sdk::{hash::Hash, stake_history::Epoch},
};

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

impl AccountMetaFlags {
    pub fn new_from(optional_fields: &AccountMetaOptionalFields) -> Self {
        let mut flags = AccountMetaFlags::default();
        flags.set_has_rent_epoch(optional_fields.rent_epoch.is_some());
        flags.set_has_account_hash(optional_fields.account_hash.is_some());
        flags.set_has_write_version(optional_fields.write_version.is_some());
        flags
    }
}

/// The in-memory struct for the optional fields for tiered account meta.
///
/// Note that the storage representation of the optional fields might be
/// different from its in-memory representation.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct AccountMetaOptionalFields {
    /// the epoch at which its associated account will next owe rent
    pub rent_epoch: Option<Epoch>,
    /// the hash of its associated account
    pub account_hash: Option<Hash>,
    /// Order of stores of its associated account to an accounts file will
    /// determine 'latest' account data per pubkey.
    pub write_version: Option<StoredMetaWriteVersion>,
}

impl AccountMetaOptionalFields {
    /// The size of the optional fields in bytes (excluding the boolean flags).
    pub fn size(&self) -> usize {
        self.rent_epoch.map_or(0, |_| std::mem::size_of::<Epoch>())
            + self.account_hash.map_or(0, |_| std::mem::size_of::<Hash>())
            + self
                .write_version
                .map_or(0, |_| std::mem::size_of::<StoredMetaWriteVersion>())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

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

    fn update_and_verify_flags(opt_fields: &AccountMetaOptionalFields) {
        let flags: AccountMetaFlags = AccountMetaFlags::new_from(opt_fields);
        assert_eq!(flags.has_rent_epoch(), opt_fields.rent_epoch.is_some());
        assert_eq!(flags.has_account_hash(), opt_fields.account_hash.is_some());
        assert_eq!(
            flags.has_write_version(),
            opt_fields.write_version.is_some()
        );
        assert_eq!(flags.reserved(), 0u32);
    }

    #[test]
    fn test_optional_fields_update_flags() {
        let test_epoch = 5432312;
        let test_write_version = 231;

        for rent_epoch in [None, Some(test_epoch)] {
            for account_hash in [None, Some(Hash::new_unique())] {
                for write_version in [None, Some(test_write_version)] {
                    update_and_verify_flags(&AccountMetaOptionalFields {
                        rent_epoch,
                        account_hash,
                        write_version,
                    });
                }
            }
        }
    }

    #[test]
    fn test_optional_fields_size() {
        let test_epoch = 5432312;
        let test_write_version = 231;

        for rent_epoch in [None, Some(test_epoch)] {
            for account_hash in [None, Some(Hash::new_unique())] {
                for write_version in [None, Some(test_write_version)] {
                    let opt_fields = AccountMetaOptionalFields {
                        rent_epoch,
                        account_hash,
                        write_version,
                    };
                    assert_eq!(
                        opt_fields.size(),
                        rent_epoch.map_or(0, |_| std::mem::size_of::<Epoch>())
                            + account_hash.map_or(0, |_| std::mem::size_of::<Hash>())
                            + write_version
                                .map_or(0, |_| std::mem::size_of::<StoredMetaWriteVersion>())
                    );
                }
            }
        }
    }
}

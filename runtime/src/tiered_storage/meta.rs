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

/// A trait that allows different implementations of the account meta that
/// support different tiers of the accounts storage.
pub trait TieredAccountMeta: Sized {
    /// Constructs a TieredAcountMeta instance.
    fn new() -> Self;

    /// A builder function that initializes lamports.
    fn with_lamports(self, lamports: u64) -> Self;

    /// A builder function that initializes the number of padding bytes
    /// for the account data associated with the current meta.
    fn with_account_data_padding(self, padding: u8) -> Self;

    /// A builder function that initializes the owner's index.
    fn with_owner_index(self, index: u32) -> Self;

    /// A builder function that initializes the account data size.
    /// The size here represents the logical data size without compression.
    fn with_data_size(self, data_size: u64) -> Self;

    /// A builder function that initializes the AccountMetaFlags of the current
    /// meta.
    fn with_flags(self, flags: &AccountMetaFlags) -> Self;

    /// Returns the balance of the lamports associated with the account.
    fn lamports(&self) -> u64;

    /// Returns the number of padding bytes for the associated account data
    fn account_data_padding(&self) -> u8;

    /// Returns the size of its account data if the current accout meta
    /// shares its account block with other account meta entries.
    ///
    /// Otherwise, None will be returned.
    fn data_size_for_shared_block(&self) -> Option<usize>;

    /// Returns the index to the accounts' owner in the current AccountsFile.
    fn owner_index(&self) -> u32;

    /// Returns the AccountMetaFlags of the current meta.
    fn flags(&self) -> &AccountMetaFlags;

    /// Returns true if the TieredAccountMeta implementation supports multiple
    /// accounts sharing one account block.
    fn supports_shared_account_block() -> bool;
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

    /// Given the specified AccountMetaFlags, returns the size of its
    /// associated AccountMetaOptionalFields.
    pub fn size_from_flags(flags: &AccountMetaFlags) -> usize {
        let mut fields_size = 0;
        if flags.has_rent_epoch() {
            fields_size += std::mem::size_of::<Epoch>();
        }
        if flags.has_account_hash() {
            fields_size += std::mem::size_of::<Hash>();
        }
        if flags.has_write_version() {
            fields_size += std::mem::size_of::<StoredMetaWriteVersion>();
        }

        fields_size
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
                    assert_eq!(
                        opt_fields.size(),
                        AccountMetaOptionalFields::size_from_flags(&AccountMetaFlags::new_from(
                            &opt_fields
                        ))
                    );
                }
            }
        }
    }
}

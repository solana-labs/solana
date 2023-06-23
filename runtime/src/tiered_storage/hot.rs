#![allow(dead_code)]
//! The account meta and related structs for hot accounts.

use {
    crate::tiered_storage::meta::{AccountMetaFlags, TieredAccountMeta},
    modular_bitfield::prelude::*,
};

/// The maximum number of padding bytes used in a hot account entry.
const MAX_HOT_PADDING: u8 = 7;

/// The maximum allowed value for the owner index of a hot account.
const MAX_HOT_OWNER_INDEX: u32 = (1 << 29) - 1;

#[bitfield(bits = 32)]
#[repr(C)]
#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
struct HotMetaPackedFields {
    /// A hot account entry consists of the following elements:
    ///
    /// * HotAccountMeta
    /// * [u8] account data
    /// * 0-7 bytes padding
    /// * optional fields
    ///
    /// The following field records the number of padding bytes used
    /// in its hot account entry.
    padding: B3,
    /// The index to the owner of a hot account inside an AccountsFile.
    owner_index: B29,
}

/// The storage and in-memory representation of the metadata entry for a
/// hot account.
#[derive(Debug, PartialEq, Eq)]
#[repr(C)]
pub struct HotAccountMeta {
    /// The balance of this account.
    lamports: u64,
    /// Stores important fields in a packed struct.
    packed_fields: HotMetaPackedFields,
    /// Stores boolean flags and existence of each optional field.
    flags: AccountMetaFlags,
}

impl TieredAccountMeta for HotAccountMeta {
    /// Construct a HotAccountMeta instance.
    fn new() -> Self {
        HotAccountMeta {
            lamports: 0,
            packed_fields: HotMetaPackedFields::default(),
            flags: AccountMetaFlags::new(),
        }
    }

    /// A builder function that initializes lamports.
    fn with_lamports(mut self, lamports: u64) -> Self {
        self.lamports = lamports;
        self
    }

    /// A builder function that initializes the number of padding bytes
    /// for the account data associated with the current meta.
    fn with_account_data_padding(mut self, padding: u8) -> Self {
        if padding > MAX_HOT_PADDING {
            panic!("padding exceeds MAX_HOT_PADDING");
        }
        self.packed_fields.set_padding(padding);
        self
    }

    /// A builder function that initializes the owner's index.
    fn with_owner_index(mut self, owner_index: u32) -> Self {
        if owner_index > MAX_HOT_OWNER_INDEX {
            panic!("owner_index exceeds MAX_HOT_OWNER_INDEX");
        }
        self.packed_fields.set_owner_index(owner_index);
        self
    }

    /// A builder function that initializes the account data size.
    fn with_data_size(self, _data_size: u64) -> Self {
        // Hot meta does not store its data size as it derives its data length
        // by comparing the offets of two consecutive account meta entries.
        self
    }

    /// A builder function that initializes the AccountMetaFlags of the current
    /// meta.
    fn with_flags(mut self, flags: &AccountMetaFlags) -> Self {
        self.flags = *flags;
        self
    }

    /// Returns the balance of the lamports associated with the account.
    fn lamports(&self) -> u64 {
        self.lamports
    }

    /// Returns the number of padding bytes for the associated account data
    fn account_data_padding(&self) -> u8 {
        self.packed_fields.padding()
    }

    /// Always return None as a HotAccountMeta entry never shares its account
    /// block with other account meta entries.
    fn data_size_for_shared_block(&self) -> Option<usize> {
        None
    }

    /// Returns the index to the accounts' owner in the current AccountsFile.
    fn owner_index(&self) -> u32 {
        self.packed_fields.owner_index()
    }

    /// Returns the AccountMetaFlags of the current meta.
    fn flags(&self) -> &AccountMetaFlags {
        &self.flags
    }

    /// Always returns false as HotAccountMeta does not support multiple
    /// meta entries sharing the same account block.
    fn supports_shared_account_block() -> bool {
        false
    }
}

#[cfg(test)]
pub mod tests {
    use {
        super::*,
        crate::tiered_storage::meta::AccountMetaOptionalFields,
        memoffset::offset_of,
        solana_sdk::{hash::Hash, stake_history::Epoch},
    };

    #[test]
    fn test_hot_account_meta_layout() {
        assert_eq!(offset_of!(HotAccountMeta, lamports), 0x00);
        assert_eq!(offset_of!(HotAccountMeta, packed_fields), 0x08);
        assert_eq!(offset_of!(HotAccountMeta, flags), 0x0C);
        assert_eq!(std::mem::size_of::<HotAccountMeta>(), 16);
    }

    #[test]
    fn test_packed_fields() {
        const TEST_PADDING: u8 = 7;
        const TEST_OWNER_INDEX: u32 = 0x1fff_ef98;
        let mut packed_fields = HotMetaPackedFields::default();
        packed_fields.set_padding(TEST_PADDING);
        packed_fields.set_owner_index(TEST_OWNER_INDEX);
        assert_eq!(packed_fields.padding(), TEST_PADDING);
        assert_eq!(packed_fields.owner_index(), TEST_OWNER_INDEX);
    }

    #[test]
    fn test_packed_fields_max_values() {
        let mut packed_fields = HotMetaPackedFields::default();
        packed_fields.set_padding(MAX_HOT_PADDING);
        packed_fields.set_owner_index(MAX_HOT_OWNER_INDEX);
        assert_eq!(packed_fields.padding(), MAX_HOT_PADDING);
        assert_eq!(packed_fields.owner_index(), MAX_HOT_OWNER_INDEX);
    }

    #[test]
    fn test_hot_meta_max_values() {
        let meta = HotAccountMeta::new()
            .with_account_data_padding(MAX_HOT_PADDING)
            .with_owner_index(MAX_HOT_OWNER_INDEX);

        assert_eq!(meta.account_data_padding(), MAX_HOT_PADDING);
        assert_eq!(meta.owner_index(), MAX_HOT_OWNER_INDEX);
    }

    #[test]
    #[should_panic(expected = "padding exceeds MAX_HOT_PADDING")]
    fn test_hot_meta_padding_exceeds_limit() {
        HotAccountMeta::new().with_account_data_padding(MAX_HOT_PADDING + 1);
    }

    #[test]
    #[should_panic(expected = "owner_index exceeds MAX_HOT_OWNER_INDEX")]
    fn test_hot_meta_owner_index_exceeds_limit() {
        HotAccountMeta::new().with_owner_index(MAX_HOT_OWNER_INDEX + 1);
    }

    #[test]
    fn test_hot_account_meta() {
        const TEST_LAMPORTS: u64 = 2314232137;
        const TEST_PADDING: u8 = 5;
        const TEST_OWNER_INDEX: u32 = 0x1fef_1234;
        const TEST_RENT_EPOCH: Epoch = 7;

        let optional_fields = AccountMetaOptionalFields {
            rent_epoch: Some(TEST_RENT_EPOCH),
            account_hash: Some(Hash::new_unique()),
            write_version: None,
        };

        let flags = AccountMetaFlags::new_from(&optional_fields);
        let meta = HotAccountMeta::new()
            .with_lamports(TEST_LAMPORTS)
            .with_account_data_padding(TEST_PADDING)
            .with_owner_index(TEST_OWNER_INDEX)
            .with_flags(&flags);

        assert_eq!(meta.lamports(), TEST_LAMPORTS);
        assert_eq!(meta.account_data_padding(), TEST_PADDING);
        assert_eq!(meta.data_size_for_shared_block(), None);
        assert_eq!(meta.owner_index(), TEST_OWNER_INDEX);
        assert_eq!(*meta.flags(), flags);
    }
}

use {
    crate::{
        account_storage::meta::StoredMetaWriteVersion,
        tiered_storage::{
            file::TieredStorageFile, footer::TieredStorageFooter, AccountDataBlockWriter,
        },
    },
    ::solana_sdk::{hash::Hash, stake_history::Epoch},
    serde::{Deserialize, Serialize},
    std::mem::size_of,
};

lazy_static! {
    pub static ref DEFAULT_ACCOUNT_HASH: Hash = Hash::default();
}

pub const ACCOUNT_DATA_ENTIRE_BLOCK: u16 = std::u16::MAX;

// TODO(yhchiang): this function needs to be fixed.
pub(crate) fn get_compressed_block_size(
    _footer: &TieredStorageFooter,
    _metas: &Vec<impl TieredAccountMeta>,
    _index: usize,
) -> usize {
    unimplemented!();
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct AccountMetaFlags {
    flags: u32,
}

impl AccountMetaFlags {
    pub const EXECUTABLE: u32 = 1u32;
    pub const HAS_RENT_EPOCH: u32 = 1u32 << 1;
    pub const HAS_ACCOUNT_HASH: u32 = 1u32 << 2;
    pub const HAS_WRITE_VERSION: u32 = 1u32 << 3;
    // TODO(yhchiang): might not be needed.
    pub const HAS_DATA_LENGTH: u32 = 1u32 << 4;

    pub fn new() -> Self {
        Self { flags: 0 }
    }

    pub fn new_from(value: u32) -> Self {
        Self { flags: value }
    }

    pub fn with_bit(mut self, bit_field: u32, value: bool) -> Self {
        self.set(bit_field, value);

        self
    }

    pub fn to_value(self) -> u32 {
        self.flags
    }

    pub fn set(&mut self, bit_field: u32, value: bool) {
        if value == true {
            self.flags |= bit_field;
        } else {
            self.flags &= !bit_field;
        }
    }

    pub fn get(flags: &u32, bit_field: u32) -> bool {
        (flags & bit_field) > 0
    }

    pub fn get_value(&self) -> u32 {
        self.flags
    }

    pub fn get_value_mut(&mut self) -> &mut u32 {
        &mut self.flags
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct AccountMetaOptionalFields {
    pub rent_epoch: Option<Epoch>,
    pub account_hash: Option<Hash>,
    pub write_version_obsolete: Option<StoredMetaWriteVersion>,
}

impl AccountMetaOptionalFields {
    /// Returns the 16-bit value where each bit represesnts whether one
    /// optional field has a Some value.
    pub fn update_flags(&self, flags_value: &mut u32) {
        let mut flags = AccountMetaFlags::new_from(*flags_value);
        flags.set(AccountMetaFlags::HAS_RENT_EPOCH, self.rent_epoch.is_some());
        flags.set(
            AccountMetaFlags::HAS_ACCOUNT_HASH,
            self.account_hash.is_some(),
        );
        flags.set(
            AccountMetaFlags::HAS_WRITE_VERSION,
            self.write_version_obsolete.is_some(),
        );
        *flags_value = flags.to_value();
    }

    pub fn size(&self) -> usize {
        let mut size_in_bytes = 0;
        if self.rent_epoch.is_some() {
            size_in_bytes += size_of::<Epoch>();
        }
        if self.account_hash.is_some() {
            size_in_bytes += size_of::<Hash>();
        }
        if self.write_version_obsolete.is_some() {
            size_in_bytes += size_of::<StoredMetaWriteVersion>();
        }

        size_in_bytes
    }

    pub fn write(&self, writer: &mut AccountDataBlockWriter) -> std::io::Result<usize> {
        let mut length = 0;
        if let Some(rent_epoch) = self.rent_epoch {
            length += writer.write_type(&rent_epoch)?;
        }
        if let Some(hash) = self.account_hash {
            length += writer.write_type(&hash)?;
        }
        if let Some(write_version) = self.write_version_obsolete {
            length += writer.write_type(&write_version)?;
        }

        Ok(length)
    }
}

pub trait TieredAccountMeta {
    fn new() -> Self;

    fn is_blob_account_data(data_len: u64) -> bool;

    fn with_lamports(&mut self, _lamports: u64) -> &mut Self {
        unimplemented!();
    }

    fn with_block_offset(&mut self, _offset: u64) -> &mut Self {
        unimplemented!();
    }

    fn with_data_tailing_paddings(&mut self, _paddings: u8) -> &mut Self {
        unimplemented!();
    }

    fn with_owner_local_id(&mut self, _local_id: u32) -> &mut Self {
        unimplemented!();
    }

    fn with_uncompressed_data_size(&mut self, _data_size: u64) -> &mut Self {
        unimplemented!();
    }

    fn with_intra_block_offset(&mut self, _offset: u16) -> &mut Self {
        unimplemented!();
    }

    fn with_flags(&mut self, _flags: u32) -> &mut Self {
        unimplemented!();
    }

    fn with_optional_fields(&mut self, _fields: &AccountMetaOptionalFields) -> &mut Self {
        unimplemented!();
    }

    fn lamports(&self) -> u64;
    fn block_offset(&self) -> u64;
    fn set_block_offset(&mut self, offset: u64);
    fn padding_bytes(&self) -> u8;
    fn uncompressed_data_size(&self) -> usize {
        unimplemented!();
    }
    fn intra_block_offset(&self) -> u16;
    fn owner_local_id(&self) -> u32;
    fn flags_get(&self, bit_field: u32) -> bool;
    fn rent_epoch(&self, data_block: &[u8]) -> Option<Epoch>;
    fn account_hash<'a>(&self, data_block: &'a [u8]) -> &'a Hash;
    fn write_version(&self, data_block: &[u8]) -> Option<StoredMetaWriteVersion>;
    fn optional_fields_size(&self) -> usize {
        let mut size_in_bytes = 0;
        if self.flags_get(AccountMetaFlags::HAS_RENT_EPOCH) {
            size_in_bytes += size_of::<Epoch>();
        }
        if self.flags_get(AccountMetaFlags::HAS_ACCOUNT_HASH) {
            size_in_bytes += size_of::<Hash>();
        }
        if self.flags_get(AccountMetaFlags::HAS_WRITE_VERSION) {
            size_in_bytes += size_of::<StoredMetaWriteVersion>();
        }
        if self.flags_get(AccountMetaFlags::HAS_DATA_LENGTH) {
            size_in_bytes += size_of::<u64>();
        }

        size_in_bytes
    }

    fn optional_fields_offset<'a>(&self, data_block: &'a [u8]) -> usize;
    fn data_len(&self, data_block: &[u8]) -> usize;
    fn account_data<'a>(&self, data_block: &'a [u8]) -> &'a [u8];
    fn is_blob_account(&self) -> bool;
    fn write_account_meta_entry(&self, ads_file: &TieredStorageFile) -> std::io::Result<usize>;
    fn stored_size(
        footer: &TieredStorageFooter,
        metas: &Vec<impl TieredAccountMeta>,
        i: usize,
    ) -> usize;
}

#[cfg(test)]
pub mod tests {
    use crate::tiered_storage::meta::AccountMetaFlags;

    impl AccountMetaFlags {
        pub(crate) fn get_test(&self, bit_field: u32) -> bool {
            (self.flags & bit_field) > 0
        }
    }

    #[test]
    fn test_flags() {
        let mut flags = AccountMetaFlags::new();
        assert_eq!(flags.get_test(AccountMetaFlags::EXECUTABLE), false);
        assert_eq!(flags.get_test(AccountMetaFlags::HAS_RENT_EPOCH), false);

        flags.set(AccountMetaFlags::EXECUTABLE, true);
        assert_eq!(flags.get_test(AccountMetaFlags::EXECUTABLE), true);
        assert_eq!(flags.get_test(AccountMetaFlags::HAS_RENT_EPOCH), false);

        flags.set(AccountMetaFlags::HAS_RENT_EPOCH, true);
        assert_eq!(flags.get_test(AccountMetaFlags::EXECUTABLE), true);
        assert_eq!(flags.get_test(AccountMetaFlags::HAS_RENT_EPOCH), true);

        flags.set(AccountMetaFlags::EXECUTABLE, false);
        assert_eq!(flags.get_test(AccountMetaFlags::EXECUTABLE), false);
        assert_eq!(flags.get_test(AccountMetaFlags::HAS_RENT_EPOCH), true);

        flags.set(AccountMetaFlags::HAS_RENT_EPOCH, false);
        assert_eq!(flags.get_test(AccountMetaFlags::EXECUTABLE), false);
        assert_eq!(flags.get_test(AccountMetaFlags::HAS_RENT_EPOCH), false);
    }
}

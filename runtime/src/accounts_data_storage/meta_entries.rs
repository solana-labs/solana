use {
    crate::{
        accounts_data_storage::{
            error::AccountsDataStorageError, file::AccountsDataStorageFile, AccountDataBlockWriter,
        },
        append_vec::StoredMetaWriteVersion,
    },
    ::solana_sdk::{hash::Hash, stake_history::Epoch},
    serde::{Deserialize, Serialize},
    std::mem::size_of,
};

pub const ACCOUNT_META_ENTRY_SIZE_BYTES: u32 = 32;
pub const ACCOUNT_DATA_ENTIRE_BLOCK: u16 = std::u16::MAX;

type Result<T> = std::result::Result<T, AccountsDataStorageError>;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct AccountMetaFlags {
    flags: u64,
}

lazy_static! {
    pub static ref DEFAULT_ACCOUNT_HASH: Hash = Hash::default();
}

impl AccountMetaFlags {
    pub const EXECUTABLE: u64 = 1u64;
    pub const HAS_RENT_EPOCH: u64 = 1u64 << 1;
    pub const HAS_ACCOUNT_HASH: u64 = 1u64 << 2;
    pub const HAS_WRITE_VERSION: u64 = 1u64 << 3;
    pub const HAS_DATA_LENGTH: u64 = 1u64 << 4;

    pub fn new() -> Self {
        Self { flags: 0 }
    }

    pub fn new_from(value: u64) -> Self {
        Self { flags: value }
    }

    pub fn with_bit(mut self, bit_field: u64, value: bool) -> Self {
        self.set(bit_field, value);

        self
    }

    pub fn to_value(self) -> u64 {
        self.flags
    }

    pub fn set(&mut self, bit_field: u64, value: bool) {
        if value == true {
            self.flags |= bit_field;
        } else {
            self.flags &= !bit_field;
        }
    }

    pub fn get(flags: &u64, bit_field: u64) -> bool {
        (flags & bit_field) > 0
    }

    pub fn get_value(&self) -> u64 {
        self.flags
    }

    pub fn get_value_mut(&mut self) -> &mut u64 {
        &mut self.flags
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct AccountMetaStorageEntry {
    pub lamports: u64,
    pub block_offset: u64,
    pub uncompressed_data_size: u16,
    pub intra_block_offset: u16,
    pub owner_local_id: u32,
    pub flags: u64,
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
    pub fn update_flags(&self, flags_value: &mut u64) {
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

    pub fn write(&self, writer: &mut AccountDataBlockWriter) -> Result<usize> {
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

impl AccountMetaStorageEntry {
    pub fn new() -> Self {
        Self {
            ..AccountMetaStorageEntry::default()
        }
    }

    pub fn with_lamports(mut self, lamports: u64) -> Self {
        self.lamports = lamports;
        self
    }

    pub fn with_block_offset(mut self, offset: u64) -> Self {
        self.block_offset = offset;
        self
    }

    pub fn with_owner_local_id(mut self, local_id: u32) -> Self {
        self.owner_local_id = local_id;
        self
    }

    pub fn with_uncompressed_data_size(mut self, data_size: u16) -> Self {
        self.uncompressed_data_size = data_size;
        self
    }

    pub fn with_intra_block_offset(mut self, offset: u16) -> Self {
        self.intra_block_offset = offset;
        self
    }

    pub fn with_flags(mut self, flags: u64) -> Self {
        self.flags = flags;
        self
    }

    pub fn with_optional_fields(mut self, fields: &AccountMetaOptionalFields) -> Self {
        fields.update_flags(&mut self.flags);

        self
    }

    pub fn flags_get(&self, bit_field: u64) -> bool {
        AccountMetaFlags::get(&self.flags, bit_field)
    }

    pub fn account_data<'a>(&self, data_block: &'a [u8]) -> &'a [u8] {
        &data_block[(self.intra_block_offset as usize)
            ..(self.intra_block_offset + self.uncompressed_data_size) as usize]
    }

    pub fn rent_epoch(&self, data_block: &[u8]) -> Option<Epoch> {
        let offset = self.optional_fields_offset(data_block);
        if self.flags_get(AccountMetaFlags::HAS_RENT_EPOCH) {
            unsafe {
                let unaligned =
                    std::ptr::addr_of!(data_block[offset..offset + std::mem::size_of::<Epoch>()])
                        as *const Epoch;
                return Some(std::ptr::read_unaligned(unaligned));
            }
        }
        None
    }

    pub fn account_hash<'a>(&self, data_block: &'a [u8]) -> &'a Hash {
        let mut offset = self.optional_fields_offset(data_block);
        if self.flags_get(AccountMetaFlags::HAS_RENT_EPOCH) {
            offset += std::mem::size_of::<Epoch>();
        }
        if self.flags_get(AccountMetaFlags::HAS_ACCOUNT_HASH) {
            unsafe {
                let raw_ptr = std::slice::from_raw_parts(
                    data_block[offset..offset + std::mem::size_of::<Hash>()].as_ptr() as *const u8,
                    std::mem::size_of::<Hash>(),
                );
                let ptr: *const Hash = raw_ptr.as_ptr() as *const Hash;
                return &*ptr;
            }
        }
        return &DEFAULT_ACCOUNT_HASH;
    }

    pub fn write_version(&self, data_block: &[u8]) -> Option<StoredMetaWriteVersion> {
        let mut offset = self.optional_fields_offset(data_block);
        if self.flags_get(AccountMetaFlags::HAS_RENT_EPOCH) {
            offset += std::mem::size_of::<Epoch>();
        }
        if self.flags_get(AccountMetaFlags::HAS_ACCOUNT_HASH) {
            offset += std::mem::size_of::<Hash>();
        }
        if self.flags_get(AccountMetaFlags::HAS_WRITE_VERSION) {
            unsafe {
                let unaligned = std::ptr::addr_of!(
                    data_block[offset..offset + std::mem::size_of::<StoredMetaWriteVersion>()]
                ) as *const StoredMetaWriteVersion;
                return Some(std::ptr::read_unaligned(unaligned));
            }
        }
        None
    }

    pub fn data_length(&self, data_block: &[u8]) -> Option<u64> {
        let mut offset = self.optional_fields_offset(data_block);
        if self.flags_get(AccountMetaFlags::HAS_RENT_EPOCH) {
            offset += std::mem::size_of::<Epoch>();
        }
        if self.flags_get(AccountMetaFlags::HAS_ACCOUNT_HASH) {
            offset += std::mem::size_of::<Hash>();
        }
        if self.flags_get(AccountMetaFlags::HAS_WRITE_VERSION) {
            offset += std::mem::size_of::<StoredMetaWriteVersion>();
        }
        if self.flags_get(AccountMetaFlags::HAS_DATA_LENGTH) {
            unsafe {
                let unaligned =
                    std::ptr::addr_of!(data_block[offset..offset + std::mem::size_of::<u64>()])
                        as *const u64;
                return Some(std::ptr::read_unaligned(unaligned));
            }
        }
        None
    }

    pub fn optional_fields_size(&self) -> usize {
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

    fn optional_fields_offset<'a>(&self, data_block: &'a [u8]) -> usize {
        if self.is_blob_account() {
            return data_block.len().saturating_sub(self.optional_fields_size());
        }
        (self.intra_block_offset + self.uncompressed_data_size) as usize
    }

    pub fn get_account_data<'a>(&self, data_block: &'a Vec<u8>) -> &'a [u8] {
        &data_block[(self.intra_block_offset as usize)..self.optional_fields_offset(data_block)]
    }

    pub fn is_blob_account(&self) -> bool {
        self.uncompressed_data_size == ACCOUNT_DATA_ENTIRE_BLOCK && self.intra_block_offset == 0
    }

    pub fn write_account_meta_entry(&self, ads_file: &AccountsDataStorageFile) -> Result<usize> {
        ads_file.write_type(self)?;

        Ok(std::mem::size_of::<AccountMetaStorageEntry>())
    }

    pub fn new_from_file(ads_file: &AccountsDataStorageFile) -> Result<Self> {
        let mut entry = AccountMetaStorageEntry::new();
        ads_file.read_type(&mut entry)?;

        Ok(entry)
    }
}

impl Default for AccountMetaStorageEntry {
    fn default() -> Self {
        Self {
            lamports: 0,
            block_offset: 0,
            owner_local_id: 0,
            uncompressed_data_size: 0,
            intra_block_offset: 0,
            flags: AccountMetaFlags::new().to_value(),
        }
    }
}

#[cfg(test)]
pub mod tests {
    use {
        crate::{
            accounts_data_storage::{
                file::AccountsDataStorageFile,
                meta_entries::{
                    AccountMetaFlags, AccountMetaOptionalFields, AccountMetaStorageEntry,
                },
            },
            append_vec::{test_utils::get_append_vec_path, StoredMetaWriteVersion},
        },
        ::solana_sdk::{hash::Hash, stake_history::Epoch},
        memoffset::offset_of,
    };

    impl AccountMetaFlags {
        pub(crate) fn get_test(&self, bit_field: u64) -> bool {
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

    #[test]
    fn test_account_meta_entry() {
        let path = get_append_vec_path("test_account_meta_entry");

        const TEST_LAMPORT: u64 = 7;
        const BLOCK_OFFSET: u64 = 56987;
        const OWNER_LOCAL_ID: u32 = 54;
        const UNCOMPRESSED_LENGTH: u16 = 0;
        const LOCAL_OFFSET: u16 = 82;
        const TEST_RENT_EPOCH: Epoch = 7;
        const TEST_WRITE_VERSION: StoredMetaWriteVersion = 0;

        let optional_fields = AccountMetaOptionalFields {
            rent_epoch: Some(TEST_RENT_EPOCH),
            account_hash: Some(Hash::new_unique()),
            write_version_obsolete: Some(TEST_WRITE_VERSION),
        };

        let expected_entry = AccountMetaStorageEntry::new()
            .with_lamports(TEST_LAMPORT)
            .with_block_offset(BLOCK_OFFSET)
            .with_owner_local_id(OWNER_LOCAL_ID)
            .with_uncompressed_data_size(UNCOMPRESSED_LENGTH)
            .with_intra_block_offset(LOCAL_OFFSET)
            .with_flags(
                AccountMetaFlags::new()
                    .with_bit(AccountMetaFlags::EXECUTABLE, true)
                    .to_value(),
            )
            .with_optional_fields(&optional_fields);

        {
            let mut ads_file = AccountsDataStorageFile::new(&path.path, true);
            expected_entry
                .write_account_meta_entry(&mut ads_file)
                .unwrap();
        }

        let mut ads_file = AccountsDataStorageFile::new(&path.path, true);
        let entry = AccountMetaStorageEntry::new_from_file(&mut ads_file).unwrap();

        assert_eq!(expected_entry, entry);
        assert_eq!(entry.flags_get(AccountMetaFlags::EXECUTABLE), true);
        assert_eq!(entry.flags_get(AccountMetaFlags::HAS_RENT_EPOCH), true);
    }

    #[test]
    fn test_meta_entry_layout() {
        assert_eq!(offset_of!(AccountMetaStorageEntry, lamports), 0x00);
        assert_eq!(offset_of!(AccountMetaStorageEntry, block_offset), 0x08);
        assert_eq!(
            offset_of!(AccountMetaStorageEntry, uncompressed_data_size),
            0x10
        );
        assert_eq!(
            offset_of!(AccountMetaStorageEntry, intra_block_offset),
            0x12
        );
        assert_eq!(offset_of!(AccountMetaStorageEntry, owner_local_id), 0x14);
        assert_eq!(offset_of!(AccountMetaStorageEntry, flags), 0x18);
    }
}

use {
    crate::tiered_storage::{
        error::TieredStorageError, file::TieredStorageFile, index::AccountIndexFormat,
        mmap_utils::get_type, TieredStorageResult as TsResult,
    },
    memmap2::Mmap,
    solana_sdk::{hash::Hash, pubkey::Pubkey},
    std::{mem, path::Path},
};

pub const FOOTER_FORMAT_VERSION: u64 = 1;

/// The size of the footer struct + the magic number at the end.
pub const FOOTER_SIZE: usize =
    mem::size_of::<TieredStorageFooter>() + mem::size_of::<TieredStorageMagicNumber>();
static_assertions::const_assert_eq!(mem::size_of::<TieredStorageFooter>(), 160);

/// The size of the ending part of the footer.  This size should remain unchanged
/// even when the footer's format changes.
pub const FOOTER_TAIL_SIZE: usize = 24;

/// The ending 8 bytes of a valid tiered account storage file.
pub const FOOTER_MAGIC_NUMBER: u64 = 0x502A2AB5; // SOLALABS -> SOLANA LABS

#[derive(Debug, PartialEq, Eq)]
#[repr(C)]
pub struct TieredStorageMagicNumber(pub u64);

impl Default for TieredStorageMagicNumber {
    fn default() -> Self {
        Self(FOOTER_MAGIC_NUMBER)
    }
}

#[repr(u16)]
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Eq,
    Hash,
    PartialEq,
    num_enum::IntoPrimitive,
    num_enum::TryFromPrimitive,
)]
pub enum AccountMetaFormat {
    #[default]
    Hot = 0,
}

#[repr(u16)]
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Eq,
    Hash,
    PartialEq,
    num_enum::IntoPrimitive,
    num_enum::TryFromPrimitive,
)]
pub enum AccountBlockFormat {
    #[default]
    AlignedRaw = 0,
    Lz4 = 1,
}

#[repr(u16)]
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Eq,
    Hash,
    PartialEq,
    num_enum::IntoPrimitive,
    num_enum::TryFromPrimitive,
)]
pub enum OwnersBlockFormat {
    #[default]
    LocalIndex = 0,
}

#[derive(Debug, PartialEq, Eq, Clone)]
#[repr(C)]
pub struct TieredStorageFooter {
    // formats
    /// The format of the account meta entry.
    pub account_meta_format: AccountMetaFormat,
    /// The format of the owners block.
    pub owners_block_format: OwnersBlockFormat,
    /// The format of the account index block.
    pub account_index_format: AccountIndexFormat,
    /// The format of the account block.
    pub account_block_format: AccountBlockFormat,

    // Account-block related
    /// The number of account entries.
    pub account_entry_count: u32,
    /// The size of each account meta entry in bytes.
    pub account_meta_entry_size: u32,
    /// The default size of an account block before compression.
    ///
    /// If the size of one account (meta + data + optional fields) before
    /// compression is bigger than this number, than it is considered a
    /// blob account and it will have its own account block.
    pub account_block_size: u64,

    // Owner-related
    /// The number of owners.
    pub owner_count: u32,
    /// The size of each owner entry.
    pub owner_entry_size: u32,

    // Offsets
    // Note that offset to the account blocks is omitted as it's always 0.
    /// The offset pointing to the first byte of the account index block.
    pub account_index_offset: u64,
    /// The offset pointing to the first byte of the owners block.
    pub owners_offset: u64,

    // account range
    /// The smallest account address in this file.
    pub min_account_address: Pubkey,
    /// The largest account address in this file.
    pub max_account_address: Pubkey,

    /// A hash that represents a tiered accounts file for consistency check.
    pub hash: Hash,

    // The below fields belong to footer tail.
    // The sum of their sizes should match FOOTER_TAIL_SIZE.
    /// The size of the footer including the magic number.
    pub footer_size: u64,
    /// The format version of the tiered accounts file.
    pub format_version: u64,
    // This field is persisted in the storage but not in this struct.
    // The number should match FOOTER_MAGIC_NUMBER.
    // pub magic_number: u64,
}

impl Default for TieredStorageFooter {
    fn default() -> Self {
        Self {
            account_meta_format: AccountMetaFormat::default(),
            owners_block_format: OwnersBlockFormat::default(),
            account_index_format: AccountIndexFormat::default(),
            account_block_format: AccountBlockFormat::default(),
            account_entry_count: 0,
            account_meta_entry_size: 0,
            account_block_size: 0,
            owner_count: 0,
            owner_entry_size: 0,
            account_index_offset: 0,
            owners_offset: 0,
            hash: Hash::new_unique(),
            min_account_address: Pubkey::default(),
            max_account_address: Pubkey::default(),
            footer_size: FOOTER_SIZE as u64,
            format_version: FOOTER_FORMAT_VERSION,
        }
    }
}

impl TieredStorageFooter {
    pub fn new_from_path(path: impl AsRef<Path>) -> TsResult<Self> {
        let file = TieredStorageFile::new_readonly(path);
        Self::new_from_footer_block(&file)
    }

    pub fn write_footer_block(&self, file: &TieredStorageFile) -> TsResult<()> {
        file.write_type(self)?;
        file.write_type(&TieredStorageMagicNumber::default())?;

        Ok(())
    }

    pub fn new_from_footer_block(file: &TieredStorageFile) -> TsResult<Self> {
        let mut footer_size: u64 = 0;
        let mut footer_version: u64 = 0;
        let mut magic_number = TieredStorageMagicNumber(0);

        file.seek_from_end(-(FOOTER_TAIL_SIZE as i64))?;
        file.read_type(&mut footer_size)?;
        file.read_type(&mut footer_version)?;
        file.read_type(&mut magic_number)?;

        if magic_number != TieredStorageMagicNumber::default() {
            return Err(TieredStorageError::MagicNumberMismatch(
                TieredStorageMagicNumber::default().0,
                magic_number.0,
            ));
        }

        let mut footer = Self::default();
        file.seek_from_end(-(footer_size as i64))?;
        file.read_type(&mut footer)?;

        Ok(footer)
    }

    pub fn new_from_mmap(map: &Mmap) -> TsResult<&TieredStorageFooter> {
        let offset = map.len().saturating_sub(FOOTER_TAIL_SIZE);
        let (footer_size, offset) = get_type::<u64>(map, offset)?;
        let (_footer_version, offset) = get_type::<u64>(map, offset)?;
        let (magic_number, _offset) = get_type::<TieredStorageMagicNumber>(map, offset)?;

        if *magic_number != TieredStorageMagicNumber::default() {
            return Err(TieredStorageError::MagicNumberMismatch(
                TieredStorageMagicNumber::default().0,
                magic_number.0,
            ));
        }

        let (footer, _offset) =
            get_type::<TieredStorageFooter>(map, map.len().saturating_sub(*footer_size as usize))?;

        Ok(footer)
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            append_vec::test_utils::get_append_vec_path, tiered_storage::file::TieredStorageFile,
        },
        memoffset::offset_of,
        solana_sdk::hash::Hash,
    };

    #[test]
    fn test_footer() {
        let path = get_append_vec_path("test_file_footer");
        let expected_footer = TieredStorageFooter {
            account_meta_format: AccountMetaFormat::Hot,
            owners_block_format: OwnersBlockFormat::LocalIndex,
            account_index_format: AccountIndexFormat::AddressAndOffset,
            account_block_format: AccountBlockFormat::AlignedRaw,
            account_entry_count: 300,
            account_meta_entry_size: 24,
            account_block_size: 4096,
            owner_count: 250,
            owner_entry_size: 32,
            account_index_offset: 1069600,
            owners_offset: 1081200,
            hash: Hash::new_unique(),
            min_account_address: Pubkey::default(),
            max_account_address: Pubkey::new_unique(),
            footer_size: FOOTER_SIZE as u64,
            format_version: FOOTER_FORMAT_VERSION,
        };

        // Persist the expected footer.
        {
            let file = TieredStorageFile::new_writable(&path.path);
            expected_footer.write_footer_block(&file).unwrap();
        }

        // Reopen the same storage, and expect the persisted footer is
        // the same as what we have written.
        {
            let footer = TieredStorageFooter::new_from_path(&path.path).unwrap();
            assert_eq!(expected_footer, footer);
        }
    }

    #[test]
    fn test_footer_layout() {
        assert_eq!(offset_of!(TieredStorageFooter, account_meta_format), 0x00);
        assert_eq!(offset_of!(TieredStorageFooter, owners_block_format), 0x02);
        assert_eq!(offset_of!(TieredStorageFooter, account_index_format), 0x04);
        assert_eq!(offset_of!(TieredStorageFooter, account_block_format), 0x06);
        assert_eq!(offset_of!(TieredStorageFooter, account_entry_count), 0x08);
        assert_eq!(
            offset_of!(TieredStorageFooter, account_meta_entry_size),
            0x0C
        );
        assert_eq!(offset_of!(TieredStorageFooter, account_block_size), 0x10);
        assert_eq!(offset_of!(TieredStorageFooter, owner_count), 0x18);
        assert_eq!(offset_of!(TieredStorageFooter, owner_entry_size), 0x1C);
        assert_eq!(offset_of!(TieredStorageFooter, account_index_offset), 0x20);
        assert_eq!(offset_of!(TieredStorageFooter, owners_offset), 0x28);
        assert_eq!(offset_of!(TieredStorageFooter, min_account_address), 0x30);
        assert_eq!(offset_of!(TieredStorageFooter, max_account_address), 0x50);
        assert_eq!(offset_of!(TieredStorageFooter, hash), 0x70);
        assert_eq!(offset_of!(TieredStorageFooter, footer_size), 0x90);
        assert_eq!(offset_of!(TieredStorageFooter, format_version), 0x98);
    }
}

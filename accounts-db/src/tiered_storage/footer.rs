use {
    crate::tiered_storage::{
        error::TieredStorageError,
        file::{TieredReadableFile, TieredStorageMagicNumber, TieredWritableFile},
        index::IndexBlockFormat,
        mmap_utils::{get_pod, get_type},
        owners::OwnersBlockFormat,
        TieredStorageResult,
    },
    bytemuck::Zeroable,
    memmap2::Mmap,
    num_enum::TryFromPrimitiveError,
    solana_sdk::{hash::Hash, pubkey::Pubkey},
    std::{mem, path::Path},
    thiserror::Error,
};

pub const FOOTER_FORMAT_VERSION: u64 = 1;

/// The size of the footer struct + the magic number at the end.
pub const FOOTER_SIZE: usize =
    mem::size_of::<TieredStorageFooter>() + mem::size_of::<TieredStorageMagicNumber>();
static_assertions::const_assert_eq!(mem::size_of::<TieredStorageFooter>(), 160);

/// The size of the ending part of the footer.  This size should remain unchanged
/// even when the footer's format changes.
pub const FOOTER_TAIL_SIZE: usize = 24;

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
    // Temporarily comment out to avoid unimplemented!() block
    // Cold = 1,
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

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
#[repr(C)]
pub struct TieredStorageFooter {
    // formats
    /// The format of the account meta entry.
    pub account_meta_format: AccountMetaFormat,
    /// The format of the owners block.
    pub owners_block_format: OwnersBlockFormat,
    /// The format of the account index block.
    pub index_block_format: IndexBlockFormat,
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
    pub index_block_offset: u64,
    /// The offset pointing to the first byte of the owners block.
    pub owners_block_offset: u64,

    // account range
    /// The smallest account address in this file.
    pub min_account_address: Pubkey,
    /// The largest account address in this file.
    pub max_account_address: Pubkey,

    /// A hash that represents a tiered accounts file for consistency check.
    pub hash: Hash,

    /// The format version of the tiered accounts file.
    pub format_version: u64,
    // The below fields belong to footer tail.
    // The sum of their sizes should match FOOTER_TAIL_SIZE.
    /// The size of the footer including the magic number.
    pub footer_size: u64,
    // This field is persisted in the storage but not in this struct.
    // The number should match FILE_MAGIC_NUMBER.
    // pub magic_number: u64,
}

// It is undefined behavior to read/write uninitialized bytes.
// The `Pod` marker trait indicates there are no uninitialized bytes.
// In order to safely guarantee a type is POD, it cannot have any padding.
const _: () = assert!(
    std::mem::size_of::<TieredStorageFooter>()
        == std::mem::size_of::<AccountMetaFormat>()
         + std::mem::size_of::<OwnersBlockFormat>()
         + std::mem::size_of::<IndexBlockFormat>()
         + std::mem::size_of::<AccountBlockFormat>()
         + std::mem::size_of::<u32>() // account_entry_count
         + std::mem::size_of::<u32>() // account_meta_entry_size
         + std::mem::size_of::<u64>() // account_block_size
         + std::mem::size_of::<u32>() // owner_count
         + std::mem::size_of::<u32>() // owner_entry_size
         + std::mem::size_of::<u64>() // index_block_offset
         + std::mem::size_of::<u64>() // owners_block_offset
         + std::mem::size_of::<Pubkey>() // min_account_address
         + std::mem::size_of::<Pubkey>() // max_account_address
         + std::mem::size_of::<Hash>() // hash
         + std::mem::size_of::<u64>() // format_version
         + std::mem::size_of::<u64>(), // footer_size
    "TieredStorageFooter cannot have any padding"
);

impl Default for TieredStorageFooter {
    fn default() -> Self {
        Self {
            account_meta_format: AccountMetaFormat::default(),
            owners_block_format: OwnersBlockFormat::default(),
            index_block_format: IndexBlockFormat::default(),
            account_block_format: AccountBlockFormat::default(),
            account_entry_count: 0,
            account_meta_entry_size: 0,
            account_block_size: 0,
            owner_count: 0,
            owner_entry_size: 0,
            index_block_offset: 0,
            owners_block_offset: 0,
            hash: Hash::new_unique(),
            min_account_address: Pubkey::default(),
            max_account_address: Pubkey::default(),
            format_version: FOOTER_FORMAT_VERSION,
            footer_size: FOOTER_SIZE as u64,
        }
    }
}

impl TieredStorageFooter {
    pub fn new_from_path(path: impl AsRef<Path>) -> TieredStorageResult<Self> {
        let file = TieredReadableFile::new(path)?;
        Self::new_from_footer_block(&file)
    }

    pub fn write_footer_block(&self, file: &mut TieredWritableFile) -> TieredStorageResult<()> {
        // SAFETY: The footer does not contain any uninitialized bytes.
        unsafe { file.write_type(self)? };
        file.write_pod(&TieredStorageMagicNumber::default())?;

        Ok(())
    }

    pub fn new_from_footer_block(file: &TieredReadableFile) -> TieredStorageResult<Self> {
        file.seek_from_end(-(FOOTER_TAIL_SIZE as i64))?;

        let mut footer_version: u64 = 0;
        file.read_pod(&mut footer_version)?;
        if footer_version != FOOTER_FORMAT_VERSION {
            return Err(TieredStorageError::InvalidFooterVersion(footer_version));
        }

        let mut footer_size: u64 = 0;
        file.read_pod(&mut footer_size)?;
        if footer_size != FOOTER_SIZE as u64 {
            return Err(TieredStorageError::InvalidFooterSize(
                footer_size,
                FOOTER_SIZE as u64,
            ));
        }

        let mut magic_number = TieredStorageMagicNumber::zeroed();
        file.read_pod(&mut magic_number)?;
        if magic_number != TieredStorageMagicNumber::default() {
            return Err(TieredStorageError::MagicNumberMismatch(
                TieredStorageMagicNumber::default().0,
                magic_number.0,
            ));
        }

        let mut footer = Self::default();
        file.seek_from_end(-(footer_size as i64))?;
        // SAFETY: We sanitize the footer to ensure all the bytes are
        // actually safe to interpret as a TieredStorageFooter.
        unsafe { file.read_type(&mut footer)? };
        Self::sanitize(&footer)?;

        Ok(footer)
    }

    pub fn new_from_mmap(mmap: &Mmap) -> TieredStorageResult<&TieredStorageFooter> {
        let offset = mmap.len().saturating_sub(FOOTER_TAIL_SIZE);

        let (footer_version, offset) = get_pod::<u64>(mmap, offset)?;
        if *footer_version != FOOTER_FORMAT_VERSION {
            return Err(TieredStorageError::InvalidFooterVersion(*footer_version));
        }

        let (&footer_size, offset) = get_pod::<u64>(mmap, offset)?;
        if footer_size != FOOTER_SIZE as u64 {
            return Err(TieredStorageError::InvalidFooterSize(
                footer_size,
                FOOTER_SIZE as u64,
            ));
        }

        let (magic_number, _offset) = get_pod::<TieredStorageMagicNumber>(mmap, offset)?;
        if *magic_number != TieredStorageMagicNumber::default() {
            return Err(TieredStorageError::MagicNumberMismatch(
                TieredStorageMagicNumber::default().0,
                magic_number.0,
            ));
        }

        let footer_offset = mmap.len().saturating_sub(footer_size as usize);
        // SAFETY: We sanitize the footer to ensure all the bytes are
        // actually safe to interpret as a TieredStorageFooter.
        let (footer, _offset) = unsafe { get_type::<TieredStorageFooter>(mmap, footer_offset)? };
        Self::sanitize(footer)?;

        Ok(footer)
    }

    /// Sanitizes the footer
    ///
    /// Since the various formats only have specific valid values, they must be sanitized
    /// prior to use.  This ensures the formats are valid to interpret as (rust) enums.
    fn sanitize(footer: &Self) -> Result<(), SanitizeFooterError> {
        let account_meta_format_u16 =
            unsafe { &*(&footer.account_meta_format as *const _ as *const u16) };
        let owners_block_format_u16 =
            unsafe { &*(&footer.owners_block_format as *const _ as *const u16) };
        let index_block_format_u16 =
            unsafe { &*(&footer.index_block_format as *const _ as *const u16) };
        let account_block_format_u16 =
            unsafe { &*(&footer.account_block_format as *const _ as *const u16) };

        _ = AccountMetaFormat::try_from(*account_meta_format_u16)
            .map_err(SanitizeFooterError::InvalidAccountMetaFormat)?;
        _ = OwnersBlockFormat::try_from(*owners_block_format_u16)
            .map_err(SanitizeFooterError::InvalidOwnersBlockFormat)?;
        _ = IndexBlockFormat::try_from(*index_block_format_u16)
            .map_err(SanitizeFooterError::InvalidIndexBlockFormat)?;
        _ = AccountBlockFormat::try_from(*account_block_format_u16)
            .map_err(SanitizeFooterError::InvalidAccountBlockFormat)?;

        // Since we just sanitized the formats within the footer,
        // it is now safe to read them as (rust) enums.
        //
        // from https://doc.rust-lang.org/reference/items/enumerations.html#casting:
        // > If an enumeration is unit-only (with no tuple and struct variants),
        // > then its discriminant can be directly accessed with a numeric cast;
        //
        // from https://doc.rust-lang.org/reference/items/enumerations.html#pointer-casting:
        // > If the enumeration specifies a primitive representation,
        // > then the discriminant may be reliably accessed via unsafe pointer casting
        Ok(())
    }
}

/// Errors that can happen while sanitizing the footer
#[derive(Error, Debug)]
pub enum SanitizeFooterError {
    #[error("invalid account meta format: {0}")]
    InvalidAccountMetaFormat(#[from] TryFromPrimitiveError<AccountMetaFormat>),

    #[error("invalid owners block format: {0}")]
    InvalidOwnersBlockFormat(#[from] TryFromPrimitiveError<OwnersBlockFormat>),

    #[error("invalid index block format: {0}")]
    InvalidIndexBlockFormat(#[from] TryFromPrimitiveError<IndexBlockFormat>),

    #[error("invalid account block format: {0}")]
    InvalidAccountBlockFormat(#[from] TryFromPrimitiveError<AccountBlockFormat>),
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            append_vec::test_utils::get_append_vec_path, tiered_storage::file::TieredWritableFile,
        },
        memoffset::offset_of,
        solana_sdk::hash::Hash,
    };

    #[test]
    fn test_footer() {
        let path = get_append_vec_path("test_file_footer");
        let expected_footer = TieredStorageFooter {
            account_meta_format: AccountMetaFormat::Hot,
            owners_block_format: OwnersBlockFormat::AddressesOnly,
            index_block_format: IndexBlockFormat::AddressesThenOffsets,
            account_block_format: AccountBlockFormat::AlignedRaw,
            account_entry_count: 300,
            account_meta_entry_size: 24,
            account_block_size: 4096,
            owner_count: 250,
            owner_entry_size: 32,
            index_block_offset: 1069600,
            owners_block_offset: 1081200,
            hash: Hash::new_unique(),
            min_account_address: Pubkey::default(),
            max_account_address: Pubkey::new_unique(),
            format_version: FOOTER_FORMAT_VERSION,
            footer_size: FOOTER_SIZE as u64,
        };

        // Persist the expected footer.
        {
            let mut file = TieredWritableFile::new(&path.path).unwrap();
            expected_footer.write_footer_block(&mut file).unwrap();
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
        assert_eq!(offset_of!(TieredStorageFooter, index_block_format), 0x04);
        assert_eq!(offset_of!(TieredStorageFooter, account_block_format), 0x06);
        assert_eq!(offset_of!(TieredStorageFooter, account_entry_count), 0x08);
        assert_eq!(
            offset_of!(TieredStorageFooter, account_meta_entry_size),
            0x0C
        );
        assert_eq!(offset_of!(TieredStorageFooter, account_block_size), 0x10);
        assert_eq!(offset_of!(TieredStorageFooter, owner_count), 0x18);
        assert_eq!(offset_of!(TieredStorageFooter, owner_entry_size), 0x1C);
        assert_eq!(offset_of!(TieredStorageFooter, index_block_offset), 0x20);
        assert_eq!(offset_of!(TieredStorageFooter, owners_block_offset), 0x28);
        assert_eq!(offset_of!(TieredStorageFooter, min_account_address), 0x30);
        assert_eq!(offset_of!(TieredStorageFooter, max_account_address), 0x50);
        assert_eq!(offset_of!(TieredStorageFooter, hash), 0x70);
        assert_eq!(offset_of!(TieredStorageFooter, format_version), 0x90);
        assert_eq!(offset_of!(TieredStorageFooter, footer_size), 0x98);
    }

    #[test]
    fn test_sanitize() {
        // test: all good
        {
            let footer = TieredStorageFooter::default();
            let result = TieredStorageFooter::sanitize(&footer);
            assert!(result.is_ok());
        }

        // test: bad account meta format
        {
            let mut footer = TieredStorageFooter::default();
            unsafe {
                std::ptr::write(
                    &mut footer.account_meta_format as *mut _ as *mut u16,
                    0xBAD0,
                );
            }
            let result = TieredStorageFooter::sanitize(&footer);
            assert!(matches!(
                result,
                Err(SanitizeFooterError::InvalidAccountMetaFormat(_))
            ));
        }

        // test: bad owners block format
        {
            let mut footer = TieredStorageFooter::default();
            unsafe {
                std::ptr::write(
                    &mut footer.owners_block_format as *mut _ as *mut u16,
                    0xBAD0,
                );
            }
            let result = TieredStorageFooter::sanitize(&footer);
            assert!(matches!(
                result,
                Err(SanitizeFooterError::InvalidOwnersBlockFormat(_))
            ));
        }

        // test: bad index block format
        {
            let mut footer = TieredStorageFooter::default();
            unsafe {
                std::ptr::write(&mut footer.index_block_format as *mut _ as *mut u16, 0xBAD0);
            }
            let result = TieredStorageFooter::sanitize(&footer);
            assert!(matches!(
                result,
                Err(SanitizeFooterError::InvalidIndexBlockFormat(_))
            ));
        }

        // test: bad account block format
        {
            let mut footer = TieredStorageFooter::default();
            unsafe {
                std::ptr::write(
                    &mut footer.account_block_format as *mut _ as *mut u16,
                    0xBAD0,
                );
            }
            let result = TieredStorageFooter::sanitize(&footer);
            assert!(matches!(
                result,
                Err(SanitizeFooterError::InvalidAccountBlockFormat(_))
            ));
        }
    }
}

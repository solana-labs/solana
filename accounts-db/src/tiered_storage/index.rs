use {
    crate::tiered_storage::{
        file::TieredWritableFile, footer::TieredStorageFooter, mmap_utils::get_pod,
        TieredStorageResult,
    },
    bytemuck::{Pod, Zeroable},
    memmap2::Mmap,
    solana_sdk::pubkey::Pubkey,
};

/// The in-memory struct for the writing index block.
#[derive(Debug)]
pub struct AccountIndexWriterEntry<'a, Offset: AccountOffset> {
    /// The account address.
    pub address: &'a Pubkey,
    /// The offset to the account.
    pub offset: Offset,
}

/// The offset to an account.
pub trait AccountOffset: Clone + Copy + Pod + Zeroable {}

/// The offset to an account/address entry in the accounts index block.
/// This can be used to obtain the AccountOffset and address by looking through
/// the accounts index block.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Pod, Zeroable)]
pub struct IndexOffset(pub u32);

// Ensure there are no implicit padding bytes
const _: () = assert!(std::mem::size_of::<IndexOffset>() == 4);

/// The index format of a tiered accounts file.
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
pub enum IndexBlockFormat {
    /// This format optimizes the storage size by storing only account addresses
    /// and block offsets.  It skips storing the size of account data by storing
    /// account block entries and index block entries in the same order.
    #[default]
    AddressesThenOffsets = 0,
}

// Ensure there are no implicit padding bytes
const _: () = assert!(std::mem::size_of::<IndexBlockFormat>() == 2);

impl IndexBlockFormat {
    /// Persists the specified index_entries to the specified file and returns
    /// the total number of bytes written.
    pub fn write_index_block(
        &self,
        file: &mut TieredWritableFile,
        index_entries: &[AccountIndexWriterEntry<impl AccountOffset>],
    ) -> TieredStorageResult<usize> {
        match self {
            Self::AddressesThenOffsets => {
                let mut bytes_written = 0;
                for index_entry in index_entries {
                    bytes_written += file.write_pod(index_entry.address)?;
                }
                for index_entry in index_entries {
                    bytes_written += file.write_pod(&index_entry.offset)?;
                }
                Ok(bytes_written)
            }
        }
    }

    /// Returns the address of the account given the specified index.
    pub fn get_account_address<'a>(
        &self,
        mmap: &'a Mmap,
        footer: &TieredStorageFooter,
        index_offset: IndexOffset,
    ) -> TieredStorageResult<&'a Pubkey> {
        let offset = match self {
            Self::AddressesThenOffsets => {
                debug_assert!(index_offset.0 < footer.account_entry_count);
                footer.index_block_offset as usize
                    + std::mem::size_of::<Pubkey>() * (index_offset.0 as usize)
            }
        };

        debug_assert!(
            offset.saturating_add(std::mem::size_of::<Pubkey>())
                <= footer.owners_block_offset as usize,
            "reading IndexOffset ({}) would exceed index block boundary ({}).",
            offset,
            footer.owners_block_offset,
        );

        let (address, _) = get_pod::<Pubkey>(mmap, offset)?;
        Ok(address)
    }

    /// Returns the offset to the account given the specified index.
    pub fn get_account_offset<Offset: AccountOffset>(
        &self,
        mmap: &Mmap,
        footer: &TieredStorageFooter,
        index_offset: IndexOffset,
    ) -> TieredStorageResult<Offset> {
        let offset = match self {
            Self::AddressesThenOffsets => {
                debug_assert!(index_offset.0 < footer.account_entry_count);
                footer.index_block_offset as usize
                    + std::mem::size_of::<Pubkey>() * footer.account_entry_count as usize
                    + std::mem::size_of::<Offset>() * index_offset.0 as usize
            }
        };

        debug_assert!(
            offset.saturating_add(std::mem::size_of::<Offset>())
                <= footer.owners_block_offset as usize,
            "reading IndexOffset ({}) would exceed index block boundary ({}).",
            offset,
            footer.owners_block_offset,
        );

        let (account_offset, _) = get_pod::<Offset>(mmap, offset)?;

        Ok(*account_offset)
    }

    /// Returns the size of one index entry.
    pub fn entry_size<Offset: AccountOffset>(&self) -> usize {
        match self {
            Self::AddressesThenOffsets => {
                std::mem::size_of::<Pubkey>() + std::mem::size_of::<Offset>()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::tiered_storage::{
            file::TieredWritableFile,
            hot::{HotAccountOffset, HOT_ACCOUNT_ALIGNMENT},
        },
        memmap2::MmapOptions,
        rand::Rng,
        std::fs::OpenOptions,
        tempfile::TempDir,
    };

    #[test]
    fn test_address_and_offset_indexer() {
        const ENTRY_COUNT: usize = 100;
        let mut footer = TieredStorageFooter {
            account_entry_count: ENTRY_COUNT as u32,
            ..TieredStorageFooter::default()
        };
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_address_and_offset_indexer");
        let addresses: Vec<_> = std::iter::repeat_with(Pubkey::new_unique)
            .take(ENTRY_COUNT)
            .collect();
        let mut rng = rand::thread_rng();
        let index_entries: Vec<_> = addresses
            .iter()
            .map(|address| AccountIndexWriterEntry {
                address,
                offset: HotAccountOffset::new(
                    rng.gen_range(0..u32::MAX) as usize * HOT_ACCOUNT_ALIGNMENT,
                )
                .unwrap(),
            })
            .collect();

        {
            let mut file = TieredWritableFile::new(&path).unwrap();
            let indexer = IndexBlockFormat::AddressesThenOffsets;
            let cursor = indexer
                .write_index_block(&mut file, &index_entries)
                .unwrap();
            footer.owners_block_offset = cursor as u64;
        }

        let indexer = IndexBlockFormat::AddressesThenOffsets;
        let file = OpenOptions::new()
            .read(true)
            .create(false)
            .open(&path)
            .unwrap();
        let mmap = unsafe { MmapOptions::new().map(&file).unwrap() };
        for (i, index_entry) in index_entries.iter().enumerate() {
            let account_offset = indexer
                .get_account_offset::<HotAccountOffset>(&mmap, &footer, IndexOffset(i as u32))
                .unwrap();
            assert_eq!(index_entry.offset, account_offset);
            let address = indexer
                .get_account_address(&mmap, &footer, IndexOffset(i as u32))
                .unwrap();
            assert_eq!(index_entry.address, address);
        }
    }

    #[test]
    #[should_panic(expected = "index_offset.0 < footer.account_entry_count")]
    fn test_get_account_address_out_of_bounds() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir
            .path()
            .join("test_get_account_address_out_of_bounds");

        let footer = TieredStorageFooter {
            account_entry_count: 100,
            index_block_format: IndexBlockFormat::AddressesThenOffsets,
            ..TieredStorageFooter::default()
        };

        {
            // we only write a footer here as the test should hit an assert
            // failure before it actually reads the file.
            let mut file = TieredWritableFile::new(&path).unwrap();
            footer.write_footer_block(&mut file).unwrap();
        }

        let file = OpenOptions::new()
            .read(true)
            .create(false)
            .open(&path)
            .unwrap();
        let mmap = unsafe { MmapOptions::new().map(&file).unwrap() };
        footer
            .index_block_format
            .get_account_address(&mmap, &footer, IndexOffset(footer.account_entry_count))
            .unwrap();
    }

    #[test]
    #[should_panic(expected = "would exceed index block boundary")]
    fn test_get_account_address_exceeds_index_block_boundary() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir
            .path()
            .join("test_get_account_address_exceeds_index_block_boundary");

        let footer = TieredStorageFooter {
            account_entry_count: 100,
            index_block_format: IndexBlockFormat::AddressesThenOffsets,
            index_block_offset: 1024,
            // only holds one index entry
            owners_block_offset: 1024 + std::mem::size_of::<HotAccountOffset>() as u64,
            ..TieredStorageFooter::default()
        };

        {
            // we only write a footer here as the test should hit an assert
            // failure before it actually reads the file.
            let mut file = TieredWritableFile::new(&path).unwrap();
            footer.write_footer_block(&mut file).unwrap();
        }

        let file = OpenOptions::new()
            .read(true)
            .create(false)
            .open(&path)
            .unwrap();
        let mmap = unsafe { MmapOptions::new().map(&file).unwrap() };
        // IndexOffset does not exceed the account_entry_count but exceeds
        // the index block boundary.
        footer
            .index_block_format
            .get_account_address(&mmap, &footer, IndexOffset(2))
            .unwrap();
    }

    #[test]
    #[should_panic(expected = "index_offset.0 < footer.account_entry_count")]
    fn test_get_account_offset_out_of_bounds() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir
            .path()
            .join("test_get_account_offset_out_of_bounds");

        let footer = TieredStorageFooter {
            account_entry_count: 100,
            index_block_format: IndexBlockFormat::AddressesThenOffsets,
            ..TieredStorageFooter::default()
        };

        {
            // we only write a footer here as the test should hit an assert
            // failure before we actually read the file.
            let mut file = TieredWritableFile::new(&path).unwrap();
            footer.write_footer_block(&mut file).unwrap();
        }

        let file = OpenOptions::new()
            .read(true)
            .create(false)
            .open(&path)
            .unwrap();
        let mmap = unsafe { MmapOptions::new().map(&file).unwrap() };
        footer
            .index_block_format
            .get_account_offset::<HotAccountOffset>(
                &mmap,
                &footer,
                IndexOffset(footer.account_entry_count),
            )
            .unwrap();
    }

    #[test]
    #[should_panic(expected = "would exceed index block boundary")]
    fn test_get_account_offset_exceeds_index_block_boundary() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir
            .path()
            .join("test_get_account_offset_exceeds_index_block_boundary");

        let footer = TieredStorageFooter {
            account_entry_count: 100,
            index_block_format: IndexBlockFormat::AddressesThenOffsets,
            index_block_offset: 1024,
            // only holds one index entry
            owners_block_offset: 1024 + std::mem::size_of::<HotAccountOffset>() as u64,
            ..TieredStorageFooter::default()
        };

        {
            // we only write a footer here as the test should hit an assert
            // failure before we actually read the file.
            let mut file = TieredWritableFile::new(&path).unwrap();
            footer.write_footer_block(&mut file).unwrap();
        }

        let file = OpenOptions::new()
            .read(true)
            .create(false)
            .open(&path)
            .unwrap();
        let mmap = unsafe { MmapOptions::new().map(&file).unwrap() };
        // IndexOffset does not exceed the account_entry_count but exceeds
        // the index block boundary.
        footer
            .index_block_format
            .get_account_offset::<HotAccountOffset>(&mmap, &footer, IndexOffset(2))
            .unwrap();
    }
}

use {
    crate::tiered_storage::{
        file::TieredStorageFile, footer::TieredStorageFooter, mmap_utils::get_type,
        TieredStorageResult,
    },
    memmap2::Mmap,
    solana_sdk::pubkey::Pubkey,
};

/// The in-memory struct for the writing index block.
/// The actual storage format of a tiered account index entry might be different
/// from this.
#[derive(Debug)]
pub struct AccountIndexWriterEntry<'a> {
    pub address: &'a Pubkey,
    pub block_offset: u64,
    pub intra_block_offset: u64,
}

/// The offset to an account stored inside its accounts block.
/// This struct is used to access the meta and data of an account by looking through
/// its accounts block.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccountOffset {
    /// The offset to the accounts block that contains the account meta/data.
    pub block: usize,
}

/// The offset to an account/address entry in the accounts index block.
/// This can be used to obtain the AccountOffset and address by looking through
/// the accounts index block.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IndexOffset(pub usize);

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
    /// and offsets.  It skips storing the size of account data by storing account
    /// block entries and index block entries in the same order.
    #[default]
    AddressAndOffset = 0,
}

impl IndexBlockFormat {
    /// Persists the specified index_entries to the specified file and returns
    /// the total number of bytes written.
    pub fn write_index_block(
        &self,
        file: &TieredStorageFile,
        index_entries: &[AccountIndexWriterEntry],
    ) -> TieredStorageResult<usize> {
        match self {
            Self::AddressAndOffset => {
                let mut bytes_written = 0;
                for index_entry in index_entries {
                    bytes_written += file.write_type(index_entry.address)?;
                }
                for index_entry in index_entries {
                    bytes_written += file.write_type(&index_entry.block_offset)?;
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
        let account_offset = match self {
            Self::AddressAndOffset => {
                footer.index_block_offset as usize + std::mem::size_of::<Pubkey>() * index_offset.0
            }
        };
        let (address, _) = get_type::<Pubkey>(mmap, account_offset)?;
        Ok(address)
    }

    /// Returns the offset to the account given the specified index.
    pub fn get_account_offset(
        &self,
        mmap: &Mmap,
        footer: &TieredStorageFooter,
        index_offset: IndexOffset,
    ) -> TieredStorageResult<AccountOffset> {
        match self {
            Self::AddressAndOffset => {
                let account_offset = footer.index_block_offset as usize
                    + std::mem::size_of::<Pubkey>() * footer.account_entry_count as usize
                    + index_offset.0 * std::mem::size_of::<u64>();
                let (account_block_offset, _) = get_type(mmap, account_offset)?;
                Ok(AccountOffset {
                    block: *account_block_offset,
                })
            }
        }
    }

    /// Returns the size of one index entry.
    pub fn entry_size(&self) -> usize {
        match self {
            Self::AddressAndOffset => std::mem::size_of::<Pubkey>() + std::mem::size_of::<u64>(),
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*, crate::tiered_storage::file::TieredStorageFile, memmap2::MmapOptions, rand::Rng,
        std::fs::OpenOptions, tempfile::TempDir,
    };

    #[test]
    fn test_address_and_offset_indexer() {
        const ENTRY_COUNT: usize = 100;
        let footer = TieredStorageFooter {
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
                block_offset: rng.gen_range(128..2048),
                intra_block_offset: 0,
            })
            .collect();

        {
            let file = TieredStorageFile::new_writable(&path).unwrap();
            let indexer = IndexBlockFormat::AddressAndOffset;
            indexer.write_index_block(&file, &index_entries).unwrap();
        }

        let indexer = IndexBlockFormat::AddressAndOffset;
        let file = OpenOptions::new()
            .read(true)
            .create(false)
            .open(&path)
            .unwrap();
        let mmap = unsafe { MmapOptions::new().map(&file).unwrap() };
        for (i, index_entry) in index_entries.iter().enumerate() {
            let account_offset = indexer
                .get_account_offset(&mmap, &footer, IndexOffset(i))
                .unwrap();
            assert_eq!(index_entry.block_offset, account_offset.block as u64);
            let address = indexer
                .get_account_address(&mmap, &footer, IndexOffset(i))
                .unwrap();
            assert_eq!(index_entry.address, address);
        }
    }
}

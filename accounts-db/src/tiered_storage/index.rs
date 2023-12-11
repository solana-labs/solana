use {
    crate::tiered_storage::{
        file::TieredStorageFile, footer::TieredStorageFooter, mmap_utils::get_type,
        TieredStorageResult,
    },
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
pub trait AccountOffset {}

/// The offset to an account/address entry in the accounts index block.
/// This can be used to obtain the AccountOffset and address by looking through
/// the accounts index block.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IndexOffset(pub u32);

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
    AddressAndBlockOffsetOnly = 0,
}

impl IndexBlockFormat {
    /// Persists the specified index_entries to the specified file and returns
    /// the total number of bytes written.
    pub fn write_index_block(
        &self,
        file: &TieredStorageFile,
        index_entries: &[AccountIndexWriterEntry<impl AccountOffset>],
    ) -> TieredStorageResult<usize> {
        match self {
            Self::AddressAndBlockOffsetOnly => {
                let mut bytes_written = 0;
                for index_entry in index_entries {
                    bytes_written += file.write_type(index_entry.address)?;
                }
                for index_entry in index_entries {
                    bytes_written += file.write_type(&index_entry.offset)?;
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
            Self::AddressAndBlockOffsetOnly => {
                footer.index_block_offset as usize
                    + std::mem::size_of::<Pubkey>() * (index_offset.0 as usize)
            }
        };
        let (address, _) = get_type::<Pubkey>(mmap, account_offset)?;
        Ok(address)
    }

    /// Returns the offset to the account given the specified index.
    pub fn get_account_offset<Offset: AccountOffset + Copy>(
        &self,
        mmap: &Mmap,
        footer: &TieredStorageFooter,
        index_offset: IndexOffset,
    ) -> TieredStorageResult<Offset> {
        match self {
            Self::AddressAndBlockOffsetOnly => {
                let offset = footer.index_block_offset as usize
                    + std::mem::size_of::<Pubkey>() * footer.account_entry_count as usize
                    + std::mem::size_of::<Offset>() * index_offset.0 as usize;
                let (account_offset, _) = get_type::<Offset>(mmap, offset)?;

                Ok(*account_offset)
            }
        }
    }

    /// Returns the size of one index entry.
    pub fn entry_size<Offset: AccountOffset>(&self) -> usize {
        match self {
            Self::AddressAndBlockOffsetOnly => {
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
            file::TieredStorageFile,
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
                offset: HotAccountOffset::new(
                    rng.gen_range(0..u32::MAX) as usize * HOT_ACCOUNT_ALIGNMENT,
                )
                .unwrap(),
            })
            .collect();

        {
            let file = TieredStorageFile::new_writable(&path).unwrap();
            let indexer = IndexBlockFormat::AddressAndBlockOffsetOnly;
            indexer.write_index_block(&file, &index_entries).unwrap();
        }

        let indexer = IndexBlockFormat::AddressAndBlockOffsetOnly;
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
}

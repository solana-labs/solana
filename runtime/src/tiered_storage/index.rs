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
pub struct AccountIndexWriterEntry<'a> {
    pub address: &'a Pubkey,
    pub block_offset: u64,
    pub intra_block_offset: u64,
}

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
pub enum AccountIndexFormat {
    /// This format optimizes the storage size by storing only account addresses
    /// and offsets.  It skips storing the size of account data by storing account
    /// block entries and index block entries in the same order.
    #[default]
    AddressAndOffset = 0,
}

impl AccountIndexFormat {
    /// Persists the specified index_entries to the specified file and returns
    /// the total number of bytes written.
    pub fn write_index_block(
        &self,
        file: &TieredStorageFile,
        index_entries: &Vec<AccountIndexWriterEntry>,
    ) -> TieredStorageResult<u64> {
        match self {
            Self::AddressAndOffset => {
                let mut cursor: u64 = 0;
                for index_entry in index_entries {
                    cursor += file.write_type(index_entry.address)? as u64;
                }
                for index_entry in index_entries {
                    cursor += file.write_type(&index_entry.block_offset)? as u64;
                }
                Ok(cursor)
            }
        }
    }

    /// Returns the address of the account given the specified index.
    pub fn get_account_address<'a>(
        &self,
        map: &'a Mmap,
        footer: &TieredStorageFooter,
        index: usize,
    ) -> TieredStorageResult<&'a Pubkey> {
        let offset = match self {
            Self::AddressAndOffset => {
                footer.account_index_offset as usize + std::mem::size_of::<Pubkey>() * index
            }
        };
        let (address, _) = get_type::<Pubkey>(map, offset)?;
        Ok(address)
    }

    /// Returns the offset to the accout given the specified index
    /// to the index block.
    pub fn get_account_offset(
        &self,
        map: &Mmap,
        footer: &TieredStorageFooter,
        index: usize,
    ) -> TieredStorageResult<u64> {
        match self {
            Self::AddressAndOffset => {
                let offset = footer.account_index_offset as usize
                    + std::mem::size_of::<Pubkey>() * footer.account_entry_count as usize
                    + index * std::mem::size_of::<u64>();
                let (meta_offset, _) = get_type(map, offset)?;
                Ok(*meta_offset)
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
pub mod tests {
    use {
        super::*,
        crate::{
            append_vec::test_utils::get_append_vec_path, tiered_storage::file::TieredStorageFile,
        },
        memmap2::MmapOptions,
        rand::Rng,
        std::fs::OpenOptions,
    };

    #[test]
    fn test_address_and_offset_indexer() {
        const ENTRY_COUNT: u32 = 100;
        let footer = TieredStorageFooter {
            account_entry_count: ENTRY_COUNT,
            ..TieredStorageFooter::default()
        };
        let path = get_append_vec_path("test_address_and_offset_indexer");
        let addresses: Vec<_> = std::iter::repeat_with(|| Pubkey::new_unique())
            .take(ENTRY_COUNT.try_into().unwrap())
            .collect();
        let mut rng = rand::thread_rng();
        let index_entries: Vec<_> = addresses
            .iter()
            .map(|address| AccountIndexWriterEntry {
                address,
                block_offset: rng.gen_range(128, 2048),
                intra_block_offset: 0,
            })
            .collect();

        {
            let file = TieredStorageFile::new_writable(&path.path);
            let indexer = AccountIndexFormat::AddressAndOffset;
            indexer.write_index_block(&file, &index_entries).unwrap();
        }

        let indexer = AccountIndexFormat::AddressAndOffset;
        let file = OpenOptions::new()
            .read(true)
            .create(false)
            .open(&path.path)
            .unwrap();
        let map = unsafe { MmapOptions::new().map(&file).unwrap() };
        for (i, index_entry) in index_entries.iter().enumerate() {
            assert_eq!(
                index_entry.block_offset,
                indexer.get_account_offset(&map, &footer, i).unwrap()
            );
            let address = indexer.get_account_address(&map, &footer, i).unwrap();
            assert_eq!(index_entry.address, address);
        }
    }
}

use {
    crate::tiered_storage::{
        error::TieredStorageError,
        file::TieredStorageFile,
        footer::TieredStorageFooter,
        mmap_utils::{get_slice, get_type},
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
        index: usize,
    ) -> TieredStorageResult<&'a Pubkey> {
        let offset = match self {
            Self::AddressAndOffset => {
                footer.account_index_offset as usize + std::mem::size_of::<Pubkey>() * index
            }
        };
        let (address, _) = get_type::<Pubkey>(mmap, offset)?;
        Ok(address)
    }

    /// Returns the offset and size of the account block that contains
    /// the account associated with the specified index from the given
    /// mmap.
    fn get_account_block_info(
        &self,
        mmap: &Mmap,
        footer: &TieredStorageFooter,
        index: usize,
    ) -> TieredStorageResult<(u64, usize)> {
        match self {
            Self::AddressAndOffset => {
                // obtain the offset of the index entry of the specified account
                let index_offset = footer.account_index_offset as usize
                    + std::mem::size_of::<Pubkey>() * footer.account_entry_count as usize
                    + index * std::mem::size_of::<u64>();
                // then, obtain the account block offset of the account
                let (account_block_offset, next_index_offset) =
                    get_type::<u64>(mmap, index_offset)?;

                // As an storage size optimization of this index format, the
                // size of an account block isn't persisted.  Instead, it can be
                // obtained by finding the next account block offset that is
                // different from the current account block, and the difference
                // between the two account block offsets is the size of the
                // account block.

                // As the owners block locates right after the index block, it
                // is also the ending offset of the index block (exclusive).
                let index_block_ending_offset: usize = footer.owners_offset.try_into().unwrap();

                // Obtain the offset of the next account block to derive the
                // size of the current account block.
                //
                // If next_index_offset exceeds the index block, it means the
                // current account is the last account entry.  In this case,
                // we use the account index block offset (which locates right
                // after the accounts blocks section) as the next accoutn block
                // offset.
                let next_account_block_offset = if next_index_offset >= index_block_ending_offset {
                    footer.account_index_offset
                } else {
                    let (account_block_offset, _) = get_type::<u64>(mmap, next_index_offset)?;
                    *account_block_offset
                };

                if next_account_block_offset > footer.account_index_offset {
                    return Err(TieredStorageError::InvalidAccountBlockOffset(
                        next_account_block_offset,
                        footer.account_index_offset,
                    ));
                }
                Ok((
                    *account_block_offset,
                    next_account_block_offset.saturating_sub(*account_block_offset) as usize,
                ))
            }
        }
    }

    /// Returns the account block of the account associated with the
    /// specified index from the given mmap.
    pub fn get_account_block<'a>(
        &self,
        mmap: &'a Mmap,
        footer: &TieredStorageFooter,
        index: usize,
    ) -> TieredStorageResult<&'a [u8]> {
        let (offset, len) = self.get_account_block_info(mmap, footer, index)?;
        let (account_block, _) = get_slice(mmap, offset as usize, len)?;

        Ok(account_block)
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
    fn test_index_format_address_and_offset() {
        const ENTRY_COUNT: usize = 100;
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_address_and_offset_indexer");
        let addresses: Vec<_> = std::iter::repeat_with(Pubkey::new_unique)
            .take(ENTRY_COUNT)
            .collect();
        let mut rng = rand::thread_rng();
        let mut block_offset = 0;
        let account_data_sizes: Vec<_> = std::iter::repeat_with(|| rng.gen_range(1, 128) * 8)
            .take(ENTRY_COUNT)
            .collect();

        let index_entries: Vec<_> = addresses
            .iter()
            .zip(account_data_sizes.iter())
            .map(|(address, size)| {
                println!("blk_offset {} size {}", block_offset, size);
                let entry = AccountIndexWriterEntry {
                    address,
                    block_offset,
                    intra_block_offset: 0,
                };
                block_offset += size;
                entry
            })
            .collect();

        let total_account_blocks_size = block_offset;
        let indexer = AccountIndexFormat::AddressAndOffset;
        let footer = TieredStorageFooter {
            account_entry_count: ENTRY_COUNT as u32,
            // the account index block locates right after the account blocks section
            account_index_offset: total_account_blocks_size,
            // the owners block locates right after the account index block
            owners_offset: total_account_blocks_size + (indexer.entry_size() * ENTRY_COUNT) as u64,
            ..TieredStorageFooter::default()
        };

        {
            let file = TieredStorageFile::new_writable(&path).unwrap();
            let test_account_blocks: Vec<u8> = (0..total_account_blocks_size)
                .map(|i| (i % 256) as u8)
                .collect();
            // generate the test account blocks that we will verify later
            file.write_bytes(&test_account_blocks).unwrap();
            indexer.write_index_block(&file, &index_entries).unwrap();
        }

        let indexer = AccountIndexFormat::AddressAndOffset;
        let file = OpenOptions::new()
            .read(true)
            .create(false)
            .open(&path)
            .unwrap();
        let mmap = unsafe { MmapOptions::new().map(&file).unwrap() };

        for i in 0..ENTRY_COUNT {
            // verify the account block info stored in the index block.
            let (block_offset, block_size) =
                indexer.get_account_block_info(&mmap, &footer, i).unwrap();
            println!("{i} {} {}", block_offset, block_size);
            assert_eq!(index_entries[i].block_offset, block_offset);
            assert_eq!(account_data_sizes[i], block_size as u64);
            assert_eq!(
                *indexer.get_account_address(&mmap, &footer, i).unwrap(),
                addresses[i]
            );

            // verify the account block by how we generate it
            let account_block = indexer.get_account_block(&mmap, &footer, i).unwrap();
            assert_eq!(
                account_block,
                (block_offset..block_offset + block_size as u64)
                    .map(|i| (i % 256) as u8)
                    .collect::<Vec<_>>()
            );
        }
    }
}

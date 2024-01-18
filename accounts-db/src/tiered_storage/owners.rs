use {
    crate::tiered_storage::{
        file::TieredStorageFile, footer::TieredStorageFooter, mmap_utils::get_pod,
        TieredStorageResult,
    },
    memmap2::Mmap,
    solana_sdk::pubkey::Pubkey,
};

/// Owner block holds a set of unique addresses of account owners,
/// and an account meta has a owner_offset field for accessing
/// it's owner address.
#[derive(Debug)]
pub struct OwnersBlock;

/// The offset to an owner entry in the owners block.
/// This is used to obtain the address of the account owner.
///
/// Note that as its internal type is u32, it means the maximum number of
/// unique owners in one TieredStorageFile is 2^32.
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd)]
pub struct OwnerOffset(pub u32);

/// OwnersBlock is persisted as a consecutive bytes of pubkeys without any
/// meta-data.  For each account meta, it has a owner_offset field to
/// access its owner's address in the OwnersBlock.
impl OwnersBlock {
    /// Persists the provided owners' addresses into the specified file.
    pub fn write_owners_block(
        file: &TieredStorageFile,
        addresses: &[Pubkey],
    ) -> TieredStorageResult<usize> {
        let mut bytes_written = 0;
        for address in addresses {
            bytes_written += file.write_pod(address)?;
        }

        Ok(bytes_written)
    }

    /// Returns the owner address associated with the specified owner_offset
    /// and footer inside the input mmap.
    pub fn get_owner_address<'a>(
        mmap: &'a Mmap,
        footer: &TieredStorageFooter,
        owner_offset: OwnerOffset,
    ) -> TieredStorageResult<&'a Pubkey> {
        let offset = footer.owners_block_offset as usize
            + (std::mem::size_of::<Pubkey>() * owner_offset.0 as usize);
        let (pubkey, _) = get_pod::<Pubkey>(mmap, offset)?;

        Ok(pubkey)
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*, crate::tiered_storage::file::TieredStorageFile, memmap2::MmapOptions,
        std::fs::OpenOptions, tempfile::TempDir,
    };

    #[test]
    fn test_owners_block() {
        // Generate a new temp path that is guaranteed to NOT already have a file.
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_owners_block");
        const NUM_OWNERS: u32 = 10;

        let addresses: Vec<_> = std::iter::repeat_with(Pubkey::new_unique)
            .take(NUM_OWNERS as usize)
            .collect();

        let footer = TieredStorageFooter {
            // Set owners_block_offset to 0 as we didn't write any account
            // meta/data nor index block.
            owners_block_offset: 0,
            ..TieredStorageFooter::default()
        };

        {
            let file = TieredStorageFile::new_writable(&path).unwrap();

            OwnersBlock::write_owners_block(&file, &addresses).unwrap();

            // while the test only focuses on account metas, writing a footer
            // here is necessary to make it a valid tiered-storage file.
            footer.write_footer_block(&file).unwrap();
        }

        let file = OpenOptions::new().read(true).open(path).unwrap();
        let mmap = unsafe { MmapOptions::new().map(&file).unwrap() };

        for (i, address) in addresses.iter().enumerate() {
            assert_eq!(
                OwnersBlock::get_owner_address(&mmap, &footer, OwnerOffset(i as u32)).unwrap(),
                address
            );
        }
    }
}

use {
    crate::tiered_storage::{
        file::TieredWritableFile, footer::TieredStorageFooter, mmap_utils::get_pod,
        TieredStorageResult,
    },
    indexmap::set::IndexSet,
    memmap2::Mmap,
    solana_sdk::pubkey::Pubkey,
};

/// The offset to an owner entry in the owners block.
/// This is used to obtain the address of the account owner.
///
/// Note that as its internal type is u32, it means the maximum number of
/// unique owners in one TieredStorageFile is 2^32.
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd)]
pub struct OwnerOffset(pub u32);

lazy_static! {
    pub static ref OWNER_NO_OWNER: Pubkey = Pubkey::default();
}

/// Owner block holds a set of unique addresses of account owners,
/// and an account meta has a owner_offset field for accessing
/// it's owner address.
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
    /// This format persists OwnerBlock as a consecutive bytes of pubkeys
    /// without any meta-data.  For each account meta, it has a owner_offset
    /// field to access its owner's address in the OwnersBlock.
    #[default]
    AddressesOnly = 0,
}

impl OwnersBlockFormat {
    /// Persists the provided owners' addresses into the specified file.
    pub fn write_owners_block(
        &self,
        file: &mut TieredWritableFile,
        owners_table: &OwnersTable,
    ) -> TieredStorageResult<usize> {
        match self {
            Self::AddressesOnly => {
                let mut bytes_written = 0;
                for address in &owners_table.owners_set {
                    bytes_written += file.write_pod(*address)?;
                }

                Ok(bytes_written)
            }
        }
    }

    /// Returns the owner address associated with the specified owner_offset
    /// and footer inside the input mmap.
    pub fn get_owner_address<'a>(
        &self,
        mmap: &'a Mmap,
        footer: &TieredStorageFooter,
        owner_offset: OwnerOffset,
    ) -> TieredStorageResult<&'a Pubkey> {
        match self {
            Self::AddressesOnly => {
                let offset = footer.owners_block_offset as usize
                    + (std::mem::size_of::<Pubkey>() * owner_offset.0 as usize);
                let (pubkey, _) = get_pod::<Pubkey>(mmap, offset)?;

                Ok(pubkey)
            }
        }
    }
}

/// The in-memory representation of owners block for write.
/// It manages a set of unique addresses of account owners.
#[derive(Debug, Default)]
pub struct OwnersTable<'a> {
    owners_set: IndexSet<&'a Pubkey>,
}

/// OwnersBlock is persisted as a consecutive bytes of pubkeys without any
/// meta-data.  For each account meta, it has a owner_offset field to
/// access its owner's address in the OwnersBlock.
impl<'a> OwnersTable<'a> {
    /// Add the specified pubkey as the owner into the OwnersWriterTable
    /// if the specified pubkey has not existed in the OwnersWriterTable
    /// yet.  In any case, the function returns its OwnerOffset.
    pub fn insert(&mut self, pubkey: &'a Pubkey) -> OwnerOffset {
        let (offset, _existed) = self.owners_set.insert_full(pubkey);

        OwnerOffset(offset as u32)
    }

    /// Returns the number of unique owner addresses in the table.
    pub fn len(&self) -> usize {
        self.owners_set.len()
    }

    /// Returns true if the OwnersTable is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*, crate::tiered_storage::file::TieredWritableFile, memmap2::MmapOptions,
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
            let mut file = TieredWritableFile::new(&path).unwrap();

            let mut owners_table = OwnersTable::default();
            addresses.iter().for_each(|owner_address| {
                owners_table.insert(owner_address);
            });
            footer
                .owners_block_format
                .write_owners_block(&mut file, &owners_table)
                .unwrap();

            // while the test only focuses on account metas, writing a footer
            // here is necessary to make it a valid tiered-storage file.
            footer.write_footer_block(&mut file).unwrap();
        }

        let file = OpenOptions::new().read(true).open(path).unwrap();
        let mmap = unsafe { MmapOptions::new().map(&file).unwrap() };

        for (i, address) in addresses.iter().enumerate() {
            assert_eq!(
                footer
                    .owners_block_format
                    .get_owner_address(&mmap, &footer, OwnerOffset(i as u32))
                    .unwrap(),
                address
            );
        }
    }

    #[test]
    fn test_owners_table() {
        let mut owners_table = OwnersTable::default();
        const NUM_OWNERS: usize = 99;

        let addresses: Vec<_> = std::iter::repeat_with(Pubkey::new_unique)
            .take(NUM_OWNERS)
            .collect();

        // as we insert sequentially, we expect each entry has same OwnerOffset
        // as its index inside the Vector.
        for (i, address) in addresses.iter().enumerate() {
            assert_eq!(owners_table.insert(address), OwnerOffset(i as u32));
        }

        let cloned_addresses = addresses.clone();

        // insert again and expect the same OwnerOffset
        for (i, address) in cloned_addresses.iter().enumerate() {
            assert_eq!(owners_table.insert(address), OwnerOffset(i as u32));
        }

        // make sure the size of the resulting owner table is the same
        // as the input
        assert_eq!(owners_table.owners_set.len(), addresses.len());
    }
}

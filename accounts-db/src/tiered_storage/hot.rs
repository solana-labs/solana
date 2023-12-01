#![allow(dead_code)]
//! The account meta and related structs for hot accounts.

use {
    crate::{
        accounts_hash::AccountHash,
        tiered_storage::{
            byte_block,
            footer::{
                AccountBlockFormat, AccountMetaFormat, OwnersBlockFormat, TieredStorageFooter,
            },
            index::{AccountOffset, IndexBlockFormat, IndexOffset},
            meta::{AccountMetaFlags, AccountMetaOptionalFields, TieredAccountMeta},
            mmap_utils::get_type,
            owners::{OwnerOffset, OwnersBlock},
            TieredStorageFormat, TieredStorageResult,
        },
    },
    memmap2::{Mmap, MmapOptions},
    modular_bitfield::prelude::*,
    solana_sdk::{pubkey::Pubkey, stake_history::Epoch},
    std::{fs::OpenOptions, option::Option, path::Path},
};

pub const HOT_FORMAT: TieredStorageFormat = TieredStorageFormat {
    meta_entry_size: std::mem::size_of::<HotAccountMeta>(),
    account_meta_format: AccountMetaFormat::Hot,
    owners_block_format: OwnersBlockFormat::LocalIndex,
    index_block_format: IndexBlockFormat::AddressAndBlockOffsetOnly,
    account_block_format: AccountBlockFormat::AlignedRaw,
};

/// The maximum number of padding bytes used in a hot account entry.
const MAX_HOT_PADDING: u8 = 7;

/// The maximum allowed value for the owner index of a hot account.
const MAX_HOT_OWNER_OFFSET: OwnerOffset = OwnerOffset((1 << 29) - 1);

/// The multiplier for converting AccountOffset to the internal hot account
/// offset.  This increases the maximum size of a hot accounts file.
const HOT_ACCOUNT_OFFSET_MULTIPLIER: usize = 8;

#[bitfield(bits = 32)]
#[repr(C)]
#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
struct HotMetaPackedFields {
    /// A hot account entry consists of the following elements:
    ///
    /// * HotAccountMeta
    /// * [u8] account data
    /// * 0-7 bytes padding
    /// * optional fields
    ///
    /// The following field records the number of padding bytes used
    /// in its hot account entry.
    padding: B3,
    /// The index to the owner of a hot account inside an AccountsFile.
    owner_offset: B29,
}

/// The storage and in-memory representation of the metadata entry for a
/// hot account.
#[derive(Debug, PartialEq, Eq)]
#[repr(C)]
pub struct HotAccountMeta {
    /// The balance of this account.
    lamports: u64,
    /// Stores important fields in a packed struct.
    packed_fields: HotMetaPackedFields,
    /// Stores boolean flags and existence of each optional field.
    flags: AccountMetaFlags,
}

impl TieredAccountMeta for HotAccountMeta {
    /// Construct a HotAccountMeta instance.
    fn new() -> Self {
        HotAccountMeta {
            lamports: 0,
            packed_fields: HotMetaPackedFields::default(),
            flags: AccountMetaFlags::new(),
        }
    }

    /// A builder function that initializes lamports.
    fn with_lamports(mut self, lamports: u64) -> Self {
        self.lamports = lamports;
        self
    }

    /// A builder function that initializes the number of padding bytes
    /// for the account data associated with the current meta.
    fn with_account_data_padding(mut self, padding: u8) -> Self {
        if padding > MAX_HOT_PADDING {
            panic!("padding exceeds MAX_HOT_PADDING");
        }
        self.packed_fields.set_padding(padding);
        self
    }

    /// A builder function that initializes the owner's index.
    fn with_owner_offset(mut self, owner_offset: OwnerOffset) -> Self {
        if owner_offset > MAX_HOT_OWNER_OFFSET {
            panic!("owner_offset exceeds MAX_HOT_OWNER_OFFSET");
        }
        self.packed_fields.set_owner_offset(owner_offset.0);
        self
    }

    /// A builder function that initializes the account data size.
    fn with_account_data_size(self, _account_data_size: u64) -> Self {
        // Hot meta does not store its data size as it derives its data length
        // by comparing the offets of two consecutive account meta entries.
        self
    }

    /// A builder function that initializes the AccountMetaFlags of the current
    /// meta.
    fn with_flags(mut self, flags: &AccountMetaFlags) -> Self {
        self.flags = *flags;
        self
    }

    /// Returns the balance of the lamports associated with the account.
    fn lamports(&self) -> u64 {
        self.lamports
    }

    /// Returns the number of padding bytes for the associated account data
    fn account_data_padding(&self) -> u8 {
        self.packed_fields.padding()
    }

    /// Returns the index to the accounts' owner in the current AccountsFile.
    fn owner_offset(&self) -> OwnerOffset {
        OwnerOffset(self.packed_fields.owner_offset())
    }

    /// Returns the AccountMetaFlags of the current meta.
    fn flags(&self) -> &AccountMetaFlags {
        &self.flags
    }

    /// Always returns false as HotAccountMeta does not support multiple
    /// meta entries sharing the same account block.
    fn supports_shared_account_block() -> bool {
        false
    }

    /// Returns the epoch that this account will next owe rent by parsing
    /// the specified account block.  None will be returned if this account
    /// does not persist this optional field.
    fn rent_epoch(&self, account_block: &[u8]) -> Option<Epoch> {
        self.flags()
            .has_rent_epoch()
            .then(|| {
                let offset = self.optional_fields_offset(account_block)
                    + AccountMetaOptionalFields::rent_epoch_offset(self.flags());
                byte_block::read_type::<Epoch>(account_block, offset).copied()
            })
            .flatten()
    }

    /// Returns the account hash by parsing the specified account block.  None
    /// will be returned if this account does not persist this optional field.
    fn account_hash<'a>(&self, account_block: &'a [u8]) -> Option<&'a AccountHash> {
        self.flags()
            .has_account_hash()
            .then(|| {
                let offset = self.optional_fields_offset(account_block)
                    + AccountMetaOptionalFields::account_hash_offset(self.flags());
                byte_block::read_type::<AccountHash>(account_block, offset)
            })
            .flatten()
    }

    /// Returns the offset of the optional fields based on the specified account
    /// block.
    fn optional_fields_offset(&self, account_block: &[u8]) -> usize {
        account_block
            .len()
            .saturating_sub(AccountMetaOptionalFields::size_from_flags(&self.flags))
    }

    /// Returns the length of the data associated to this account based on the
    /// specified account block.
    fn account_data_size(&self, account_block: &[u8]) -> usize {
        self.optional_fields_offset(account_block)
            .saturating_sub(self.account_data_padding() as usize)
    }

    /// Returns the data associated to this account based on the specified
    /// account block.
    fn account_data<'a>(&self, account_block: &'a [u8]) -> &'a [u8] {
        &account_block[..self.account_data_size(account_block)]
    }
}

/// The reader to a hot accounts file.
#[derive(Debug)]
pub struct HotStorageReader {
    mmap: Mmap,
    footer: TieredStorageFooter,
}

impl HotStorageReader {
    /// Constructs a HotStorageReader from the specified path.
    pub fn new_from_path(path: impl AsRef<Path>) -> TieredStorageResult<Self> {
        let file = OpenOptions::new().read(true).open(path)?;
        let mmap = unsafe { MmapOptions::new().map(&file)? };
        // Here we are copying the footer, as accessing any data in a
        // TieredStorage instance requires accessing its Footer.
        // This can help improve cache locality and reduce the overhead
        // of indirection associated with memory-mapped accesses.
        let footer = *TieredStorageFooter::new_from_mmap(&mmap)?;

        Ok(Self { mmap, footer })
    }

    /// Returns the footer of the underlying tiered-storage accounts file.
    pub fn footer(&self) -> &TieredStorageFooter {
        &self.footer
    }

    /// Returns the number of files inside the underlying tiered-storage
    /// accounts file.
    pub fn num_accounts(&self) -> usize {
        self.footer.account_entry_count as usize
    }

    /// Returns the account meta located at the specified offset.
    fn get_account_meta_from_offset(
        &self,
        account_offset: AccountOffset,
    ) -> TieredStorageResult<&HotAccountMeta> {
        let internal_account_offset = account_offset.block as usize * HOT_ACCOUNT_OFFSET_MULTIPLIER;

        let (meta, _) = get_type::<HotAccountMeta>(&self.mmap, internal_account_offset)?;
        Ok(meta)
    }

    /// Returns the offset to the account given the specified index.
    fn get_account_offset(&self, index_offset: IndexOffset) -> TieredStorageResult<AccountOffset> {
        self.footer
            .index_block_format
            .get_account_offset(&self.mmap, &self.footer, index_offset)
    }

    /// Returns the address of the account associated with the specified index.
    fn get_account_address(&self, index: IndexOffset) -> TieredStorageResult<&Pubkey> {
        self.footer
            .index_block_format
            .get_account_address(&self.mmap, &self.footer, index)
    }

    /// Returns the address of the account owner given the specified
    /// owner_offset.
    fn get_owner_address(&self, owner_offset: OwnerOffset) -> TieredStorageResult<&Pubkey> {
        OwnersBlock::get_owner_address(&self.mmap, &self.footer, owner_offset)
    }
}

#[cfg(test)]
pub mod tests {
    use {
        super::*,
        crate::tiered_storage::{
            byte_block::ByteBlockWriter,
            file::TieredStorageFile,
            footer::{
                AccountBlockFormat, AccountMetaFormat, OwnersBlockFormat, TieredStorageFooter,
                FOOTER_SIZE,
            },
            hot::{HotAccountMeta, HotStorageReader},
            index::{AccountIndexWriterEntry, AccountOffset, IndexBlockFormat, IndexOffset},
            meta::{AccountMetaFlags, AccountMetaOptionalFields, TieredAccountMeta},
        },
        memoffset::offset_of,
        rand::Rng,
        solana_sdk::{hash::Hash, pubkey::Pubkey, stake_history::Epoch},
        tempfile::TempDir,
    };

    #[test]
    fn test_hot_account_meta_layout() {
        assert_eq!(offset_of!(HotAccountMeta, lamports), 0x00);
        assert_eq!(offset_of!(HotAccountMeta, packed_fields), 0x08);
        assert_eq!(offset_of!(HotAccountMeta, flags), 0x0C);
        assert_eq!(std::mem::size_of::<HotAccountMeta>(), 16);
    }

    #[test]
    fn test_packed_fields() {
        const TEST_PADDING: u8 = 7;
        const TEST_OWNER_OFFSET: u32 = 0x1fff_ef98;
        let mut packed_fields = HotMetaPackedFields::default();
        packed_fields.set_padding(TEST_PADDING);
        packed_fields.set_owner_offset(TEST_OWNER_OFFSET);
        assert_eq!(packed_fields.padding(), TEST_PADDING);
        assert_eq!(packed_fields.owner_offset(), TEST_OWNER_OFFSET);
    }

    #[test]
    fn test_packed_fields_max_values() {
        let mut packed_fields = HotMetaPackedFields::default();
        packed_fields.set_padding(MAX_HOT_PADDING);
        packed_fields.set_owner_offset(MAX_HOT_OWNER_OFFSET.0);
        assert_eq!(packed_fields.padding(), MAX_HOT_PADDING);
        assert_eq!(packed_fields.owner_offset(), MAX_HOT_OWNER_OFFSET.0);
    }

    #[test]
    fn test_hot_meta_max_values() {
        let meta = HotAccountMeta::new()
            .with_account_data_padding(MAX_HOT_PADDING)
            .with_owner_offset(MAX_HOT_OWNER_OFFSET);

        assert_eq!(meta.account_data_padding(), MAX_HOT_PADDING);
        assert_eq!(meta.owner_offset(), MAX_HOT_OWNER_OFFSET);
    }

    #[test]
    #[should_panic(expected = "padding exceeds MAX_HOT_PADDING")]
    fn test_hot_meta_padding_exceeds_limit() {
        HotAccountMeta::new().with_account_data_padding(MAX_HOT_PADDING + 1);
    }

    #[test]
    #[should_panic(expected = "owner_offset exceeds MAX_HOT_OWNER_OFFSET")]
    fn test_hot_meta_owner_offset_exceeds_limit() {
        HotAccountMeta::new().with_owner_offset(OwnerOffset(MAX_HOT_OWNER_OFFSET.0 + 1));
    }

    #[test]
    fn test_hot_account_meta() {
        const TEST_LAMPORTS: u64 = 2314232137;
        const TEST_PADDING: u8 = 5;
        const TEST_OWNER_OFFSET: OwnerOffset = OwnerOffset(0x1fef_1234);
        const TEST_RENT_EPOCH: Epoch = 7;

        let optional_fields = AccountMetaOptionalFields {
            rent_epoch: Some(TEST_RENT_EPOCH),
            account_hash: Some(AccountHash(Hash::new_unique())),
        };

        let flags = AccountMetaFlags::new_from(&optional_fields);
        let meta = HotAccountMeta::new()
            .with_lamports(TEST_LAMPORTS)
            .with_account_data_padding(TEST_PADDING)
            .with_owner_offset(TEST_OWNER_OFFSET)
            .with_flags(&flags);

        assert_eq!(meta.lamports(), TEST_LAMPORTS);
        assert_eq!(meta.account_data_padding(), TEST_PADDING);
        assert_eq!(meta.owner_offset(), TEST_OWNER_OFFSET);
        assert_eq!(*meta.flags(), flags);
    }

    #[test]
    fn test_hot_account_meta_full() {
        let account_data = [11u8; 83];
        let padding = [0u8; 5];

        const TEST_LAMPORT: u64 = 2314232137;
        const OWNER_OFFSET: u32 = 0x1fef_1234;
        const TEST_RENT_EPOCH: Epoch = 7;

        let optional_fields = AccountMetaOptionalFields {
            rent_epoch: Some(TEST_RENT_EPOCH),
            account_hash: Some(AccountHash(Hash::new_unique())),
        };

        let flags = AccountMetaFlags::new_from(&optional_fields);
        let expected_meta = HotAccountMeta::new()
            .with_lamports(TEST_LAMPORT)
            .with_account_data_padding(padding.len().try_into().unwrap())
            .with_owner_offset(OwnerOffset(OWNER_OFFSET))
            .with_flags(&flags);

        let mut writer = ByteBlockWriter::new(AccountBlockFormat::AlignedRaw);
        writer.write_type(&expected_meta).unwrap();
        writer.write_type(&account_data).unwrap();
        writer.write_type(&padding).unwrap();
        writer.write_optional_fields(&optional_fields).unwrap();
        let buffer = writer.finish().unwrap();

        let meta = byte_block::read_type::<HotAccountMeta>(&buffer, 0).unwrap();
        assert_eq!(expected_meta, *meta);
        assert!(meta.flags().has_rent_epoch());
        assert!(meta.flags().has_account_hash());
        assert_eq!(meta.account_data_padding() as usize, padding.len());

        let account_block = &buffer[std::mem::size_of::<HotAccountMeta>()..];
        assert_eq!(
            meta.optional_fields_offset(account_block),
            account_block
                .len()
                .saturating_sub(AccountMetaOptionalFields::size_from_flags(&flags))
        );
        assert_eq!(account_data.len(), meta.account_data_size(account_block));
        assert_eq!(account_data, meta.account_data(account_block));
        assert_eq!(meta.rent_epoch(account_block), optional_fields.rent_epoch);
        assert_eq!(
            *(meta.account_hash(account_block).unwrap()),
            optional_fields.account_hash.unwrap()
        );
    }

    #[test]
    fn test_hot_storage_footer() {
        // Generate a new temp path that is guaranteed to NOT already have a file.
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_hot_storage_footer");
        let expected_footer = TieredStorageFooter {
            account_meta_format: AccountMetaFormat::Hot,
            owners_block_format: OwnersBlockFormat::LocalIndex,
            index_block_format: IndexBlockFormat::AddressAndBlockOffsetOnly,
            account_block_format: AccountBlockFormat::AlignedRaw,
            account_entry_count: 300,
            account_meta_entry_size: 16,
            account_block_size: 4096,
            owner_count: 250,
            owner_entry_size: 32,
            index_block_offset: 1069600,
            owners_block_offset: 1081200,
            hash: Hash::new_unique(),
            min_account_address: Pubkey::default(),
            max_account_address: Pubkey::new_unique(),
            footer_size: FOOTER_SIZE as u64,
            format_version: 1,
        };

        {
            let file = TieredStorageFile::new_writable(&path).unwrap();
            expected_footer.write_footer_block(&file).unwrap();
        }

        // Reopen the same storage, and expect the persisted footer is
        // the same as what we have written.
        {
            let hot_storage = HotStorageReader::new_from_path(&path).unwrap();
            assert_eq!(expected_footer, *hot_storage.footer());
        }
    }

    #[test]
    fn test_hot_storage_get_account_meta_from_offset() {
        // Generate a new temp path that is guaranteed to NOT already have a file.
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_hot_storage_footer");

        const NUM_ACCOUNTS: u32 = 10;
        let mut rng = rand::thread_rng();

        let hot_account_metas: Vec<_> = (0..NUM_ACCOUNTS)
            .map(|_| {
                HotAccountMeta::new()
                    .with_lamports(rng.gen_range(0..u64::MAX))
                    .with_owner_offset(OwnerOffset(rng.gen_range(0..NUM_ACCOUNTS)))
            })
            .collect();

        let account_offsets: Vec<_>;
        let footer = TieredStorageFooter {
            account_meta_format: AccountMetaFormat::Hot,
            account_entry_count: NUM_ACCOUNTS,
            ..TieredStorageFooter::default()
        };
        {
            let file = TieredStorageFile::new_writable(&path).unwrap();
            let mut current_offset = 0;

            account_offsets = hot_account_metas
                .iter()
                .map(|meta| {
                    let prev_offset = current_offset;
                    current_offset += file.write_type(meta).unwrap() as u32;
                    assert_eq!(prev_offset % HOT_ACCOUNT_OFFSET_MULTIPLIER as u32, 0);
                    AccountOffset {
                        block: prev_offset / HOT_ACCOUNT_OFFSET_MULTIPLIER as u32,
                    }
                })
                .collect();
            // while the test only focuses on account metas, writing a footer
            // here is necessary to make it a valid tiered-storage file.
            footer.write_footer_block(&file).unwrap();
        }

        let hot_storage = HotStorageReader::new_from_path(&path).unwrap();

        for (offset, expected_meta) in account_offsets.iter().zip(hot_account_metas.iter()) {
            let meta = hot_storage.get_account_meta_from_offset(*offset).unwrap();
            assert_eq!(meta, expected_meta);
        }
        assert_eq!(&footer, hot_storage.footer());
    }

    #[test]
    fn test_hot_storage_get_account_offset_and_address() {
        // Generate a new temp path that is guaranteed to NOT already have a file.
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir
            .path()
            .join("test_hot_storage_get_account_offset_and_address");
        const NUM_ACCOUNTS: u32 = 10;
        let mut rng = rand::thread_rng();

        let addresses: Vec<_> = std::iter::repeat_with(Pubkey::new_unique)
            .take(NUM_ACCOUNTS as usize)
            .collect();

        let index_writer_entries: Vec<_> = addresses
            .iter()
            .map(|address| AccountIndexWriterEntry {
                address,
                block_offset: rng.gen_range(0..u32::MAX),
                intra_block_offset: rng.gen_range(0..4096),
            })
            .collect();

        let footer = TieredStorageFooter {
            account_meta_format: AccountMetaFormat::Hot,
            account_entry_count: NUM_ACCOUNTS,
            // Set index_block_offset to 0 as we didn't write any account
            // meta/data in this test
            index_block_offset: 0,
            ..TieredStorageFooter::default()
        };
        {
            let file = TieredStorageFile::new_writable(&path).unwrap();

            footer
                .index_block_format
                .write_index_block(&file, &index_writer_entries)
                .unwrap();

            // while the test only focuses on account metas, writing a footer
            // here is necessary to make it a valid tiered-storage file.
            footer.write_footer_block(&file).unwrap();
        }

        let hot_storage = HotStorageReader::new_from_path(&path).unwrap();
        for (i, index_writer_entry) in index_writer_entries.iter().enumerate() {
            let account_offset = hot_storage
                .get_account_offset(IndexOffset(i as u32))
                .unwrap();
            assert_eq!(account_offset.block, index_writer_entry.block_offset);

            let account_address = hot_storage
                .get_account_address(IndexOffset(i as u32))
                .unwrap();
            assert_eq!(account_address, index_writer_entry.address);
        }
    }

    #[test]
    fn test_hot_storage_get_owner_address() {
        // Generate a new temp path that is guaranteed to NOT already have a file.
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_hot_storage_get_owner_address");
        const NUM_OWNERS: usize = 10;

        let addresses: Vec<_> = std::iter::repeat_with(Pubkey::new_unique)
            .take(NUM_OWNERS)
            .collect();

        let footer = TieredStorageFooter {
            account_meta_format: AccountMetaFormat::Hot,
            // Set owners_block_offset to 0 as we didn't write any account
            // meta/data nor index block in this test
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

        let hot_storage = HotStorageReader::new_from_path(&path).unwrap();
        for (i, address) in addresses.iter().enumerate() {
            assert_eq!(
                hot_storage
                    .get_owner_address(OwnerOffset(i as u32))
                    .unwrap(),
                address,
            );
        }
    }
}

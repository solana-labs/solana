#![allow(dead_code)]
//! The account meta and related structs for hot accounts.

use {
    crate::{
        account_storage::meta::StoredMetaWriteVersion,
        accounts_file::ALIGN_BOUNDARY_OFFSET,
        append_vec::MatchAccountOwnerError,
        tiered_storage::{
            byte_block,
            footer::TieredStorageFooter,
            meta::{AccountMetaFlags, AccountMetaOptionalFields, TieredAccountMeta},
            mmap_utils::{get_slice, get_type},
            readable::TieredReadableAccount,
            TieredStorageResult,
        },
    },
    memmap2::{Mmap, MmapOptions},
    modular_bitfield::prelude::*,
    solana_sdk::{hash::Hash, pubkey::Pubkey, stake_history::Epoch},
    std::{fs::OpenOptions, option::Option, path::Path},
};

/// The maximum number of padding bytes used in a hot account entry.
const MAX_HOT_PADDING: u8 = 7;

/// The maximum allowed value for the owner index of a hot account.
const MAX_HOT_OWNER_INDEX: u32 = (1 << 29) - 1;

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
    owner_index: B29,
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
    fn with_owner_index(mut self, owner_index: u32) -> Self {
        if owner_index > MAX_HOT_OWNER_INDEX {
            panic!("owner_index exceeds MAX_HOT_OWNER_INDEX");
        }
        self.packed_fields.set_owner_index(owner_index);
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
    fn owner_index(&self) -> u32 {
        self.packed_fields.owner_index()
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
    fn account_hash<'a>(&self, account_block: &'a [u8]) -> Option<&'a Hash> {
        self.flags()
            .has_account_hash()
            .then(|| {
                let offset = self.optional_fields_offset(account_block)
                    + AccountMetaOptionalFields::account_hash_offset(self.flags());
                byte_block::read_type::<Hash>(account_block, offset)
            })
            .flatten()
    }

    /// Returns the write version by parsing the specified account block.  None
    /// will be returned if this account does not persist this optional field.
    fn write_version(&self, account_block: &[u8]) -> Option<StoredMetaWriteVersion> {
        self.flags
            .has_write_version()
            .then(|| {
                let offset = self.optional_fields_offset(account_block)
                    + AccountMetaOptionalFields::write_version_offset(self.flags());
                byte_block::read_type::<StoredMetaWriteVersion>(account_block, offset).copied()
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
pub struct HotAccountsFileReader {
    map: Mmap,
    footer: TieredStorageFooter,
}

impl HotAccountsFileReader {
    /// Constructs a HotAccountsFileReader from the specified path.
    pub fn new_from_path<P: AsRef<Path>>(path: P) -> TieredStorageResult<Self> {
        let file = OpenOptions::new()
            .read(true)
            .create(false)
            .open(path.as_ref())?;
        let map = unsafe { MmapOptions::new().map(&file)? };
        let footer = TieredStorageFooter::new_from_mmap(&map)?.clone();
        assert!(map.len() > 0);

        Ok(Self { map, footer })
    }

    /// Returns the footer of the associated HotAccountsFile.
    pub fn footer(&self) -> &TieredStorageFooter {
        &self.footer
    }

    /// Returns the number of files inside the HotAccountsFile.
    pub fn num_accounts(&self) -> usize {
        self.footer.account_entry_count as usize
    }

    /// Finds and returns the index to the specified owners that matches the
    /// owner of the account associated with the specified multiplied_index.
    ///
    /// Err(MatchAccountOwnerError::NoMatch) will be returned if the specified
    /// owners do not contain account owner.
    /// Err(MatchAccountOwnerError::UnableToLoad) will be returned if the
    /// specified multiplied_index causes a data overrun.
    pub fn account_matches_owners(
        &self,
        multiplied_index: usize,
        owners: &[&Pubkey],
    ) -> Result<usize, MatchAccountOwnerError> {
        let index = Self::multiplied_index_to_index(multiplied_index);
        if index >= self.num_accounts() {
            return Err(MatchAccountOwnerError::UnableToLoad);
        }

        let owner = self.get_owner_address(index).unwrap();
        owners
            .iter()
            .position(|entry| &owner == entry)
            .ok_or(MatchAccountOwnerError::NoMatch)
    }

    /// Converts a multiplied index to an index.
    ///
    /// This is a temporary workaround to work with existing AccountInfo
    /// implementation that ties to AppendVec with the assumption that the offset
    /// is a multiple of ALIGN_BOUNDARY_OFFSET, while tiered storage actually talks
    /// about index instead of offset.
    fn multiplied_index_to_index(multiplied_index: usize) -> usize {
        multiplied_index / ALIGN_BOUNDARY_OFFSET
    }

    /// Returns the account meta associated with the specified index.
    ///
    /// Note that this function takes an index instead of a multiplied_index.
    fn get_account_meta(&self, index: usize) -> TieredStorageResult<&HotAccountMeta> {
        let offset = self.footer.account_index_format.get_account_block_offset(
            &self.map,
            &self.footer,
            index,
        )? as usize;
        self.get_account_meta_from_offset(offset)
    }

    /// Returns the account meta located at the specified offset.
    fn get_account_meta_from_offset<'a>(
        &'a self,
        offset: usize,
    ) -> TieredStorageResult<&'a HotAccountMeta> {
        let (meta, _): (&'a HotAccountMeta, _) = get_type(&self.map, offset)?;
        Ok(meta)
    }

    /// Returns the address of the account associated with the specified index.
    ///
    /// Note that this function takes an index instead of a multiplied_index.
    fn get_account_address(&self, index: usize) -> TieredStorageResult<&Pubkey> {
        self.footer
            .account_index_format
            .get_account_address(&self.map, &self.footer, index)
    }

    /// Returns the address of the account owner associated with the specified index.
    ///
    /// Note that this function takes an index instead of a multiplied_index.
    fn get_owner_address<'a>(&'a self, index: usize) -> TieredStorageResult<&'a Pubkey> {
        let meta = self.get_account_meta(index)?;
        let offset = self.footer.owners_offset as usize
            + (std::mem::size_of::<Pubkey>() * (meta.owner_index() as usize));
        let (pubkey, _): (&'a Pubkey, _) = get_type(&self.map, offset)?;
        Ok(pubkey)
    }

    /// Computes and returns the size of the acount block that contains
    /// the account associated with the specified index given the offset
    /// to the account meta and its index.
    ///
    /// Note that this function takes an index instead of a multiplied_index.
    fn get_account_block_size(&self, meta_offset: usize, index: usize) -> usize {
        if (index + 1) as u32 == self.footer.account_entry_count {
            assert!(self.footer.account_index_offset as usize > meta_offset);
            return self.footer.account_index_offset as usize
                - meta_offset
                - std::mem::size_of::<HotAccountMeta>();
        }

        let next_meta_offset = self
            .footer
            .account_index_format
            .get_account_block_offset(&self.map, &self.footer, index + 1)
            .unwrap() as usize;

        next_meta_offset
            .saturating_sub(meta_offset)
            .saturating_sub(std::mem::size_of::<HotAccountMeta>())
    }

    /// Returns the account block that contains the account associated with
    /// the specified index given the offset to the account meta and its index.
    ///
    /// Note that this function takes an index instead of a multiplied_index.
    fn get_account_block<'a>(
        &'a self,
        meta_offset: usize,
        index: usize,
    ) -> TieredStorageResult<&'a [u8]> {
        let (data, _): (&'a [u8], _) = get_slice(
            &self.map,
            meta_offset + std::mem::size_of::<HotAccountMeta>(),
            self.get_account_block_size(meta_offset, index),
        )?;

        Ok(data)
    }

    /// Returns the account associated with the specified multiplied_index
    /// or None if the specified multiplied_index causes a data overrun.
    ///
    /// See [`multiplied_index_to_index`].
    pub fn get_account<'a>(
        &'a self,
        multiplied_index: usize,
    ) -> Option<(TieredReadableAccount<'a, HotAccountMeta>, usize)> {
        let index = Self::multiplied_index_to_index(multiplied_index);
        if index >= self.footer.account_entry_count as usize {
            return None;
        }

        let meta_offset = self
            .footer
            .account_index_format
            .get_account_block_offset(&self.map, &self.footer, index)
            .unwrap() as usize;
        let meta: &'a HotAccountMeta = self.get_account_meta_from_offset(meta_offset).unwrap();
        let address: &'a Pubkey = self.get_account_address(index).unwrap();
        let owner: &'a Pubkey = self.get_owner_address(index).unwrap();
        let account_block: &'a [u8] = self.get_account_block(meta_offset, index).unwrap();

        Some((
            TieredReadableAccount {
                meta,
                address,
                owner,
                index: multiplied_index,
                account_block,
            },
            multiplied_index + ALIGN_BOUNDARY_OFFSET,
        ))
    }
}

#[cfg(test)]
pub mod tests {
    use {
        super::*,
        crate::{
            account_storage::meta::StoredMetaWriteVersion,
            tiered_storage::{
                byte_block::ByteBlockWriter,
                file::TieredStorageFile,
                footer::{
                    AccountBlockFormat, AccountMetaFormat, OwnersBlockFormat, TieredStorageFooter,
                    FOOTER_SIZE,
                },
                hot::{HotAccountMeta, HotAccountsFileReader},
                index::AccountIndexFormat,
                meta::{AccountMetaFlags, AccountMetaOptionalFields, TieredAccountMeta},
            },
        },
        ::solana_sdk::{hash::Hash, pubkey::Pubkey, stake_history::Epoch},
        memoffset::offset_of,
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
        const TEST_OWNER_INDEX: u32 = 0x1fff_ef98;
        let mut packed_fields = HotMetaPackedFields::default();
        packed_fields.set_padding(TEST_PADDING);
        packed_fields.set_owner_index(TEST_OWNER_INDEX);
        assert_eq!(packed_fields.padding(), TEST_PADDING);
        assert_eq!(packed_fields.owner_index(), TEST_OWNER_INDEX);
    }

    #[test]
    fn test_packed_fields_max_values() {
        let mut packed_fields = HotMetaPackedFields::default();
        packed_fields.set_padding(MAX_HOT_PADDING);
        packed_fields.set_owner_index(MAX_HOT_OWNER_INDEX);
        assert_eq!(packed_fields.padding(), MAX_HOT_PADDING);
        assert_eq!(packed_fields.owner_index(), MAX_HOT_OWNER_INDEX);
    }

    #[test]
    fn test_hot_meta_max_values() {
        let meta = HotAccountMeta::new()
            .with_account_data_padding(MAX_HOT_PADDING)
            .with_owner_index(MAX_HOT_OWNER_INDEX);

        assert_eq!(meta.account_data_padding(), MAX_HOT_PADDING);
        assert_eq!(meta.owner_index(), MAX_HOT_OWNER_INDEX);
    }

    #[test]
    #[should_panic(expected = "padding exceeds MAX_HOT_PADDING")]
    fn test_hot_meta_padding_exceeds_limit() {
        HotAccountMeta::new().with_account_data_padding(MAX_HOT_PADDING + 1);
    }

    #[test]
    #[should_panic(expected = "owner_index exceeds MAX_HOT_OWNER_INDEX")]
    fn test_hot_meta_owner_index_exceeds_limit() {
        HotAccountMeta::new().with_owner_index(MAX_HOT_OWNER_INDEX + 1);
    }

    #[test]
    fn test_hot_account_meta() {
        const TEST_LAMPORTS: u64 = 2314232137;
        const TEST_PADDING: u8 = 5;
        const TEST_OWNER_INDEX: u32 = 0x1fef_1234;
        const TEST_RENT_EPOCH: Epoch = 7;

        let optional_fields = AccountMetaOptionalFields {
            rent_epoch: Some(TEST_RENT_EPOCH),
            account_hash: Some(Hash::new_unique()),
            write_version: None,
        };

        let flags = AccountMetaFlags::new_from(&optional_fields);
        let meta = HotAccountMeta::new()
            .with_lamports(TEST_LAMPORTS)
            .with_account_data_padding(TEST_PADDING)
            .with_owner_index(TEST_OWNER_INDEX)
            .with_flags(&flags);

        assert_eq!(meta.lamports(), TEST_LAMPORTS);
        assert_eq!(meta.account_data_padding(), TEST_PADDING);
        assert_eq!(meta.owner_index(), TEST_OWNER_INDEX);
        assert_eq!(*meta.flags(), flags);
    }

    #[test]
    fn test_hot_account_meta_full() {
        let account_data = [11u8; 83];
        let padding = [0u8; 5];

        const TEST_LAMPORT: u64 = 2314232137;
        const OWNER_INDEX: u32 = 0x1fef_1234;
        const TEST_RENT_EPOCH: Epoch = 7;
        const TEST_WRITE_VERSION: StoredMetaWriteVersion = 0;

        let optional_fields = AccountMetaOptionalFields {
            rent_epoch: Some(TEST_RENT_EPOCH),
            account_hash: Some(Hash::new_unique()),
            write_version: Some(TEST_WRITE_VERSION),
        };

        let flags = AccountMetaFlags::new_from(&optional_fields);
        let expected_meta = HotAccountMeta::new()
            .with_lamports(TEST_LAMPORT)
            .with_account_data_padding(padding.len().try_into().unwrap())
            .with_owner_index(OWNER_INDEX)
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
        assert!(meta.flags().has_write_version());
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
        assert_eq!(
            meta.write_version(account_block),
            optional_fields.write_version
        );
    }

    #[test]
    fn test_hot_storage_footer() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_hot_storage_footer");
        let expected_footer = TieredStorageFooter {
            account_meta_format: AccountMetaFormat::Hot,
            owners_block_format: OwnersBlockFormat::LocalIndex,
            account_index_format: AccountIndexFormat::AddressAndOffset,
            account_block_format: AccountBlockFormat::AlignedRaw,
            account_entry_count: 300,
            account_meta_entry_size: 16,
            account_block_size: 4096,
            owner_count: 250,
            owner_entry_size: 32,
            account_index_offset: 1069600,
            owners_offset: 1081200,
            hash: Hash::new_unique(),
            min_account_address: Pubkey::default(),
            max_account_address: Pubkey::new_unique(),
            footer_size: FOOTER_SIZE as u64,
            format_version: 1,
        };

        {
            let file = TieredStorageFile::new_writable(&path);
            expected_footer.write_footer_block(&file).unwrap();
        }

        // Reopen the same storage, and expect the persisted footer is
        // the same as what we have written.
        {
            let hot_storage = HotAccountsFileReader::new_from_path(&path).unwrap();
            assert_eq!(expected_footer, *hot_storage.footer());
        }
    }
}

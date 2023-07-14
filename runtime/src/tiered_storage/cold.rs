/*
use {
    crate::{
        account_storage::meta::{StoredAccountMeta, StoredMetaWriteVersion},
        accounts_file::ALIGN_BOUNDARY_OFFSET,
        append_vec::MatchAccountOwnerError,
        tiered_storage::{
            byte_block::ByteBlockReader,
            file::TieredStorageFile,
            footer::{
                AccountBlockFormat, AccountIndexFormat, AccountMetaFormat, OwnersBlockFormat,
                TieredFileFormat, TieredStorageFooter,
            },
            meta::{
                get_compressed_block_size, AccountMetaFlags, TieredAccountMeta,
                ACCOUNT_DATA_ENTIRE_BLOCK, DEFAULT_ACCOUNT_HASH,
            },
            readable::TieredReadableAccount,
            TieredStorageResult,
        },
    },
    log::*,
    solana_sdk::{hash::Hash, pubkey::Pubkey, stake_history::Epoch},
    std::{collections::HashMap, path::Path},
};

pub static COLD_FORMAT: TieredFileFormat = TieredFileFormat {
    meta_entry_size: std::mem::size_of::<ColdAccountMeta>(),
    account_meta_format: AccountMetaFormat::Cold,
    owners_block_format: OwnersBlockFormat::LocalIndex,
    account_index_format: AccountIndexFormat::AddressAndOffset,
    account_block_format: AccountBlockFormat::Lz4,
};

#[derive(Debug)]
pub struct ColdStorageReader {
    pub(crate) footer: TieredStorageFooter,
    pub(crate) metas: Vec<ColdAccountMeta>,
    accounts: Vec<Pubkey>,
    owners: Vec<Pubkey>,
    data_blocks: HashMap<u64, Vec<u8>>,
}

impl ColdStorageReader {
    pub fn new_from_file(file_path: impl AsRef<Path>) -> TieredStorageResult<ColdStorageReader> {
        let storage = TieredStorageFile::new_writable(file_path.as_ref());
        let footer = ColdReaderBuilder::read_footer_block(&storage)?;

        let metas = ColdReaderBuilder::read_account_metas_block(&storage, &footer)?;
        let accounts = ColdReaderBuilder::read_account_addresses_block(&storage, &footer)?;
        let owners = ColdReaderBuilder::read_owners_block(&storage, &footer)?;
        let data_blocks = ColdReaderBuilder::read_data_blocks(&storage, &footer, &metas)?;

        info!(
            "[Cold] Opening cold storage from {}.  Footer: {:?}",
            file_path.as_ref().display(),
            footer
        );

        Ok(ColdStorageReader {
            footer,
            metas,
            accounts,
            owners,
            data_blocks,
        })
    }

    pub fn num_accounts(&self) -> usize {
        self.footer.account_entry_count.try_into().unwrap()
    }

    fn multiplied_index_to_index(multiplied_index: usize) -> usize {
        // This is a temporary workaround to work with existing AccountInfo
        // implementation that ties to AppendVec with the assumption that the offset
        // is a multiple of ALIGN_BOUNDARY_OFFSET, while tiered storage actually talks
        // about index instead of offset.
        multiplied_index / ALIGN_BOUNDARY_OFFSET
    }

    pub fn account_matches_owners(
        &self,
        multiplied_index: usize,
        owners: &[&Pubkey],
    ) -> Result<usize, MatchAccountOwnerError> {
        let index = Self::multiplied_index_to_index(multiplied_index);
        if index >= self.metas.len() {
            return Err(MatchAccountOwnerError::UnableToLoad);
        }

        owners
            .iter()
            .position(|entry| &&self.owners[self.metas[index].owner_local_id() as usize] == entry)
            .ok_or(MatchAccountOwnerError::NoMatch)
    }

    pub fn get_account<'a>(
        &'a self,
        multiplied_index: usize,
    ) -> Option<(StoredAccountMeta<'a>, usize)> {
        let index = Self::multiplied_index_to_index(multiplied_index);
        if index >= self.metas.len() {
            return None;
        }
        if let Some(data_block) = self.data_blocks.get(&self.metas[index].block_offset()) {
            return Some((
                StoredAccountMeta::Cold(TieredReadableAccountMeta {
                    meta: &self.metas[index],
                    pubkey: &self.accounts[index],
                    owner: &self.owners[self.metas[index].owner_local_id() as usize],
                    index: multiplied_index,
                    data_block: data_block,
                }),
                multiplied_index + ALIGN_BOUNDARY_OFFSET,
            ));
        }
        None
    }
}

pub(crate) struct ColdReaderBuilder {}

impl ColdReaderBuilder {
    fn read_footer_block(storage: &TieredStorageFile) -> TieredStorageResult<TieredStorageFooter> {
        TieredStorageFooter::new_from_footer_block(&storage)
    }

    fn read_account_metas_block(
        storage: &TieredStorageFile,
        footer: &TieredStorageFooter,
    ) -> TieredStorageResult<Vec<ColdAccountMeta>> {
        let mut metas: Vec<ColdAccountMeta> =
            Vec::with_capacity(footer.account_entry_count as usize);

        (&storage).seek(0)?;

        for _ in 0..footer.account_entry_count {
            metas.push(ColdAccountMeta::new_from_file(&storage)?);
        }

        Ok(metas)
    }

    fn read_account_addresses_block(
        storage: &TieredStorageFile,
        footer: &TieredStorageFooter,
    ) -> TieredStorageResult<Vec<Pubkey>> {
        Self::read_pubkeys_block(
            storage,
            footer.account_index_offset,
            footer.account_entry_count,
        )
    }

    fn read_owners_block(
        storage: &TieredStorageFile,
        footer: &TieredStorageFooter,
    ) -> TieredStorageResult<Vec<Pubkey>> {
        Self::read_pubkeys_block(storage, footer.owners_offset, footer.owner_count)
    }

    fn read_pubkeys_block(
        storage: &TieredStorageFile,
        offset: u64,
        count: u32,
    ) -> TieredStorageResult<Vec<Pubkey>> {
        let mut addresses: Vec<Pubkey> = Vec::with_capacity(count as usize);
        (&storage).seek(offset)?;
        for _ in 0..count {
            let mut pubkey = Pubkey::default();
            (&storage).read_type(&mut pubkey)?;
            addresses.push(pubkey);
        }

        Ok(addresses)
    }

    pub fn read_data_blocks(
        storage: &TieredStorageFile,
        footer: &TieredStorageFooter,
        metas: &Vec<ColdAccountMeta>,
    ) -> TieredStorageResult<HashMap<u64, Vec<u8>>> {
        let count = footer.account_entry_count as usize;
        let mut data_blocks = HashMap::<u64, Vec<u8>>::new();
        for i in 0..count {
            Self::update_data_block_map(&mut data_blocks, storage, footer, metas, i)?;
        }
        Ok(data_blocks)
    }

    fn update_data_block_map(
        data_blocks: &mut HashMap<u64, Vec<u8>>,
        storage: &TieredStorageFile,
        footer: &TieredStorageFooter,
        metas: &Vec<ColdAccountMeta>,
        index: usize,
    ) -> TieredStorageResult<()> {
        let block_offset = &metas[index].block_offset();
        if !data_blocks.contains_key(&block_offset) {
            let data_block = Self::read_data_block(storage, footer, metas, index).unwrap();

            data_blocks.insert(metas[index].block_offset(), data_block);
        }
        Ok(())
    }

    pub fn read_data_block(
        storage: &TieredStorageFile,
        footer: &TieredStorageFooter,
        metas: &Vec<ColdAccountMeta>,
        index: usize,
    ) -> TieredStorageResult<Vec<u8>> {
        let compressed_block_size = get_compressed_block_size(footer, metas, index) as usize;

        (&storage).seek(metas[index].block_offset())?;

        let mut buffer: Vec<u8> = vec![0; compressed_block_size];
        (&storage).read_bytes(&mut buffer)?;

        // TODO(yhchiang): encoding from footer
        Ok(ByteBlockReader::decode(
            AccountBlockFormat::Lz4,
            &buffer[..],
        )?)
    }
}

#[derive(Debug, PartialEq, Eq)]
#[repr(C)]
pub struct ColdAccountMeta {
    lamports: u64,
    block_offset: u64,
    uncompressed_data_size: u16,
    intra_block_offset: u16,
    owner_local_id: u32,
    flags: AccountMetaFlags,
    // It will still be 32 bytes even without this field.
    _unused: u32,
}

impl TieredAccountMeta for ColdAccountMeta {
    fn new() -> Self {
        Self {
            ..ColdAccountMeta::default()
        }
    }

    fn is_blob_account_data(data_len: u64) -> bool {
        data_len > ACCOUNT_DATA_BLOCK_SIZE as u64
    }

    fn with_lamports(&mut self, lamports: u64) -> &mut Self {
        self.lamports = lamports;
        self
    }

    fn with_block_offset(&mut self, offset: u64) -> &mut Self {
        self.block_offset = offset;
        self
    }

    fn with_data_tailing_paddings(&mut self, _paddings: u8) -> &mut Self {
        // cold storage never has paddings.
        self
    }

    fn with_owner_local_id(&mut self, local_id: u32) -> &mut Self {
        self.owner_local_id = local_id;
        self
    }

    fn with_uncompressed_data_size(&mut self, data_size: u64) -> &mut Self {
        assert!(ACCOUNT_DATA_ENTIRE_BLOCK <= std::u16::MAX);
        if data_size >= ACCOUNT_DATA_ENTIRE_BLOCK as u64 {
            self.uncompressed_data_size = ACCOUNT_DATA_ENTIRE_BLOCK;
        } else {
            self.uncompressed_data_size = data_size as u16;
        }
        self
    }

    fn with_intra_block_offset(&mut self, offset: u16) -> &mut Self {
        self.intra_block_offset = offset;
        self
    }

    fn with_flags(&mut self, flags: &AccountMetaFlags) -> &mut Self {
        self.flags = *flags;
        self
    }

    fn support_shared_byte_block() -> bool {
        true
    }

    fn lamports(&self) -> u64 {
        self.lamports
    }

    fn block_offset(&self) -> u64 {
        self.block_offset
    }

    fn set_block_offset(&mut self, offset: u64) {
        self.block_offset = offset;
    }

    fn padding_bytes(&self) -> u8 {
        0u8
    }

    fn intra_block_offset(&self) -> u16 {
        self.intra_block_offset
    }

    fn owner_local_id(&self) -> u32 {
        self.owner_local_id
    }

    fn flags(&self) -> &AccountMetaFlags {
        &self.flags
    }

    fn uncompressed_data_size(&self) -> usize {
        self.uncompressed_data_size as usize
    }

    fn rent_epoch(&self, data_block: &[u8]) -> Option<Epoch> {
        let offset = self.optional_fields_offset(data_block);
        if self.flags.has_rent_epoch() {
            unsafe {
                let unaligned =
                    std::ptr::addr_of!(data_block[offset..offset + std::mem::size_of::<Epoch>()])
                        as *const Epoch;
                return Some(std::ptr::read_unaligned(unaligned));
            }
        }
        None
    }

    fn account_hash<'a>(&self, data_block: &'a [u8]) -> &'a Hash {
        let mut offset = self.optional_fields_offset(data_block);
        if self.flags.has_rent_epoch() {
            offset += std::mem::size_of::<Epoch>();
        }
        if self.flags.has_account_hash() {
            unsafe {
                let raw_ptr = std::slice::from_raw_parts(
                    data_block[offset..offset + std::mem::size_of::<Hash>()].as_ptr() as *const u8,
                    std::mem::size_of::<Hash>(),
                );
                let ptr: *const Hash = raw_ptr.as_ptr() as *const Hash;
                return &*ptr;
            }
        }
        return &DEFAULT_ACCOUNT_HASH;
    }

    fn write_version(&self, data_block: &[u8]) -> Option<StoredMetaWriteVersion> {
        let mut offset = self.optional_fields_offset(data_block);
        if self.flags.has_rent_epoch() {
            offset += std::mem::size_of::<Epoch>();
        }
        if self.flags.has_account_hash() {
            offset += std::mem::size_of::<Hash>();
        }
        if self.flags.has_write_version() {
            unsafe {
                let unaligned = std::ptr::addr_of!(
                    data_block[offset..offset + std::mem::size_of::<StoredMetaWriteVersion>()]
                ) as *const StoredMetaWriteVersion;
                return Some(std::ptr::read_unaligned(unaligned));
            }
        }
        None
    }

    fn data_len(&self, data_block: &[u8]) -> usize {
        self.optional_fields_offset(data_block)
            .saturating_sub(self.intra_block_offset as usize)
    }

    fn optional_fields_offset<'a>(&self, data_block: &'a [u8]) -> usize {
        if self.is_blob_account() {
            return data_block.len().saturating_sub(self.optional_fields_size());
        }
        (self.intra_block_offset + self.uncompressed_data_size) as usize
    }

    fn account_data<'a>(&self, data_block: &'a [u8]) -> &'a [u8] {
        &data_block[(self.intra_block_offset as usize)..self.optional_fields_offset(data_block)]
    }

    fn is_blob_account(&self) -> bool {
        self.uncompressed_data_size == ACCOUNT_DATA_ENTIRE_BLOCK && self.intra_block_offset == 0
    }

    fn stored_size(
        footer: &TieredStorageFooter,
        metas: &Vec<impl TieredAccountMeta>,
        i: usize,
    ) -> usize {
        let compressed_block_size = get_compressed_block_size(footer, metas, i);

        let data_size = if metas[i].is_blob_account() {
            compressed_block_size
        } else {
            let compression_rate: f64 =
                compressed_block_size as f64 / Self::get_raw_block_size(metas, i) as f64;

            ((metas[i].uncompressed_data_size() as usize + metas[i].optional_fields_size()) as f64
                / compression_rate) as usize
        };

        return std::mem::size_of::<ColdAccountMeta>() + data_size;
    }
}

impl ColdAccountMeta {
    fn new_from_file(file: &TieredStorageFile) -> TieredStorageResult<Self> {
        let mut entry = ColdAccountMeta::new();
        file.read_type(&mut entry)?;

        Ok(entry)
    }

    pub fn get_raw_block_size(metas: &Vec<impl TieredAccountMeta>, index: usize) -> usize {
        let mut block_size = 0;

        for i in index..metas.len() {
            if metas[i].block_offset() == metas[index].block_offset() {
                block_size += metas[i].uncompressed_data_size();
            } else {
                break;
            }
        }

        block_size.try_into().unwrap()
    }
}

impl Default for ColdAccountMeta {
    fn default() -> Self {
        Self {
            lamports: 0,
            block_offset: 0,
            owner_local_id: 0,
            uncompressed_data_size: 0,
            intra_block_offset: 0,
            flags: AccountMetaFlags::new(),
            _unused: 0,
        }
    }
}

#[cfg(test)]
pub mod tests {
    use {
        crate::{
            account_storage::meta::StoredMetaWriteVersion,
            append_vec::test_utils::get_append_vec_path,
            tiered_storage::{
                cold::ColdAccountMeta,
                file::TieredStorageFile,
                meta::{AccountMetaFlags, AccountMetaOptionalFields, TieredAccountMeta},
            },
        },
        ::solana_sdk::{hash::Hash, stake_history::Epoch},
        memoffset::offset_of,
    };

    #[test]
    fn test_account_meta_entry() {
        let path = get_append_vec_path("test_account_meta_entry");

        const TEST_LAMPORT: u64 = 7;
        const BLOCK_OFFSET: u64 = 56987;
        const OWNER_LOCAL_ID: u32 = 54;
        const UNCOMPRESSED_LENGTH: u64 = 255;
        const LOCAL_OFFSET: u16 = 82;
        const TEST_RENT_EPOCH: Epoch = 7;
        const TEST_WRITE_VERSION: StoredMetaWriteVersion = 0;

        let optional_fields = AccountMetaOptionalFields {
            rent_epoch: Some(TEST_RENT_EPOCH),
            account_hash: Some(Hash::new_unique()),
            write_version: Some(TEST_WRITE_VERSION),
        };

        let mut expected_entry = ColdAccountMeta::new();
        let mut flags = AccountMetaFlags::new_from(&optional_fields);
        flags.set_executable(true);
        expected_entry
            .with_lamports(TEST_LAMPORT)
            .with_block_offset(BLOCK_OFFSET)
            .with_owner_local_id(OWNER_LOCAL_ID)
            .with_uncompressed_data_size(UNCOMPRESSED_LENGTH)
            .with_intra_block_offset(LOCAL_OFFSET)
            .with_flags(&flags);

        {
            let ads_file = TieredStorageFile::new_writable(&path.path);
            ads_file.write_type(&expected_entry);
        }

        let mut file = TieredStorageFile::new_readonly(&path.path);
        let entry = ColdAccountMeta::new_from_file(&mut file).unwrap();

        assert_eq!(expected_entry, entry);
        assert!(entry.flags.executable());
        assert!(entry.flags.has_rent_epoch());
    }

    #[test]
    fn test_cold_account_meta_layout() {
        const COLD_META_SIZE_BYTES: usize = 32;
        assert_eq!(offset_of!(ColdAccountMeta, lamports), 0x00);
        assert_eq!(offset_of!(ColdAccountMeta, block_offset), 0x08);
        assert_eq!(offset_of!(ColdAccountMeta, uncompressed_data_size), 0x10);
        assert_eq!(offset_of!(ColdAccountMeta, intra_block_offset), 0x12);
        assert_eq!(offset_of!(ColdAccountMeta, owner_local_id), 0x14);
        assert_eq!(offset_of!(ColdAccountMeta, flags), 0x18);
        assert_eq!(std::mem::size_of::<ColdAccountMeta>(), COLD_META_SIZE_BYTES);
    }
}
*/

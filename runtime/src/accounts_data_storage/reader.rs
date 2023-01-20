use {
    crate::{
        account_storage::meta::{StoredAccountMeta, StoredMeta, StoredMetaWriteVersion},
        accounts_data_storage::{
            data_block::{AccountDataBlock, AccountDataBlockFormat},
            error::AccountsDataStorageError,
            file::AccountsDataStorageFile,
            footer::AccountsDataStorageFooter,
            meta_entries::{AccountMetaFlags, AccountMetaStorageEntry},
        },
    },
    solana_sdk::{
        account::{Account, AccountSharedData, ReadableAccount},
        hash::Hash,
        pubkey::Pubkey,
        stake_history::Epoch,
    },
    std::{collections::HashMap, path::Path},
};

pub type Result<T> = std::result::Result<T, AccountsDataStorageError>;

#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct TieredStorageReader {
    pub(crate) footer: AccountsDataStorageFooter,
    pub(crate) metas: Vec<AccountMetaStorageEntry>,
    pub(crate) accounts: Vec<Pubkey>,
    pub(crate) owners: Vec<Pubkey>,
    pub(crate) data_blocks: HashMap<u64, Vec<u8>>,
}

impl TieredStorageReader {
    pub fn new_from_path(path: &Path) -> Self {
        TieredStorageReaderBuilder::new_reader(path).unwrap()
    }

    pub fn get_account<'a>(&'a self, index: usize) -> Option<(StoredAccountMeta<'a>, usize)> {
        if index >= self.metas.len() {
            return None;
        }
        if let Some(data_block) = self.data_blocks.get(&self.metas[index].block_offset) {
            return Some((
                StoredAccountMeta::Tiered(TieredAccountMeta {
                    meta: &self.metas[index],
                    pubkey: &self.accounts[index],
                    owner: &self.owners[self.metas[index].owner_local_id as usize],
                    index: index,
                    data_block: data_block,
                }),
                index + 1,
            ));
        }
        None
    }
}

#[derive(PartialEq, Eq, Debug)]
#[allow(dead_code)]
pub struct TieredAccountMeta<'a> {
    meta: &'a AccountMetaStorageEntry,
    pubkey: &'a Pubkey,
    owner: &'a Pubkey,
    index: usize,
    // this data block may be shared with other accounts
    data_block: &'a [u8],
}

#[allow(dead_code)]
impl<'a> TieredAccountMeta<'a> {
    pub fn pubkey(&self) -> &Pubkey {
        &self.pubkey
    }

    pub fn hash(&self) -> &Hash {
        self.meta.account_hash(self.data_block)
    }

    pub fn offset(&self) -> usize {
        self.index
    }

    pub fn data_len(&self) -> u64 {
        self.meta.account_data(self.data_block).len() as u64
    }

    pub fn stored_size(&self) -> usize {
        self.data_len() as usize / 2
            + std::mem::size_of::<AccountMetaStorageEntry>()
            + std::mem::size_of::<Pubkey>() // account's pubkey
            + std::mem::size_of::<Pubkey>() // owner's pubkey
    }

    pub fn clone_account(&self) -> AccountSharedData {
        AccountSharedData::from(Account {
            lamports: self.lamports(),
            owner: *self.owner(),
            executable: self.executable(),
            rent_epoch: self.rent_epoch(),
            data: self.data().to_vec(),
        })
    }

    pub fn write_version(&self) -> StoredMetaWriteVersion {
        if let Some(write_version) = self.meta.write_version(self.data_block) {
            return write_version;
        }
        0
    }

    ///////////////////////////////////////////////////////////////////////////
    // Unimlpemented

    pub fn meta(&self) -> &StoredMeta {
        unimplemented!();
    }

    pub fn set_meta(&mut self, _meta: &'a StoredMeta) {
        unimplemented!();
    }

    pub(crate) fn sanitize(&self) -> bool {
        unimplemented!();
    }

    pub(crate) fn sanitize_executable(&self) -> bool {
        unimplemented!();
    }

    pub(crate) fn sanitize_lamports(&self) -> bool {
        unimplemented!();
    }

    pub(crate) fn ref_executable_byte(&self) -> &u8 {
        unimplemented!();
    }
}

impl<'a> ReadableAccount for TieredAccountMeta<'a> {
    fn lamports(&self) -> u64 {
        self.meta.lamports
    }
    fn owner(&self) -> &Pubkey {
        self.owner
    }
    fn executable(&self) -> bool {
        self.meta.flags_get(AccountMetaFlags::EXECUTABLE)
    }
    fn rent_epoch(&self) -> Epoch {
        if let Some(rent_epoch) = self.meta.rent_epoch(self.data_block) {
            return rent_epoch;
        }
        std::u64::MAX
    }
    fn data(&self) -> &[u8] {
        self.meta.account_data(self.data_block)
    }
}

pub(crate) struct TieredStorageReaderBuilder {}

impl TieredStorageReaderBuilder {
    #[allow(dead_code)]
    fn new_reader(file_path: &Path) -> Result<TieredStorageReader> {
        let storage = AccountsDataStorageFile::new(file_path, false /* create */);
        let footer = Self::read_footer_block(&storage).unwrap();

        let metas = Self::read_account_metas_block(&storage, &footer).unwrap();
        let accounts = Self::read_account_addresses_block(&storage, &footer).unwrap();
        let owners = Self::read_owners_block(&storage, &footer).unwrap();
        let data_blocks = Self::read_data_blocks(&storage, &footer, &metas).unwrap();

        Ok(TieredStorageReader {
            footer,
            metas,
            accounts,
            owners,
            data_blocks,
        })
    }

    #[allow(dead_code)]
    fn read_footer_block(storage: &AccountsDataStorageFile) -> Result<AccountsDataStorageFooter> {
        AccountsDataStorageFooter::new_from_footer_block(&storage)
    }

    #[allow(dead_code)]
    fn read_account_metas_block(
        storage: &AccountsDataStorageFile,
        footer: &AccountsDataStorageFooter,
    ) -> Result<Vec<AccountMetaStorageEntry>> {
        let mut metas: Vec<AccountMetaStorageEntry> =
            Vec::with_capacity(footer.account_meta_count as usize);

        (&storage).seek(footer.account_metas_offset)?;

        for _ in 0..footer.account_meta_count {
            metas.push(AccountMetaStorageEntry::new_from_file(&storage)?);
        }

        Ok(metas)
    }

    #[allow(dead_code)]
    fn read_account_addresses_block(
        storage: &AccountsDataStorageFile,
        footer: &AccountsDataStorageFooter,
    ) -> Result<Vec<Pubkey>> {
        Self::read_pubkeys_block(
            storage,
            footer.account_pubkeys_offset,
            footer.account_meta_count,
        )
    }

    #[allow(dead_code)]
    fn read_owners_block(
        storage: &AccountsDataStorageFile,
        footer: &AccountsDataStorageFooter,
    ) -> Result<Vec<Pubkey>> {
        Self::read_pubkeys_block(storage, footer.owners_offset, footer.owner_count)
    }

    #[allow(dead_code)]
    fn read_pubkeys_block(
        storage: &AccountsDataStorageFile,
        offset: u64,
        count: u32,
    ) -> Result<Vec<Pubkey>> {
        let mut addresses: Vec<Pubkey> = Vec::with_capacity(count as usize);
        (&storage).seek(offset)?;
        for _ in 0..count {
            let mut pubkey = Pubkey::default();
            (&storage).read_type(&mut pubkey)?;
            addresses.push(pubkey);
        }

        Ok(addresses)
    }

    #[allow(dead_code)]
    pub fn read_data_blocks(
        storage: &AccountsDataStorageFile,
        footer: &AccountsDataStorageFooter,
        metas: &Vec<AccountMetaStorageEntry>,
    ) -> Result<HashMap<u64, Vec<u8>>> {
        let count = footer.account_meta_count as usize;
        let mut data_blocks = HashMap::<u64, Vec<u8>>::new();
        for i in 0..count {
            Self::update_data_block_map(&mut data_blocks, storage, footer, metas, i)?;
        }
        Ok(data_blocks)
    }

    #[allow(dead_code)]
    fn update_data_block_map(
        data_blocks: &mut HashMap<u64, Vec<u8>>,
        storage: &AccountsDataStorageFile,
        footer: &AccountsDataStorageFooter,
        metas: &Vec<AccountMetaStorageEntry>,
        index: usize,
    ) -> Result<()> {
        let block_offset = &metas[index].block_offset;
        if !data_blocks.contains_key(&block_offset) {
            let data_block = Self::read_data_block(storage, footer, metas, index).unwrap();

            data_blocks.insert(metas[index].block_offset, data_block);
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub fn read_data_block(
        storage: &AccountsDataStorageFile,
        footer: &AccountsDataStorageFooter,
        metas: &Vec<AccountMetaStorageEntry>,
        index: usize,
    ) -> Result<Vec<u8>> {
        let compressed_block_size = Self::get_compressed_block_size(footer, metas, index) as usize;

        (&storage).seek(metas[index].block_offset)?;

        let mut buffer: Vec<u8> = vec![0; compressed_block_size];
        (&storage).read_bytes(&mut buffer)?;

        // TODO(yhchiang): encoding from footer
        Ok(AccountDataBlock::decode(
            AccountDataBlockFormat::Lz4,
            &buffer[..],
        )?)
    }

    pub(crate) fn get_compressed_block_size(
        footer: &AccountsDataStorageFooter,
        metas: &Vec<AccountMetaStorageEntry>,
        index: usize,
    ) -> usize {
        let mut block_size = footer.account_metas_offset - metas[index].block_offset;

        for i in index..metas.len() {
            if metas[i].block_offset == metas[index].block_offset {
                continue;
            }
            block_size = metas[i].block_offset - metas[index].block_offset;
            break;
        }

        block_size.try_into().unwrap()
    }
}

/*
    #[test]
    fn test_account_pubkeys_block() {
        let path = get_append_vec_path("test_account_pubkeys_block");
        let mut expected_pubkeys: Vec<Pubkey> = vec![];
        const ENTRY_COUNT: u32 = 1024;

        {
            let ads = AccountsDataStorageWriter::new(&path.path);
            let mut footer = AccountsDataStorageFooter::new();
            let mut cursor = 0;
            for _ in 0..ENTRY_COUNT {
                expected_pubkeys.push(Pubkey::new_unique());
            }
            ads.write_account_pubkeys_block(&mut cursor, &mut footer, &expected_pubkeys)
                .unwrap();
        }

        let ads = AccountsDataStorage::new_for_test(&path.path, false);
        let pubkeys: Vec<Pubkey> = ads.read_pubkeys_block(0, ENTRY_COUNT).unwrap();
        assert_eq!(expected_pubkeys, pubkeys);
    }
*/

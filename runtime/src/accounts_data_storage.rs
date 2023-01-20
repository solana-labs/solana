//! docs/src/proposals/append-vec-storage.md

pub mod data_block;
pub mod error;
pub mod file;
pub mod footer;
pub mod meta_entries;
pub mod reader;
pub mod writer;

use {
    crate::{
        account_storage::meta::{
            StorableAccountsWithHashesAndWriteVersions, StoredAccountInfo, StoredAccountMeta,
        },
        append_vec::{AppendVec, APPEND_VEC_MMAPPED_FILES_OPEN},
        storable_accounts::StorableAccounts,
    },
    data_block::AccountDataBlockWriter,
    error::AccountsDataStorageError,
    file::AccountsDataStorageFile,
    log::log_enabled,
    once_cell::sync::OnceCell,
    reader::TieredStorageReader,
    solana_sdk::{account::ReadableAccount, hash::Hash},
    std::{
        borrow::Borrow,
        fs::remove_file,
        path::{Path, PathBuf},
        sync::atomic::Ordering,
    },
    writer::AccountsDataStorageWriter,
};

pub const ACCOUNT_DATA_BLOCK_SIZE: usize = 4096;
pub const ACCOUNTS_DATA_STORAGE_FORMAT_VERSION: u64 = 1;

pub type Result<T> = std::result::Result<T, AccountsDataStorageError>;

lazy_static! {
    pub static ref HASH_DEFAULT: Hash = Hash::default();
}

#[derive(Debug)]
pub struct AccountsDataStorage {
    reader: OnceCell<TieredStorageReader>,
    storage: AccountsDataStorageFile,
    path: PathBuf,
    remove_on_drop: bool,
}

impl Drop for AccountsDataStorage {
    fn drop(&mut self) {
        if self.remove_on_drop {
            APPEND_VEC_MMAPPED_FILES_OPEN.fetch_sub(1, Ordering::Relaxed);
            if let Err(_e) = remove_file(&self.path) {
                // promote this to panic soon.
                // disabled due to many false positive warnings while running tests.
                // blocked by rpc's upgrade to jsonrpc v17
                //error!("AppendVec failed to remove {:?}: {:?}", &self.path, e);
                inc_new_counter_info!("append_vec_drop_fail", 1);
            }
        }
    }
}

impl AccountsDataStorage {
    /// Create a new accounts-state-file
    pub fn new(file_path: &Path, create: bool) -> Self {
        if create {
            let _ignored = remove_file(file_path);
            Self {
                reader: OnceCell::<TieredStorageReader>::new(),
                storage: AccountsDataStorageFile::new(file_path, create),
                path: file_path.to_path_buf(),
                remove_on_drop: true,
            }
        } else {
            let reader = OnceCell::<TieredStorageReader>::new();
            reader
                .set(TieredStorageReader::new_from_path(file_path))
                .unwrap();
            Self {
                reader,
                storage: AccountsDataStorageFile::new(file_path, create),
                path: file_path.to_path_buf(),
                remove_on_drop: true,
            }
        }
    }

    pub fn get_account<'a>(&'a self, index: usize) -> Option<(StoredAccountMeta<'a>, usize)> {
        if let Some(reader) = self.reader.get() {
            return reader.get_account(index);
        }
        None
    }

    pub fn get_path(&self) -> PathBuf {
        self.path.clone()
    }

    pub fn accounts(&self, mut index: usize) -> Vec<StoredAccountMeta> {
        let mut accounts = vec![];
        while let Some((account, next)) = self.get_account(index) {
            accounts.push(account);
            index = next;
        }
        accounts
    }

    // Returns the Vec of offsets corresponding to the input accounts to later
    // construct AccountInfo
    pub fn append_accounts<
        'a,
        'b,
        T: ReadableAccount + Sync,
        U: StorableAccounts<'a, T>,
        V: Borrow<Hash>,
    >(
        &self,
        accounts: &StorableAccountsWithHashesAndWriteVersions<'a, 'b, T, U, V>,
        skip: usize,
    ) -> Option<Vec<StoredAccountInfo>> {
        if self.is_read_only() {
            return None;
        }

        let result: Option<Vec<StoredAccountInfo>>;
        {
            let writer = AccountsDataStorageWriter::new(&self.path);
            result = writer.append_accounts(accounts, skip);
        }
        if self
            .reader
            .set(TieredStorageReader::new_from_path(&self.path))
            .is_ok()
        {
            return None;
        }
        result
    }

    pub fn file_size(&self) -> Result<u64> {
        Ok(self.storage.file.metadata().unwrap().len())
    }

    pub fn is_read_only(&self) -> bool {
        self.reader.get().is_some()
    }

    pub fn write_from_append_vec(&self, append_vec: &AppendVec) -> Result<()> {
        let writer = AccountsDataStorageWriter::new(&self.path);
        writer.write_from_append_vec(&append_vec)
    }

    ///////////////////////////////////////////////////////////////////////////////

    pub fn set_no_remove_on_drop(&mut self) {
        self.remove_on_drop = false;
    }

    pub fn flush(&self) -> std::io::Result<()> {
        Ok(())
    }

    pub fn reset(&self) {}

    pub fn remaining_bytes(&self) -> u64 {
        self.capacity() - self.len() as u64
    }

    pub fn len(&self) -> usize {
        self.file_size().unwrap_or(0).try_into().unwrap()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn capacity(&self) -> u64 {
        std::u64::MAX
    }

    pub fn is_ancient(&self) -> bool {
        false
    }
}

#[cfg(test)]
pub mod tests {
    use {
        crate::{
            account_storage::meta::{StorableAccountsWithHashesAndWriteVersions, StoredMeta},
            accounts_data_storage::{
                data_block::AccountDataBlockFormat,
                file::AccountsDataStorageFile,
                footer::{AccountsDataStorageFooter, FOOTER_SIZE},
                meta_entries::{AccountMetaStorageEntry, ACCOUNT_META_ENTRY_SIZE_BYTES},
                reader::TieredStorageReader,
                AccountsDataStorage, ACCOUNTS_DATA_STORAGE_FORMAT_VERSION, ACCOUNT_DATA_BLOCK_SIZE,
            },
            append_vec::{
                test_utils::{create_test_account_from_len, get_append_vec_path, TempFile},
                AppendVec,
            },
        },
        once_cell::sync::OnceCell,
        solana_sdk::{account::AccountSharedData, clock::Slot, hash::Hash, pubkey::Pubkey},
        std::{collections::HashMap, mem, path::Path},
    };

    impl AccountsDataStorage {
        fn new_for_test(file_path: &Path, create: bool) -> Self {
            Self {
                reader: OnceCell::<TieredStorageReader>::new(),
                storage: AccountsDataStorageFile::new(file_path, create),
                path: file_path.to_path_buf(),
                remove_on_drop: false,
            }
        }
    }

    /*
    #[test]
    fn test_account_metas_block() {
        let path = get_append_vec_path("test_account_metas_block");

        const ENTRY_COUNT: u64 = 128;
        const TEST_LAMPORT_BASE: u64 = 48372;
        const BLOCK_OFFSET_BASE: u64 = 3423;
        const DATA_LENGTH: u16 = 976;
        const TEST_RENT_EPOCH: Epoch = 327;
        const TEST_WRITE_VERSION: StoredMetaWriteVersion = 543432;
        let mut expected_metas: Vec<AccountMetaStorageEntry> = vec![];

        {
            let ads = AccountsDataStorageWriter::new(&path.path);
            let mut footer = AccountsDataStorageFooter::new();
            let mut cursor = 0;
            let meta_per_block = (ACCOUNT_DATA_BLOCK_SIZE as u16) / DATA_LENGTH;
            for i in 0..ENTRY_COUNT {
                expected_metas.push(
                    AccountMetaStorageEntry::new()
                        .with_lamports(i * TEST_LAMPORT_BASE)
                        .with_block_offset(i * BLOCK_OFFSET_BASE)
                        .with_owner_local_id(i as u32)
                        .with_uncompressed_data_size(DATA_LENGTH)
                        .with_intra_block_offset(((i as u16) % meta_per_block) * DATA_LENGTH)
                        .with_flags(
                            AccountMetaFlags::new()
                                .with_bit(AccountMetaFlags::EXECUTABLE, i % 2 == 0)
                                .to_value(),
                        )
                        .with_optional_fields(&AccountMetaOptionalFields {
                            rent_epoch: if i % 2 == 1 {
                                Some(TEST_RENT_EPOCH)
                            } else {
                                None
                            },
                            account_hash: if i % 2 == 0 {
                                Some(Hash::new_unique())
                            } else {
                                None
                            },
                            write_version_obsolete: if i % 2 == 1 {
                                Some(TEST_WRITE_VERSION)
                            } else {
                                None
                            },
                        }),
                );
            }
            ads.write_account_metas_block(&mut cursor, &mut footer, &expected_metas)
                .unwrap();
        }

        let ads = AccountsDataStorage::new_for_test(&path.path, false);
        let metas: Vec<AccountMetaStorageEntry> =
            ads.read_account_metas_block(0, ENTRY_COUNT as u32).unwrap();
        assert_eq!(expected_metas, metas);
        for i in 0..ENTRY_COUNT as usize {
            assert_eq!(
                metas[i].flags_get(AccountMetaFlags::HAS_RENT_EPOCH),
                i % 2 == 1
            );
            assert_eq!(
                metas[i].flags_get(AccountMetaFlags::HAS_ACCOUNT_HASH),
                i % 2 == 0
            );
            assert_eq!(
                metas[i].flags_get(AccountMetaFlags::HAS_WRITE_VERSION),
                i % 2 == 1
            );
        }
    }*/

    fn create_test_append_vec(
        path: &str,
        data_sizes: &[usize],
    ) -> (HashMap<Pubkey, (StoredMeta, AccountSharedData)>, AppendVec) {
        let av_path = get_append_vec_path(path);
        let av = AppendVec::new(&av_path.path, true, 100 * 1024 * 1024);
        let mut test_accounts: HashMap<Pubkey, (StoredMeta, AccountSharedData)> = HashMap::new();

        for size in data_sizes {
            let account = create_test_account_from_len(*size);
            let index = av.append_account_test(&account).unwrap();
            assert_eq!(av.get_account_test(index).unwrap(), account);
            test_accounts.insert(account.0.pubkey, account);
        }

        (test_accounts, av)
    }

    fn ads_writer_test_help(path_prefix: &str, account_data_sizes: &[usize]) {
        write_from_append_vec_test_helper(
            &(path_prefix.to_owned() + "_from_append_vec"),
            account_data_sizes,
        );
        append_accounts_test_helper(
            &(path_prefix.to_owned() + "_append_accounts"),
            account_data_sizes,
        );
    }

    fn append_accounts_test_helper(path_prefix: &str, account_data_sizes: &[usize]) {
        let account_count = account_data_sizes.len();
        let (test_accounts, _av) =
            create_test_append_vec(&(path_prefix.to_owned() + "_av"), account_data_sizes);

        let slot_ignored = Slot::MAX;
        let accounts: Vec<(Pubkey, AccountSharedData)> = test_accounts
            .clone()
            .into_iter()
            .map(|(pubkey, acc)| (pubkey, acc.1))
            .collect();
        let mut accounts_ref: Vec<(&Pubkey, &AccountSharedData)> = Vec::new();

        for (x, y) in &accounts {
            accounts_ref.push((&x, &y));
        }

        let slice = &accounts_ref[..];
        let account_data = (slot_ignored, slice);
        let mut write_versions = Vec::new();

        for (_pubkey, acc) in &test_accounts {
            write_versions.push(acc.0.write_version_obsolete);
        }

        let mut hashes = Vec::new();
        let mut hashes_ref = Vec::new();
        let mut hashes_map = HashMap::new();

        for _ in 0..write_versions.len() {
            hashes.push(Hash::new_unique());
        }
        for i in 0..write_versions.len() {
            hashes_ref.push(&hashes[i]);
        }
        for i in 0..write_versions.len() {
            hashes_map.insert(accounts[i].0, &hashes[i]);
        }

        let storable_accounts =
            StorableAccountsWithHashesAndWriteVersions::new_with_hashes_and_write_versions(
                &account_data,
                hashes_ref,
                write_versions,
            );

        let ads_path = get_append_vec_path(&(path_prefix.to_owned() + "_ads"));
        {
            let ads = AccountsDataStorage::new_for_test(&ads_path.path, true);
            ads.append_accounts(&storable_accounts, 0);
        }

        verify_account_data_storage(account_count, &test_accounts, &ads_path, &hashes_map);
    }

    fn write_from_append_vec_test_helper(path_prefix: &str, account_data_sizes: &[usize]) {
        let account_count = account_data_sizes.len();
        let (test_accounts, av) =
            create_test_append_vec(&(path_prefix.to_owned() + "_av"), account_data_sizes);

        let ads_path = get_append_vec_path(&(path_prefix.to_owned() + "_ads"));
        {
            let ads = AccountsDataStorage::new_for_test(&ads_path.path, true);
            ads.write_from_append_vec(&av).unwrap();
        }

        verify_account_data_storage(account_count, &test_accounts, &ads_path, &HashMap::new());
    }

    impl AccountsDataStorage {
        pub(crate) fn footer(&self) -> Option<&AccountsDataStorageFooter> {
            if let Some(reader) = self.reader.get() {
                return Some(&reader.footer);
            }
            None
        }

        pub(crate) fn metas(&self) -> Option<&Vec<AccountMetaStorageEntry>> {
            if let Some(reader) = self.reader.get() {
                return Some(&reader.metas);
            }
            None
        }
    }

    /*
    fn verify_account_data_storage2(
        account_count: usize,
        test_accounts: &HashMap<Pubkey, (StoredMeta, AccountSharedData)>,
        ads_path: &TempFile,
        hashes_map: &HashMap<Pubkey, &Hash>,
    ) {
        let ads = AccountsDataStorage::new(&ads_path.path, false);
        let footer = ads.footer().unwrap();

        let expected_footer = AccountsDataStorageFooter {
            account_meta_count: account_count as u32,
            account_meta_entry_size: ACCOUNT_META_ENTRY_SIZE_BYTES,
            account_data_block_size: ACCOUNT_DATA_BLOCK_SIZE as u64,
            owner_count: account_count as u32,
            owner_entry_size: mem::size_of::<Pubkey>() as u32,
            // This number should be the total compressed account data size.
            account_metas_offset: footer.account_metas_offset,
            account_pubkeys_offset: footer.account_pubkeys_offset,
            owners_offset: footer.account_pubkeys_offset
                + (account_count * mem::size_of::<Pubkey>()) as u64,
            // TODO(yhchiang): not yet implemented
            data_block_format: AccountDataBlockFormat::Lz4,
            // TODO(yhchiang): not yet implemented
            hash: footer.hash,
            // TODO(yhchiang): fix this
            min_account_address: Hash::default(),
            max_account_address: Hash::default(),
            format_version: ACCOUNTS_DATA_STORAGE_FORMAT_VERSION,
            footer_size: FOOTER_SIZE as u64,
        };
        assert_eq!(*footer, expected_footer);

        let mut index = 0;
        let mut count_from_ads = 0;

        while let Some((account, next)) = ads.get_account(index) {
            index = next;
            count_from_ads += 1;
            let expected_account = &test_accounts[account.pubkey()];
            let expected_hash = &hashes_map[account.pubkey()];
            verify_account(&account, expected_account);
            assert_eq!(account.hash(), *expected_hash);
        }
        assert_eq!(&count_from_ads, &account_count);
    }
    */

    fn verify_account_data_storage(
        account_count: usize,
        test_accounts: &HashMap<Pubkey, (StoredMeta, AccountSharedData)>,
        ads_path: &TempFile,
        hashes_map: &HashMap<Pubkey, &Hash>,
    ) {
        let ads = AccountsDataStorage::new(&ads_path.path, false);
        let footer = ads.footer().unwrap();

        let expected_footer = AccountsDataStorageFooter {
            account_meta_count: account_count as u32,
            account_meta_entry_size: ACCOUNT_META_ENTRY_SIZE_BYTES,
            account_data_block_size: ACCOUNT_DATA_BLOCK_SIZE as u64,
            owner_count: account_count as u32,
            owner_entry_size: mem::size_of::<Pubkey>() as u32,
            // This number should be the total compressed account data size.
            account_metas_offset: footer.account_metas_offset,
            account_pubkeys_offset: footer.account_pubkeys_offset,
            owners_offset: footer.account_pubkeys_offset
                + (account_count * mem::size_of::<Pubkey>()) as u64,
            // TODO(yhchiang): not yet implemented
            data_block_format: AccountDataBlockFormat::Lz4,
            // TODO(yhchiang): not yet implemented
            hash: footer.hash,
            // TODO(yhchiang): fix this
            min_account_address: Hash::default(),
            max_account_address: Hash::default(),
            format_version: ACCOUNTS_DATA_STORAGE_FORMAT_VERSION,
            footer_size: FOOTER_SIZE as u64,
        };
        assert_eq!(*footer, expected_footer);

        let metas = ads.metas().unwrap();
        assert_eq!(metas.len(), account_count);

        let mut index = 0;
        let mut count_from_ads = 0;
        while let Some((account, next)) = ads.get_account(index) {
            index = next;
            count_from_ads += 1;
            let expected_account = &test_accounts[account.pubkey()];
            assert_eq!(account.clone_account(), expected_account.1);

            if hashes_map.len() > 0 {
                let expected_hash = &hashes_map[account.pubkey()];
                assert_eq!(account.hash(), *expected_hash);
            }

            let stored_meta_from_storage = StoredMeta {
                write_version_obsolete: account.write_version(),
                pubkey: *account.pubkey(),
                data_len: account.data_len(),
            };
            assert_eq!(stored_meta_from_storage, expected_account.0);
        }
        assert_eq!(&count_from_ads, &account_count);
    }

    #[test]
    fn test_write_from_append_vec_one_small() {
        ads_writer_test_help("test_write_from_append_vec_one_small", &[255]);
    }

    #[test]
    fn test_write_from_append_vec_one_big() {
        ads_writer_test_help("test_write_from_append_vec_one_big", &[25500]);
    }

    #[test]
    fn test_write_from_append_vec_one_10_mb() {
        ads_writer_test_help("test_write_from_append_vec_one_10_mb", &[10 * 1024 * 1024]);
    }

    #[test]
    fn test_write_from_append_vec_multiple_blobs() {
        ads_writer_test_help(
            "test_write_from_append_vec_multiple_blobs",
            &[5000, 6000, 7000, 8000, 5500, 10241023, 9999],
        );
    }

    #[test]
    fn test_write_from_append_vec_one_data_block() {
        ads_writer_test_help(
            "test_write_from_append_vec_one_data_block",
            &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
        );
    }

    #[test]
    fn test_write_from_append_vec_mixed_block() {
        ads_writer_test_help(
            "test_write_from_append_vec_mixed_block",
            &[
                1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 1000, 2000, 3000, 4000, 9, 8, 7, 6, 5, 4, 3, 2, 1,
            ],
        );
    }
}

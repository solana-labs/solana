pub mod byte_block;
pub mod cold;
pub mod error;
pub mod file;
pub mod footer;
pub mod hot;
pub mod index;
pub mod meta;
pub mod mmap_utils;
pub mod readable;
pub mod writer;

use {
    crate::{
        account_storage::meta::{
            StorableAccountsWithHashesAndWriteVersions, StoredAccountInfo, StoredAccountMeta,
        },
        append_vec::MatchAccountOwnerError,
        storable_accounts::StorableAccounts,
    },
    error::TieredStorageError,
    footer::TieredFileFormat,
    log::log_enabled,
    once_cell::sync::OnceCell,
    readable::TieredStorageReader,
    solana_sdk::{account::ReadableAccount, hash::Hash, pubkey::Pubkey},
    std::{
        borrow::Borrow,
        fs::{remove_file, OpenOptions},
        path::{Path, PathBuf},
    },
    writer::TieredStorageWriter,
};

pub type TieredStorageResult<T> = Result<T, TieredStorageError>;

pub const ACCOUNT_DATA_BLOCK_SIZE: usize = 4096;
pub const ACCOUNTS_DATA_STORAGE_FORMAT_VERSION: u64 = 1;

lazy_static! {
    pub static ref HASH_DEFAULT: Hash = Hash::default();
}

#[derive(Debug)]
pub struct TieredStorage {
    reader: OnceCell<TieredStorageReader>,
    // This format will be used when creating new data
    format: Option<&'static TieredFileFormat>,
    path: PathBuf,
    remove_on_drop: bool,
}

impl Drop for TieredStorage {
    fn drop(&mut self) {
        if self.remove_on_drop {
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

impl TieredStorage {
    pub fn new(file_path: &Path, format: Option<&'static TieredFileFormat>) -> Self {
        if format.is_some() {
            let _ignored = remove_file(file_path);
            Self {
                reader: OnceCell::<TieredStorageReader>::new(),
                format: format,
                path: file_path.to_path_buf(),
                remove_on_drop: true,
            }
        } else {
            let (accounts_file, _) = Self::new_from_file(file_path).unwrap();
            return accounts_file;
        }
    }

    pub fn new_from_file<P: AsRef<std::path::Path>>(path: P) -> TieredStorageResult<(Self, usize)> {
        let reader = TieredStorageReader::new_from_path(path.as_ref())?;
        let count = reader.num_accounts();
        let reader_cell = OnceCell::<TieredStorageReader>::new();
        reader_cell.set(reader).unwrap();
        Ok((
            Self {
                reader: reader_cell,
                format: None,
                path: path.as_ref().to_path_buf(),
                remove_on_drop: true,
            },
            count,
        ))
    }

    pub fn account_matches_owners(
        &self,
        multiplied_index: usize,
        owners: &[&Pubkey],
    ) -> Result<usize, MatchAccountOwnerError> {
        if let Some(reader) = self.reader.get() {
            return reader.account_matches_owners(multiplied_index, owners);
        }

        Err(MatchAccountOwnerError::UnableToLoad)
    }

    pub fn get_account<'a>(
        &'a self,
        multiplied_index: usize,
    ) -> Option<(StoredAccountMeta<'a>, usize)> {
        if multiplied_index % 1024 == 0 {
            log::info!(
                "TieredStorage::get_account(): fetch {} account at file {:?}",
                multiplied_index,
                self.path
            );
        }
        if let Some(reader) = self.reader.get() {
            return reader.get_account(multiplied_index);
        }
        None
    }

    pub fn get_path(&self) -> PathBuf {
        self.path.clone()
    }

    pub fn accounts(&self, mut multiplied_index: usize) -> Vec<StoredAccountMeta> {
        log::info!(
            "TieredStorage::accounts(): fetch all accounts after {} at file {:?}",
            multiplied_index,
            self.path
        );
        let mut accounts = vec![];
        while let Some((account, next)) = self.get_account(multiplied_index) {
            accounts.push(account);
            multiplied_index = next;
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
        log::info!("TieredStorage::append_accounts(): file {:?}", self.path);
        if self.is_read_only() {
            log::error!("TieredStorage::append_accounts(): attempt to append accounts to read only file {:?}", self.path);
            return None;
        }

        let result: Option<Vec<StoredAccountInfo>>;
        {
            let writer = TieredStorageWriter::new(&self.path, self.format.unwrap());
            result = writer.append_accounts(accounts, skip);
        }

        if self
            .reader
            .set(TieredStorageReader::new_from_path(&self.path).unwrap())
            .is_err()
        {
            panic!(
                "TieredStorage::append_accounts(): unable to create reader for file {:?}",
                self.path
            );
        }
        log::info!(
            "TieredStorage::append_accounts(): successfully appended {} accounts to file {:?}",
            accounts.len() - skip,
            self.path
        );
        result
    }

    pub fn file_size(&self) -> TieredStorageResult<u64> {
        let file = OpenOptions::new()
            .read(true)
            .create(false)
            .open(self.path.to_path_buf())?;
        Ok(file.metadata()?.len())
    }

    pub fn is_read_only(&self) -> bool {
        self.reader.get().is_some()
    }

    /*
    pub fn write_from_append_vec(&self, append_vec: &AppendVec) -> TieredStorageResult<()> {
        let writer = TieredStorageWriter::new(&self.path, self.format.unwrap());
        writer.write_from_append_vec(&append_vec)?;

        self.reader
            .set(TieredStorageReader::new_from_path(&self.path)?)
            .map_err(|_| TieredStorageError::ReaderInitializationFailure())
    }*/

    ///////////////////////////////////////////////////////////////////////////////

    pub fn set_no_remove_on_drop(&mut self) {
        self.remove_on_drop = false;
    }

    pub fn remaining_bytes(&self) -> u64 {
        if self.is_read_only() {
            return 0;
        }
        std::u64::MAX
    }

    pub fn len(&self) -> usize {
        self.file_size().unwrap_or(0).try_into().unwrap()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    ///////////////////////////////////////////////////////////////////////////////
    // unimplemented

    pub fn flush(&self) -> TieredStorageResult<()> {
        Ok(())
    }

    pub fn reset(&self) {}

    pub fn capacity(&self) -> u64 {
        if self.is_read_only() {
            return self.len().try_into().unwrap();
        }
        self.len().try_into().unwrap()
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
            append_vec::{
                test_utils::{create_test_account_from_len, get_append_vec_path, TempFile},
                AppendVec,
            },
            tiered_storage::{
                // cold::COLD_FORMAT,
                footer::{TieredFileFormat, TieredStorageFooter, FOOTER_SIZE},
                hot::HOT_FORMAT,
                index::AccountIndexFormat,
                readable::TieredStorageReader,
                TieredStorage,
                ACCOUNTS_DATA_STORAGE_FORMAT_VERSION,
                ACCOUNT_DATA_BLOCK_SIZE,
            },
        },
        once_cell::sync::OnceCell,
        solana_sdk::{
            account::{AccountSharedData, ReadableAccount},
            clock::Slot,
            hash::Hash,
            pubkey::Pubkey,
        },
        std::{collections::HashMap, mem, path::Path},
    };

    impl TieredStorage {
        fn new_for_test(file_path: &Path, format: &'static TieredFileFormat) -> Self {
            Self {
                reader: OnceCell::<TieredStorageReader>::new(),
                format: Some(format),
                path: file_path.to_path_buf(),
                remove_on_drop: false,
            }
        }

        fn footer(&self) -> Option<&TieredStorageFooter> {
            if let Some(reader) = self.reader.get() {
                return Some(reader.footer());
            }
            None
        }
    }

    impl TieredStorageReader {
        fn footer(&self) -> &TieredStorageFooter {
            match self {
                // Self::Cold(cs) => &cs.footer,
                Self::Hot(hs) => hs.footer(),
            }
        }
    }

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

    fn ads_writer_test_help(
        path_prefix: &str,
        account_data_sizes: &[usize],
        format: &'static TieredFileFormat,
    ) {
        /*
        write_from_append_vec_test_helper(
            &(path_prefix.to_owned() + "_from_append_vec"),
            account_data_sizes,
        );
        */
        append_accounts_test_helper(
            &(path_prefix.to_owned() + "_append_accounts"),
            account_data_sizes,
            format,
        );
    }

    fn append_accounts_test_helper(
        path_prefix: &str,
        account_data_sizes: &[usize],
        format: &'static TieredFileFormat,
    ) {
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
            let ads = TieredStorage::new_for_test(&ads_path.path, format);
            ads.append_accounts(&storable_accounts, 0);
        }

        verify_account_data_storage(
            account_count,
            &test_accounts,
            &ads_path,
            &hashes_map,
            format,
        );
    }

    /*
    fn write_from_append_vec_test_helper(
        path_prefix: &str,
        account_data_sizes: &[usize],
        format: &'static TieredFileFormat,
    ) {
        let account_count = account_data_sizes.len();
        let (test_accounts, av) =
            create_test_append_vec(&(path_prefix.to_owned() + "_av"), account_data_sizes);

        let ads_path = get_append_vec_path(&(path_prefix.to_owned() + "_ads"));
        {
            let ads = TieredStorage::new_for_test(&ads_path.path, format);
            ads.write_from_append_vec(&av).unwrap();
        }

        verify_account_data_storage(
            account_count,
            &test_accounts,
            &ads_path,
            &HashMap::new(),
            format,
        );
    }
    */

    fn verify_account_data_storage(
        account_count: usize,
        test_accounts: &HashMap<Pubkey, (StoredMeta, AccountSharedData)>,
        ads_path: &TempFile,
        hashes_map: &HashMap<Pubkey, &Hash>,
        format: &'static TieredFileFormat,
    ) {
        let ads = TieredStorage::new(&ads_path.path, None);
        let footer = ads.footer().unwrap();
        let indexer = AccountIndexFormat::AddressAndOffset;

        let expected_footer = TieredStorageFooter {
            account_meta_format: format.account_meta_format.clone(),
            owners_block_format: format.owners_block_format.clone(),
            account_index_format: format.account_index_format.clone(),
            account_block_format: format.account_block_format.clone(),
            account_entry_count: account_count as u32,
            account_meta_entry_size: format.meta_entry_size as u32,
            account_block_size: ACCOUNT_DATA_BLOCK_SIZE as u64,
            owner_count: account_count as u32,
            owner_entry_size: mem::size_of::<Pubkey>() as u32,
            // This number should be the total compressed account data size.
            account_index_offset: footer.account_index_offset,
            owners_offset: footer.account_index_offset
                + (account_count * indexer.entry_size()) as u64,
            // TODO(yhchiang): reach out Brooks on how to obtain the new hash
            hash: footer.hash,
            // TODO(yhchiang): fix this
            min_account_address: Pubkey::default(),
            max_account_address: Pubkey::default(),
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
            assert_eq!(account.to_account_shared_data(), expected_account.1);

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
        ads_writer_test_help(
            "test_write_from_append_vec_one_small_hot",
            &[255],
            &HOT_FORMAT,
        );
        ads_writer_test_help(
            "test_write_from_append_vec_one_small_cold",
            &[255],
            &HOT_FORMAT, //&COLD_FORMAT YHCHIANG
        );
    }

    #[test]
    fn test_write_from_append_vec_one_big() {
        ads_writer_test_help(
            "test_write_from_append_vec_one_big_hot",
            &[25500],
            &HOT_FORMAT,
        );
        ads_writer_test_help(
            "test_write_from_append_vec_one_big_cold",
            &[25500],
            &HOT_FORMAT, //&COLD_FORMAT YHCHIANG
        );
    }

    #[test]
    fn test_write_from_append_vec_one_10_mb() {
        ads_writer_test_help(
            "test_write_from_append_vec_one_10_mb_hot",
            &[10 * 1024 * 1024],
            &HOT_FORMAT,
        );
        ads_writer_test_help(
            "test_write_from_append_vec_one_10_mb_cold",
            &[10 * 1024 * 1024],
            &HOT_FORMAT, //&COLD_FORMAT YHCHIANG
        );
    }

    #[test]
    fn test_write_from_append_vec_multiple_blobs() {
        ads_writer_test_help(
            "test_write_from_append_vec_multiple_blobs_hot",
            &[5000, 6000, 7000, 8000, 5500, 10241023, 9999],
            &HOT_FORMAT,
        );
        ads_writer_test_help(
            "test_write_from_append_vec_multiple_blobs_cold",
            &[5000, 6000, 7000, 8000, 5500, 10241023, 9999],
            &HOT_FORMAT, //&COLD_FORMAT YHCHIANG
        );
    }

    #[test]
    fn test_write_from_append_vec_one_data_block() {
        ads_writer_test_help(
            "test_write_from_append_vec_one_data_block_hot",
            &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
            &HOT_FORMAT,
        );
        ads_writer_test_help(
            "test_write_from_append_vec_one_data_block_cold",
            &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
            &HOT_FORMAT, //&COLD_FORMAT YHCHIANG
        );
    }

    #[test]
    fn test_write_from_append_vec_mixed_block() {
        ads_writer_test_help(
            "test_write_from_append_vec_mixed_block_hot",
            &[
                1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 1000, 2000, 3000, 4000, 9, 8, 7, 6, 5, 4, 3, 2, 1,
            ],
            &HOT_FORMAT,
        );
        ads_writer_test_help(
            "test_write_from_append_vec_mixed_block_cold",
            &[
                1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 1000, 2000, 3000, 4000, 9, 8, 7, 6, 5, 4, 3, 2, 1,
            ],
            &HOT_FORMAT, //&COLD_FORMAT YHCHIANG
        );
    }
}

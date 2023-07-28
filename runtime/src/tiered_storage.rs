#![allow(dead_code)]

pub mod byte_block;
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
        account_storage::meta::{StorableAccountsWithHashesAndWriteVersions, StoredAccountInfo},
        storable_accounts::StorableAccounts,
    },
    error::TieredStorageError,
    footer::{AccountBlockFormat, AccountMetaFormat, OwnersBlockFormat},
    index::AccountIndexFormat,
    once_cell::sync::OnceCell,
    readable::TieredStorageReader,
    solana_sdk::{account::ReadableAccount, hash::Hash},
    std::{
        borrow::Borrow,
        fs::OpenOptions,
        path::{Path, PathBuf},
    },
    writer::TieredStorageWriter,
};

pub type TieredStorageResult<T> = Result<T, TieredStorageError>;

/// The struct that defines the formats of all building blocks of a
/// TieredStorage.
#[derive(Clone, Debug)]
pub struct TieredStorageFormat {
    pub meta_entry_size: usize,
    pub account_meta_format: AccountMetaFormat,
    pub owners_block_format: OwnersBlockFormat,
    pub account_index_format: AccountIndexFormat,
    pub account_block_format: AccountBlockFormat,
}

#[derive(Debug)]
pub struct TieredStorage {
    reader: OnceCell<TieredStorageReader>,
    format: Option<TieredStorageFormat>,
    path: PathBuf,
}

impl TieredStorage {
    /// Creates a new writable instance of TieredStorage based on the
    /// specified path and TieredStorageFormat.
    ///
    /// Note that the actual file will not be created until write_accounts
    /// is called.
    pub fn new_writable(path: impl Into<PathBuf>, format: TieredStorageFormat) -> Self {
        Self {
            reader: OnceCell::<TieredStorageReader>::new(),
            format: Some(format),
            path: path.into(),
        }
    }

    /// Creates a new read-only instance of TieredStorage from the
    /// specified path.
    pub fn new_readonly(path: impl Into<PathBuf>) -> TieredStorageResult<Self> {
        let pathbuf = path.into();
        let reader = TieredStorageReader::new_from_path(&pathbuf)?;
        let reader_cell = OnceCell::with_value(reader);
        Ok(Self {
            reader: reader_cell,
            format: None,
            path: pathbuf,
        })
    }

    /// Returns the path to this TieredStorage.
    pub fn path(&self) -> &Path {
        self.path.as_path()
    }

    /// Returns the number of accounts in this TieredStorage
    pub fn num_accounts(&self) -> usize {
        self.reader.get().map(|r| r.num_accounts()).unwrap_or(0)
    }

    /// Writes the specified accounts into this TieredStorage.
    ///
    /// Note that this function can only be called once per a TieredStorage
    /// file.  TieredStorageError::AttemptToUpdateReadOnly will be returned
    /// if this function is invoked more than once.
    pub fn write_accounts<
        'a,
        'b,
        T: ReadableAccount + Sync,
        U: StorableAccounts<'a, T>,
        V: Borrow<Hash>,
    >(
        &self,
        accounts: &StorableAccountsWithHashesAndWriteVersions<'a, 'b, T, U, V>,
        skip: usize,
    ) -> TieredStorageResult<Vec<StoredAccountInfo>> {
        if self.is_read_only() {
            return Err(TieredStorageError::AttemptToUpdateReadOnly(
                self.path.to_path_buf(),
            ));
        }

        let result;

        {
            // self.format must be Some as write_accounts can only be called on a
            // TieredStorage instance created via new_writable() where its format
            // field is required.
            assert!(self.format.is_some());
            let writer = TieredStorageWriter::new(&self.path, self.format.as_ref().unwrap())?;
            result = writer.write_accounts(accounts, skip)
        }

        self.reader
            .set(TieredStorageReader::new_from_path(&self.path).unwrap())
            .map_err(|_| TieredStorageError::UnableToCreateReader(self.path.to_path_buf()))?;

        result
    }

    /// Returns true if the TieredStorage is read-only.
    pub fn is_read_only(&self) -> bool {
        self.reader.get().is_some()
    }

    /// Returns the size of the underlying accounts file.
    pub fn file_size(&self) -> TieredStorageResult<u64> {
        let file = OpenOptions::new().read(true).open(&self.path);

        Ok(file
            .and_then(|file| file.metadata())
            .map(|metadata| metadata.len())
            .unwrap_or(0))
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::account_storage::meta::StoredMetaWriteVersion,
        footer::{TieredStorageFooter, TieredStorageMagicNumber},
        hot::HOT_FORMAT,
        solana_sdk::{account::AccountSharedData, clock::Slot, pubkey::Pubkey},
        tempfile::NamedTempFile,
    };

    impl TieredStorage {
        fn footer(&self) -> Option<&TieredStorageFooter> {
            self.reader.get().map(|r| r.footer())
        }
    }

    /// Simply invoke write_accounts with empty vector to allow the tiered storage
    /// to persist non-account blocks such as footer, index block, etc.
    fn write_zero_accounts(
        tiered_storage: &TieredStorage,
        expected_result: TieredStorageResult<Vec<StoredAccountInfo>>,
    ) {
        let slot_ignored = Slot::MAX;
        let account_refs = Vec::<(&Pubkey, &AccountSharedData)>::new();
        let account_data = (slot_ignored, account_refs.as_slice());
        let storable_accounts =
            StorableAccountsWithHashesAndWriteVersions::new_with_hashes_and_write_versions(
                &account_data,
                Vec::<&Hash>::new(),
                Vec::<StoredMetaWriteVersion>::new(),
            );

        let result = tiered_storage.write_accounts(&storable_accounts, 0);

        match (&result, &expected_result) {
            (
                Err(TieredStorageError::AttemptToUpdateReadOnly(_)),
                Err(TieredStorageError::AttemptToUpdateReadOnly(_)),
            ) => {}
            (Err(TieredStorageError::Unsupported()), Err(TieredStorageError::Unsupported())) => {}
            _ => {
                assert_eq!(format!("{:?}", result), format!("{:?}", expected_result));
            }
        };

        assert!(tiered_storage.is_read_only());
        assert_eq!(
            tiered_storage.file_size().unwrap() as usize,
            std::mem::size_of::<TieredStorageFooter>()
                + std::mem::size_of::<TieredStorageMagicNumber>()
        );
    }

    #[test]
    fn test_new_meta_file_only() {
        let path = NamedTempFile::new().unwrap().path().to_path_buf();
        {
            let tiered_storage = TieredStorage::new_writable(&path, HOT_FORMAT.clone());
            assert!(!tiered_storage.is_read_only());
            assert_eq!(tiered_storage.path(), path);
            assert_eq!(tiered_storage.file_size().unwrap(), 0);

            // Expect the result to be TieredStorageError::Unsupported as the feature
            // is not yet fully supported, but we can still check its partial results
            // in the test.
            write_zero_accounts(&tiered_storage, Err(TieredStorageError::Unsupported()));
        }

        let tiered_storage_readonly = TieredStorage::new_readonly(&path).unwrap();
        let footer = tiered_storage_readonly.footer().unwrap();
        assert!(tiered_storage_readonly.is_read_only());
        assert_eq!(tiered_storage_readonly.num_accounts(), 0);
        assert_eq!(footer.account_meta_format, HOT_FORMAT.account_meta_format);
        assert_eq!(footer.owners_block_format, HOT_FORMAT.owners_block_format);
        assert_eq!(footer.account_index_format, HOT_FORMAT.account_index_format);
        assert_eq!(footer.account_block_format, HOT_FORMAT.account_block_format);
        assert_eq!(
            tiered_storage_readonly.file_size().unwrap() as usize,
            std::mem::size_of::<TieredStorageFooter>()
                + std::mem::size_of::<TieredStorageMagicNumber>()
        );
    }

    #[test]
    fn test_write_accounts_twice() {
        let path = NamedTempFile::new().unwrap().path().to_path_buf();
        let tiered_storage = TieredStorage::new_writable(path.clone(), HOT_FORMAT.clone());
        // Expect the result to be TieredStorageError::Unsupported as the feature
        // is not yet fully supported, but we can still check its partial results
        // in the test.
        write_zero_accounts(&tiered_storage, Err(TieredStorageError::Unsupported()));
        // Expect AttemptToUpdateReadOnly error as write_accounts can only
        // be invoked once.
        write_zero_accounts(
            &tiered_storage,
            Err(TieredStorageError::AttemptToUpdateReadOnly(path)),
        );
    }
}

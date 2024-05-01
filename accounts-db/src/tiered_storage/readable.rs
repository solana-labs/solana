use {
    crate::{
        account_storage::meta::StoredAccountMeta,
        accounts_file::MatchAccountOwnerError,
        append_vec::IndexInfo,
        tiered_storage::{
            file::TieredReadableFile,
            footer::{AccountMetaFormat, TieredStorageFooter},
            hot::HotStorageReader,
            index::IndexOffset,
            TieredStorageResult,
        },
    },
    solana_sdk::{account::AccountSharedData, pubkey::Pubkey},
    std::path::Path,
};

/// The reader of a tiered storage instance.
#[derive(Debug)]
pub enum TieredStorageReader {
    Hot(HotStorageReader),
}

impl TieredStorageReader {
    /// Creates a reader for the specified tiered storage accounts file.
    pub fn new_from_path(path: impl AsRef<Path>) -> TieredStorageResult<Self> {
        let file = TieredReadableFile::new(&path)?;
        let footer = TieredStorageFooter::new_from_footer_block(&file)?;
        match footer.account_meta_format {
            AccountMetaFormat::Hot => Ok(Self::Hot(HotStorageReader::new(file)?)),
        }
    }

    /// Returns the size of the underlying storage.
    pub fn len(&self) -> usize {
        match self {
            Self::Hot(hot) => hot.len(),
        }
    }

    /// Returns whether the nderlying storage is empty.
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Hot(hot) => hot.is_empty(),
        }
    }

    pub fn capacity(&self) -> u64 {
        match self {
            Self::Hot(hot) => hot.capacity(),
        }
    }

    /// Returns the footer of the associated HotAccountsFile.
    pub fn footer(&self) -> &TieredStorageFooter {
        match self {
            Self::Hot(hot) => hot.footer(),
        }
    }

    /// Returns the total number of accounts.
    pub fn num_accounts(&self) -> usize {
        match self {
            Self::Hot(hot) => hot.num_accounts(),
        }
    }

    /// Returns the account located at the specified index offset.
    pub fn get_account_shared_data(
        &self,
        index_offset: IndexOffset,
    ) -> TieredStorageResult<Option<AccountSharedData>> {
        match self {
            Self::Hot(hot) => hot.get_account_shared_data(index_offset),
        }
    }

    /// calls `callback` with the account located at the specified index offset.
    pub fn get_stored_account_meta_callback<Ret>(
        &self,
        index_offset: IndexOffset,
        callback: impl for<'local> FnMut(StoredAccountMeta<'local>) -> Ret,
    ) -> TieredStorageResult<Option<Ret>> {
        match self {
            Self::Hot(hot) => hot.get_stored_account_meta_callback(index_offset, callback),
        }
    }

    /// Returns Ok(index_of_matching_owner) if the account owner at
    /// `account_offset` is one of the pubkeys in `owners`.
    ///
    /// Returns Err(MatchAccountOwnerError::NoMatch) if the account has 0
    /// lamports or the owner is not one of the pubkeys in `owners`.
    ///
    /// Returns Err(MatchAccountOwnerError::UnableToLoad) if there is any internal
    /// error that causes the data unable to load, including `account_offset`
    /// causes a data overrun.
    pub fn account_matches_owners(
        &self,
        index_offset: IndexOffset,
        owners: &[Pubkey],
    ) -> Result<usize, MatchAccountOwnerError> {
        match self {
            Self::Hot(hot) => {
                let account_offset = hot
                    .get_account_offset(index_offset)
                    .map_err(|_| MatchAccountOwnerError::UnableToLoad)?;
                hot.account_matches_owners(account_offset, owners)
            }
        }
    }

    /// iterate over all pubkeys
    pub fn scan_pubkeys(&self, callback: impl FnMut(&Pubkey)) -> TieredStorageResult<()> {
        match self {
            Self::Hot(hot) => hot.scan_pubkeys(callback),
        }
    }

    /// iterate over all entries to put in index
    pub(crate) fn scan_index(&self, callback: impl FnMut(IndexInfo)) -> TieredStorageResult<()> {
        match self {
            Self::Hot(hot) => hot.scan_index(callback),
        }
    }

    /// Iterate over all accounts and call `callback` with each account.
    pub(crate) fn scan_accounts(
        &self,
        callback: impl for<'local> FnMut(StoredAccountMeta<'local>),
    ) -> TieredStorageResult<()> {
        match self {
            Self::Hot(hot) => hot.scan_accounts(callback),
        }
    }

    /// for each offset in `sorted_offsets`, return the account size
    pub(crate) fn get_account_sizes(
        &self,
        sorted_offsets: &[usize],
    ) -> TieredStorageResult<Vec<usize>> {
        match self {
            Self::Hot(hot) => hot.get_account_sizes(sorted_offsets),
        }
    }

    /// Returns a slice suitable for use when archiving tiered storages
    pub fn data_for_archive(&self) -> &[u8] {
        match self {
            Self::Hot(hot) => hot.data_for_archive(),
        }
    }
}

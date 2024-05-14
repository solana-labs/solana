use {
    crate::{
        account_storage::meta::StoredAccountMeta,
        accounts_file::MatchAccountOwnerError,
        tiered_storage::{
            file::TieredReadableFile,
            footer::{AccountMetaFormat, TieredStorageFooter},
            hot::HotStorageReader,
            index::IndexOffset,
            TieredStorageResult,
        },
    },
    solana_sdk::pubkey::Pubkey,
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
    pub fn get_account(
        &self,
        index_offset: IndexOffset,
    ) -> TieredStorageResult<Option<(StoredAccountMeta<'_>, IndexOffset)>> {
        match self {
            Self::Hot(hot) => hot.get_account(index_offset),
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

    /// Return a vector of account metadata for each account, starting from
    /// `index_offset`
    pub fn accounts(
        &self,
        index_offset: IndexOffset,
    ) -> TieredStorageResult<Vec<StoredAccountMeta>> {
        match self {
            Self::Hot(hot) => hot.accounts(index_offset),
        }
    }
}

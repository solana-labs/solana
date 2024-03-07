use {
    crate::{
        account_storage::meta::StoredAccountMeta,
        accounts_file::MatchAccountOwnerError,
        tiered_storage::{
            footer::{AccountMetaFormat, TieredStorageFooter},
            hot::HotStorageReader,
            index::IndexOffset,
            meta::TieredAccountMeta,
            TieredStorageResult,
        },
    },
    solana_sdk::{
        account::ReadableAccount, pubkey::Pubkey, rent_collector::RENT_EXEMPT_RENT_EPOCH,
        stake_history::Epoch,
    },
    std::path::Path,
};

/// The struct that offers read APIs for accessing a TieredAccount.
#[derive(PartialEq, Eq, Debug)]
pub struct TieredReadableAccount<'accounts_file, M: TieredAccountMeta> {
    /// TieredAccountMeta
    pub meta: &'accounts_file M,
    /// The address of the account
    pub address: &'accounts_file Pubkey,
    /// The address of the account owner
    pub owner: &'accounts_file Pubkey,
    /// The index for accessing the account inside its belonging AccountsFile
    pub index: IndexOffset,
    /// The account block that contains this account.  Note that this account
    /// block may be shared with other accounts.
    pub account_block: &'accounts_file [u8],
}

impl<'accounts_file, M: TieredAccountMeta> TieredReadableAccount<'accounts_file, M> {
    /// Returns the address of this account.
    pub fn address(&self) -> &'accounts_file Pubkey {
        self.address
    }

    /// Returns the index to this account in its AccountsFile.
    pub fn index(&self) -> IndexOffset {
        self.index
    }

    /// Returns the data associated to this account.
    pub fn data(&self) -> &'accounts_file [u8] {
        self.meta.account_data(self.account_block)
    }
}

impl<'accounts_file, M: TieredAccountMeta> ReadableAccount
    for TieredReadableAccount<'accounts_file, M>
{
    /// Returns the balance of the lamports of this account.
    fn lamports(&self) -> u64 {
        self.meta.lamports()
    }

    /// Returns the address of the owner of this account.
    fn owner(&self) -> &'accounts_file Pubkey {
        self.owner
    }

    /// Returns true if the data associated to this account is executable.
    fn executable(&self) -> bool {
        self.meta.flags().executable()
    }

    /// Returns the epoch that this account will next owe rent by parsing
    /// the specified account block.  RENT_EXEMPT_RENT_EPOCH will be returned
    /// if the account is rent-exempt.
    ///
    /// For a zero-lamport account, Epoch::default() will be returned to
    /// default states of an AccountSharedData.
    fn rent_epoch(&self) -> Epoch {
        self.meta
            .rent_epoch(self.account_block)
            .unwrap_or(if self.lamports() != 0 {
                RENT_EXEMPT_RENT_EPOCH
            } else {
                // While there is no valid-values for any fields of a zero
                // lamport account, here we return Epoch::default() to
                // match the default states of AccountSharedData.  Otherwise,
                // a hash mismatch will occur.
                Epoch::default()
            })
    }

    /// Returns the data associated to this account.
    fn data(&self) -> &'accounts_file [u8] {
        self.data()
    }
}

/// The reader of a tiered storage instance.
#[derive(Debug)]
pub enum TieredStorageReader {
    Hot(HotStorageReader),
}

impl TieredStorageReader {
    /// Creates a reader for the specified tiered storage accounts file.
    pub fn new_from_path(path: impl AsRef<Path>) -> TieredStorageResult<Self> {
        let footer = TieredStorageFooter::new_from_path(&path)?;
        match footer.account_meta_format {
            AccountMetaFormat::Hot => Ok(Self::Hot(HotStorageReader::new_from_path(path)?)),
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

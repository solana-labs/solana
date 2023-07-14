use {
    crate::{
        account_storage::meta::{StoredAccountMeta, StoredMetaWriteVersion},
        append_vec::MatchAccountOwnerError,
        tiered_storage::{
            footer::{AccountMetaFormat, TieredStorageFooter},
            hot::HotStorageReader,
            meta::TieredAccountMeta,
            TieredStorageResult,
        },
    },
    solana_sdk::{account::ReadableAccount, hash::Hash, pubkey::Pubkey, stake_history::Epoch},
    std::path::Path,
};

/// The struct that offers read APIs for accessing a TieredAccount.
#[derive(PartialEq, Eq, Debug)]
pub struct TieredReadableAccount<'accounts_file, M: TieredAccountMeta> {
    /// TieredAccountMeta
    pub(crate) meta: &'accounts_file M,
    /// The address of the account
    pub(crate) address: &'accounts_file Pubkey,
    /// The address of the account owner
    pub(crate) owner: &'accounts_file Pubkey,
    /// The index for accessing the account inside its belonging Storage
    pub(crate) index: usize,
    /// The account block that contains this account.  Note that this account
    /// block may be shared with other accounts.
    pub(crate) account_block: &'accounts_file [u8],
}

impl<'accounts_file, M: TieredAccountMeta> TieredReadableAccount<'accounts_file, M> {
    /// Returns the address of this account.
    pub fn address(&self) -> &'accounts_file Pubkey {
        self.address
    }

    /// Returns the hash of this account.
    pub fn hash(&self) -> Option<&'accounts_file Hash> {
        self.meta.account_hash(self.account_block)
    }

    /// Returns the index to this account in its Storage.
    pub fn index(&self) -> usize {
        self.index
    }

    /// Returns the write version of the account.
    pub fn write_version(&self) -> Option<StoredMetaWriteVersion> {
        self.meta.write_version(self.account_block)
    }

    pub fn stored_size(&self) -> usize {
        std::mem::size_of::<M>()
            + std::mem::size_of::<Pubkey>()
            + std::mem::size_of::<Pubkey>()
            + self.account_block.len()
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
    ///
    /// Temporarily unimplemented!() as program runtime v2 will use
    /// a different API for executable.
    fn executable(&self) -> bool {
        self.meta.flags().executable()
    }

    /// Returns the epoch that this account will next owe rent by parsing
    /// the specified account block.  Epoch::MAX will be returned if the account
    /// is rent-exempt.
    fn rent_epoch(&self) -> Epoch {
        self.meta
            .rent_epoch(self.account_block)
            .unwrap_or(Epoch::MAX)
    }

    /// Returns the data associated to this account.
    fn data(&self) -> &'accounts_file [u8] {
        self.data()
    }
}

/// The reader of a tiered accounts file.
#[derive(Debug)]
pub enum TieredStorageReader {
    // Cold(ColdStorageReader),
    Hot(HotStorageReader),
}

impl TieredStorageReader {
    /// Creates a reader for the specified tiered storage accounts file.
    pub fn new_from_path<P: AsRef<Path>>(path: P) -> TieredStorageResult<Self> {
        let footer = TieredStorageFooter::new_from_path(&path)?;
        match footer.account_meta_format {
            // AccountMetaFormat::Cold => Ok(Self::Cold(ColdStorageReader::new_from_file(path)?)),
            AccountMetaFormat::Hot => Ok(Self::Hot(HotStorageReader::new_from_path(path)?)),
        }
    }

    /// Returns the total number of accounts.
    pub fn num_accounts(&self) -> usize {
        match self {
            // Self::Cold(cs) => cs.num_accounts(),
            Self::Hot(hs) => hs.num_accounts(),
        }
    }

    /// Given the account associated with the specified index, this
    /// function returns the index to the specified input `owners` vector if the
    /// specified account is owned by the owner located at the returned index.
    ///
    /// Otherwise, MatchAccountOwnerError will be returned.
    pub fn account_matches_owners(
        &self,
        index: usize,
        owners: &[&Pubkey],
    ) -> Result<usize, MatchAccountOwnerError> {
        match self {
            // Self::Cold(cs) => cs.account_matches_owners(index, owners),
            Self::Hot(hs) => hs.account_matches_owners(index, owners),
        }
    }

    /// Returns (account metadata, next_index) pair for the account at the
    /// specified `index` if any.  Otherwise return None.  The function also
    /// returns the multiplied index to the next entry.
    pub fn get_account<'a>(&'a self, index: usize) -> Option<(StoredAccountMeta<'a>, usize)> {
        match self {
            // Self::Cold(cs) => cs.get_account(index),
            Self::Hot(hs) => hs.get_account(index),
        }
    }
}

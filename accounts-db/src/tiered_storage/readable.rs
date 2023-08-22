use {
    crate::{
        account_storage::meta::StoredMetaWriteVersion,
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
    pub meta: &'accounts_file M,
    /// The address of the account
    pub address: &'accounts_file Pubkey,
    /// The address of the account owner
    pub owner: &'accounts_file Pubkey,
    /// The index for accessing the account inside its belonging AccountsFile
    pub index: usize,
    /// The account block that contains this account.  Note that this account
    /// block may be shared with other accounts.
    pub account_block: &'accounts_file [u8],
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

    /// Returns the index to this account in its AccountsFile.
    pub fn index(&self) -> usize {
        self.index
    }

    /// Returns the write version of the account.
    pub fn write_version(&self) -> Option<StoredMetaWriteVersion> {
        self.meta.write_version(self.account_block)
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
        unimplemented!();
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
}

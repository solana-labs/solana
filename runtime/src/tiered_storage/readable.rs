use {
    crate::{
        account_storage::meta::StoredMetaWriteVersion, tiered_storage::meta::TieredAccountMeta,
    },
    solana_sdk::{
        account::{Account, AccountSharedData, ReadableAccount},
        hash::Hash,
        pubkey::Pubkey,
        stake_history::Epoch,
    },
};

/// The struct that offers read APIs for accessing a TieredAccount.
#[derive(PartialEq, Eq, Debug)]
pub struct TieredReadableAccount<'a, M: TieredAccountMeta> {
    /// TieredAccountMeta
    pub(crate) meta: &'a M,
    /// The address of the account
    pub(crate) address: &'a Pubkey,
    /// The address of the account owner
    pub(crate) owner: &'a Pubkey,
    /// The index for accessing the account inside its belonging AccountsFile
    pub(crate) index: usize,
    /// The account block that contains this account.  Note that this account
    /// block may be shared with other accounts.
    pub(crate) account_block: &'a [u8],
}

impl<'a, M: TieredAccountMeta> TieredReadableAccount<'a, M> {
    /// Returns the address of this account.
    pub fn address(&self) -> &'a Pubkey {
        self.address
    }

    /// Returns the hash of this account.
    pub fn hash(&self) -> Option<&'a Hash> {
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
}

impl<'a, M: TieredAccountMeta> ReadableAccount for TieredReadableAccount<'a, M> {
    /// Returns the balance of the lamports of this account.
    fn lamports(&self) -> u64 {
        self.meta.lamports()
    }

    /// Returns the address of the owner of this account.
    fn owner(&self) -> &'a Pubkey {
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
    fn data(&self) -> &'a [u8] {
        self.meta.account_data(self.account_block)
    }
}

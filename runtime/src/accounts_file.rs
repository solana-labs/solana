use {
    crate::{
        account_storage::meta::{
            StorableAccountsWithHashesAndWriteVersions, StoredAccountInfo, StoredAccountMeta,
        },
        append_vec::{AppendVec, AppendVecError, MatchAccountOwnerError},
        storable_accounts::StorableAccounts,
        tiered_storage::{error::TieredStorageError, hot::HOT_FORMAT, TieredStorage},
    },
    solana_sdk::{account::ReadableAccount, clock::Slot, hash::Hash, pubkey::Pubkey},
    std::{
        borrow::Borrow,
        mem,
        path::{Path, PathBuf},
    },
    thiserror::Error,
};

// Data placement should be aligned at the next boundary. Without alignment accessing the memory may
// crash on some architectures.
pub const ALIGN_BOUNDARY_OFFSET: usize = mem::size_of::<u64>();
#[macro_export]
macro_rules! u64_align {
    ($addr: expr) => {
        ($addr + (ALIGN_BOUNDARY_OFFSET - 1)) & !(ALIGN_BOUNDARY_OFFSET - 1)
    };
}

#[derive(Error, Debug)]
/// An enum for AccountsFile related errors.
pub enum AccountsFileError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("AppendVecError: {0}")]
    AppendVecError(#[from] AppendVecError),

    #[error("TieredStorageError: {0}")]
    TieredStorageError(#[from] TieredStorageError),
}

pub type Result<T> = std::result::Result<T, AccountsFileError>;

#[derive(Debug)]
/// An enum for accessing an accounts file which can be implemented
/// under different formats.
pub enum AccountsFile {
    AppendVec(AppendVec),
    Tiered(TieredStorage),
}

impl AccountsFile {
    /// Create an AccountsFile instance from the specified path.
    ///
    /// The second element of the returned tuple is the number of accounts in the
    /// accounts file.
    pub fn new_from_file(path: impl AsRef<Path>, current_len: usize) -> Result<(Self, usize)> {
        if let Ok((ts, num_accounts)) = TieredStorage::new_from_file(path.as_ref()) {
            log::info!("YH: Open {:?} as tiered storage", path.as_ref());
            return Ok((Self::Tiered(ts), num_accounts));
        }

        let (av, num_accounts) = AppendVec::new_from_file(path.as_ref(), current_len)?;
        Ok((Self::AppendVec(av), num_accounts))
    }

    /*
    pub fn new_cold_entry(file_path: &Path, create: bool) -> Self {
        Self::Tiered(TieredStorage::new(
            file_path,
            create.then_some(&COLD_FORMAT),
        ))
    }
    */

    pub fn new_hot_entry(file_path: &Path, create: bool) -> Self {
        Self::Tiered(TieredStorage::new(file_path, create.then_some(&HOT_FORMAT)))
    }

    /// By default, all AccountsFile will remove its underlying file on
    /// drop.  Calling this function to disable such behavior for this
    /// instance.
    pub fn set_no_remove_on_drop(&mut self) {
        match self {
            Self::AppendVec(av) => av.set_no_remove_on_drop(),
            Self::Tiered(ts) => ts.set_no_remove_on_drop(),
        }
    }

    pub fn flush(&self) -> Result<()> {
        match self {
            Self::AppendVec(av) => av.flush(),
            Self::Tiered(..) => Ok(()),
        }
    }

    pub fn reset(&self) {
        match self {
            Self::AppendVec(av) => av.reset(),
            Self::Tiered(..) => {}
        }
    }

    pub fn remaining_bytes(&self) -> u64 {
        match self {
            Self::AppendVec(av) => av.remaining_bytes(),
            Self::Tiered(ts) => ts.remaining_bytes(),
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Self::AppendVec(av) => av.len(),
            Self::Tiered(ts) => ts.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Self::AppendVec(av) => av.is_empty(),
            Self::Tiered(ts) => ts.is_empty(),
        }
    }

    pub fn capacity(&self) -> u64 {
        match self {
            Self::AppendVec(av) => av.capacity(),
            Self::Tiered(ts) => ts.capacity(),
        }
    }

    pub fn is_recyclable(&self) -> bool {
        match self {
            Self::AppendVec(_) => true,
            Self::Tiered(_) => false,
        }
    }

    pub fn file_name(slot: Slot, id: impl std::fmt::Display) -> String {
        format!("{slot}.{id}")
    }

    /// Return (account metadata, next_index) pair for the account at the
    /// specified `index` if any.  Otherwise return None.   Also return the
    /// index of the next entry.
    pub fn get_account(&self, index: usize) -> Option<(StoredAccountMeta<'_>, usize)> {
        match self {
            Self::AppendVec(av) => av.get_account(index),
            Self::Tiered(ts) => ts.get_account(index),
        }
    }

    pub fn account_matches_owners(
        &self,
        offset: usize,
        owners: &[&Pubkey],
    ) -> std::result::Result<usize, MatchAccountOwnerError> {
        match self {
            Self::AppendVec(av) => av.account_matches_owners(offset, owners),
            Self::Tiered(ts) => ts.account_matches_owners(offset, owners),
        }
    }

    /// Return the path of the underlying account file.
    pub fn get_path(&self) -> PathBuf {
        match self {
            Self::AppendVec(av) => av.get_path(),
            Self::Tiered(ts) => ts.get_path(),
        }
    }

    /// Return iterator for account metadata
    pub fn account_iter(&self) -> AccountsFileIter {
        AccountsFileIter::new(self)
    }

    /// Return a vector of account metadata for each account, starting from `offset`.
    pub fn accounts(&self, offset: usize) -> Vec<StoredAccountMeta> {
        match self {
            Self::AppendVec(av) => av.accounts(offset),
            Self::Tiered(ts) => ts.accounts(offset),
        }
    }

    /// Copy each account metadata, account and hash to the internal buffer.
    /// If there is no room to write the first entry, None is returned.
    /// Otherwise, returns the starting offset of each account metadata.
    /// Plus, the final return value is the offset where the next entry would be appended.
    /// So, return.len() is 1 + (number of accounts written)
    /// After each account is appended, the internal `current_len` is updated
    /// and will be available to other threads.
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
        match self {
            Self::AppendVec(av) => av.append_accounts(accounts, skip),
            Self::Tiered(ts) => ts.append_accounts(accounts, skip),
        }
    }
}

pub struct AccountsFileIter<'a> {
    file_entry: &'a AccountsFile,
    offset: usize,
}

impl<'a> AccountsFileIter<'a> {
    pub fn new(file_entry: &'a AccountsFile) -> Self {
        Self {
            file_entry,
            offset: 0,
        }
    }
}

impl<'a> Iterator for AccountsFileIter<'a> {
    type Item = StoredAccountMeta<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some((account, next_offset)) = self.file_entry.get_account(self.offset) {
            self.offset = next_offset;
            Some(account)
        } else {
            None
        }
    }
}

#[cfg(test)]
pub mod tests {
    use crate::accounts_file::AccountsFile;
    impl AccountsFile {
        pub(crate) fn set_current_len_for_tests(&self, len: usize) {
            match self {
                Self::AppendVec(av) => av.set_current_len_for_tests(len),
                Self::Tiered(..) => todo!(),
            }
        }
    }
}

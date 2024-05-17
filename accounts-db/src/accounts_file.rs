use {
    crate::{
        account_info::AccountInfo,
        account_storage::meta::StoredAccountMeta,
        accounts_db::AccountsFileId,
        append_vec::{AppendVec, AppendVecError, IndexInfo},
        storable_accounts::StorableAccounts,
        tiered_storage::{
            error::TieredStorageError, hot::HOT_FORMAT, index::IndexOffset, TieredStorage,
        },
    },
    solana_sdk::{account::AccountSharedData, clock::Slot, pubkey::Pubkey},
    std::{
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

#[derive(Error, Debug, PartialEq, Eq)]
pub enum MatchAccountOwnerError {
    #[error("The account owner does not match with the provided list")]
    NoMatch,
    #[error("Unable to load the account")]
    UnableToLoad,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum StorageAccess {
    #[default]
    /// storages should be accessed by Mmap
    Mmap,
}

pub type Result<T> = std::result::Result<T, AccountsFileError>;

#[derive(Debug)]
/// An enum for accessing an accounts file which can be implemented
/// under different formats.
pub enum AccountsFile {
    AppendVec(AppendVec),
    TieredStorage(TieredStorage),
}

impl AccountsFile {
    /// Create an AccountsFile instance from the specified path.
    ///
    /// The second element of the returned tuple is the number of accounts in the
    /// accounts file.
    pub fn new_from_file(
        path: impl Into<PathBuf>,
        current_len: usize,
        storage_access: StorageAccess,
    ) -> Result<(Self, usize)> {
        let (av, num_accounts) = AppendVec::new_from_file(path, current_len, storage_access)?;
        Ok((Self::AppendVec(av), num_accounts))
    }

    /// true if this storage can possibly be appended to (independent of capacity check)
    pub(crate) fn can_append(&self) -> bool {
        match self {
            Self::AppendVec(av) => av.can_append(),
            // once created, tiered storages cannot be appended to
            Self::TieredStorage(_) => false,
        }
    }

    /// if storage is not readonly, reopen another instance that is read only
    pub(crate) fn reopen_as_readonly(&self) -> Option<Self> {
        match self {
            Self::AppendVec(av) => av.reopen_as_readonly().map(Self::AppendVec),
            Self::TieredStorage(_) => None,
        }
    }

    pub fn flush(&self) -> Result<()> {
        match self {
            Self::AppendVec(av) => av.flush(),
            Self::TieredStorage(_) => Ok(()),
        }
    }

    pub fn reset(&self) {
        match self {
            Self::AppendVec(av) => av.reset(),
            Self::TieredStorage(_) => {}
        }
    }

    pub fn remaining_bytes(&self) -> u64 {
        match self {
            Self::AppendVec(av) => av.remaining_bytes(),
            Self::TieredStorage(ts) => ts.capacity().saturating_sub(ts.len() as u64),
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Self::AppendVec(av) => av.len(),
            Self::TieredStorage(ts) => ts.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Self::AppendVec(av) => av.is_empty(),
            Self::TieredStorage(ts) => ts.is_empty(),
        }
    }

    pub fn capacity(&self) -> u64 {
        match self {
            Self::AppendVec(av) => av.capacity(),
            Self::TieredStorage(ts) => ts.capacity(),
        }
    }

    pub fn file_name(slot: Slot, id: AccountsFileId) -> String {
        format!("{slot}.{id}")
    }

    /// calls `callback` with the account located at the specified index offset.
    pub fn get_stored_account_meta_callback<Ret>(
        &self,
        offset: usize,
        callback: impl for<'local> FnMut(StoredAccountMeta<'local>) -> Ret,
    ) -> Option<Ret> {
        match self {
            Self::AppendVec(av) => av.get_stored_account_meta_callback(offset, callback),
            // Note: The conversion here is needed as the AccountsDB currently
            // assumes all offsets are multiple of 8 while TieredStorage uses
            // IndexOffset that is equivalent to AccountInfo::reduced_offset.
            Self::TieredStorage(ts) => ts
                .reader()?
                .get_stored_account_meta_callback(
                    IndexOffset(AccountInfo::get_reduced_offset(offset)),
                    callback,
                )
                .ok()?,
        }
    }

    /// return an `AccountSharedData` for an account at `offset`, if any.  Otherwise return None.
    pub(crate) fn get_account_shared_data(&self, offset: usize) -> Option<AccountSharedData> {
        match self {
            Self::AppendVec(av) => av.get_account_shared_data(offset),
            Self::TieredStorage(ts) => {
                // Note: The conversion here is needed as the AccountsDB currently
                // assumes all offsets are multiple of 8 while TieredStorage uses
                // IndexOffset that is equivalent to AccountInfo::reduced_offset.
                let index_offset = IndexOffset(AccountInfo::get_reduced_offset(offset));
                ts.reader()?.get_account_shared_data(index_offset).ok()?
            }
        }
    }

    pub fn account_matches_owners(
        &self,
        offset: usize,
        owners: &[Pubkey],
    ) -> std::result::Result<usize, MatchAccountOwnerError> {
        match self {
            Self::AppendVec(av) => av.account_matches_owners(offset, owners),
            // Note: The conversion here is needed as the AccountsDB currently
            // assumes all offsets are multiple of 8 while TieredStorage uses
            // IndexOffset that is equivalent to AccountInfo::reduced_offset.
            Self::TieredStorage(ts) => {
                let Some(reader) = ts.reader() else {
                    return Err(MatchAccountOwnerError::UnableToLoad);
                };
                reader.account_matches_owners(
                    IndexOffset(AccountInfo::get_reduced_offset(offset)),
                    owners,
                )
            }
        }
    }

    /// Return the path of the underlying account file.
    pub fn path(&self) -> &Path {
        match self {
            Self::AppendVec(av) => av.path(),
            Self::TieredStorage(ts) => ts.path(),
        }
    }

    /// Iterate over all accounts and call `callback` with each account.
    pub(crate) fn scan_accounts(
        &self,
        callback: impl for<'local> FnMut(StoredAccountMeta<'local>),
    ) {
        match self {
            Self::AppendVec(av) => av.scan_accounts(callback),
            Self::TieredStorage(ts) => {
                if let Some(reader) = ts.reader() {
                    _ = reader.scan_accounts(callback);
                }
            }
        }
    }

    /// for each offset in `sorted_offsets`, return the account size
    pub(crate) fn get_account_sizes(&self, sorted_offsets: &[usize]) -> Vec<usize> {
        match self {
            Self::AppendVec(av) => av.get_account_sizes(sorted_offsets),
            Self::TieredStorage(ts) => ts
                .reader()
                .and_then(|reader| reader.get_account_sizes(sorted_offsets).ok())
                .unwrap_or_default(),
        }
    }

    /// iterate over all entries to put in index
    pub(crate) fn scan_index(&self, callback: impl FnMut(IndexInfo)) {
        match self {
            Self::AppendVec(av) => av.scan_index(callback),
            Self::TieredStorage(ts) => {
                if let Some(reader) = ts.reader() {
                    _ = reader.scan_index(callback);
                }
            }
        }
    }

    /// iterate over all pubkeys
    pub fn scan_pubkeys(&self, callback: impl FnMut(&Pubkey)) {
        match self {
            Self::AppendVec(av) => av.scan_pubkeys(callback),
            Self::TieredStorage(ts) => {
                if let Some(reader) = ts.reader() {
                    _ = reader.scan_pubkeys(callback);
                }
            }
        }
    }

    /// Copy each account metadata, account and hash to the internal buffer.
    /// If there is no room to write the first entry, None is returned.
    /// Otherwise, returns the starting offset of each account metadata.
    /// Plus, the final return value is the offset where the next entry would be appended.
    /// So, return.len() is 1 + (number of accounts written)
    /// After each account is appended, the internal `current_len` is updated
    /// and will be available to other threads.
    pub fn append_accounts<'a>(
        &self,
        accounts: &impl StorableAccounts<'a>,
        skip: usize,
    ) -> Option<StoredAccountsInfo> {
        match self {
            Self::AppendVec(av) => av.append_accounts(accounts, skip),
            // Note: The conversion here is needed as the AccountsDB currently
            // assumes all offsets are multiple of 8 while TieredStorage uses
            // IndexOffset that is equivalent to AccountInfo::reduced_offset.
            Self::TieredStorage(ts) => ts
                .write_accounts(accounts, skip, &HOT_FORMAT)
                .map(|mut stored_accounts_info| {
                    stored_accounts_info.offsets.iter_mut().for_each(|offset| {
                        *offset = AccountInfo::reduced_offset_to_offset(*offset as u32);
                    });
                    stored_accounts_info
                })
                .ok(),
        }
    }

    /// Returns the way to access this accounts file when archiving
    pub fn internals_for_archive(&self) -> InternalsForArchive {
        match self {
            Self::AppendVec(av) => av.internals_for_archive(),
            Self::TieredStorage(ts) => InternalsForArchive::Mmap(
                ts.reader()
                    .expect("must be a reader when archiving")
                    .data_for_archive(),
            ),
        }
    }
}

/// An enum that creates AccountsFile instance with the specified format.
#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
pub enum AccountsFileProvider {
    #[default]
    AppendVec,
    HotStorage,
}

impl AccountsFileProvider {
    pub fn new_writable(&self, path: impl Into<PathBuf>, file_size: u64) -> AccountsFile {
        match self {
            Self::AppendVec => {
                AccountsFile::AppendVec(AppendVec::new(path, true, file_size as usize))
            }
            Self::HotStorage => AccountsFile::TieredStorage(TieredStorage::new_writable(path)),
        }
    }
}

/// The access method to use when archiving an AccountsFile
#[derive(Debug)]
pub enum InternalsForArchive<'a> {
    /// Accessing the internals is done via Mmap
    Mmap(&'a [u8]),
    /// Accessing the internals is done via File I/O
    FileIo(&'a Path),
}

/// Information after storing accounts
#[derive(Debug)]
pub struct StoredAccountsInfo {
    /// offset in the storage where each account was stored
    pub offsets: Vec<usize>,
    /// total size of all the stored accounts
    pub size: usize,
}

#[cfg(test)]
pub mod tests {
    use crate::accounts_file::AccountsFile;
    impl AccountsFile {
        pub(crate) fn set_current_len_for_tests(&self, len: usize) {
            match self {
                Self::AppendVec(av) => av.set_current_len_for_tests(len),
                Self::TieredStorage(_) => {}
            }
        }
    }
}

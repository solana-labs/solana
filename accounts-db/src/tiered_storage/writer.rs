//! docs/src/proposals/append-vec-storage.md

use {
    crate::{
        account_storage::meta::{
            StorableAccountsWithHashesAndWriteVersions, StoredAccountInfo, StoredMetaWriteVersion,
        },
        storable_accounts::StorableAccounts,
        tiered_storage::{
            error::TieredStorageError, file::TieredStorageFile, footer::TieredStorageFooter,
            TieredStorageFormat, TieredStorageResult,
        },
    },
    solana_sdk::{account::ReadableAccount, hash::Hash},
    std::{borrow::Borrow, path::Path},
};

const EMPTY_ACCOUNT_DATA: [u8; 0] = [0u8; 0];
const PADDING: [u8; 8] = [0x8; 8];

/// A helper function that extracts the lamports, rent epoch, and account data
/// from the specified ReadableAccount, or returns the default of these values
/// when the account is None (e.g. a zero-lamport account).
fn get_account_fields<T: ReadableAccount + Sync>(account: Option<&T>) -> (u64, u64, &[u8]) {
    if let Some(account) = account {
        return (account.lamports(), account.rent_epoch(), account.data());
    }

    (0, u64::MAX, &EMPTY_ACCOUNT_DATA)
}

#[derive(Debug)]
pub struct TieredStorageWriter<'format> {
    storage: TieredStorageFile,
    format: &'format TieredStorageFormat,
}

impl<'format> TieredStorageWriter<'format> {
    pub fn new(
        file_path: impl AsRef<Path>,
        format: &'format TieredStorageFormat,
    ) -> TieredStorageResult<Self> {
        Ok(Self {
            storage: TieredStorageFile::new_writable(file_path)?,
            format,
        })
    }

    /// Persists a single account to a dedicated new account block and
    /// return its stored size.
    ///
    /// The function currently only supports HotAccountMeta, and will
    /// be extended to cover more TieredAccountMeta in future PRs.
    fn write_single_account<T: ReadableAccount + Sync>(
        &self,
        _account: Option<&T>,
        _account_hash: &Hash,
        _write_version: StoredMetaWriteVersion,
        footer: &mut TieredStorageFooter,
    ) -> TieredStorageResult<()> {
        footer.account_entry_count += 1;

        // TODO(yhchiang): implementation will be included in separate PRs.

        Ok(())
    }

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
        let mut footer = TieredStorageFooter {
            account_meta_format: self.format.account_meta_format,
            owners_block_format: self.format.owners_block_format,
            account_block_format: self.format.account_block_format,
            account_index_format: self.format.account_index_format,
            ..TieredStorageFooter::default()
        };

        let len = accounts.accounts.len();
        for i in skip..len {
            let (account, _, hash, write_version) = accounts.get(i);

            self.write_single_account(account, hash, write_version, &mut footer)?;
        }

        footer.write_footer_block(&self.storage)?;

        Err(TieredStorageError::Unsupported())
    }
}

//! docs/src/proposals/append-vec-storage.md

use {
    crate::{
        account_storage::meta::{StorableAccountsWithHashesAndWriteVersions, StoredAccountInfo},
        storable_accounts::StorableAccounts,
        tiered_storage::{
            error::TieredStorageError,
            file::TieredStorageFile,
            footer::{AccountMetaFormat, TieredStorageFooter},
            hot::HotAccountMeta,
            meta::TieredAccountMeta,
            TieredStorageFormat, TieredStorageResult,
        },
    },
    solana_sdk::{account::ReadableAccount, hash::Hash},
    std::{borrow::Borrow, path::Path},
};

#[derive(Debug)]
pub struct TieredStorageWriter<'format> {
    storage: TieredStorageFile,
    format: &'format TieredStorageFormat,
}

impl<'format> TieredStorageWriter<'format> {
    pub fn new(
        file_path: &Path,
        format: &'format TieredStorageFormat,
    ) -> TieredStorageResult<Self> {
        if file_path.exists() {
            return Err(TieredStorageError::ReadOnlyFileUpdateError(
                file_path.to_path_buf(),
            ));
        }
        Ok(Self {
            storage: TieredStorageFile::new_writable(file_path),
            format,
        })
    }

    fn write_accounts_impl<
        'a,
        'b,
        T: ReadableAccount + Sync,
        U: StorableAccounts<'a, T>,
        V: Borrow<Hash>,
        W: TieredAccountMeta,
    >(
        &self,
        _accounts: &StorableAccountsWithHashesAndWriteVersions<'a, 'b, T, U, V>,
        footer: TieredStorageFooter,
        mut _account_metas: Vec<W>,
        _skip: usize,
    ) -> TieredStorageResult<Vec<StoredAccountInfo>> {
        footer.write_footer_block(&self.storage)?;

        Err(TieredStorageResult::Unsupported())
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
        let footer = TieredStorageFooter {
            account_meta_format: self.format.account_meta_format,
            owners_block_format: self.format.owners_block_format,
            account_block_format: self.format.account_block_format,
            account_index_format: self.format.account_index_format,
            ..TieredStorageFooter::default()
        };
        match footer.account_meta_format {
            AccountMetaFormat::Hot => {
                self.write_accounts_impl(accounts, footer, Vec::<HotAccountMeta>::new(), skip)
            }
            AccountMetaFormat::Cold => {
                unsupported!();
                // ColdAccountMeta has not yet introduced.
                // self.write_accounts_impl(accounts, footer, Vec::<HotAccountMeta>::new(), skip)
            }
        }
    }
}

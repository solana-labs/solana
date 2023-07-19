//! docs/src/proposals/append-vec-storage.md

use {
    crate::{
        account_storage::meta::{StorableAccountsWithHashesAndWriteVersions, StoredAccountInfo},
        storable_accounts::StorableAccounts,
        tiered_storage::{
            file::TieredStorageFile,
            footer::{AccountMetaFormat, TieredStorageFooter},
            hot::HotAccountMeta,
            meta::TieredAccountMeta,
            TieredStorageFormat,
        },
    },
    solana_sdk::{account::ReadableAccount, hash::Hash},
    std::{borrow::Borrow, fs::remove_file, path::Path},
};

#[derive(Debug)]
pub struct TieredStorageWriter<'format> {
    storage: TieredStorageFile,
    format: &'format TieredStorageFormat,
}

impl<'format> TieredStorageWriter<'format> {
    pub fn new(file_path: &Path, format: &'format TieredStorageFormat) -> Self {
        let _ignored = remove_file(file_path);
        Self {
            storage: TieredStorageFile::new_writable(file_path),
            format,
        }
    }

    fn append_accounts_impl<
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
    ) -> Option<Vec<StoredAccountInfo>> {
        footer.write_footer_block(&self.storage).ok()?;
        None
    }

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
        let footer = TieredStorageFooter {
            account_meta_format: self.format.account_meta_format,
            owners_block_format: self.format.owners_block_format,
            account_block_format: self.format.account_block_format,
            account_index_format: self.format.account_index_format,
            ..TieredStorageFooter::default()
        };
        match footer.account_meta_format {
            AccountMetaFormat::Hot => {
                self.append_accounts_impl(accounts, footer, Vec::<HotAccountMeta>::new(), skip)
            }
        }
    }
}

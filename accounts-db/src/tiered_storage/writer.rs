//! docs/src/proposals/append-vec-storage.md

use {
    crate::{
        account_storage::meta::{
            StorableAccountsWithHashesAndWriteVersions, StoredAccountInfo, StoredMetaWriteVersion,
        },
        storable_accounts::StorableAccounts,
        tiered_storage::{
            byte_block::ByteBlockWriter,
            error::TieredStorageError,
            file::TieredStorageFile,
            footer::TieredStorageFooter,
            hot::HotAccountMeta,
            meta::{AccountMetaFlags, AccountMetaOptionalFields, TieredAccountMeta},
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
    fn write_single_account<T: TieredAccountMeta, U: ReadableAccount + Sync>(
        &self,
        account: Option<&U>,
        account_hash: &Hash,
        write_version: StoredMetaWriteVersion,
        footer: &mut TieredStorageFooter,
    ) -> TieredStorageResult<usize> {
        let (lamports, rent_epoch, account_data) = get_account_fields(account);

        let optional_fields =
            AccountMetaOptionalFields::new_from_fields(rent_epoch, account_hash, write_version);

        let flags = AccountMetaFlags::new_from(&optional_fields);
        let meta = T::new()
            .with_lamports(lamports)
            .with_account_data_size(account_data.len() as u64)
            .with_account_data_padding(((8 - (account_data.len() % 8)) % 8).try_into().unwrap())
            .with_flags(&flags);

        // writes the account in the following format:
        //  +------------------+
        //  | account meta     |
        //  | account data     |
        //  | padding (if any) |
        //  | optional fields  |
        //  +------------------+
        let mut writer = ByteBlockWriter::new(footer.account_block_format);
        writer.write_type(&meta)?;
        writer.write(account_data)?;
        if meta.account_data_padding() > 0 {
            writer.write(&PADDING[0..meta.account_data_padding() as usize])?;
        }
        writer.write_optional_fields(&optional_fields)?;
        let account_block = writer.finish()?;

        self.storage.write_bytes(&account_block)?;
        footer.account_entry_count += 1;

        // For now it only supports HotAccountMeta, so the intra-block offset
        // is always zero.
        Ok(account_block.len())
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

        let mut cursor: usize = 0;
        let len = accounts.accounts.len();
        for i in skip..len {
            let (account, _, hash, write_version) = accounts.get(i);

            let stored_size = self.write_single_account::<HotAccountMeta, T>(
                account,
                hash,
                write_version,
                &mut footer,
            )?;
            // advance the cursor with the stored size
            cursor += stored_size;
        }

        footer.account_index_offset = cursor as u64;
        // TODO(yhchiang): index block will be included in a separate PR.

        footer.write_footer_block(&self.storage)?;

        Err(TieredStorageError::Unsupported())
    }
}

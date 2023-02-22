//! docs/src/proposals/append-vec-storage.md

use {
    crate::{
        accounts_data_storage::{
            data_block::{AccountDataBlockFormat, AccountDataBlockWriter},
            error::AccountsDataStorageError,
            file::AccountsDataStorageFile,
            footer::AccountsDataStorageFooter,
            meta_entries::{
                AccountMetaFlags, AccountMetaOptionalFields, AccountMetaStorageEntry,
                ACCOUNT_DATA_ENTIRE_BLOCK, ACCOUNT_META_ENTRY_SIZE_BYTES,
            },
        },
        append_vec::{
            AccountMeta, AppendVec, StorableAccountsWithHashesAndWriteVersions, StoredAccountMeta,
            StoredMeta,
        },
        storable_accounts::StorableAccounts,
    },
    solana_sdk::{account::ReadableAccount, hash::Hash, pubkey::Pubkey},
    std::{borrow::Borrow, collections::HashMap, fs::remove_file, mem, path::Path},
};

pub const ACCOUNT_DATA_BLOCK_SIZE: usize = 4096;
pub const ACCOUNTS_DATA_STORAGE_FORMAT_VERSION: u64 = 1;

pub type Result<T> = std::result::Result<T, AccountsDataStorageError>;

lazy_static! {
    pub static ref HASH_DEFAULT: Hash = Hash::default();
}

pub(crate) struct AccountOwnerTable {
    pub owners_vec: Vec<Pubkey>,
    pub owners_map: HashMap<Pubkey, u32>,
}

impl AccountOwnerTable {
    pub fn new() -> Self {
        Self {
            owners_vec: vec![],
            owners_map: HashMap::new(),
        }
    }
    pub fn check_and_add(&mut self, pubkey: &Pubkey) -> u32 {
        if let Some(index) = self.owners_map.get(pubkey) {
            return index.clone();
        }
        let index: u32 = self.owners_vec.len().try_into().unwrap();
        self.owners_vec.push(*pubkey);
        self.owners_map.insert(*pubkey, index);

        index
    }
}

#[derive(Debug)]
pub struct AccountsDataStorageWriter {
    storage: AccountsDataStorageFile,
}

impl AccountsDataStorageWriter {
    /// Create a new accounts-state-file
    #[allow(dead_code)]
    pub fn new(file_path: &Path) -> Self {
        let _ignored = remove_file(file_path);
        Self {
            storage: AccountsDataStorageFile::new(file_path, true),
        }
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
    ) -> Option<Vec<usize>> {
        let mut footer = AccountsDataStorageFooter::new();
        footer.format_version = ACCOUNTS_DATA_STORAGE_FORMAT_VERSION;
        let mut cursor = 0;
        let mut account_metas: Vec<AccountMetaStorageEntry> = vec![];
        let mut account_pubkeys: Vec<Pubkey> = vec![];
        let mut owners_table = AccountOwnerTable::new();
        let mut dummy_hash: Hash = Hash::new_unique();

        let mut data_block_writer = self.new_data_block_writer();
        footer.account_data_block_size = ACCOUNT_DATA_BLOCK_SIZE as u64;

        let mut buffered_account_metas: Vec<AccountMetaStorageEntry> = vec![];
        let mut buffered_account_pubkeys: Vec<Pubkey> = vec![];

        let len = accounts.accounts.len();
        let mut account_storage_sizes: Vec<usize> = Vec::with_capacity(len);

        for i in skip..len {
            let (account, pubkey, hash, write_version_obsolete) = accounts.get(i);
            let account_meta = account
                .map(|account| AccountMeta {
                    lamports: account.lamports(),
                    owner: *account.owner(),
                    rent_epoch: account.rent_epoch(),
                    executable: account.executable(),
                })
                .unwrap_or_default();

            let stored_meta = StoredMeta {
                pubkey: *pubkey,
                data_len: account
                    .map(|account| account.data().len())
                    .unwrap_or_default() as u64,
                write_version_obsolete,
            };

            let stored_account_meta = StoredAccountMeta {
                meta: &stored_meta,
                account_meta: &account_meta,
                data: account.map(|account| account.data()).unwrap_or_default(),
                offset: 0,
                stored_size: 0,
                hash: hash,
            };

            // TODO(yhchiang): this is an estimation
            account_storage_sizes.push(
                std::mem::size_of::<AccountMetaStorageEntry>()
                    + 2 * std::mem::size_of::<Hash>()
                    + stored_meta.data_len as usize / 2,
            );

            data_block_writer = self
                .write_stored_account_meta(
                    &stored_account_meta,
                    &mut cursor,
                    &mut footer,
                    &mut account_metas,
                    &mut account_pubkeys,
                    &mut owners_table,
                    data_block_writer,
                    &mut buffered_account_metas,
                    &mut buffered_account_pubkeys,
                    &mut dummy_hash,
                )
                .unwrap();
        }

        // Persist the last block if any
        if buffered_account_metas.len() > 0 {
            self.flush_account_data_block(
                &mut cursor,
                &mut footer,
                &mut account_metas,
                &mut account_pubkeys,
                &mut buffered_account_metas,
                &mut buffered_account_pubkeys,
                data_block_writer,
            )
            .ok()?;
        }

        assert_eq!(buffered_account_metas.len(), 0);
        assert_eq!(buffered_account_pubkeys.len(), 0);
        assert_eq!(footer.account_meta_count, account_metas.len() as u32);

        self.write_account_metas_block(&mut cursor, &mut footer, &account_metas)
            .ok()?;
        self.write_account_pubkeys_block(&mut cursor, &mut footer, &account_pubkeys)
            .ok()?;

        self.write_owners_block(&mut cursor, &mut footer, &owners_table.owners_vec)
            .ok()?;

        footer.write_footer_block(&self.storage).ok()?;

        Some(account_storage_sizes)
    }

    fn new_data_block_writer(&self) -> AccountDataBlockWriter {
        return AccountDataBlockWriter::new(AccountDataBlockFormat::Lz4);
    }

    pub(crate) fn write_account_metas_block(
        &self,
        cursor: &mut u64,
        footer: &mut AccountsDataStorageFooter,
        account_metas: &Vec<AccountMetaStorageEntry>,
    ) -> Result<()> {
        let entry_size = ACCOUNT_META_ENTRY_SIZE_BYTES;
        footer.account_metas_offset = *cursor;
        footer.account_meta_entry_size = entry_size;
        for account_meta in account_metas {
            *cursor += account_meta.write_account_meta_entry(&self.storage)? as u64;
        }
        // make sure cursor advanced as what we expected
        assert_eq!(
            footer.account_metas_offset + (entry_size * account_metas.len() as u32) as u64,
            *cursor
        );

        Ok(())
    }

    pub(crate) fn write_account_pubkeys_block(
        &self,
        cursor: &mut u64,
        footer: &mut AccountsDataStorageFooter,
        pubkeys: &Vec<Pubkey>,
    ) -> Result<()> {
        footer.account_pubkeys_offset = *cursor;

        self.write_pubkeys_block(cursor, pubkeys)
    }

    fn write_owners_block(
        &self,
        cursor: &mut u64,
        footer: &mut AccountsDataStorageFooter,
        pubkeys: &Vec<Pubkey>,
    ) -> Result<()> {
        footer.owners_offset = *cursor;
        footer.owner_count = pubkeys.len() as u32;
        footer.owner_entry_size = mem::size_of::<Pubkey>() as u32;

        self.write_pubkeys_block(cursor, pubkeys)
    }

    fn write_pubkeys_block(&self, cursor: &mut u64, pubkeys: &Vec<Pubkey>) -> Result<()> {
        for pubkey in pubkeys {
            *cursor += self.storage.write_type(pubkey)? as u64;
        }

        Ok(())
    }

    fn flush_account_data_block(
        &self,
        cursor: &mut u64,
        footer: &mut AccountsDataStorageFooter,
        account_metas: &mut Vec<AccountMetaStorageEntry>,
        account_pubkeys: &mut Vec<Pubkey>,
        input_metas: &mut Vec<AccountMetaStorageEntry>,
        input_pubkeys: &mut Vec<Pubkey>,
        data_block_writer: AccountDataBlockWriter,
    ) -> Result<()> {
        // Persist the current block
        let (encoded_data, _raw_data_size) = data_block_writer.finish()?;
        self.storage.write_bytes(&encoded_data)?;

        assert_eq!(input_metas.len(), input_pubkeys.len());

        for input_meta in &mut input_metas.into_iter() {
            input_meta.block_offset = *cursor;
        }
        for input_meta in &mut input_metas.into_iter() {
            assert_eq!(input_meta.block_offset, *cursor);
        }
        footer.account_meta_count += input_metas.len() as u32;
        account_metas.append(input_metas);
        account_pubkeys.append(input_pubkeys);

        *cursor += encoded_data.len() as u64;
        assert_eq!(input_metas.len(), 0);
        assert_eq!(input_pubkeys.len(), 0);

        Ok(())
    }

    fn write_stored_account_meta(
        &self,
        account: &StoredAccountMeta,
        cursor: &mut u64,
        footer: &mut AccountsDataStorageFooter,
        account_metas: &mut Vec<AccountMetaStorageEntry>,
        account_pubkeys: &mut Vec<Pubkey>,
        owners_table: &mut AccountOwnerTable,
        mut data_block: AccountDataBlockWriter,
        buffered_account_metas: &mut Vec<AccountMetaStorageEntry>,
        buffered_account_pubkeys: &mut Vec<Pubkey>,
        _hash: &mut Hash,
    ) -> Result<AccountDataBlockWriter> {
        if !account.sanitize() {
            // Not Ok
        }

        let optional_fields = AccountMetaOptionalFields {
            rent_epoch: Some(account.account_meta.rent_epoch),
            account_hash: Some(*account.hash),
            write_version_obsolete: Some(account.meta.write_version_obsolete),
        };

        if account.data.len() > ACCOUNT_DATA_BLOCK_SIZE {
            self.write_blob_account_data_block(
                cursor,
                footer,
                account_metas,
                account_pubkeys,
                owners_table,
                account,
            )?;
            return Ok(data_block);
        }

        // If the current data cannot fit in the current block, then
        // persist the current block.
        if data_block.len() + account.data.len() + optional_fields.size() > ACCOUNT_DATA_BLOCK_SIZE
        {
            self.flush_account_data_block(
                cursor,
                footer,
                account_metas,
                account_pubkeys,
                buffered_account_metas,
                buffered_account_pubkeys,
                data_block,
            )?;
            data_block = self.new_data_block_writer();
        }

        let owner_local_id = owners_table.check_and_add(&account.account_meta.owner);
        let local_offset = data_block.len();

        data_block.write(account.data, account.data.len())?;
        optional_fields.write(&mut data_block)?;

        buffered_account_metas.push(
            AccountMetaStorageEntry::new()
                .with_lamports(account.account_meta.lamports)
                .with_block_offset(*cursor)
                .with_owner_local_id(owner_local_id)
                .with_uncompressed_data_size(account.data.len() as u16)
                .with_intra_block_offset(local_offset as u16)
                .with_flags(
                    AccountMetaFlags::new()
                        .with_bit(
                            AccountMetaFlags::EXECUTABLE,
                            account.account_meta.executable,
                        )
                        .to_value(),
                )
                .with_optional_fields(&optional_fields),
        );
        buffered_account_pubkeys.push(account.meta.pubkey);

        Ok(data_block)
    }

    fn write_blob_account_data_block(
        &self,
        cursor: &mut u64,
        footer: &mut AccountsDataStorageFooter,
        account_metas: &mut Vec<AccountMetaStorageEntry>,
        account_pubkeys: &mut Vec<Pubkey>,
        owners_table: &mut AccountOwnerTable,
        account: &StoredAccountMeta,
    ) -> Result<()> {
        let owner_local_id = owners_table.check_and_add(&account.account_meta.owner);
        let optional_fields = AccountMetaOptionalFields {
            rent_epoch: Some(account.account_meta.rent_epoch),
            account_hash: Some(*account.hash),
            write_version_obsolete: Some(account.meta.write_version_obsolete),
        };

        let mut writer = AccountDataBlockWriter::new(AccountDataBlockFormat::Lz4);
        writer.write(&account.data, account.data.len())?;
        optional_fields.write(&mut writer)?;

        let (data, _uncompressed_len) = writer.finish().unwrap();
        let compressed_length = data.len();
        self.storage.write_bytes(&data)?;

        account_metas.push(
            AccountMetaStorageEntry::new()
                .with_lamports(account.account_meta.lamports)
                .with_block_offset(*cursor)
                .with_owner_local_id(owner_local_id)
                .with_uncompressed_data_size(ACCOUNT_DATA_ENTIRE_BLOCK)
                .with_intra_block_offset(0)
                .with_flags(
                    AccountMetaFlags::new()
                        .with_bit(
                            AccountMetaFlags::EXECUTABLE,
                            account.account_meta.executable,
                        )
                        .to_value(),
                )
                .with_optional_fields(&optional_fields),
        );
        account_pubkeys.push(account.meta.pubkey);

        *cursor += compressed_length as u64;
        footer.account_meta_count += 1;

        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////////

    #[allow(dead_code)]
    pub fn write_from_append_vec(&self, append_vec: &AppendVec) -> Result<()> {
        let mut footer = AccountsDataStorageFooter::new();
        footer.format_version = ACCOUNTS_DATA_STORAGE_FORMAT_VERSION;
        let mut cursor = 0;
        let mut account_metas: Vec<AccountMetaStorageEntry> = vec![];
        let mut account_pubkeys: Vec<Pubkey> = vec![];
        let mut owners_table = AccountOwnerTable::new();
        let mut hash: Hash = Hash::new_unique();

        self.write_account_data_blocks(
            &mut cursor,
            &mut footer,
            &mut account_metas,
            &mut account_pubkeys,
            &mut owners_table,
            &mut hash,
            &append_vec,
        )?;

        self.write_account_metas_block(&mut cursor, &mut footer, &account_metas)?;
        self.write_account_pubkeys_block(&mut cursor, &mut footer, &account_pubkeys)?;

        self.write_owners_block(&mut cursor, &mut footer, &owners_table.owners_vec)?;

        footer.write_footer_block(&self.storage)?;

        Ok(())
    }

    #[allow(dead_code)]
    fn write_account_data_blocks(
        &self,
        cursor: &mut u64,
        footer: &mut AccountsDataStorageFooter,
        account_metas: &mut Vec<AccountMetaStorageEntry>,
        account_pubkeys: &mut Vec<Pubkey>,
        owners_table: &mut AccountOwnerTable,
        // TODO(yhchiang): update hash
        _hash: &mut Hash,
        append_vec: &AppendVec,
    ) -> Result<()> {
        let mut offset = 0;
        footer.account_data_block_size = ACCOUNT_DATA_BLOCK_SIZE as u64;

        let mut buffered_account_metas: Vec<AccountMetaStorageEntry> = vec![];
        let mut buffered_account_pubkeys: Vec<Pubkey> = vec![];
        let mut data_block_writer = self.new_data_block_writer();

        while let Some((account, next_offset)) = append_vec.get_account(offset) {
            offset = next_offset;
            data_block_writer = self.write_stored_account_meta(
                &account,
                cursor,
                footer,
                account_metas,
                account_pubkeys,
                owners_table,
                data_block_writer,
                &mut buffered_account_metas,
                &mut buffered_account_pubkeys,
                _hash,
            )?;
        }

        // Persist the last block if any
        if buffered_account_metas.len() > 0 {
            self.flush_account_data_block(
                cursor,
                footer,
                account_metas,
                account_pubkeys,
                &mut buffered_account_metas,
                &mut buffered_account_pubkeys,
                data_block_writer,
            )?;
        }

        assert_eq!(buffered_account_metas.len(), 0);
        assert_eq!(buffered_account_pubkeys.len(), 0);
        assert_eq!(footer.account_meta_count, account_metas.len() as u32);

        Ok(())
    }
}

use {
    crate::accounts_data_storage::{
        data_block::AccountDataBlockFormat, error::AccountsDataStorageError,
        file::AccountsDataStorageFile,
    },
    serde::{Deserialize, Serialize},
    solana_sdk::hash::Hash,
    std::mem,
};

type Result<T> = std::result::Result<T, AccountsDataStorageError>;

pub(crate) const FOOTER_SIZE: i64 =
    (mem::size_of::<AccountsDataStorageFooter>() + mem::size_of::<u64>()) as i64;
pub(crate) const FOOTER_TAIL_SIZE: i64 = 24;

pub(crate) const FOOTER_MAGIC_NUMBER: u64 = 0x501A2AB5; // SOLALABS -> SOLANA LABS

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct AccountsDataStorageMagicNumber {
    pub magic: u64,
}

impl AccountsDataStorageMagicNumber {
    pub fn new() -> Self {
        Self { magic: 0 }
    }
    fn default() -> Self {
        Self {
            magic: FOOTER_MAGIC_NUMBER,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct AccountsDataStorageFooter {
    // regular accounts' stats
    pub account_meta_count: u32,
    pub account_meta_entry_size: u32,
    pub account_data_block_size: u64,

    // owner's stats
    pub owner_count: u32,
    pub owner_entry_size: u32,

    // offsets
    pub account_metas_offset: u64,
    pub account_pubkeys_offset: u64,
    pub owners_offset: u64,

    // misc
    pub data_block_format: AccountDataBlockFormat,
    pub hash: Hash,

    // account range
    pub min_account_address: Hash,
    pub max_account_address: Hash,

    // tailing information
    pub footer_size: u64,
    pub format_version: u64,
    // This field is persisted in the storage but not in this struct.
    // pub magic_number: u64,  // FOOTER_MAGIC_NUMBER
}

impl AccountsDataStorageFooter {
    pub fn new() -> Self {
        Self { ..Self::default() }
    }
}

impl Default for AccountsDataStorageFooter {
    fn default() -> Self {
        Self {
            account_meta_count: 0,
            account_meta_entry_size: 0,
            account_data_block_size: 0,
            owner_count: 0,
            owner_entry_size: 0,
            account_metas_offset: 0,
            account_pubkeys_offset: 0,
            owners_offset: 0,
            data_block_format: AccountDataBlockFormat::Lz4,
            hash: Hash::new_unique(),
            min_account_address: Hash::default(),
            max_account_address: Hash::default(),
            footer_size: FOOTER_SIZE as u64,
            format_version: 1,
        }
    }
}

impl AccountsDataStorageFooter {
    pub fn write_footer_block(&self, ads_file: &AccountsDataStorageFile) -> Result<()> {
        ads_file.write_type(self)?;
        ads_file.write_type(&AccountsDataStorageMagicNumber::default())?;

        Ok(())
    }

    pub fn new_from_footer_block(ads_file: &AccountsDataStorageFile) -> Result<Self> {
        let mut footer_size: u64 = 0;
        let mut footer_version: u64 = 0;
        let mut magic_number = AccountsDataStorageMagicNumber::new();

        ads_file.seek_from_end(-FOOTER_TAIL_SIZE)?;
        ads_file.read_type(&mut footer_size)?;
        ads_file.read_type(&mut footer_version)?;
        ads_file.read_type(&mut magic_number)?;

        if magic_number != AccountsDataStorageMagicNumber::default() {
            return Err(AccountsDataStorageError::MagicNumberMismatch);
        }

        let mut footer = Self::new();
        ads_file.seek_from_end(-(footer_size as i64))?;
        ads_file.read_type(&mut footer)?;

        Ok(footer)
    }
}

#[cfg(test)]
pub mod tests {
    use {
        crate::{
            accounts_data_storage::{
                data_block::AccountDataBlockFormat,
                file::AccountsDataStorageFile,
                footer::{AccountsDataStorageFooter, FOOTER_SIZE},
            },
            append_vec::test_utils::get_append_vec_path,
        },
        memoffset::offset_of,
        solana_sdk::hash::Hash,
        std::mem,
    };

    #[test]
    /// Make sure the in-memory size is what we expected.
    fn test_footer_size() {
        assert_eq!(
            mem::size_of::<AccountsDataStorageFooter>() + mem::size_of::<u64>(),
            FOOTER_SIZE as usize
        );
    }

    #[test]
    fn test_footer() {
        let path = get_append_vec_path("test_file_footer");
        let expected_footer = AccountsDataStorageFooter {
            account_meta_count: 300,
            account_meta_entry_size: 24,
            account_data_block_size: 4096,
            owner_count: 250,
            owner_entry_size: 32,
            account_metas_offset: 1062400,
            account_pubkeys_offset: 1069600,
            owners_offset: 1081200,
            data_block_format: AccountDataBlockFormat::Lz4,
            hash: Hash::new_unique(),
            min_account_address: Hash::default(),
            max_account_address: Hash::default(),
            footer_size: FOOTER_SIZE as u64,
            format_version: 1,
        };

        {
            let ads_file = AccountsDataStorageFile::new(&path.path, true);
            expected_footer.write_footer_block(&ads_file).unwrap();
        }

        // Reopen the same storage, and expect the persisted footer is
        // the same as what we have written.
        {
            let ads_file = AccountsDataStorageFile::new(&path.path, true);
            let footer = AccountsDataStorageFooter::new_from_footer_block(&ads_file).unwrap();
            assert_eq!(expected_footer, footer);
        }
    }

    #[test]
    fn test_footer_layout() {
        assert_eq!(
            offset_of!(AccountsDataStorageFooter, account_meta_count),
            0x00
        );
        assert_eq!(
            offset_of!(AccountsDataStorageFooter, account_meta_entry_size),
            0x04
        );
        assert_eq!(
            offset_of!(AccountsDataStorageFooter, account_data_block_size),
            0x08
        );
        assert_eq!(offset_of!(AccountsDataStorageFooter, owner_count), 0x10);
        assert_eq!(
            offset_of!(AccountsDataStorageFooter, owner_entry_size),
            0x14
        );
        assert_eq!(
            offset_of!(AccountsDataStorageFooter, account_metas_offset),
            0x18
        );
        assert_eq!(
            offset_of!(AccountsDataStorageFooter, account_pubkeys_offset),
            0x20
        );
        assert_eq!(offset_of!(AccountsDataStorageFooter, owners_offset), 0x28);
        assert_eq!(
            offset_of!(AccountsDataStorageFooter, data_block_format),
            0x30
        );
        assert_eq!(offset_of!(AccountsDataStorageFooter, hash), 0x38);
        assert_eq!(
            offset_of!(AccountsDataStorageFooter, min_account_address),
            0x58
        );
        assert_eq!(
            offset_of!(AccountsDataStorageFooter, max_account_address),
            0x78
        );
        assert_eq!(offset_of!(AccountsDataStorageFooter, footer_size), 0x98);
        assert_eq!(offset_of!(AccountsDataStorageFooter, format_version), 0xA0);
    }
}

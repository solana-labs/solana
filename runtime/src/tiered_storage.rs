#![allow(dead_code)]

pub mod byte_block;
pub mod error;
pub mod file;
pub mod footer;
pub mod hot;
pub mod index;
pub mod meta;
pub mod mmap_utils;
pub mod readable;

use {
    error::TieredStorageError,
    footer::{AccountBlockFormat, AccountMetaFormat, OwnersBlockFormat},
    index::AccountIndexFormat,
    std::path::{Path, PathBuf},
};

pub type TieredStorageResult<T> = Result<T, TieredStorageError>;

/// The struct that defines the formats of all building blocks of a
/// TieredAccountsFile.
#[derive(Debug)]
pub struct TieredAccountsFileFormat {
    pub meta_entry_size: usize,
    pub account_meta_format: AccountMetaFormat,
    pub owners_block_format: OwnersBlockFormat,
    pub account_index_format: AccountIndexFormat,
    pub account_block_format: AccountBlockFormat,
}

/// The struct for a TieredAccountsFile.
#[derive(Debug)]
pub struct TieredAccountsFile {
    format: Option<&'static TieredAccountsFileFormat>,
    path: PathBuf,
}

impl TieredAccountsFile {
    /// Creates a new writable instance of TieredAccountsFile basedon the
    /// specified file_path and TieredAccountsFileFormat.
    ///
    /// Note that the actual file will not be created until append_accounts
    /// is called.
    pub fn new_writable(
        file_path: &Path,
        format: &'static TieredAccountsFileFormat,
    ) -> TieredStorageResult<Self> {
        Ok(Self {
            format: Some(format),
            path: file_path.to_path_buf(),
        })
    }

    /// Returns the path to this TieredAccountsFile.
    pub fn get_path(&self) -> PathBuf {
        self.path.clone()
    }
}

#[cfg(test)]
pub mod tests {
    use {super::*, hot::HOT_FORMAT, tempfile::TempDir};

    #[test]
    fn test_new_writable() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test_new");
        let ta_file = TieredAccountsFile::new_writable(&path, &HOT_FORMAT).unwrap();

        assert_eq!(ta_file.get_path(), path);
    }
}

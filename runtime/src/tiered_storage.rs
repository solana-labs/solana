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
    std::path::PathBuf,
};

pub type TieredStorageResult<T> = Result<T, TieredStorageError>;

/// The struct that defines the formats of all building blocks of a
/// TieredStorage.
#[derive(Clone, Debug)]
pub struct TieredStorageFormat {
    pub meta_entry_size: usize,
    pub account_meta_format: AccountMetaFormat,
    pub owners_block_format: OwnersBlockFormat,
    pub account_index_format: AccountIndexFormat,
    pub account_block_format: AccountBlockFormat,
}

/// The struct for a TieredStorage.
#[derive(Debug)]
pub struct TieredStorage {
    format: Option<TieredStorageFormat>,
    path: PathBuf,
}

impl TieredStorage {
    /// Creates a new writable instance of TieredStorage based on the
    /// specified path and TieredStorageFormat.
    ///
    /// Note that the actual file will not be created until append_accounts
    /// is called.
    pub fn new_writable(
        path: impl Into<PathBuf>,
        format: &TieredStorageFormat,
    ) -> TieredStorageResult<Self> {
        Ok(Self {
            format: Some(format.clone()),
            path: path.into(),
        })
    }

    /// Returns the path to this TieredStorage.
    pub fn path(&self) -> &PathBuf {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use {super::*, hot::HOT_FORMAT, tempfile::NamedTempFile};

    #[test]
    fn test_new_writable() {
        let temp_file = NamedTempFile::new().unwrap();
        let ts = TieredStorage::new_writable(temp_file.path(), &HOT_FORMAT).unwrap();

        assert_eq!(ts.path(), temp_file.path());
    }
}

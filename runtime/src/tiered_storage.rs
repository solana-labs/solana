pub mod byte_block;
pub mod error;
pub mod file;
pub mod footer;
pub mod hot;
pub mod index;
pub mod meta;
pub mod mmap_utils;
pub mod readable;

use crate::tiered_storage::error::TieredStorageError;

pub type TieredStorageResult<T> = Result<T, TieredStorageError>;

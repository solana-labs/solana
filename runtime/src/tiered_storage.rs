pub mod byte_block;
pub mod error;
pub mod file;
pub mod footer;
pub mod meta;
pub mod mmap_utils;

use crate::tiered_storage::error::TieredStorageError;

pub type TieredStorageResult<T> = Result<T, TieredStorageError>;

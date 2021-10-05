#![allow(clippy::integer_arithmetic)]
mod bucket;
pub mod bucket_api;
mod bucket_item;
pub mod bucket_map;
mod bucket_stats;
mod bucket_storage;
mod index_entry;

pub type MaxSearch = u8;
pub type RefCount = u64;

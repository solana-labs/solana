#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(min_specialization))]
#![allow(clippy::integer_arithmetic)]
#![allow(clippy::mut_from_ref)]
mod bucket;
mod bucket_item;
pub mod bucket_map;
mod bucket_stats;
mod bucket_storage;
mod index_entry;

pub type MaxSearch = u8;
pub type RefCount = u64;

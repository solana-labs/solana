#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(min_specialization))]
#![allow(clippy::arithmetic_side_effects)]

#[macro_use]
extern crate lazy_static;

pub mod account_info;
pub mod account_locks;
pub mod account_storage;
pub mod accounts;
mod accounts_cache;
pub mod accounts_db;
pub mod accounts_file;
pub mod accounts_hash;
pub mod accounts_index;
pub mod accounts_index_storage;
pub mod accounts_partition;
pub mod accounts_update_notifier_interface;
mod active_stats;
pub mod ancestors;
mod ancient_append_vecs;
pub mod append_vec;
pub mod blockhash_queue;
mod bucket_map_holder;
mod bucket_map_holder_stats;
mod buffered_reader;
mod cache_hash_data;
mod cache_hash_data_stats;
pub mod contains;
pub mod epoch_accounts_hash;
mod file_io;
pub mod hardened_unpack;
pub mod partitioned_rewards;
pub mod pubkey_bins;
mod read_only_accounts_cache;
mod rolling_bit_field;
pub mod secondary_index;
pub mod shared_buffer_reader;
pub mod sorted_storages;
pub mod stake_rewards;
pub mod storable_accounts;
pub mod tiered_storage;
pub mod utils;
mod verify_accounts_hash_in_background;
pub mod waitable_condvar;

// the accounts-hash-cache-tool needs access to these types
pub use {
    accounts_hash::CalculateHashIntermediate as CacheHashDataFileEntry,
    cache_hash_data::{
        parse_filename as parse_cache_hash_data_filename, Header as CacheHashDataFileHeader,
        ParsedFilename as ParsedCacheHashDataFilename,
    },
};

#[macro_use]
extern crate solana_metrics;
#[macro_use]
extern crate serde_derive;

#[cfg_attr(feature = "frozen-abi", macro_use)]
#[cfg(feature = "frozen-abi")]
extern crate solana_frozen_abi_macro;

#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(min_specialization))]
#![allow(clippy::arithmetic_side_effects)]

#[macro_use]
extern crate lazy_static;

pub mod account_info;
pub mod account_overrides;
pub mod account_rent_state;
pub mod account_storage;
pub mod accounts;
pub mod accounts_cache;
pub mod accounts_db;
pub mod accounts_file;
pub mod accounts_hash;
pub mod accounts_index;
pub mod accounts_index_storage;
pub mod accounts_partition;
pub mod accounts_update_notifier_interface;
pub mod active_stats;
pub mod ancestors;
pub mod ancient_append_vecs;
pub mod append_vec;
pub mod blockhash_queue;
pub mod bucket_map_holder;
pub mod bucket_map_holder_stats;
pub mod cache_hash_data;
pub mod cache_hash_data_stats;
pub mod contains;
pub mod epoch_accounts_hash;
pub mod hardened_unpack;
pub mod in_mem_accounts_index;
pub mod inline_spl_token;
pub mod inline_spl_token_2022;
pub mod nonce_info;
pub mod partitioned_rewards;
mod pubkey_bins;
mod read_only_accounts_cache;
pub mod rent_collector;
pub mod rent_debits;
mod rolling_bit_field;
pub mod secondary_index;
pub mod shared_buffer_reader;
pub mod sorted_storages;
pub mod stake_rewards;
pub mod storable_accounts;
pub mod tiered_storage;
pub mod transaction_error_metrics;
pub mod transaction_results;
mod verify_accounts_hash_in_background;
pub mod waitable_condvar;

#[macro_use]
extern crate solana_metrics;
#[macro_use]
extern crate serde_derive;

#[macro_use]
extern crate solana_frozen_abi_macro;

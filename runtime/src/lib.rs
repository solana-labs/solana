#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(min_specialization))]
#![allow(clippy::integer_arithmetic)]

#[macro_use]
extern crate lazy_static;

pub mod account_info;
pub mod account_overrides;
pub mod account_rent_state;
pub mod account_storage;
pub mod accounts;
pub mod accounts_background_service;
pub mod accounts_cache;
pub mod accounts_db;
pub mod accounts_hash;
pub mod accounts_index;
pub mod accounts_index_storage;
pub mod accounts_update_notifier_interface;
mod active_stats;
pub mod ancestors;
mod ancient_append_vecs;
pub mod append_vec;
pub mod bank;
pub mod bank_client;
pub mod bank_forks;
pub mod bank_utils;
pub mod block_cost_limits;
pub mod blockhash_queue;
pub mod bucket_map_holder;
pub mod bucket_map_holder_stats;
pub mod builtins;
pub mod cache_hash_data;
pub mod cache_hash_data_stats;
pub mod commitment;
pub mod contains;
pub mod cost_model;
pub mod cost_tracker;
pub mod epoch_accounts_hash;
pub mod epoch_stakes;
pub mod genesis_utils;
pub mod hardened_unpack;
pub mod in_mem_accounts_index;
pub mod inline_spl_associated_token_account;
pub mod inline_spl_token;
pub mod inline_spl_token_2022;
pub mod loader_utils;
pub mod message_processor;
pub mod non_circulating_supply;
mod nonce_keyed_account;
pub mod prioritization_fee;
pub mod prioritization_fee_cache;
mod pubkey_bins;
mod read_only_accounts_cache;
pub mod rent_collector;
mod rent_paying_accounts_by_partition;
mod rolling_bit_field;
pub mod root_bank_cache;
pub mod runtime_config;
pub mod secondary_index;
pub mod serde_snapshot;
mod shared_buffer_reader;
pub mod snapshot_archive_info;
pub mod snapshot_config;
pub mod snapshot_hash;
pub mod snapshot_minimizer;
pub mod snapshot_package;
pub mod snapshot_utils;
pub mod sorted_storages;
mod stake_account;
pub mod stake_history;
pub mod stake_weighted_timestamp;
pub mod stakes;
pub mod static_ids;
pub mod status_cache;
mod storable_accounts;
mod system_instruction_processor;
pub mod transaction_batch;
pub mod transaction_error_metrics;
pub mod transaction_priority_details;
mod verify_accounts_hash_in_background;
pub mod vote_account;
pub mod vote_parser;
pub mod vote_sender_types;
pub mod vote_transaction;
pub mod waitable_condvar;

#[macro_use]
extern crate solana_metrics;
#[macro_use]
extern crate serde_derive;

#[macro_use]
extern crate solana_frozen_abi_macro;

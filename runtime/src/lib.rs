#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(specialization))]
#![allow(clippy::integer_arithmetic)]
pub mod accounts;
pub mod accounts_background_service;
pub mod accounts_cache;
pub mod accounts_db;
pub mod accounts_hash;
pub mod accounts_index;
pub mod append_vec;
pub mod bank;
pub mod bank_client;
pub mod bank_forks;
pub mod bank_utils;
mod blockhash_queue;
pub mod bloom;
pub mod builtins;
pub mod commitment;
pub mod contains;
pub mod epoch_stakes;
pub mod genesis_utils;
pub mod hardened_unpack;
pub mod inline_spl_token_v2_0;
pub mod instruction_recorder;
pub mod loader_utils;
pub mod log_collector;
pub mod message_processor;
mod native_loader;
pub mod rent_collector;
pub mod secondary_index;
pub mod serde_snapshot;
pub mod snapshot_package;
pub mod snapshot_utils;
pub mod stakes;
pub mod status_cache;
mod system_instruction_processor;
pub mod transaction_batch;
pub mod vote_account;
pub mod vote_sender_types;

#[macro_use]
extern crate solana_metrics;
#[macro_use]
extern crate serde_derive;

#[macro_use]
extern crate solana_frozen_abi_macro;

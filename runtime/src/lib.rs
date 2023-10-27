#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(min_specialization))]
#![allow(clippy::arithmetic_side_effects)]

#[macro_use]
extern crate lazy_static;

pub mod accounts_background_service;
pub mod bank;
pub mod bank_client;
pub mod bank_forks;
pub mod bank_utils;
pub mod builtins;
pub mod commitment;
mod epoch_rewards_hasher;
pub mod epoch_stakes;
pub mod genesis_utils;
pub mod inline_spl_associated_token_account;
pub mod loader_utils;
pub mod non_circulating_supply;
pub mod prioritization_fee;
pub mod prioritization_fee_cache;
pub mod root_bank_cache;
pub mod runtime_config;
pub mod serde_snapshot;
pub mod snapshot_archive_info;
pub mod snapshot_bank_utils;
pub mod snapshot_config;
pub mod snapshot_hash;
pub mod snapshot_minimizer;
pub mod snapshot_package;
pub mod snapshot_utils;
mod stake_account;
pub mod stake_history;
pub mod stake_weighted_timestamp;
pub mod stakes;
pub mod static_ids;
pub mod status_cache;
pub mod transaction_batch;
pub mod transaction_priority_details;

#[macro_use]
extern crate solana_metrics;
#[macro_use]
extern crate serde_derive;

#[macro_use]
extern crate solana_frozen_abi_macro;

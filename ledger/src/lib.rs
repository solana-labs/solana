#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(min_specialization))]
#![allow(clippy::arithmetic_side_effects)]

pub mod bank_forks_utils;
pub mod bigtable_delete;
pub mod bigtable_upload;
pub mod bigtable_upload_service;
pub mod block_error;
#[macro_use]
pub mod blockstore;
pub mod ancestor_iterator;
pub mod blockstore_db;
pub mod blockstore_meta;
pub mod blockstore_metrics;
pub mod blockstore_options;
pub mod blockstore_processor;
pub mod entry_notifier_interface;
pub mod entry_notifier_service;
pub mod genesis_utils;
pub mod leader_schedule;
pub mod leader_schedule_cache;
pub mod leader_schedule_utils;
pub mod next_slots_iterator;
pub mod rooted_slot_iterator;
pub mod shred;
mod shredder;
pub mod sigverify_shreds;
pub mod slot_stats;
mod staking_utils;
pub mod token_balances;
pub mod use_snapshot_archives_at_startup;

#[macro_use]
extern crate solana_metrics;

#[macro_use]
extern crate log;

#[macro_use]
extern crate lazy_static;

#[macro_use]
extern crate solana_frozen_abi_macro;

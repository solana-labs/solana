#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(min_specialization))]
#![allow(clippy::arithmetic_side_effects)]
#![recursion_limit = "2048"]
//! The `solana` library implements the Solana high-performance blockchain architecture.
//! It includes a full Rust implementation of the architecture (see
//! [Validator](validator/struct.Validator.html)) as well as hooks to GPU implementations of its most
//! paralellizable components (i.e. [SigVerify](sigverify/index.html)).  It also includes
//! command-line tools to spin up validators and a Rust library
//!

pub mod accounts_hash_verifier;
pub mod admin_rpc_post_init;
pub mod banking_stage;
pub mod banking_trace;
pub mod cache_block_meta_service;
pub mod cluster_info_vote_listener;
pub mod cluster_slots_service;
pub mod commitment_service;
pub mod completed_data_sets_service;
pub mod consensus;
pub mod cost_update_service;
pub mod drop_bank_service;
pub mod fetch_stage;
pub mod gen_keys;
pub mod ledger_cleanup_service;
pub mod ledger_metric_report_service;
pub mod next_leader;
pub mod optimistic_confirmation_verifier;
pub mod poh_timing_report_service;
pub mod poh_timing_reporter;
pub mod repair;
pub mod replay_stage;
mod result;
pub mod rewards_recorder_service;
pub mod sample_performance_service;
mod shred_fetch_stage;
pub mod sigverify;
pub mod sigverify_stage;
pub mod snapshot_packager_service;
pub mod staked_nodes_updater_service;
pub mod stats_reporter_service;
pub mod system_monitor_service;
pub mod tpu;
mod tpu_entry_notifier;
pub mod tracer_packet_stats;
pub mod tvu;
pub mod unfrozen_gossip_verified_vote_hashes;
pub mod validator;
pub mod verified_vote_packets;
pub mod vote_simulator;
pub mod voting_service;
pub mod warm_quic_cache_service;
pub mod window_service;

#[macro_use]
extern crate eager;

#[macro_use]
extern crate log;

#[macro_use]
extern crate serde_derive;

#[macro_use]
extern crate solana_metrics;

#[macro_use]
extern crate solana_frozen_abi_macro;

#[cfg(test)]
#[macro_use]
extern crate assert_matches;

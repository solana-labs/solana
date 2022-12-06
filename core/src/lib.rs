#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(min_specialization))]
#![allow(clippy::integer_arithmetic)]
#![recursion_limit = "2048"]
//! The `solana` library implements the Solana high-performance blockchain architecture.
//! It includes a full Rust implementation of the architecture (see
//! [Validator](server/struct.Validator.html)) as well as hooks to GPU implementations of its most
//! paralellizable components (i.e. [SigVerify](sigverify/index.html)).  It also includes
//! command-line tools to spin up validators and a Rust library
//!

pub mod accounts_hash_verifier;
pub mod ancestor_hashes_service;
pub mod banking_stage;
pub mod broadcast_stage;
pub mod cache_block_meta_service;
pub mod cluster_info_vote_listener;
pub mod cluster_nodes;
pub mod cluster_slot_state_verifier;
pub mod cluster_slots;
pub mod cluster_slots_service;
pub mod commitment_service;
pub mod completed_data_sets_service;
pub mod consensus;
pub mod cost_update_service;
pub mod drop_bank_service;
pub mod duplicate_repair_status;
pub mod fetch_stage;
pub mod find_packet_sender_stake_stage;
pub mod fork_choice;
pub mod forward_packet_batches_by_accounts;
pub mod gen_keys;
pub mod heaviest_subtree_fork_choice;
pub mod immutable_deserialized_packet;
mod latest_unprocessed_votes;
pub mod latest_validator_votes_for_frozen_banks;
pub mod leader_slot_banking_stage_metrics;
pub mod leader_slot_banking_stage_timing_metrics;
pub mod ledger_cleanup_service;
pub mod ledger_metric_report_service;
pub mod multi_iterator_scanner;
pub mod optimistic_confirmation_verifier;
pub mod outstanding_requests;
pub mod packet_deserializer;
mod packet_hasher;
pub mod packet_threshold;
pub mod poh_timing_report_service;
pub mod poh_timing_reporter;
pub mod progress_map;
pub mod qos_service;
pub mod read_write_account_set;
pub mod repair_generic_traversal;
pub mod repair_response;
pub mod repair_service;
pub mod repair_weight;
pub mod repair_weighted_traversal;
pub mod replay_stage;
pub mod request_response;
mod result;
pub mod retransmit_stage;
pub mod rewards_recorder_service;
pub mod sample_performance_service;
pub mod serve_repair;
pub mod serve_repair_service;
mod shred_fetch_stage;
pub mod sigverify;
pub mod sigverify_shreds;
pub mod sigverify_stage;
pub mod snapshot_packager_service;
pub mod staked_nodes_updater_service;
pub mod stats_reporter_service;
pub mod system_monitor_service;
mod tower1_7_14;
pub mod tower_storage;
pub mod tpu;
pub mod tracer_packet_stats;
pub mod tree_diff;
pub mod tvu;
pub mod unfrozen_gossip_verified_vote_hashes;
pub mod unprocessed_packet_batches;
pub mod unprocessed_transaction_storage;
pub mod validator;
pub mod verified_vote_packets;
pub mod vote_simulator;
pub mod vote_stake_tracker;
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
extern crate matches;

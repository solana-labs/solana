//! The `solana` library implements the Solana high-performance blockchain architecture.
//! It includes a full Rust implementation of the architecture (see
//! [Validator](server/struct.Validator.html)) as well as hooks to GPU implementations of its most
//! paralellizable components (i.e. [SigVerify](sigverify/index.html)).  It also includes
//! command-line tools to spin up validators and a Rust library
//!

pub mod accounts_background_service;
pub mod accounts_hash_verifier;
pub mod banking_stage;
pub mod broadcast_stage;
pub mod cluster_info_vote_listener;
pub mod commitment;
mod deprecated;
pub mod shred_fetch_stage;
#[macro_use]
pub mod contact_info;
pub mod cluster_info;
pub mod cluster_slots;
pub mod consensus;
pub mod crds;
pub mod crds_gossip;
pub mod crds_gossip_error;
pub mod crds_gossip_pull;
pub mod crds_gossip_push;
pub mod crds_value;
pub mod epoch_slots;
pub mod fetch_stage;
pub mod gen_keys;
pub mod gossip_service;
pub mod ledger_cleanup_service;
pub mod local_vote_signer_service;
pub mod non_circulating_supply;
pub mod poh_recorder;
pub mod poh_service;
pub mod progress_map;
pub mod pubkey_references;
pub mod repair_response;
pub mod repair_service;
pub mod replay_stage;
mod result;
pub mod retransmit_stage;
pub mod rewards_recorder_service;
pub mod rpc;
pub mod rpc_error;
pub mod rpc_pubsub;
pub mod rpc_pubsub_service;
pub mod rpc_service;
pub mod rpc_subscriptions;
pub mod serve_repair;
pub mod serve_repair_service;
pub mod sigverify;
pub mod sigverify_shreds;
pub mod sigverify_stage;
pub mod snapshot_packager_service;
pub mod tpu;
pub mod transaction_status_service;
pub mod tvu;
pub mod validator;
pub mod verified_vote_packets;
pub mod weighted_shuffle;
pub mod window_service;

#[macro_use]
extern crate solana_bpf_loader_program;

#[macro_use]
extern crate solana_budget_program;

#[macro_use]
extern crate log;

#[macro_use]
extern crate serde_derive;

#[cfg(test)]
#[macro_use]
extern crate serde_json;

#[macro_use]
extern crate solana_metrics;

#[cfg(test)]
#[macro_use]
extern crate matches;

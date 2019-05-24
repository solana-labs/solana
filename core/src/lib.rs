//! The `solana` library implements the Solana high-performance blockchain architecture.
//! It includes a full Rust implementation of the architecture (see
//! [Validator](server/struct.Validator.html)) as well as hooks to GPU implementations of its most
//! paralellizable components (i.e. [SigVerify](sigverify/index.html)).  It also includes
//! command-line tools to spin up fullnodes and a Rust library
//!

pub mod bank_forks;
pub mod banking_stage;
pub mod blob_fetch_stage;
pub mod broadcast_stage;
#[cfg(feature = "chacha")]
pub mod chacha;
#[cfg(all(feature = "chacha", feature = "cuda"))]
pub mod chacha_cuda;
pub mod cluster_info_vote_listener;
#[macro_use]
pub mod contact_info;
pub mod crds;
pub mod crds_gossip;
pub mod crds_gossip_error;
pub mod crds_gossip_pull;
pub mod crds_gossip_push;
pub mod crds_value;
#[macro_use]
pub mod blocktree;
pub mod blockstream;
pub mod blockstream_service;
pub mod blocktree_processor;
pub mod cluster;
pub mod cluster_info;
pub mod cluster_info_repair_listener;
pub mod cluster_tests;
pub mod entry;
pub mod erasure;
pub mod fetch_stage;
pub mod gen_keys;
pub mod genesis_utils;
pub mod gossip_service;
pub mod leader_schedule;
pub mod leader_schedule_cache;
pub mod leader_schedule_utils;
pub mod local_cluster;
pub mod local_vote_signer_service;
pub mod locktower;
pub mod packet;
pub mod poh;
pub mod poh_recorder;
pub mod poh_service;
pub mod recvmmsg;
pub mod repair_service;
pub mod replay_stage;
pub mod replicator;
pub mod result;
pub mod retransmit_stage;
pub mod rpc;
pub mod rpc_pubsub;
pub mod rpc_pubsub_service;
pub mod rpc_service;
pub mod rpc_subscriptions;
pub mod service;
pub mod sigverify;
pub mod sigverify_stage;
pub mod staking_utils;
pub mod storage_stage;
pub mod streamer;
pub mod test_tx;
pub mod tpu;
pub mod tvu;
pub mod validator;
pub mod window_service;

#[macro_use]
extern crate solana_budget_program;

#[cfg(test)]
#[cfg(any(feature = "chacha", feature = "cuda"))]
#[macro_use]
extern crate hex_literal;

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

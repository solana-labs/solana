//! The `solana` library implements the Solana high-performance blockchain architecture.
//! It includes a full Rust implementation of the architecture (see
//! [Fullnode](server/struct.Fullnode.html)) as well as hooks to GPU implementations of its most
//! paralellizable components (i.e. [SigVerify](sigverify/index.html)).  It also includes
//! command-line tools to spin up fullnodes and a Rust library
//! (see [ThinClient](thin_client/struct.ThinClient.html)) to interact with them.
//!

#![cfg_attr(feature = "unstable", feature(test))]
#[macro_use]
pub mod counter;
pub mod accounts;
pub mod bank;
pub mod bank_checkpoint;
pub mod bank_state;
pub mod banking_stage;
pub mod blob_fetch_stage;
pub mod bloom;
pub mod broadcast_service;
#[cfg(feature = "chacha")]
pub mod chacha;
#[cfg(all(feature = "chacha", feature = "cuda"))]
pub mod chacha_cuda;
pub mod checkpoints;
pub mod client;
pub mod cluster_info_vote_listener;
pub mod crds;
pub mod crds_gossip;
pub mod crds_gossip_error;
pub mod crds_gossip_pull;
pub mod crds_gossip_push;
pub mod crds_value;
pub mod forks;
#[macro_use]
pub mod contact_info;
pub mod cluster_info;
pub mod compute_leader_confirmation_service;
pub mod db_ledger;
pub mod db_window;
pub mod entry;
pub mod entry_stream;
#[cfg(feature = "erasure")]
pub mod erasure;
pub mod fetch_stage;
pub mod fullnode;
pub mod gen_keys;
pub mod genesis_block;
pub mod gossip_service;
pub mod last_id_queue;
pub mod leader_scheduler;
pub mod local_vote_signer_service;
pub mod packet;
pub mod poh;
pub mod poh_recorder;
pub mod poh_service;
pub mod recvmmsg;
pub mod replay_stage;
pub mod replicator;
pub mod result;
pub mod retransmit_stage;
pub mod rpc;
pub mod rpc_mock;
pub mod rpc_pubsub;
pub mod rpc_request;
pub mod runtime;
pub mod service;
pub mod sigverify;
pub mod sigverify_stage;
pub mod status_cache;
pub mod storage_stage;
pub mod streamer;
pub mod test_tx;
pub mod thin_client;
pub mod tpu;
pub mod tpu_forwarder;
pub mod tvu;
pub mod voting_keypair;
#[cfg(test)]
pub mod window;
pub mod window_service;

#[cfg(test)]
#[cfg(any(feature = "chacha", feature = "cuda"))]
#[macro_use]
extern crate hex_literal;

#[macro_use]
extern crate log;

#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate serde_json;

#[cfg(test)]
#[macro_use]
extern crate matches;

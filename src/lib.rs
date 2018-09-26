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
pub mod bank;
pub mod banking_stage;
pub mod blob_fetch_stage;
pub mod bpf_loader;
pub mod broadcast_stage;
pub mod budget;
pub mod budget_instruction;
pub mod budget_transaction;
#[cfg(feature = "chacha")]
pub mod chacha;
#[cfg(all(feature = "chacha", feature = "cuda"))]
pub mod chacha_cuda;
pub mod choose_gossip_peer_strategy;
pub mod client;
#[macro_use]
pub mod cluster_info;
pub mod budget_program;
pub mod drone;
pub mod entry;
pub mod entry_writer;
#[cfg(feature = "erasure")]
pub mod erasure;
pub mod fetch_stage;
pub mod fullnode;
pub mod hash;
pub mod leader_scheduler;
pub mod leader_vote_stage;
pub mod ledger;
pub mod ledger_write_stage;
pub mod loader_transaction;
pub mod logger;
pub mod metrics;
pub mod mint;
pub mod native_loader;
pub mod ncp;
pub mod netutil;
pub mod packet;
pub mod payment_plan;
pub mod poh;
pub mod poh_recorder;
pub mod poh_service;
pub mod recvmmsg;
pub mod replicate_stage;
pub mod replicator;
pub mod request;
pub mod request_processor;
pub mod request_stage;
pub mod result;
pub mod retransmit_stage;
pub mod rpc;
pub mod rpc_pubsub;
pub mod rpc_request;
pub mod rpu;
pub mod service;
pub mod signature;
pub mod sigverify;
pub mod sigverify_stage;
pub mod storage_program;
pub mod storage_transaction;
pub mod store_ledger_stage;
pub mod streamer;
pub mod system_program;
pub mod system_transaction;
pub mod thin_client;
pub mod tictactoe_dashboard_program;
pub mod tictactoe_program;
pub mod timing;
pub mod token_program;
pub mod tpu;
pub mod transaction;
pub mod tvu;
pub mod vote_program;
pub mod vote_stage;
pub mod vote_transaction;
pub mod wallet;
pub mod window;
pub mod window_service;
extern crate bincode;
extern crate bs58;
extern crate byteorder;
extern crate bytes;
extern crate chrono;
extern crate clap;
extern crate dirs;
extern crate elf;
extern crate generic_array;
#[cfg(test)]
#[cfg(any(feature = "chacha", feature = "cuda"))]
#[macro_use]
extern crate hex_literal;
extern crate ipnetwork;
extern crate itertools;
extern crate libc;
extern crate libloading;
#[macro_use]
extern crate log;
extern crate nix;
extern crate pnet_datalink;
extern crate rayon;
extern crate reqwest;
extern crate ring;
extern crate serde;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate serde_json;
extern crate serde_cbor;
extern crate sha2;
extern crate socket2;
extern crate solana_jsonrpc_core as jsonrpc_core;
extern crate solana_jsonrpc_http_server as jsonrpc_http_server;
#[macro_use]
extern crate solana_jsonrpc_macros as jsonrpc_macros;
extern crate solana_jsonrpc_pubsub as jsonrpc_pubsub;
extern crate solana_jsonrpc_ws_server as jsonrpc_ws_server;
extern crate solana_sdk;
extern crate sys_info;
extern crate tokio;
extern crate tokio_codec;
extern crate untrusted;

#[cfg(test)]
#[macro_use]
extern crate matches;

extern crate influx_db_client;
extern crate rand;

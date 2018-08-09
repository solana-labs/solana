//! The `solana` library implements the Solana high-performance blockchain architecture.
//! It includes a full Rust implementation of the architecture (see
//! [Server](server/struct.Server.html)) as well as hooks to GPU implementations of its most
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
pub mod broadcast_stage;
pub mod budget;
pub mod choose_gossip_peer_strategy;
pub mod client;
pub mod crdt;
pub mod drone;
pub mod entry;
pub mod entry_writer;
#[cfg(feature = "erasure")]
pub mod erasure;
pub mod fetch_stage;
pub mod fullnode;
pub mod hash;
pub mod ledger;
pub mod logger;
pub mod metrics;
pub mod mint;
pub mod nat;
pub mod ncp;
pub mod packet;
pub mod payment_plan;
pub mod record_stage;
pub mod recorder;
pub mod replicate_stage;
pub mod request;
pub mod request_processor;
pub mod request_stage;
pub mod result;
pub mod retransmit_stage;
pub mod rpu;
pub mod service;
pub mod signature;
pub mod sigverify;
pub mod sigverify_stage;
pub mod streamer;
pub mod thin_client;
pub mod timing;
pub mod tpu;
pub mod transaction;
pub mod tvu;
pub mod vote_stage;
pub mod voting;
pub mod wallet;
pub mod window;
pub mod write_stage;
extern crate bincode;
extern crate bs58;
extern crate byteorder;
extern crate chrono;
extern crate generic_array;
extern crate itertools;
extern crate libc;
#[macro_use]
extern crate log;
extern crate rayon;
extern crate ring;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate pnet_datalink;
extern crate serde_json;
extern crate sha2;
extern crate sys_info;
extern crate untrusted;

#[cfg(test)]
#[macro_use]
extern crate matches;

extern crate influx_db_client;
extern crate rand;

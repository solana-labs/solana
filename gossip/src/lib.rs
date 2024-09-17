#![cfg_attr(feature = "frozen-abi", feature(min_specialization))]
#![allow(clippy::arithmetic_side_effects)]

pub mod cluster_info;
pub mod cluster_info_metrics;
pub mod contact_info;
pub mod crds;
pub mod crds_entry;
pub mod crds_gossip;
pub mod crds_gossip_error;
pub mod crds_gossip_pull;
pub mod crds_gossip_push;
pub mod crds_shards;
pub mod crds_value;
mod deprecated;
pub mod duplicate_shred;
pub mod duplicate_shred_handler;
pub mod duplicate_shred_listener;
pub mod epoch_slots;
pub mod gossip_error;
pub mod gossip_service;
#[macro_use]
mod legacy_contact_info;
pub mod ping_pong;
mod push_active_set;
mod received_cache;
pub mod restart_crds_values;
pub mod weighted_shuffle;

#[macro_use]
extern crate log;

#[cfg(test)]
#[macro_use]
extern crate assert_matches;

#[macro_use]
extern crate serde_derive;

#[cfg_attr(feature = "frozen-abi", macro_use)]
#[cfg(feature = "frozen-abi")]
extern crate solana_frozen_abi_macro;

#[macro_use]
extern crate solana_metrics;

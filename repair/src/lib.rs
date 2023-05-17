#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(min_specialization))]
#![allow(clippy::integer_arithmetic)]
#![recursion_limit = "2048"]

pub mod ancestor_hashes_service;
pub mod cluster_slot_state_verifier;
pub mod duplicate_repair_status;
pub mod outstanding_requests;
pub mod packet_threshold;
pub mod repair_generic_traversal;
pub mod repair_response;
pub mod repair_service;
pub mod repair_weight;
pub mod repair_weighted_traversal;
pub mod request_response;
pub mod serve_repair;
pub mod serve_repair_service;

#[macro_use]
extern crate log;

#[macro_use]
extern crate serde_derive;

#[macro_use]
extern crate solana_metrics;

//#[macro_use]
//extern crate solana_frozen_abi_macro;

#[cfg(test)]
#[macro_use]
extern crate matches;

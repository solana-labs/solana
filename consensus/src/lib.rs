#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(min_specialization))]
#![allow(clippy::arithmetic_side_effects)]
#![recursion_limit = "2048"]
//! The `solana` library implements the Solana high-performance blockchain architecture.
//! It includes a full Rust implementation of the architecture (see
//! [Validator](validator/struct.Validator.html)) as well as hooks to GPU implementations of its most
//! paralellizable components (i.e. [SigVerify](sigverify/index.html)).  It also includes
//! command-line tools to spin up validators and a Rust library
//!

pub mod cluster_slots;
pub mod cluster_slots_service;
pub mod consensus;
pub mod fork_choice;
pub mod heaviest_subtree_fork_choice;
pub mod latest_validator_votes_for_frozen_banks;
pub mod progress_map;
mod tower1_14_11;
mod tower1_7_14;
pub mod tower_storage;
pub mod tree_diff;
pub mod vote_stake_tracker;

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

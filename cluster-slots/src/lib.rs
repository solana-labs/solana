#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(min_specialization))]
#![allow(clippy::integer_arithmetic)]
#![recursion_limit = "2048"]

pub mod cluster_slots;
pub mod cluster_slots_service;

#[macro_use]
extern crate log;

#[macro_use]
extern crate solana_metrics;

#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(min_specialization))]
#![deny(clippy::integer_arithmetic)]
#![deny(clippy::indexing_slicing)]
#![recursion_limit = "2048"]

#[macro_use]
extern crate eager;

#[macro_use]
extern crate solana_metrics;

pub mod accounts_data_meter;
pub mod compute_budget;
pub mod executor;
pub mod executor_cache;
pub mod invoke_context;
pub mod log_collector;
pub mod pre_account;
pub mod prioritization_fee;
pub mod stable_log;
pub mod sysvar_cache;
pub mod timings;

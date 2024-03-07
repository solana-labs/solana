#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(min_specialization))]
#![deny(clippy::arithmetic_side_effects)]
#![deny(clippy::indexing_slicing)]
#![recursion_limit = "2048"]

#[macro_use]
extern crate eager;

#[macro_use]
extern crate solana_metrics;

pub use solana_rbpf;
pub mod compute_budget;
pub mod compute_budget_processor;
pub mod invoke_context;
pub mod loaded_programs;
pub mod log_collector;
pub mod message_processor;
pub mod prioritization_fee;
pub mod runtime_config;
pub mod stable_log;
pub mod sysvar_cache;
pub mod timings;

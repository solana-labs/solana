#![cfg_attr(feature = "frozen-abi", feature(min_specialization))]
#![deny(clippy::arithmetic_side_effects)]
#![deny(clippy::indexing_slicing)]

#[macro_use]
extern crate solana_metrics;

pub use solana_rbpf;
pub mod invoke_context;
pub mod loaded_programs;
pub mod mem_pool;
pub mod stable_log;
pub mod sysvar_cache;

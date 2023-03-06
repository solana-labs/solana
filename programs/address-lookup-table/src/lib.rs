#![allow(incomplete_features)]
#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(specialization))]
#![cfg_attr(RUSTC_NEEDS_PROC_MACRO_HYGIENE, feature(proc_macro_hygiene))]

pub mod error;
pub mod instruction;
#[cfg(not(target_os = "solana"))]
pub mod processor;
pub mod state;

pub use solana_sdk::address_lookup_table_program::{check_id, id};

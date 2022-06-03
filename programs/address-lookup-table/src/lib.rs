#![allow(incomplete_features)]
#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(specialization))]
#![cfg_attr(RUSTC_NEEDS_PROC_MACRO_HYGIENE, feature(proc_macro_hygiene))]

use solana_program::declare_id;

pub mod error;
pub mod instruction;
#[cfg(not(target_os = "solana"))]
pub mod processor;
pub mod state;

declare_id!("AddressLookupTab1e1111111111111111111111111");

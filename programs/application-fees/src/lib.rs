#![allow(incomplete_features)]
#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(specialization))]
#![cfg_attr(RUSTC_NEEDS_PROC_MACRO_HYGIENE, feature(proc_macro_hygiene))]

use solana_program::declare_id;

#[cfg(not(target_os = "solana"))]
pub mod processor;

pub mod instruction;

declare_id!("App1icationFees1111111111111111111111111111");

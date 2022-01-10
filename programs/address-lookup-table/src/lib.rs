#![allow(incomplete_features)]
#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(specialization))]
#![cfg_attr(RUSTC_NEEDS_PROC_MACRO_HYGIENE, feature(proc_macro_hygiene))]

use solana_sdk::declare_id;

pub mod instruction;
pub mod processor;
pub mod state;

declare_id!("AddressLookupTab1e1111111111111111111111111");

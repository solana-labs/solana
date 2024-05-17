#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(min_specialization))]
pub mod bloom;

#[cfg_attr(feature = "frozen-abi", macro_use)]
#[cfg(feature = "frozen-abi")]
extern crate solana_frozen_abi_macro;

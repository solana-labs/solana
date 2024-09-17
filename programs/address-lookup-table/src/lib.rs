#![allow(incomplete_features)]
#![cfg_attr(feature = "frozen-abi", feature(specialization))]

#[cfg(not(target_os = "solana"))]
pub mod processor;

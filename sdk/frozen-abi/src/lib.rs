#![allow(incomplete_features)]
#![cfg_attr(feature = "frozen-abi", feature(specialization))]

// Allows macro expansion of `use ::solana_frozen_abi::*` to work within this crate
extern crate self as solana_frozen_abi;

#[cfg(feature = "frozen-abi")]
pub mod abi_digester;
#[cfg(feature = "frozen-abi")]
pub mod abi_example;
#[cfg(feature = "frozen-abi")]
mod hash;

#[cfg(feature = "frozen-abi")]
#[macro_use]
extern crate solana_frozen_abi_macro;

// Not public API. Referenced by macro-generated code.
#[doc(hidden)]
pub mod __private {
    #[doc(hidden)]
    pub use log;
}

#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(min_specialization))]
#![allow(clippy::integer_arithmetic)]

mod instruction_processor;
mod native_loader;

pub use instruction_processor::*;
pub use native_loader::*;

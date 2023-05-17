#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(min_specialization))]
#![allow(clippy::integer_arithmetic)]
#![recursion_limit = "2048"]

pub mod result;

#[cfg(test)]
#[macro_use]
extern crate matches;

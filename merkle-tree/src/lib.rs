#![allow(clippy::integer_arithmetic)]

#[cfg(target_os = "solana")]
#[macro_use]
extern crate matches;

pub mod merkle_tree;
pub use merkle_tree::MerkleTree;

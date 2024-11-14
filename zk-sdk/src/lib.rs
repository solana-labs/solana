//! The `solana-zk-sdk` crate contains tools to create and verify zero-knowledge proofs on
//! encrypted data.

// The warning `clippy::op_ref` is disabled to allow efficient operator arithmetic of structs that
// implement the `Copy` trait.
//
// ```
// let opening_0: PedersenOpening = PedersenOpening::new_rand();
// let opening_1: PedersenOpening = PedersenOpening::new_rand();
//
// // since PedersenOpening implement `Copy`, `opening_0` and `opening_1` will be copied as
// // parameters before `opening_sum` is computed.
// let opening_sum = opening_0 + opening_1;
//
// // if passed in as references, the extra copies will not occur
// let opening_sum = &opening_0 + &opening_1;
// ```
//
// `clippy::op_ref` is turned off to prevent clippy from warning that this is not idiomatic code.
#![allow(clippy::arithmetic_side_effects, clippy::op_ref)]

pub mod encryption;
pub mod errors;
pub mod pod;
mod range_proof;
mod sigma_proofs;
mod transcript;
pub mod zk_elgamal_proof_program;

/// Byte length of a compressed Ristretto point or scalar in Curve255519
const UNIT_LEN: usize = 32;
/// Byte length of a compressed Ristretto point in Curve25519
const RISTRETTO_POINT_LEN: usize = UNIT_LEN;
/// Byte length of a scalar in Curve25519
const SCALAR_LEN: usize = UNIT_LEN;

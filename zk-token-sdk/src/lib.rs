#![allow(clippy::integer_arithmetic, clippy::op_ref)]

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

#[cfg(not(target_os = "solana"))]
#[macro_use]
pub(crate) mod macros;
#[cfg(not(target_os = "solana"))]
pub mod encryption;
#[cfg(not(target_os = "solana"))]
pub mod errors;
#[cfg(not(target_os = "solana"))]
mod range_proof;
#[cfg(not(target_os = "solana"))]
mod sigma_proofs;
#[cfg(not(target_os = "solana"))]
mod transcript;

// TODO: re-organize visibility
pub mod curve25519;
pub mod instruction;
pub mod zk_token_elgamal;
pub mod zk_token_proof_instruction;
pub mod zk_token_proof_program;

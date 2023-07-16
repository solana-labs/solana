//! Collection of encryption-related data structures and algorithms used in the Solana zk-token
//! protocol.
//!
//! The module contains implementations of the following cryptographic objects:
//! - Pedersen commitments that uses the prime-order Ristretto representation of Curve25519.
//! [curve25519-dalek](https://docs.rs/curve25519-dalek/latest/curve25519_dalek/ristretto/index.html)
//! is used for the Ristretto group implementation.
//! - The twisted ElGamal scheme, which converts Pedersen commitments into a public-key encryption
//! scheme.
//! - Basic type-wrapper around the AES-GCM-SIV symmetric authenticated encryption scheme
//! implemented by [aes-gcm-siv](https://docs.rs/aes-gcm-siv/latest/aes_gcm_siv/) crate.

pub mod auth_encryption;
pub mod discrete_log;
pub mod elgamal;
pub mod grouped_elgamal;
pub mod pedersen;

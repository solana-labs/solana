//! Collection of encryption-related data structures and algorithms used in the Solana zk-token
//! protocol.
//!
//! The module contains implementations of the following cryptographic objects:
//! - Pedersen commitments that uses the prime-order Ristretto representation of Curve25519.
//!   [curve25519-dalek](https://docs.rs/curve25519-dalek/latest/curve25519_dalek/ristretto/index.html)
//!   is used for the Ristretto group implementation.
//! - The twisted ElGamal scheme, which converts Pedersen commitments into a public-key encryption
//!   scheme.
//! - Basic type-wrapper around the AES-GCM-SIV symmetric authenticated encryption scheme
//!   implemented by [aes-gcm-siv](https://docs.rs/aes-gcm-siv/latest/aes_gcm_siv/) crate.

use crate::{RISTRETTO_POINT_LEN, SCALAR_LEN};

#[cfg(not(target_os = "solana"))]
#[macro_use]
pub(crate) mod macros;
#[cfg(not(target_os = "solana"))]
pub mod auth_encryption;
#[cfg(not(target_os = "solana"))]
pub mod discrete_log;
#[cfg(not(target_os = "solana"))]
pub mod elgamal;
#[cfg(not(target_os = "solana"))]
pub mod grouped_elgamal;
#[cfg(not(target_os = "solana"))]
pub mod pedersen;
pub mod pod;

/// Byte length of an authenticated encryption secret key
pub const AE_KEY_LEN: usize = 16;

/// Byte length of a complete authenticated encryption ciphertext component that includes the
/// ciphertext and nonce components
pub const AE_CIPHERTEXT_LEN: usize = 36;

/// Byte length of a decrypt handle
pub const DECRYPT_HANDLE_LEN: usize = RISTRETTO_POINT_LEN;

/// Byte length of an ElGamal ciphertext
pub const ELGAMAL_CIPHERTEXT_LEN: usize = PEDERSEN_COMMITMENT_LEN + DECRYPT_HANDLE_LEN;

/// Byte length of an ElGamal public key
pub const ELGAMAL_PUBKEY_LEN: usize = RISTRETTO_POINT_LEN;

/// Byte length of an ElGamal secret key
pub const ELGAMAL_SECRET_KEY_LEN: usize = SCALAR_LEN;

/// Byte length of an ElGamal keypair
pub const ELGAMAL_KEYPAIR_LEN: usize = ELGAMAL_PUBKEY_LEN + ELGAMAL_SECRET_KEY_LEN;

/// Byte length of a Pedersen opening.
pub const PEDERSEN_OPENING_LEN: usize = SCALAR_LEN;

/// Byte length of a Pedersen commitment.
pub const PEDERSEN_COMMITMENT_LEN: usize = RISTRETTO_POINT_LEN;

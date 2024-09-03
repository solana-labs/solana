//! Collection of sigma proofs that are used in the ZK Token proof program.
//!
//! Formal documentation and security proofs for the sigma proofs in this module can be found in
//! [`ZK Token proof`] program documentation.
//!
//! [`ZK Token proof`]: https://docs.solanalabs.com/runtime/zk-token-proof

#![allow(dead_code, unused_imports)]

pub mod errors;
pub mod pod;

#[cfg(not(target_os = "solana"))]
pub mod batched_grouped_ciphertext_validity;
#[cfg(not(target_os = "solana"))]
pub mod ciphertext_ciphertext_equality;
#[cfg(not(target_os = "solana"))]
pub mod ciphertext_commitment_equality;
#[cfg(not(target_os = "solana"))]
pub mod grouped_ciphertext_validity;
#[cfg(not(target_os = "solana"))]
pub mod percentage_with_cap;
#[cfg(not(target_os = "solana"))]
pub mod pubkey_validity;
#[cfg(not(target_os = "solana"))]
pub mod zero_ciphertext;

/// Byte length of a ciphertext-commitment equality proof
pub const CIPHERTEXT_COMMITMENT_EQUALITY_PROOF_LEN: usize = 192;

/// Byte length of a ciphertext-ciphertext equality proof
pub const CIPHERTEXT_CIPHERTEXT_EQUALITY_PROOF_LEN: usize = 224;

/// Byte length of a grouped ciphertext for 2 handles validity proof
pub const GROUPED_CIPHERTEXT_2_HANDLES_VALIDITY_PROOF_LEN: usize = 160;

/// Byte length of a grouped ciphertext for 3 handles validity proof
pub const GROUPED_CIPHERTEXT_3_HANDLES_VALIDITY_PROOF_LEN: usize = 192;

/// Byte length of a batched grouped ciphertext for 2 handles validity proof
pub const BATCHED_GROUPED_CIPHERTEXT_2_HANDLES_VALIDITY_PROOF_LEN: usize = 160;

/// Byte length of a batched grouped ciphertext for 3 handles validity proof
pub const BATCHED_GROUPED_CIPHERTEXT_3_HANDLES_VALIDITY_PROOF_LEN: usize = 192;

/// Byte length of a zero-ciphertext proof
pub const ZERO_CIPHERTEXT_PROOF_LEN: usize = 96;

/// Byte length of a percentage with cap proof
pub const PERCENTAGE_WITH_CAP_PROOF_LEN: usize = 256;

/// Byte length of a public key validity proof
pub const PUBKEY_VALIDITY_PROOF_LEN: usize = 64;

#[cfg(not(target_os = "solana"))]
use {
    crate::{sigma_proofs::errors::SigmaProofVerificationError, RISTRETTO_POINT_LEN, SCALAR_LEN},
    curve25519_dalek::{ristretto::CompressedRistretto, scalar::Scalar},
};

/// Deserializes an optional slice of bytes to a compressed Ristretto point.
///
/// This is a helper function for deserializing byte encodings of sigma proofs. It is designed to
/// be used with `std::slice::Chunks`.
#[cfg(not(target_os = "solana"))]
fn ristretto_point_from_optional_slice(
    optional_slice: Option<&[u8]>,
) -> Result<CompressedRistretto, SigmaProofVerificationError> {
    let Some(slice) = optional_slice else {
        return Err(SigmaProofVerificationError::Deserialization);
    };

    if slice.len() != RISTRETTO_POINT_LEN {
        return Err(SigmaProofVerificationError::Deserialization);
    }

    CompressedRistretto::from_slice(slice).map_err(|_| SigmaProofVerificationError::Deserialization)
}

/// Deserializes an optional slice of bytes to a scalar.
///
/// This is a helper function for deserializing byte encodings of sigma proofs. It is designed to
/// be used with `std::slice::Chunks`.
#[cfg(not(target_os = "solana"))]
fn canonical_scalar_from_optional_slice(
    optional_slice: Option<&[u8]>,
) -> Result<Scalar, SigmaProofVerificationError> {
    optional_slice
        .and_then(|slice| (slice.len() == SCALAR_LEN).then_some(slice)) // if chunk is the wrong length, convert to None
        .and_then(|slice| slice.try_into().ok()) // convert to array
        .and_then(|slice| Scalar::from_canonical_bytes(slice).into_option())
        .ok_or(SigmaProofVerificationError::Deserialization)
}

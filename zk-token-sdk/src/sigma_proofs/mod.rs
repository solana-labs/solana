//! Collection of sigma proofs that are used in the ZK Token proof program.
//!
//! Formal documentation and security proofs for the sigma proofs in this module can be found in
//! [`ZK Token proof`] program documentation.
//!
//! [`ZK Token proof`]: https://edge.docs.solana.com/developing/runtime-facilities/zk-token-proof

pub mod batched_grouped_ciphertext_validity_proof;
pub mod ciphertext_ciphertext_equality_proof;
pub mod ciphertext_commitment_equality_proof;
pub mod errors;
pub mod fee_proof;
pub mod grouped_ciphertext_validity_proof;
pub mod pubkey_proof;
pub mod zero_balance_proof;

#[cfg(not(target_os = "solana"))]
use {
    crate::errors::ProofVerificationError,
    curve25519_dalek::{ristretto::CompressedRistretto, scalar::Scalar},
};

/// Deserializes an optional slice of bytes to a compressed Ristretto point.
///
/// This is a helper function for deserializing byte encodings of sigma proofs. It is designed to
/// be used with `std::slice::Chunks`.
#[cfg(not(target_os = "solana"))]
fn ristretto_point_from_optional_slice(
    optional_slice: Option<&[u8]>,
) -> Result<CompressedRistretto, ProofVerificationError> {
    let slice = optional_slice.ok_or(ProofVerificationError::Deserialization)?;
    let point_bytes = slice[..32]
        .try_into()
        .map_err(|_| ProofVerificationError::Deserialization)?;

    Ok(CompressedRistretto::from_slice(point_bytes))
}

/// Deserializes an optional slice of bytes to a scalar.
///
/// This is a helper function for deserializing byte encodings of sigma proofs. It is designed to
/// be used with `std::slice::Chunks`.
#[cfg(not(target_os = "solana"))]
fn canonical_scalar_from_optional_slice(
    optional_slice: Option<&[u8]>,
) -> Result<Scalar, ProofVerificationError> {
    let slice = optional_slice.ok_or(ProofVerificationError::Deserialization)?;
    let scalar_bytes = slice[..32]
        .try_into()
        .map_err(|_| ProofVerificationError::Deserialization)?;

    let scalar = Scalar::from_canonical_bytes(scalar_bytes)
        .ok_or(ProofVerificationError::Deserialization)?;
    Ok(scalar)
}

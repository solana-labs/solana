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
use {crate::errors::ProofVerificationError, curve25519_dalek::scalar::Scalar};

#[cfg(not(target_os = "solana"))]
fn canonical_scalar_from_slice(bytes: &[u8]) -> Result<Scalar, ProofVerificationError> {
    if bytes.len() != 32 {
        return Err(ProofVerificationError::Deserialization);
    }

    let scalar_bytes = bytes[..32]
        .try_into()
        .map_err(|_| ProofVerificationError::Deserialization)?;

    let scalar = Scalar::from_canonical_bytes(scalar_bytes)
        .ok_or(ProofVerificationError::Deserialization)?;
    Ok(scalar)
}

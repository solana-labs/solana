#[cfg(not(target_os = "solana"))]
use crate::elgamal_program::errors::ProofVerificationError;
use {
    bytemuck::Pod,
    num_derive::{FromPrimitive, ToPrimitive},
};

pub mod batched_range_proof;
pub mod ciphertext_ciphertext_equality;
pub mod ciphertext_commitment_equality;
pub mod errors;
pub mod percentage_with_cap;
pub mod pod;
pub mod pubkey;
pub mod zero_ciphertext;

#[derive(Clone, Copy, Debug, FromPrimitive, ToPrimitive, PartialEq, Eq)]
#[repr(u8)]
pub enum ProofType {
    /// Empty proof type used to distinguish if a proof context account is initialized
    Uninitialized,
    ZeroCiphertext,
    CiphertextCiphertextEquality,
    CiphertextCommitmentEquality,
    PubkeyValidity,
    PercentageWithCap,
    BatchedRangeProofU64,
    BatchedRangeProofU128,
    BatchedRangeProofU256,
}

pub trait ZkProofData<T: Pod> {
    const PROOF_TYPE: ProofType;

    fn context_data(&self) -> &T;

    #[cfg(not(target_os = "solana"))]
    fn verify_proof(&self) -> Result<(), ProofVerificationError>;
}

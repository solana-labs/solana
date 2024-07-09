#[cfg(not(target_os = "solana"))]
use crate::zk_elgamal_proof_program::errors::ProofVerificationError;
use {
    bytemuck::Pod,
    num_derive::{FromPrimitive, ToPrimitive},
};

pub mod batched_grouped_ciphertext_validity;
pub mod batched_range_proof;
pub mod ciphertext_ciphertext_equality;
pub mod ciphertext_commitment_equality;
pub mod errors;
pub mod grouped_ciphertext_validity;
pub mod percentage_with_cap;
pub mod pod;
pub mod pubkey_validity;
pub mod zero_ciphertext;

pub use {
    batched_grouped_ciphertext_validity::{
        BatchedGroupedCiphertext2HandlesValidityProofContext,
        BatchedGroupedCiphertext2HandlesValidityProofData,
        BatchedGroupedCiphertext3HandlesValidityProofContext,
        BatchedGroupedCiphertext3HandlesValidityProofData,
    },
    batched_range_proof::{
        batched_range_proof_u128::BatchedRangeProofU128Data,
        batched_range_proof_u256::BatchedRangeProofU256Data,
        batched_range_proof_u64::BatchedRangeProofU64Data, BatchedRangeProofContext,
    },
    ciphertext_ciphertext_equality::{
        CiphertextCiphertextEqualityProofContext, CiphertextCiphertextEqualityProofData,
    },
    ciphertext_commitment_equality::{
        CiphertextCommitmentEqualityProofContext, CiphertextCommitmentEqualityProofData,
    },
    grouped_ciphertext_validity::{
        GroupedCiphertext2HandlesValidityProofContext, GroupedCiphertext2HandlesValidityProofData,
        GroupedCiphertext3HandlesValidityProofContext, GroupedCiphertext3HandlesValidityProofData,
    },
    percentage_with_cap::{PercentageWithCapProofContext, PercentageWithCapProofData},
    pubkey_validity::{PubkeyValidityProofContext, PubkeyValidityProofData},
    zero_ciphertext::{ZeroCiphertextProofContext, ZeroCiphertextProofData},
};

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
    GroupedCiphertext2HandlesValidity,
    BatchedGroupedCiphertext2HandlesValidity,
    GroupedCiphertext3HandlesValidity,
    BatchedGroupedCiphertext3HandlesValidity,
}

pub trait ZkProofData<T: Pod> {
    const PROOF_TYPE: ProofType;

    fn context_data(&self) -> &T;

    #[cfg(not(target_os = "solana"))]
    fn verify_proof(&self) -> Result<(), ProofVerificationError>;
}

mod auth_encryption;
mod elgamal;
mod grouped_elgamal;
mod instruction;
mod pedersen;
mod range_proof;
mod sigma_proofs;

use {
    crate::zk_token_proof_instruction::ProofType,
    num_traits::{FromPrimitive, ToPrimitive},
    solana_program::instruction::InstructionError,
};
pub use {
    auth_encryption::AeCiphertext,
    bytemuck::{Pod, Zeroable},
    elgamal::{DecryptHandle, ElGamalCiphertext, ElGamalPubkey},
    grouped_elgamal::{GroupedElGamalCiphertext2Handles, GroupedElGamalCiphertext3Handles},
    instruction::{FeeEncryption, FeeParameters, TransferAmountCiphertext},
    pedersen::PedersenCommitment,
    range_proof::{RangeProofU128, RangeProofU256, RangeProofU64},
    sigma_proofs::{
        BatchedGroupedCiphertext2HandlesValidityProof, CiphertextCiphertextEqualityProof,
        CiphertextCommitmentEqualityProof, FeeSigmaProof, GroupedCiphertext2HandlesValidityProof,
        PubkeyValidityProof, ZeroBalanceProof,
    },
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(transparent)]
pub struct PodU16([u8; 2]);
impl From<u16> for PodU16 {
    fn from(n: u16) -> Self {
        Self(n.to_le_bytes())
    }
}
impl From<PodU16> for u16 {
    fn from(pod: PodU16) -> Self {
        Self::from_le_bytes(pod.0)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(transparent)]
pub struct PodU64([u8; 8]);
impl From<u64> for PodU64 {
    fn from(n: u64) -> Self {
        Self(n.to_le_bytes())
    }
}
impl From<PodU64> for u64 {
    fn from(pod: PodU64) -> Self {
        Self::from_le_bytes(pod.0)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(transparent)]
pub struct PodProofType(u8);
impl From<ProofType> for PodProofType {
    fn from(proof_type: ProofType) -> Self {
        Self(ToPrimitive::to_u8(&proof_type).unwrap())
    }
}
impl TryFrom<PodProofType> for ProofType {
    type Error = InstructionError;

    fn try_from(pod: PodProofType) -> Result<Self, Self::Error> {
        FromPrimitive::from_u8(pod.0).ok_or(Self::Error::InvalidAccountData)
    }
}

#[derive(Clone, Copy, Pod, Zeroable, PartialEq, Eq)]
#[repr(transparent)]
pub struct CompressedRistretto(pub [u8; 32]);

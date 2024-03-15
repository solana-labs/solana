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
    thiserror::Error,
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

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    #[error("String is the wrong size")]
    WrongSize,
    #[error("Invalid Base64 string")]
    Invalid,
}

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

macro_rules! impl_from_str {
    (TYPE = $type:ident, BYTES_LEN = $bytes_len:expr, BASE64_LEN = $base64_len:expr) => {
        impl std::str::FromStr for $type {
            type Err = crate::zk_token_elgamal::pod::ParseError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                if s.len() > $base64_len {
                    return Err(Self::Err::WrongSize);
                }
                let mut bytes = [0u8; $bytes_len];
                let decoded_len = BASE64_STANDARD
                    .decode_slice(s, &mut bytes)
                    .map_err(|_| Self::Err::Invalid)?;
                if decoded_len != $bytes_len {
                    Err(Self::Err::WrongSize)
                } else {
                    Ok($type(bytes))
                }
            }
        }
    };
}
pub(crate) use impl_from_str;

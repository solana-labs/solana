//! Errors related to proving and verifying proofs.
use {
    crate::{
        encryption::elgamal::ElGamalError,
        range_proof::errors::{RangeProofGenerationError, RangeProofVerificationError},
        sigma_proofs::errors::*,
    },
    thiserror::Error,
};

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum ProofGenerationError {
    #[error("not enough funds in account")]
    NotEnoughFunds,
    #[error("transfer fee calculation error")]
    FeeCalculation,
    #[error("illegal number of commitments")]
    IllegalCommitmentLength,
    #[error("illegal amount bit length")]
    IllegalAmountBitLength,
    #[error("invalid commitment")]
    InvalidCommitment,
    #[error("range proof generation failed")]
    RangeProof(#[from] RangeProofGenerationError),
    #[error("unexpected proof length")]
    ProofLength,
}

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum ProofVerificationError {
    #[error("range proof verification failed")]
    RangeProof(#[from] RangeProofVerificationError),
    #[error("sigma proof verification failed")]
    SigmaProof(SigmaProofType, SigmaProofVerificationError),
    #[error("ElGamal ciphertext or public key error")]
    ElGamal(#[from] ElGamalError),
    #[error("Invalid proof context")]
    ProofContext,
    #[error("illegal commitment length")]
    IllegalCommitmentLength,
    #[error("illegal amount bit length")]
    IllegalAmountBitLength,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SigmaProofType {
    EqualityProof,
    ValidityProof,
    ZeroBalanceProof,
    FeeSigmaProof,
    PubkeyValidityProof,
}

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum TranscriptError {
    #[error("point is the identity")]
    ValidationError,
}

impl From<EqualityProofVerificationError> for ProofVerificationError {
    fn from(err: EqualityProofVerificationError) -> Self {
        Self::SigmaProof(SigmaProofType::EqualityProof, err.0)
    }
}

impl From<FeeSigmaProofVerificationError> for ProofVerificationError {
    fn from(err: FeeSigmaProofVerificationError) -> Self {
        Self::SigmaProof(SigmaProofType::FeeSigmaProof, err.0)
    }
}

impl From<ZeroBalanceProofVerificationError> for ProofVerificationError {
    fn from(err: ZeroBalanceProofVerificationError) -> Self {
        Self::SigmaProof(SigmaProofType::ZeroBalanceProof, err.0)
    }
}
impl From<ValidityProofVerificationError> for ProofVerificationError {
    fn from(err: ValidityProofVerificationError) -> Self {
        Self::SigmaProof(SigmaProofType::ValidityProof, err.0)
    }
}

impl From<PubkeyValidityProofVerificationError> for ProofVerificationError {
    fn from(err: PubkeyValidityProofVerificationError) -> Self {
        Self::SigmaProof(SigmaProofType::PubkeyValidityProof, err.0)
    }
}

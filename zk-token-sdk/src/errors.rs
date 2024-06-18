//! Errors related to proving and verifying proofs.
#[cfg(not(target_os = "solana"))]
use crate::range_proof::errors::RangeProofGenerationError;
use {
    crate::{range_proof::errors::RangeProofVerificationError, sigma_proofs::errors::*},
    thiserror::Error,
};

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum ElGamalError {
    #[error("key derivation method not supported")]
    DerivationMethodNotSupported,
    #[error("seed length too short for derivation")]
    SeedLengthTooShort,
    #[error("seed length too long for derivation")]
    SeedLengthTooLong,
    #[error("failed to deserialize ciphertext")]
    CiphertextDeserialization,
    #[error("failed to deserialize public key")]
    PubkeyDeserialization,
    #[error("failed to deserialize keypair")]
    KeypairDeserialization,
    #[error("failed to deserialize secret key")]
    SecretKeyDeserialization,
}

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum AuthenticatedEncryptionError {
    #[error("key derivation method not supported")]
    DerivationMethodNotSupported,
    #[error("seed length too short for derivation")]
    SeedLengthTooShort,
    #[error("seed length too long for derivation")]
    SeedLengthTooLong,
    #[error("failed to deserialize")]
    Deserialization,
}

#[cfg(not(target_os = "solana"))]
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

#[cfg(not(target_os = "solana"))]
impl From<EqualityProofVerificationError> for ProofVerificationError {
    fn from(err: EqualityProofVerificationError) -> Self {
        Self::SigmaProof(SigmaProofType::EqualityProof, err.0)
    }
}

#[cfg(not(target_os = "solana"))]
impl From<FeeSigmaProofVerificationError> for ProofVerificationError {
    fn from(err: FeeSigmaProofVerificationError) -> Self {
        Self::SigmaProof(SigmaProofType::FeeSigmaProof, err.0)
    }
}

#[cfg(not(target_os = "solana"))]
impl From<ZeroBalanceProofVerificationError> for ProofVerificationError {
    fn from(err: ZeroBalanceProofVerificationError) -> Self {
        Self::SigmaProof(SigmaProofType::ZeroBalanceProof, err.0)
    }
}

#[cfg(not(target_os = "solana"))]
impl From<ValidityProofVerificationError> for ProofVerificationError {
    fn from(err: ValidityProofVerificationError) -> Self {
        Self::SigmaProof(SigmaProofType::ValidityProof, err.0)
    }
}

#[cfg(not(target_os = "solana"))]
impl From<PubkeyValidityProofVerificationError> for ProofVerificationError {
    fn from(err: PubkeyValidityProofVerificationError) -> Self {
        Self::SigmaProof(SigmaProofType::PubkeyValidityProof, err.0)
    }
}

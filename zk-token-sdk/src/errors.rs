//! Errors related to proving and verifying proofs.
use {
    crate::{range_proof::errors::RangeProofError, sigma_proofs::errors::*},
    thiserror::Error,
};

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum ProofError {
    #[error("invalid transfer amount range")]
    TransferAmount,
    #[error("proof generation failed")]
    Generation,
    #[error("proof failed to verify")]
    Verification,
    #[error("range proof failed to verify")]
    RangeProof,
    #[error("equality proof failed to verify")]
    EqualityProof,
    #[error("fee proof failed to verify")]
    FeeProof,
    #[error("zero-balance proof failed to verify")]
    ZeroBalanceProof,
    #[error("validity proof failed to verify")]
    ValidityProof,
    #[error("public-key sigma proof failed to verify")]
    PubkeySigmaProof,
    #[error("failed to decrypt ciphertext")]
    Decryption,
    #[error("invalid ciphertext data")]
    CiphertextDeserialization,
    #[error("invalid scalar data")]
    ScalarDeserialization,
}

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum TranscriptError {
    #[error("point is the identity")]
    ValidationError,
}

impl From<RangeProofError> for ProofError {
    fn from(_err: RangeProofError) -> Self {
        Self::RangeProof
    }
}

impl From<EqualityProofError> for ProofError {
    fn from(_err: EqualityProofError) -> Self {
        Self::EqualityProof
    }
}

impl From<FeeSigmaProofError> for ProofError {
    fn from(_err: FeeSigmaProofError) -> Self {
        Self::FeeProof
    }
}

impl From<ZeroBalanceProofError> for ProofError {
    fn from(_err: ZeroBalanceProofError) -> Self {
        Self::ZeroBalanceProof
    }
}
impl From<ValidityProofError> for ProofError {
    fn from(_err: ValidityProofError) -> Self {
        Self::ValidityProof
    }
}

impl From<PubkeySigmaProofError> for ProofError {
    fn from(_err: PubkeySigmaProofError) -> Self {
        Self::PubkeySigmaProof
    }
}

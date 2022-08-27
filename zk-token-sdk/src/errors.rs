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
    #[error(
        "`zk_token_elgamal::pod::ElGamalCiphertext` contains invalid ElGamalCiphertext ciphertext"
    )]
    InconsistentCTData,
    #[error("failed to decrypt ciphertext from transfer data")]
    Decryption,
    #[error("discrete log number of threads not power-of-two")]
    DiscreteLogThreads,
    #[error("discrete log batch size too large")]
    DiscreteLogBatchSize,
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

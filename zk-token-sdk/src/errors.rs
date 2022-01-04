//! Errors related to proving and verifying proofs.
use crate::{range_proof::errors::RangeProofError, sigma_proofs::errors::*};
use thiserror::Error;

// TODO: clean up errors for encryption
#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum ProofError {
    #[error("proof failed to verify")]
    Verification,
    #[error("range proof failed to verify")]
    RangeProof,
    #[error("sigma proof failed to verify")]
    SigmaProof,
    #[error(
        "`zk_token_elgamal::pod::ElGamalCiphertext` contains invalid ElGamalCiphertext ciphertext"
    )]
    InconsistentCTData,
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
        Self::SigmaProof
    }
}

impl From<FeeProofError> for ProofError {
    fn from(_err: FeeProofError) -> Self {
        Self::SigmaProof
    }
}

impl From<ZeroBalanceProofError> for ProofError {
    fn from(_err: ZeroBalanceProofError) -> Self {
        Self::SigmaProof
    }
}
impl From<ValidityProofError> for ProofError {
    fn from(_err: ValidityProofError) -> Self {
        Self::SigmaProof
    }
}

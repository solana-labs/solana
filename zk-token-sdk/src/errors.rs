//! Errors related to proving and verifying proofs.
use thiserror::Error;
use crate::{
    range_proof::errors::RangeProofError,
    sigma_proofs::errors::*,
};

// TODO: clean up errors for encryption
#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum ProofError {
    #[error("proof failed to verify")]
    VerificationError,
    #[error("range proof failed to verify")]
    RangeProofError,
    #[error("sigma proof failed to verify")]
    SigmaProofError,
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
        Self::RangeProofError
    }
}

impl From<EqualityProofError> for ProofError {
    fn from(_err: EqualityProofError) -> Self {
        Self::SigmaProofError
    }
}

impl From<FeeProofError> for ProofError {
    fn from(_err: FeeProofError) -> Self {
        Self::SigmaProofError
    }
}

impl From<ZeroBalanceProofError> for ProofError {
    fn from(_err: ZeroBalanceProofError) -> Self {
        Self::SigmaProofError
    }
}
impl From<ValidityProofError> for ProofError {
    fn from(_err: ValidityProofError) -> Self {
        Self::SigmaProofError
    }
}

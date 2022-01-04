//! Errors related to proving and verifying proofs.
use thiserror::Error;
use crate::range_proof::errors::RangeProofError;

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
    fn from(err: RangeProofError) -> Self {
        Self::RangeProofError
    }
}

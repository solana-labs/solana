//! Errors related to proving and verifying range proofs.
use {
    crate::errors::{ProofVerificationError, TranscriptError},
    thiserror::Error,
};

#[derive(Error, Clone, Debug, Eq, PartialEq)]
#[error("range proof verification failed: {0}")]
pub struct RangeProofError(#[from] pub(crate) ProofVerificationError);
impl_from_transcript_error!(RangeProofError);

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum RangeProofGenerationError {
    #[error("maximum generator length exceeded")]
    MaximumGeneratorLengthExceeded,
}

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum RangeProofGeneratorError {
    #[error("maximum generator length exceeded")]
    MaximumGeneratorLengthExceeded,
}

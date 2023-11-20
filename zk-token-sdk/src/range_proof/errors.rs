//! Errors related to proving and verifying range proofs.
use {
    crate::errors::{ProofVerificationError, TranscriptError},
    thiserror::Error,
};

#[derive(Error, Clone, Debug, Eq, PartialEq)]
<<<<<<< HEAD
#[error("range proof verification failed: {0}")]
pub struct RangeProofError(#[from] pub(crate) ProofVerificationError);
impl_from_transcript_error!(RangeProofError);
=======
pub enum RangeProofGenerationError {
    #[error("maximum generator length exceeded")]
    MaximumGeneratorLengthExceeded,
}

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum RangeProofVerificationError {
    #[error("required algebraic relation does not hold")]
    AlgebraicRelation,
    #[error("malformed proof")]
    Deserialization,
    #[error("multiscalar multiplication failed")]
    MultiscalarMul,
    #[error("transcript failed to produce a challenge")]
    Transcript(#[from] TranscriptError),
    #[error(
        "attempted to verify range proof with a non-power-of-two bit size or bit size is too big"
    )]
    InvalidBitSize,
    #[error("insufficient generators for the proof")]
    InvalidGeneratorsLength,
    #[error("maximum generator length exceeded")]
    MaximumGeneratorLengthExceeded,
}

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum RangeProofGeneratorError {
    #[error("maximum generator length exceeded")]
    MaximumGeneratorLengthExceeded,
}
>>>>>>> 0e6dd54f81 ([zk-token-sdk] Restrict range proof generator length and prevent 0-bit range proof (#34166))

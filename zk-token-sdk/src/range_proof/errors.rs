//! Errors related to proving and verifying range proofs.
use {crate::errors::TranscriptError, thiserror::Error};

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum RangeProofGenerationError {
    #[error("maximum generator length exceeded")]
    MaximumGeneratorLengthExceeded,
    #[error("amounts, commitments, openings, or bit lengths vectors have different lengths")]
    VectorLengthMismatch,
    #[error("invalid bit size")]
    InvalidBitSize,
    #[error("insufficient generators for the proof")]
    GeneratorLengthMismatch,
    #[error("inner product length mismatch")]
    InnerProductLengthMismatch,
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
    #[error("commitments and bit lengths vectors have different lengths")]
    VectorLengthMismatch,
}

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum RangeProofGeneratorError {
    #[error("maximum generator length exceeded")]
    MaximumGeneratorLengthExceeded,
}

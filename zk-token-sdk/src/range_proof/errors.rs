//! Errors related to proving and verifying range proofs.
use thiserror::Error;
use crate::errors::TranscriptError;

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum RangeProofError {
    #[error("the required algebraic relation does not hold")]
    AlgebraicRelationError,
    #[error("malformed proof")]
    FormatError,
    #[error("attempted to create a proof with a non-power-of-two bitsize or bitsize too big")]
    InvalidBitsize,
    #[error("insufficient generators for the proof")]
    InvalidGeneratorsLength,
    #[error("multiscalar multiplication failed")]
    MultiscalarMulError,
    #[error("transcript failed to produce a challenge")]
    TranscriptError,
    #[error("number of blinding factors do not match the number of values")]
    WrongNumBlindingFactors,
}

impl From<TranscriptError> for RangeProofError {
    fn from(err: TranscriptError) -> Self {
        Self::TranscriptError
    }
}

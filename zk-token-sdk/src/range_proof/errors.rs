//! Errors related to proving and verifying range proofs.
use {crate::errors::TranscriptError, thiserror::Error};

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum RangeProofError {
    #[error("the required algebraic relation does not hold")]
    AlgebraicRelation,
    #[error("malformed proof")]
    Format,
    #[error("attempted to create a proof with a non-power-of-two bitsize or bitsize too big")]
    InvalidBitsize,
    #[error("insufficient generators for the proof")]
    InvalidGeneratorsLength,
    #[error("multiscalar multiplication failed")]
    MultiscalarMul,
    #[error("transcript failed to produce a challenge")]
    Transcript,
    #[error("number of blinding factors do not match the number of values")]
    WrongNumBlindingFactors,
}

impl From<TranscriptError> for RangeProofError {
    fn from(_err: TranscriptError) -> Self {
        Self::Transcript
    }
}

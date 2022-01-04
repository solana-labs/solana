//! Errors related to proving and verifying sigma proofs.
use thiserror::Error;
use crate::errors::TranscriptError;

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum EqualityProof {
    #[error("the required algebraic relation does not hold")]
    AlgebraicRelationError,
    #[error("malformed proof")]
    FormatError,
    #[error("multiscalar multiplication failed")]
    MultiscalarMulError,
    #[error("transcript failed to produce a challenge")]
    TranscriptError,
}

impl From<TranscriptError> for EqualityProof {
    fn from(err: TranscriptError) -> Self {
        Self::TranscriptError
    }
}

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum ValidityProof {
    #[error("the required algebraic relation does not hold")]
    AlgebraicRelationError,
    #[error("malformed proof")]
    FormatError,
    #[error("multiscalar multiplication failed")]
    MultiscalarMulError,
    #[error("transcript failed to produce a challenge")]
    TranscriptError,
}

impl From<TranscriptError> for ValidityProof {
    fn from(err: TranscriptError) -> Self {
        Self::TranscriptError
    }
}

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum ZeroBalanceProof {
    #[error("the required algebraic relation does not hold")]
    AlgebraicRelationError,
    #[error("malformed proof")]
    FormatError,
    #[error("multiscalar multiplication failed")]
    MultiscalarMulError,
    #[error("transcript failed to produce a challenge")]
    TranscriptError,
}

impl From<TranscriptError> for ZeroBalanceProof {
    fn from(err: TranscriptError) -> Self {
        Self::TranscriptError
    }
}

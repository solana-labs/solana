//! Errors related to proving and verifying sigma proofs.
use thiserror::Error;
use crate::errors::TranscriptError;

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum EqualityProofError {
    #[error("the required algebraic relation does not hold")]
    AlgebraicRelationError,
    #[error("malformed proof")]
    FormatError,
    #[error("multiscalar multiplication failed")]
    MultiscalarMulError,
    #[error("transcript failed to produce a challenge")]
    TranscriptError,
}

impl From<TranscriptError> for EqualityProofError {
    fn from(_err: TranscriptError) -> Self {
        Self::TranscriptError
    }
}

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum ValidityProofError {
    #[error("the required algebraic relation does not hold")]
    AlgebraicRelationError,
    #[error("malformed proof")]
    FormatError,
    #[error("multiscalar multiplication failed")]
    MultiscalarMulError,
    #[error("transcript failed to produce a challenge")]
    TranscriptError,
}

impl From<TranscriptError> for ValidityProofError {
    fn from(_err: TranscriptError) -> Self {
        Self::TranscriptError
    }
}

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum ZeroBalanceProofError {
    #[error("the required algebraic relation does not hold")]
    AlgebraicRelationError,
    #[error("malformed proof")]
    FormatError,
    #[error("multiscalar multiplication failed")]
    MultiscalarMulError,
    #[error("transcript failed to produce a challenge")]
    TranscriptError,
}

impl From<TranscriptError> for ZeroBalanceProofError {
    fn from(_err: TranscriptError) -> Self {
        Self::TranscriptError
    }
}

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum FeeProofError {
    #[error("the required algebraic relation does not hold")]
    AlgebraicRelationError,
    #[error("malformed proof")]
    FormatError,
    #[error("multiscalar multiplication failed")]
    MultiscalarMulError,
    #[error("transcript failed to produce a challenge")]
    TranscriptError,
}

impl From<TranscriptError> for FeeProofError {
    fn from(_err: TranscriptError) -> Self {
        Self::TranscriptError
    }
}

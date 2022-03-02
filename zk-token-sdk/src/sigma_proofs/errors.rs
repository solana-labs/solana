//! Errors related to proving and verifying sigma proofs.
use {crate::errors::TranscriptError, thiserror::Error};

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum EqualityProofError {
    #[error("the required algebraic relation does not hold")]
    AlgebraicRelation,
    #[error("malformed proof")]
    Format,
    #[error("multiscalar multiplication failed")]
    MultiscalarMul,
    #[error("transcript failed to produce a challenge")]
    Transcript,
}

impl From<TranscriptError> for EqualityProofError {
    fn from(_err: TranscriptError) -> Self {
        Self::Transcript
    }
}

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum ValidityProofError {
    #[error("the required algebraic relation does not hold")]
    AlgebraicRelation,
    #[error("malformed proof")]
    Format,
    #[error("multiscalar multiplication failed")]
    MultiscalarMul,
    #[error("transcript failed to produce a challenge")]
    Transcript,
}

impl From<TranscriptError> for ValidityProofError {
    fn from(_err: TranscriptError) -> Self {
        Self::Transcript
    }
}

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum ZeroBalanceProofError {
    #[error("the required algebraic relation does not hold")]
    AlgebraicRelation,
    #[error("malformed proof")]
    Format,
    #[error("multiscalar multiplication failed")]
    MultiscalarMul,
    #[error("transcript failed to produce a challenge")]
    Transcript,
}

impl From<TranscriptError> for ZeroBalanceProofError {
    fn from(_err: TranscriptError) -> Self {
        Self::Transcript
    }
}

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum FeeSigmaProofError {
    #[error("the required algebraic relation does not hold")]
    AlgebraicRelation,
    #[error("malformed proof")]
    Format,
    #[error("multiscalar multiplication failed")]
    MultiscalarMul,
    #[error("transcript failed to produce a challenge")]
    Transcript,
}

impl From<TranscriptError> for FeeSigmaProofError {
    fn from(_err: TranscriptError) -> Self {
        Self::Transcript
    }
}

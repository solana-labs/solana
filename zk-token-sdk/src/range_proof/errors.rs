//! Errors related to proving and verifying proofs.
use thiserror::Error;

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum ProofError {
    #[error("the required algebraic relation does not hold")]
    AlgebraicRelationError,
    #[error("malformed proof")]
    FormatError,
    #[error("attempted to create a proof with a non-power-of-two bitsize")]
    InvalidBitsize,
    #[error("insufficient generators for the proof")]
    InvalidGeneratorsLength,
    #[error("number of blinding factors do not match the number of values")]
    WrongNumBlindingFactors,
}

//! Errors related to proving and verifying proofs.
use thiserror::Error;

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum ProofError {
    #[error("proof failed to verify")]
    VerificationError,
    #[error("malformed proof")]
    FormatError,
    #[error("number of blinding factors do not match the number of values")]
    WrongNumBlindingFactors,
    #[error("attempted to create a proof with bitsize other than \\(8\\), \\(16\\), \\(32\\), or \\(64\\)")]
    InvalidBitsize,
    #[error("insufficient generators for the proof")]
    InvalidGeneratorsLength,
    #[error(
        "`zk_token_elgamal::pod::ElGamalCiphertext` contains invalid ElGamalCiphertext ciphertext"
    )]
    InconsistentCTData,
}

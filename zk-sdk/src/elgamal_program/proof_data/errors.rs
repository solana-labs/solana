use thiserror::Error;

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum ProofDataError {
    #[error("decryption error")]
    Decryption,
    #[error("missing ciphertext")]
    MissingCiphertext,
    #[error("illegal amount bit length")]
    IllegalAmountBitLength,
    #[error("arithmetic overflow")]
    Overflow,
    #[error("invalid proof type")]
    InvalidProofType,
}

use thiserror::Error;

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum InstructionError {
    #[error("decryption error")]
    Decryption,
    #[error("missing ciphertext")]
    MissingCiphertext,
}

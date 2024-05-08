use thiserror::Error;

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum ProofVerificationError {
    #[error("Invalid proof context")]
    ProofContext,
}

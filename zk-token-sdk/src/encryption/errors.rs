//! Errors related to the twisted ElGamal encryption scheme.
use thiserror::Error;

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum DiscreteLogError {
    #[error("discrete log number of threads not power-of-two")]
    DiscreteLogThreads,
    #[error("discrete log batch size too large")]
    DiscreteLogBatchSize,
}

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum ElGamalError {
    #[error("key derivation method not supported")]
    DerivationMethodNotSupported,
}

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum AuthenticatedEncryptionError {
    #[error("key derivation method not supported")]
    DerivationMethodNotSupported,

    #[error("pubkey does not exist")]
    PubkeyDoesNotExist,
}

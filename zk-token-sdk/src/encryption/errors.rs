//! Errors related to the twisted ElGamal encryption scheme.
use thiserror::Error;

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum DiscreteLogError {
    #[error("discrete log number of threads not power-of-two")]
    DiscreteLogThreads,
    #[error("discrete log batch size too large")]
    DiscreteLogBatchSize,
}

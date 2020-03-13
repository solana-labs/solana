use crate::transaction::TransactionError;
use std::io;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("transport io error: {0}")]
    IoError(#[from] io::Error),
    #[error("transport transaction error: {0}")]
    TransactionError(#[from] TransactionError),
    #[error("transport custom error: {0}")]
    Custom(String),
}

impl TransportError {
    pub fn unwrap(&self) -> TransactionError {
        if let TransportError::TransactionError(err) = self {
            err.clone()
        } else {
            panic!("unexpected transport error")
        }
    }
}

pub type Result<T> = std::result::Result<T, TransportError>;

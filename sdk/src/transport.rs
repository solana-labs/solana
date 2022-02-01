#![cfg(feature = "full")]

use {
    crate::transaction::TransactionError,
    quinn::{ConnectError, WriteError},
    std::io,
    thiserror::Error,
};

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("transport io error: {0}")]
    IoError(#[from] io::Error),
    #[error("transport transaction error: {0}")]
    TransactionError(#[from] TransactionError),
    #[error("transport custom error: {0}")]
    Custom(String),

    #[error("transport send error: {0}")]
    SendError(#[from] WriteError),

    #[error("transport connect error: {0}")]
    ConnectError(#[from] ConnectError),
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

use crate::rpc_request;
use solana_sdk::{
    signature::SignerError, transaction::TransactionError, transport::TransportError,
};
use std::{fmt, io};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ClientError {
    Io(#[from] io::Error),
    Reqwest(#[from] reqwest::Error),
    RpcError(#[from] rpc_request::RpcError),
    SerdeJson(#[from] serde_json::error::Error),
    SigningError(#[from] SignerError),
    TransactionError(#[from] TransactionError),
    Custom(String),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "solana client error")
    }
}

impl From<TransportError> for ClientError {
    fn from(err: TransportError) -> Self {
        match err {
            TransportError::IoError(err) => Self::Io(err),
            TransportError::TransactionError(err) => Self::TransactionError(err),
            TransportError::Custom(err) => Self::Custom(err),
        }
    }
}

impl Into<TransportError> for ClientError {
    fn into(self) -> TransportError {
        match self {
            Self::Io(err) => TransportError::IoError(err),
            Self::TransactionError(err) => TransportError::TransactionError(err),
            Self::Reqwest(err) => TransportError::Custom(format!("{:?}", err)),
            Self::RpcError(err) => TransportError::Custom(format!("{:?}", err)),
            Self::SerdeJson(err) => TransportError::Custom(format!("{:?}", err)),
            Self::SigningError(err) => TransportError::Custom(format!("{:?}", err)),
            Self::Custom(err) => TransportError::Custom(format!("{:?}", err)),
        }
    }
}

pub type Result<T> = std::result::Result<T, ClientError>;

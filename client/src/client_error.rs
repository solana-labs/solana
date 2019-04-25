use crate::rpc_request;
use solana_sdk::transaction::TransactionError;
use std::{fmt, io};

#[derive(Debug)]
pub enum ClientError {
    Io(io::Error),
    Reqwest(reqwest::Error),
    RpcError(rpc_request::RpcError),
    SerdeJson(serde_json::error::Error),
    TransactionError(TransactionError),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "solana client error")
    }
}

impl std::error::Error for ClientError {}

impl From<io::Error> for ClientError {
    fn from(err: io::Error) -> ClientError {
        ClientError::Io(err)
    }
}

impl From<reqwest::Error> for ClientError {
    fn from(err: reqwest::Error) -> ClientError {
        ClientError::Reqwest(err)
    }
}

impl From<rpc_request::RpcError> for ClientError {
    fn from(err: rpc_request::RpcError) -> ClientError {
        ClientError::RpcError(err)
    }
}

impl From<serde_json::error::Error> for ClientError {
    fn from(err: serde_json::error::Error) -> ClientError {
        ClientError::SerdeJson(err)
    }
}

impl From<TransactionError> for ClientError {
    fn from(err: TransactionError) -> ClientError {
        ClientError::TransactionError(err)
    }
}

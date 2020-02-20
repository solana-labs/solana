use crate::rpc_request;
use solana_sdk::transaction::TransactionError;
use std::{fmt, io};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ClientError {
    Io(#[from] io::Error),
    Reqwest(#[from] reqwest::Error),
    RpcError(#[from] rpc_request::RpcError),
    SerdeJson(#[from] serde_json::error::Error),
    SigningError(String),
    TransactionError(#[from] TransactionError),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "solana client error")
    }
}

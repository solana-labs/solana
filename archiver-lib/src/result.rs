use serde_json;
use solana_client::client_error;
use solana_ledger::blockstore;
use solana_sdk::transport;
use std::any::Any;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ArchiverError {
    #[error("IO error")]
    IO(#[from] std::io::Error),

    #[error("blockstore error")]
    BlockstoreError(#[from] blockstore::BlockstoreError),

    #[error("crossbeam error")]
    CrossbeamSendError(#[from] crossbeam_channel::SendError<u64>),

    #[error("send error")]
    SendError(#[from] std::sync::mpsc::SendError<u64>),

    #[error("join error")]
    JoinError(Box<dyn Any + Send + 'static>),

    #[error("transport error")]
    TransportError(#[from] transport::TransportError),

    #[error("client error")]
    ClientError(#[from] client_error::ClientError),

    #[error("Json parsing error")]
    JsonError(#[from] serde_json::error::Error),

    #[error("Storage account has no balance")]
    EmptyStorageAccountBalance,

    #[error("No RPC peers..")]
    NoRpcPeers,

    #[error("Couldn't download full segment")]
    SegmentDownloadError,
}

impl std::convert::From<Box<dyn Any + Send + 'static>> for ArchiverError {
    fn from(e: Box<dyn Any + Send + 'static>) -> ArchiverError {
        ArchiverError::JoinError(e)
    }
}

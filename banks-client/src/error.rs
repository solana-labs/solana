use {
    solana_sdk::{transaction::TransactionError, transport::TransportError},
    std::io,
    tarpc::client::RpcError,
    thiserror::Error,
};

/// Errors from BanksClient
#[derive(Error, Debug)]
pub enum BanksClientError {
    #[error("client error: {0}")]
    ClientError(&'static str),

    #[error(transparent)]
    Io(#[from] io::Error),

    #[error(transparent)]
    RpcError(#[from] RpcError),

    #[error("transport transaction error: {0}")]
    TransactionError(#[from] TransactionError),
}

impl BanksClientError {
    pub fn unwrap(&self) -> TransactionError {
        if let BanksClientError::TransactionError(err) = self {
            err.clone()
        } else {
            panic!("unexpected transport error")
        }
    }
}

impl From<BanksClientError> for io::Error {
    fn from(err: BanksClientError) -> Self {
        match err {
            BanksClientError::ClientError(err) => Self::new(io::ErrorKind::Other, err.to_string()),
            BanksClientError::Io(err) => err,
            BanksClientError::RpcError(err) => Self::new(io::ErrorKind::Other, err.to_string()),
            BanksClientError::TransactionError(err) => {
                Self::new(io::ErrorKind::Other, err.to_string())
            }
        }
    }
}

impl From<BanksClientError> for TransportError {
    fn from(err: BanksClientError) -> Self {
        match err {
            BanksClientError::ClientError(err) => {
                Self::IoError(io::Error::new(io::ErrorKind::Other, err.to_string()))
            }
            BanksClientError::Io(err) => {
                Self::IoError(io::Error::new(io::ErrorKind::Other, err.to_string()))
            }
            BanksClientError::RpcError(err) => {
                Self::IoError(io::Error::new(io::ErrorKind::Other, err.to_string()))
            }
            BanksClientError::TransactionError(err) => Self::TransactionError(err),
        }
    }
}

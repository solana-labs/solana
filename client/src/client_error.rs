use crate::rpc_request;
use solana_sdk::{
    signature::SignerError, transaction::TransactionError, transport::TransportError,
};
use std::io;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ClientErrorKind {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
    #[error(transparent)]
    RpcError(#[from] rpc_request::RpcError),
    #[error(transparent)]
    SerdeJson(#[from] serde_json::error::Error),
    #[error(transparent)]
    SigningError(#[from] SignerError),
    #[error(transparent)]
    TransactionError(#[from] TransactionError),
    #[error("Custom: {0}")]
    Custom(String),
}

impl From<TransportError> for ClientErrorKind {
    fn from(err: TransportError) -> Self {
        match err {
            TransportError::IoError(err) => Self::Io(err),
            TransportError::TransactionError(err) => Self::TransactionError(err),
            TransportError::Custom(err) => Self::Custom(err),
        }
    }
}

impl Into<TransportError> for ClientErrorKind {
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

#[derive(Error, Debug)]
#[error("{kind}")]
pub struct ClientError {
    command: Option<&'static str>,
    #[source]
    #[error(transparent)]
    kind: ClientErrorKind,
}

impl ClientError {
    pub fn new_with_command(kind: ClientErrorKind, command: &'static str) -> Self {
        Self {
            command: Some(command),
            kind,
        }
    }

    pub fn into_with_command(self, command: &'static str) -> Self {
        Self {
            command: Some(command),
            ..self
        }
    }

    pub fn command(&self) -> Option<&'static str> {
        self.command
    }

    pub fn kind(&self) -> &ClientErrorKind {
        &self.kind
    }
}

impl From<ClientErrorKind> for ClientError {
    fn from(kind: ClientErrorKind) -> Self {
        Self {
            command: None,
            kind,
        }
    }
}

impl From<TransportError> for ClientError {
    fn from(err: TransportError) -> Self {
        Self {
            command: None,
            kind: err.into(),
        }
    }
}

impl Into<TransportError> for ClientError {
    fn into(self) -> TransportError {
        self.kind.into()
    }
}

impl From<std::io::Error> for ClientError {
    fn from(err: std::io::Error) -> Self {
        Self {
            command: None,
            kind: err.into(),
        }
    }
}

impl From<reqwest::Error> for ClientError {
    fn from(err: reqwest::Error) -> Self {
        Self {
            command: None,
            kind: err.into(),
        }
    }
}

impl From<rpc_request::RpcError> for ClientError {
    fn from(err: rpc_request::RpcError) -> Self {
        Self {
            command: None,
            kind: err.into(),
        }
    }
}

impl From<serde_json::error::Error> for ClientError {
    fn from(err: serde_json::error::Error) -> Self {
        Self {
            command: None,
            kind: err.into(),
        }
    }
}

impl From<SignerError> for ClientError {
    fn from(err: SignerError) -> Self {
        Self {
            command: None,
            kind: err.into(),
        }
    }
}

impl From<TransactionError> for ClientError {
    fn from(err: TransactionError) -> Self {
        Self {
            command: None,
            kind: err.into(),
        }
    }
}

pub type Result<T> = std::result::Result<T, ClientError>;

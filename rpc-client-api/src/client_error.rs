pub use reqwest;
use {
    crate::{request, response},
    solana_sdk::{
        signature::SignerError, transaction::TransactionError, transport::TransportError,
    },
    std::io,
    thiserror::Error as ThisError,
};

#[derive(ThisError, Debug)]
pub enum ErrorKind {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
    #[error(transparent)]
    RpcError(#[from] request::RpcError),
    #[error(transparent)]
    SerdeJson(#[from] serde_json::error::Error),
    #[error(transparent)]
    SigningError(#[from] SignerError),
    #[error(transparent)]
    TransactionError(#[from] TransactionError),
    #[error("Custom: {0}")]
    Custom(String),
}

impl ErrorKind {
    pub fn get_transaction_error(&self) -> Option<TransactionError> {
        match self {
            Self::RpcError(request::RpcError::RpcResponseError {
                data:
                    request::RpcResponseErrorData::SendTransactionPreflightFailure(
                        response::RpcSimulateTransactionResult {
                            err: Some(tx_err), ..
                        },
                    ),
                ..
            }) => Some(tx_err.clone()),
            Self::TransactionError(tx_err) => Some(tx_err.clone()),
            _ => None,
        }
    }
}

impl From<TransportError> for ErrorKind {
    fn from(err: TransportError) -> Self {
        match err {
            TransportError::IoError(err) => Self::Io(err),
            TransportError::TransactionError(err) => Self::TransactionError(err),
            TransportError::Custom(err) => Self::Custom(err),
        }
    }
}

impl From<ErrorKind> for TransportError {
    fn from(client_error_kind: ErrorKind) -> Self {
        match client_error_kind {
            ErrorKind::Io(err) => Self::IoError(err),
            ErrorKind::TransactionError(err) => Self::TransactionError(err),
            ErrorKind::Reqwest(err) => Self::Custom(format!("{err:?}")),
            ErrorKind::RpcError(err) => Self::Custom(format!("{err:?}")),
            ErrorKind::SerdeJson(err) => Self::Custom(format!("{err:?}")),
            ErrorKind::SigningError(err) => Self::Custom(format!("{err:?}")),
            ErrorKind::Custom(err) => Self::Custom(format!("{err:?}")),
        }
    }
}

#[derive(ThisError, Debug)]
#[error("{kind}")]
pub struct Error {
    pub request: Option<request::RpcRequest>,

    #[source]
    pub kind: ErrorKind,
}

impl Error {
    pub fn new_with_request(kind: ErrorKind, request: request::RpcRequest) -> Self {
        Self {
            request: Some(request),
            kind,
        }
    }

    pub fn into_with_request(self, request: request::RpcRequest) -> Self {
        Self {
            request: Some(request),
            ..self
        }
    }

    pub fn request(&self) -> Option<&request::RpcRequest> {
        self.request.as_ref()
    }

    pub fn kind(&self) -> &ErrorKind {
        &self.kind
    }

    pub fn get_transaction_error(&self) -> Option<TransactionError> {
        self.kind.get_transaction_error()
    }
}

impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Self {
        Self {
            request: None,
            kind,
        }
    }
}

impl From<TransportError> for Error {
    fn from(err: TransportError) -> Self {
        Self {
            request: None,
            kind: err.into(),
        }
    }
}

impl From<Error> for TransportError {
    fn from(client_error: Error) -> Self {
        client_error.kind.into()
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self {
            request: None,
            kind: err.into(),
        }
    }
}

impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Self {
        Self {
            request: None,
            kind: err.into(),
        }
    }
}

impl From<request::RpcError> for Error {
    fn from(err: request::RpcError) -> Self {
        Self {
            request: None,
            kind: err.into(),
        }
    }
}

impl From<serde_json::error::Error> for Error {
    fn from(err: serde_json::error::Error) -> Self {
        Self {
            request: None,
            kind: err.into(),
        }
    }
}

impl From<SignerError> for Error {
    fn from(err: SignerError) -> Self {
        Self {
            request: None,
            kind: err.into(),
        }
    }
}

impl From<TransactionError> for Error {
    fn from(err: TransactionError) -> Self {
        Self {
            request: None,
            kind: err.into(),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

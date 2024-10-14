use {
    quinn::{ConnectError, ConnectionError, WriteError},
    std::{
        fmt::{self, Formatter},
        io,
    },
    thiserror::Error,
};

/// Wrapper for [`io::Error`] implementing [`PartialEq`] to simplify error
/// checking for the [`QuicError`] type. The reasons why [`io::Error`] doesn't
/// implement [`PartialEq`] are discusses in
/// <https://github.com/rust-lang/rust/issues/34158>.
#[derive(Debug, Error)]
pub struct IoErrorWithPartialEq(pub io::Error);

impl PartialEq for IoErrorWithPartialEq {
    fn eq(&self, other: &Self) -> bool {
        let formatted_self = format!("{self:?}");
        let formatted_other = format!("{other:?}");
        formatted_self == formatted_other
    }
}

impl fmt::Display for IoErrorWithPartialEq {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<io::Error> for IoErrorWithPartialEq {
    fn from(err: io::Error) -> Self {
        IoErrorWithPartialEq(err)
    }
}

/// Error types that can occur when dealing with QUIC connections or
/// transmissions.
#[derive(Error, Debug, PartialEq)]
pub enum QuicError {
    #[error(transparent)]
    StreamWrite(#[from] WriteError),
    #[error(transparent)]
    Connection(#[from] ConnectionError),
    #[error(transparent)]
    Connect(#[from] ConnectError),
    #[error(transparent)]
    Endpoint(#[from] IoErrorWithPartialEq),
}

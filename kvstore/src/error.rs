use std::error::Error as StdErr;
use std::fmt;
use std::io;
use std::result::Result as StdRes;
use std::sync::mpsc::{RecvError, SendError, TryRecvError};

pub type Result<T> = StdRes<T, Error>;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Corrupted(bincode::Error),
    Channel(Box<dyn StdErr + Sync + Send>),
    Missing,
    WriteBatchFull(usize),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Corrupted(_) => write!(f, "Serialization error: Store may be corrupted"),
            Error::Channel(e) => write!(f, "Internal communication error: {}", e),
            Error::Io(e) => write!(f, "I/O error: {}", e),
            Error::Missing => write!(f, "Item not present in ledger"),
            Error::WriteBatchFull(capacity) => write!(f, "WriteBatch capacity {} full", capacity),
        }
    }
}

impl StdErr for Error {
    fn source(&self) -> Option<&(dyn StdErr + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            Error::Corrupted(ref e) => Some(e),
            Error::Channel(e) => Some(e.as_ref()),
            Error::Missing => None,
            Error::WriteBatchFull(_) => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

impl<W> From<io::IntoInnerError<W>> for Error {
    fn from(e: io::IntoInnerError<W>) -> Self {
        Error::Io(e.into())
    }
}

impl From<bincode::Error> for Error {
    fn from(e: bincode::Error) -> Self {
        Error::Corrupted(e)
    }
}

impl<T> From<SendError<T>> for Error
where
    T: Send + Sync + 'static,
{
    fn from(e: SendError<T>) -> Self {
        Error::Channel(Box::new(e))
    }
}

impl From<RecvError> for Error {
    fn from(e: RecvError) -> Self {
        Error::Channel(Box::new(e))
    }
}

impl From<TryRecvError> for Error {
    fn from(e: TryRecvError) -> Self {
        Error::Channel(Box::new(e))
    }
}

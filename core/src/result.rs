//! The `result` module exposes a Result type that propagates one of many different Error types.

use solana_gossip::{cluster_info, gossip_error::GossipError};
use solana_ledger::blockstore;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    RecvError(std::sync::mpsc::RecvError),
    CrossbeamRecvTimeoutError(crossbeam_channel::RecvTimeoutError),
    ReadyTimeoutError,
    RecvTimeoutError(std::sync::mpsc::RecvTimeoutError),
    CrossbeamSendError,
    TryCrossbeamSendError,
    Serialize(std::boxed::Box<bincode::ErrorKind>),
    ClusterInfoError(cluster_info::ClusterInfoError),
    SendError,
    BlockstoreError(blockstore::BlockstoreError),
    WeightedIndexError(rand::distributions::weighted::WeightedError),
    GossipError(GossipError),
}

pub type Result<T> = std::result::Result<T, Error>;

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "solana error")
    }
}

impl std::error::Error for Error {}

impl std::convert::From<std::sync::mpsc::RecvError> for Error {
    fn from(e: std::sync::mpsc::RecvError) -> Error {
        Error::RecvError(e)
    }
}
impl std::convert::From<crossbeam_channel::RecvTimeoutError> for Error {
    fn from(e: crossbeam_channel::RecvTimeoutError) -> Error {
        Error::CrossbeamRecvTimeoutError(e)
    }
}
impl std::convert::From<crossbeam_channel::ReadyTimeoutError> for Error {
    fn from(_e: crossbeam_channel::ReadyTimeoutError) -> Error {
        Error::ReadyTimeoutError
    }
}
impl std::convert::From<std::sync::mpsc::RecvTimeoutError> for Error {
    fn from(e: std::sync::mpsc::RecvTimeoutError) -> Error {
        Error::RecvTimeoutError(e)
    }
}
impl std::convert::From<cluster_info::ClusterInfoError> for Error {
    fn from(e: cluster_info::ClusterInfoError) -> Error {
        Error::ClusterInfoError(e)
    }
}
impl<T> std::convert::From<crossbeam_channel::SendError<T>> for Error {
    fn from(_e: crossbeam_channel::SendError<T>) -> Error {
        Error::CrossbeamSendError
    }
}
impl<T> std::convert::From<crossbeam_channel::TrySendError<T>> for Error {
    fn from(_e: crossbeam_channel::TrySendError<T>) -> Error {
        Error::TryCrossbeamSendError
    }
}
impl<T> std::convert::From<std::sync::mpsc::SendError<T>> for Error {
    fn from(_e: std::sync::mpsc::SendError<T>) -> Error {
        Error::SendError
    }
}
impl std::convert::From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        Error::Io(e)
    }
}
impl std::convert::From<std::boxed::Box<bincode::ErrorKind>> for Error {
    fn from(e: std::boxed::Box<bincode::ErrorKind>) -> Error {
        Error::Serialize(e)
    }
}
impl std::convert::From<blockstore::BlockstoreError> for Error {
    fn from(e: blockstore::BlockstoreError) -> Error {
        Error::BlockstoreError(e)
    }
}
impl std::convert::From<rand::distributions::weighted::WeightedError> for Error {
    fn from(e: rand::distributions::weighted::WeightedError) -> Error {
        Error::WeightedIndexError(e)
    }
}
impl std::convert::From<GossipError> for Error {
    fn from(e: GossipError) -> Error {
        Error::GossipError(e)
    }
}

#[cfg(test)]
mod tests {
    use crate::result::Error;
    use crate::result::Result;
    use std::io;
    use std::io::Write;
    use std::panic;
    use std::sync::mpsc::channel;
    use std::sync::mpsc::RecvError;
    use std::sync::mpsc::RecvTimeoutError;

    fn send_error() -> Result<()> {
        let (s, r) = channel();
        drop(r);
        s.send(())?;
        Ok(())
    }

    #[test]
    fn from_test() {
        assert_matches!(Error::from(RecvError {}), Error::RecvError(_));
        assert_matches!(
            Error::from(RecvTimeoutError::Timeout),
            Error::RecvTimeoutError(_)
        );
        assert_matches!(send_error(), Err(Error::SendError));
        let ioe = io::Error::new(io::ErrorKind::NotFound, "hi");
        assert_matches!(Error::from(ioe), Error::Io(_));
    }
    #[test]
    fn fmt_test() {
        write!(io::sink(), "{:?}", Error::from(RecvError {})).unwrap();
        write!(io::sink(), "{:?}", Error::from(RecvTimeoutError::Timeout)).unwrap();
        write!(io::sink(), "{:?}", send_error()).unwrap();
        write!(
            io::sink(),
            "{:?}",
            Error::from(io::Error::new(io::ErrorKind::NotFound, "hi"))
        )
        .unwrap();
    }
}

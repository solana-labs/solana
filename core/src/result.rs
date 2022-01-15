//! The `result` module exposes a Result type that propagates one of many different Error types.

use {
    solana_gossip::{cluster_info, gossip_error::GossipError},
    solana_ledger::blockstore,
};

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Recv(crossbeam_channel::RecvError),
    ReadyTimeout,
    RecvTimeout(crossbeam_channel::RecvTimeoutError),
    TrySend,
    Serialize(std::boxed::Box<bincode::ErrorKind>),
    ClusterInfo(cluster_info::ClusterInfoError),
    Send,
    Blockstore(blockstore::BlockstoreError),
    WeightedIndex(rand::distributions::weighted::WeightedError),
    Gossip(GossipError),
}

pub type Result<T> = std::result::Result<T, Error>;

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "solana error")
    }
}

impl std::error::Error for Error {}

impl std::convert::From<crossbeam_channel::RecvError> for Error {
    fn from(e: crossbeam_channel::RecvError) -> Error {
        Error::Recv(e)
    }
}
impl std::convert::From<crossbeam_channel::ReadyTimeoutError> for Error {
    fn from(_e: crossbeam_channel::ReadyTimeoutError) -> Error {
        Error::ReadyTimeout
    }
}
impl std::convert::From<crossbeam_channel::RecvTimeoutError> for Error {
    fn from(e: crossbeam_channel::RecvTimeoutError) -> Error {
        Error::RecvTimeout(e)
    }
}
impl std::convert::From<cluster_info::ClusterInfoError> for Error {
    fn from(e: cluster_info::ClusterInfoError) -> Error {
        Error::ClusterInfo(e)
    }
}
impl<T> std::convert::From<crossbeam_channel::TrySendError<T>> for Error {
    fn from(_e: crossbeam_channel::TrySendError<T>) -> Error {
        Error::TrySend
    }
}
impl<T> std::convert::From<crossbeam_channel::SendError<T>> for Error {
    fn from(_e: crossbeam_channel::SendError<T>) -> Error {
        Error::Send
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
        Error::Blockstore(e)
    }
}
impl std::convert::From<rand::distributions::weighted::WeightedError> for Error {
    fn from(e: rand::distributions::weighted::WeightedError) -> Error {
        Error::WeightedIndex(e)
    }
}
impl std::convert::From<GossipError> for Error {
    fn from(e: GossipError) -> Error {
        Error::Gossip(e)
    }
}

#[cfg(test)]
mod tests {
    use {
        crate::result::{Error, Result},
        crossbeam_channel::{unbounded, RecvError, RecvTimeoutError},
        std::{io, io::Write, panic},
    };

    fn send_error() -> Result<()> {
        let (s, r) = unbounded();
        drop(r);
        s.send(())?;
        Ok(())
    }

    #[test]
    fn from_test() {
        assert_matches!(Error::from(RecvError {}), Error::Recv(_));
        assert_matches!(
            Error::from(RecvTimeoutError::Timeout),
            Error::RecvTimeout(_)
        );
        assert_matches!(send_error(), Err(Error::Send));
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

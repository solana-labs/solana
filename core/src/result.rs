//! The `result` module exposes a Result type that propagates one of many different Error types.

use {solana_gossip::gossip_error::GossipError, solana_ledger::blockstore, thiserror::Error};

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Blockstore(#[from] blockstore::BlockstoreError),
    #[error(transparent)]
    Gossip(#[from] GossipError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("ReadyTimeout")]
    ReadyTimeout,
    #[error(transparent)]
    Recv(#[from] crossbeam_channel::RecvError),
    #[error(transparent)]
    RecvTimeout(#[from] crossbeam_channel::RecvTimeoutError),
    #[error("Send")]
    Send,
    #[error("TrySend")]
    TrySend,
}

pub type Result<T> = std::result::Result<T, Error>;

impl std::convert::From<crossbeam_channel::ReadyTimeoutError> for Error {
    fn from(_e: crossbeam_channel::ReadyTimeoutError) -> Error {
        Error::ReadyTimeout
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

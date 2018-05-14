//! The `result` module exposes a Result type that propagates one of many different Error types.

use accountant;
use bincode;
use serde_json;
use std;
use std::any::Any;

#[derive(Debug)]
pub enum Error {
    IO(std::io::Error),
    JSON(serde_json::Error),
    AddrParse(std::net::AddrParseError),
    JoinError(Box<Any + Send + 'static>),
    RecvError(std::sync::mpsc::RecvError),
    RecvTimeoutError(std::sync::mpsc::RecvTimeoutError),
    Serialize(std::boxed::Box<bincode::ErrorKind>),
    AccountingError(accountant::AccountingError),
    SendError,
    Services,
    GeneralError,
}

pub type Result<T> = std::result::Result<T, Error>;

impl std::convert::From<std::sync::mpsc::RecvError> for Error {
    fn from(e: std::sync::mpsc::RecvError) -> Error {
        Error::RecvError(e)
    }
}
impl std::convert::From<std::sync::mpsc::RecvTimeoutError> for Error {
    fn from(e: std::sync::mpsc::RecvTimeoutError) -> Error {
        Error::RecvTimeoutError(e)
    }
}
impl std::convert::From<accountant::AccountingError> for Error {
    fn from(e: accountant::AccountingError) -> Error {
        Error::AccountingError(e)
    }
}
impl<T> std::convert::From<std::sync::mpsc::SendError<T>> for Error {
    fn from(_e: std::sync::mpsc::SendError<T>) -> Error {
        Error::SendError
    }
}
impl std::convert::From<Box<Any + Send + 'static>> for Error {
    fn from(e: Box<Any + Send + 'static>) -> Error {
        Error::JoinError(e)
    }
}

impl std::convert::From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        Error::IO(e)
    }
}
impl std::convert::From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Error {
        Error::JSON(e)
    }
}
impl std::convert::From<std::net::AddrParseError> for Error {
    fn from(e: std::net::AddrParseError) -> Error {
        Error::AddrParse(e)
    }
}
impl std::convert::From<std::boxed::Box<bincode::ErrorKind>> for Error {
    fn from(e: std::boxed::Box<bincode::ErrorKind>) -> Error {
        Error::Serialize(e)
    }
}

#[cfg(test)]
mod tests {
    use result::Error;
    use result::Result;
    use serde_json;
    use std::io;
    use std::io::Write;
    use std::net::SocketAddr;
    use std::panic;
    use std::sync::mpsc::RecvError;
    use std::sync::mpsc::RecvTimeoutError;
    use std::sync::mpsc::channel;
    use std::thread;

    fn addr_parse_error() -> Result<SocketAddr> {
        let r = "12fdfasfsafsadfs".parse()?;
        Ok(r)
    }

    fn join_error() -> Result<()> {
        panic::set_hook(Box::new(|_info| {}));
        let r = thread::spawn(|| panic!("hi")).join()?;
        Ok(r)
    }
    fn json_error() -> Result<()> {
        let r = serde_json::from_slice("=342{;;;;:}".as_bytes())?;
        Ok(r)
    }
    fn send_error() -> Result<()> {
        let (s, r) = channel();
        drop(r);
        s.send(())?;
        Ok(())
    }

    #[test]
    fn from_test() {
        assert_matches!(addr_parse_error(), Err(Error::AddrParse(_)));
        assert_matches!(Error::from(RecvError {}), Error::RecvError(_));
        assert_matches!(
            Error::from(RecvTimeoutError::Timeout),
            Error::RecvTimeoutError(_)
        );
        assert_matches!(send_error(), Err(Error::SendError));
        assert_matches!(join_error(), Err(Error::JoinError(_)));
        let ioe = io::Error::new(io::ErrorKind::NotFound, "hi");
        assert_matches!(Error::from(ioe), Error::IO(_));
    }
    #[test]
    fn fmt_test() {
        write!(io::sink(), "{:?}", addr_parse_error()).unwrap();
        write!(io::sink(), "{:?}", Error::from(RecvError {})).unwrap();
        write!(io::sink(), "{:?}", Error::from(RecvTimeoutError::Timeout)).unwrap();
        write!(io::sink(), "{:?}", send_error()).unwrap();
        write!(io::sink(), "{:?}", join_error()).unwrap();
        write!(io::sink(), "{:?}", json_error()).unwrap();
        write!(
            io::sink(),
            "{:?}",
            Error::from(io::Error::new(io::ErrorKind::NotFound, "hi"))
        ).unwrap();
    }
}

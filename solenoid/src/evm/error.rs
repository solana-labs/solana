use std::io;

#[derive(Debug)]
pub enum DisassemblyError {
    IOError(io::Error),
    InvalidHexCharacter,
    TooFewBytesForPush,
}

impl std::cmp::PartialEq for DisassemblyError {
    fn eq(&self, other: &Self) -> bool {
        match other {
            DisassemblyError::IOError(rhs) => {
                if let DisassemblyError::IOError(lhs) = self {
                    rhs.kind() == lhs.kind()
                } else {
                    false
                }
            }
            DisassemblyError::InvalidHexCharacter => {
                if let DisassemblyError::InvalidHexCharacter = other {
                    true
                } else {
                    false
                }
            }
            DisassemblyError::TooFewBytesForPush => {
                if let DisassemblyError::TooFewBytesForPush = other {
                    true
                } else {
                    false
                }
            }
        }
    }
}

impl std::convert::From<io::Error> for DisassemblyError {
    fn from(err: io::Error) -> Self {
        DisassemblyError::IOError(err)
    }
}

impl std::convert::From<hex::FromHexError> for DisassemblyError {
    fn from(_: hex::FromHexError) -> Self {
        DisassemblyError::InvalidHexCharacter
    }
}

impl std::fmt::Display for DisassemblyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IOError(err) => write!(f, "Encountered IO error: {}!", err),
            Self::InvalidHexCharacter => write!(f, "Encountered invalid hex character!"),
            Self::TooFewBytesForPush => {
                write!(f, "Too few bytes availabe to parse push operation!")
            }
        }
    }
}

impl std::error::Error for DisassemblyError {}

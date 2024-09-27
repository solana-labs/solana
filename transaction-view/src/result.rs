#[derive(Debug, PartialEq, Eq)]
#[repr(u8)] // repr(u8) is used to ensure that the enum is represented as a single byte in memory.
pub enum TransactionViewError {
    ParseError,
    SanitizeError,
    AddressLookupMismatch,
}

pub type Result<T> = core::result::Result<T, TransactionViewError>;

#[derive(Debug, PartialEq, Eq)]
pub struct TransactionParsingError;
pub type Result<T> = core::result::Result<T, TransactionParsingError>; // no distinction between errors for now

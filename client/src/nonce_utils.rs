#[derive(Debug, thiserror::Error, PartialEq)]
pub enum Error {
    #[error("invalid account owner")]
    InvalidAccountOwner,
    #[error("invalid account data")]
    InvalidAccountData,
    #[error("unexpected account data size")]
    UnexpectedDataSize,
    #[error("query hash does not match stored hash")]
    InvalidHash,
    #[error("query authority does not match account authority")]
    InvalidAuthority,
    #[error("invalid state for requested operation")]
    InvalidStateForOperation,
    #[error("client error: {0}")]
    Client(String),
}

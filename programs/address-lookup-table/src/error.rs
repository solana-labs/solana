#[cfg(not(target_os = "solana"))]
use solana_program::message::AddressLoaderError;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum AddressLookupError {
    /// Attempted to lookup addresses from a table that does not exist
    #[error("Attempted to lookup addresses from a table that does not exist")]
    LookupTableAccountNotFound,

    /// Attempted to lookup addresses from an account owned by the wrong program
    #[error("Attempted to lookup addresses from an account owned by the wrong program")]
    InvalidAccountOwner,

    /// Attempted to lookup addresses from an invalid account
    #[error("Attempted to lookup addresses from an invalid account")]
    InvalidAccountData,

    /// Address lookup contains an invalid index
    #[error("Address lookup contains an invalid index")]
    InvalidLookupIndex,
}

#[cfg(not(target_os = "solana"))]
impl From<AddressLookupError> for AddressLoaderError {
    fn from(err: AddressLookupError) -> Self {
        match err {
            AddressLookupError::LookupTableAccountNotFound => Self::LookupTableAccountNotFound,
            AddressLookupError::InvalidAccountOwner => Self::InvalidAccountOwner,
            AddressLookupError::InvalidAccountData => Self::InvalidAccountData,
            AddressLookupError::InvalidLookupIndex => Self::InvalidLookupIndex,
        }
    }
}

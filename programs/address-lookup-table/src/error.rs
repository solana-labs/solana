<<<<<<< HEAD
#[cfg(not(target_arch = "bpf"))]
use solana_sdk::transaction::TransactionError;
=======
#[cfg(not(target_os = "solana"))]
use solana_program::message::AddressLoaderError;
>>>>>>> ddf95c181 (RPC: Support versioned txs in getFeeForMessage API (#28217))
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

<<<<<<< HEAD
#[cfg(not(target_arch = "bpf"))]
impl From<AddressLookupError> for TransactionError {
=======
#[cfg(not(target_os = "solana"))]
impl From<AddressLookupError> for AddressLoaderError {
>>>>>>> ddf95c181 (RPC: Support versioned txs in getFeeForMessage API (#28217))
    fn from(err: AddressLookupError) -> Self {
        match err {
            AddressLookupError::LookupTableAccountNotFound => Self::LookupTableAccountNotFound,
            AddressLookupError::InvalidAccountOwner => Self::InvalidAccountOwner,
            AddressLookupError::InvalidAccountData => Self::InvalidAccountData,
            AddressLookupError::InvalidLookupIndex => Self::InvalidLookupIndex,
        }
    }
}

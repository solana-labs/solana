use {
    super::v0::{LoadedAddresses, MessageAddressTableLookup},
    thiserror::Error,
};

#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum AddressLoaderError {
    /// Address loading from lookup tables is disabled
    #[error("Address loading from lookup tables is disabled")]
    Disabled,

    /// Failed to load slot hashes sysvar
    #[error("Failed to load slot hashes sysvar")]
    SlotHashesSysvarNotFound,

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

pub trait AddressLoader: Clone {
    fn load_addresses(
        self,
        lookups: &[MessageAddressTableLookup],
    ) -> Result<LoadedAddresses, AddressLoaderError>;
}

#[derive(Clone)]
pub enum SimpleAddressLoader {
    Disabled,
    Enabled(LoadedAddresses),
}

impl AddressLoader for SimpleAddressLoader {
    fn load_addresses(
        self,
        _lookups: &[MessageAddressTableLookup],
    ) -> Result<LoadedAddresses, AddressLoaderError> {
        match self {
            Self::Disabled => Err(AddressLoaderError::Disabled),
            Self::Enabled(loaded_addresses) => Ok(loaded_addresses),
        }
    }
}

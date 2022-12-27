//! The Epoch Accounts Hash (EAH) is a special hash of the whole accounts state that occurs once
//! per epoch.
//!
//! This hash is special because all nodes in the cluster will calculate the accounts hash at a
//! predetermined slot in the epoch and then save that result into a later Bank at a predetermined
//! slot.
//!
//! This results in all nodes effectively voting on the accounts state (at least) once per epoch.

use {crate::accounts_hash::AccountsHash, solana_sdk::hash::Hash};

mod utils;
pub use utils::*;

mod manager;
pub use manager::Manager as EpochAccountsHashManager;

/// The EpochAccountsHash holds the result after calculating the accounts hash once per epoch
#[derive(Debug, Serialize, Deserialize, Hash, PartialEq, Eq, Clone, Copy)]
pub struct EpochAccountsHash(Hash);

impl AsRef<Hash> for EpochAccountsHash {
    fn as_ref(&self) -> &Hash {
        &self.0
    }
}

impl EpochAccountsHash {
    /// Make an EpochAccountsHash from a regular accounts hash
    #[must_use]
    pub const fn new(accounts_hash: Hash) -> Self {
        Self(accounts_hash)
    }
}

impl From<AccountsHash> for EpochAccountsHash {
    fn from(accounts_hash: AccountsHash) -> EpochAccountsHash {
        Self::new(accounts_hash.0)
    }
}

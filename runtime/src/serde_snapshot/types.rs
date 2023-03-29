//! Types used by snapshots for safe serialization/deserialization
use {
    crate::accounts_hash::{AccountsDeltaHash, AccountsHash, IncrementalAccountsHash},
    serde::{Deserialize, Serialize},
    solana_sdk::hash::Hash,
};

/// Snapshot serde-safe accounts delta hash
#[derive(Clone, Default, Debug, Serialize, Deserialize, PartialEq, Eq, AbiExample)]
pub struct SerdeAccountsDeltaHash(pub Hash);

impl From<SerdeAccountsDeltaHash> for AccountsDeltaHash {
    fn from(accounts_delta_hash: SerdeAccountsDeltaHash) -> Self {
        Self(accounts_delta_hash.0)
    }
}
impl From<AccountsDeltaHash> for SerdeAccountsDeltaHash {
    fn from(accounts_delta_hash: AccountsDeltaHash) -> Self {
        Self(accounts_delta_hash.0)
    }
}

/// Snapshot serde-safe accounts hash
#[derive(Clone, Default, Debug, Serialize, Deserialize, PartialEq, Eq, AbiExample)]
pub struct SerdeAccountsHash(pub Hash);

impl From<SerdeAccountsHash> for AccountsHash {
    fn from(accounts_hash: SerdeAccountsHash) -> Self {
        Self(accounts_hash.0)
    }
}
impl From<AccountsHash> for SerdeAccountsHash {
    fn from(accounts_hash: AccountsHash) -> Self {
        Self(accounts_hash.0)
    }
}

/// Snapshot serde-safe incremental accounts hash
#[derive(Clone, Default, Debug, Serialize, Deserialize, PartialEq, Eq, AbiExample)]
pub struct SerdeIncrementalAccountsHash(pub Hash);

impl From<SerdeIncrementalAccountsHash> for IncrementalAccountsHash {
    fn from(incremental_accounts_hash: SerdeIncrementalAccountsHash) -> Self {
        Self(incremental_accounts_hash.0)
    }
}
impl From<IncrementalAccountsHash> for SerdeIncrementalAccountsHash {
    fn from(incremental_accounts_hash: IncrementalAccountsHash) -> Self {
        Self(incremental_accounts_hash.0)
    }
}

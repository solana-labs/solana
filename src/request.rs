//! The `request` module defines the messages for the thin client.

use hash::Hash;
use signature::{PublicKey, Signature};

#[cfg_attr(feature = "cargo-clippy", allow(large_enum_variant))]
#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum Request {
    GetBalance { key: PublicKey },
    GetLastId,
    GetTransactionCount,
    GetPubKeyVersion { pubkey: PublicKey },
}

impl Request {
    /// Verify the request is valid.
    pub fn verify(&self) -> bool {
        true
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Response {
    Balance { key: PublicKey, val: i64 },
    LastId { id: Hash },
    TransactionCount { transaction_count: u64 },
    PubKeyVersion { version: u64, signature: Signature },
}

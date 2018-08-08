//! The `request` module defines the messages for the thin client.

use signature::PublicKey;
use transaction::{Height, LastId};

#[cfg_attr(feature = "cargo-clippy", allow(large_enum_variant))]
#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum Request {
    GetBalance { key: PublicKey },
    GetLastId,
    GetTransactionCount,
}

impl Request {
    /// Verify the request is valid.
    pub fn verify(&self) -> bool {
        true
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Response {
    Balance {
        key: PublicKey,
        val: i64,
        height: Height,
    },
    LastId {
        id: LastId,
    },
    TransactionCount {
        transaction_count: u64,
    },
}

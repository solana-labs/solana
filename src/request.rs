//! The `request` module defines the messages for the thin client.

use bs58;
use hash::Hash;
use signature::{PublicKey, Signature};
use std::fmt;

#[cfg_attr(feature = "cargo-clippy", allow(large_enum_variant))]
#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum Request {
    GetBalance { key: PublicKey },
    GetLastId,
    GetTransactionCount,
    GetSignature { signature: Signature },
}

impl Request {
    /// Verify the request is valid.
    pub fn verify(&self) -> bool {
        true
    }
}

#[derive(Serialize, Deserialize)]
pub enum Response {
    Balance { key: PublicKey, val: i64 },
    LastId { id: Hash },
    TransactionCount { transaction_count: u64 },
    SignatureStatus { signature_status: bool },
}

// Custom formatter to display base58 public keys and hashes
impl fmt::Debug for Response {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Response::Balance { key, val } => write!(
                f,
                "Balance {{ key: {}, val: {} }}",
                bs58::encode(key).into_string(),
                val
            ),
            Response::LastId { id } => {
                write!(f, "LastId {{ id: {} }}", bs58::encode(id).into_string())
            }
            Response::TransactionCount { transaction_count } => write!(
                f,
                "TransactionCount {{ transaction_count: {} }}",
                transaction_count
            ),
            Response::SignatureStatus { signature_status } => write!(
                f,
                "SignatureStatus {{ signature_status: {} }}",
                signature_status
            ),
        }
    }
}

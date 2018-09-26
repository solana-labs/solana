//! The `request` module defines the messages for the thin client.

use account::Account;
use hash::Hash;
use pubkey::Pubkey;
use signature::Signature;

#[cfg_attr(feature = "cargo-clippy", allow(large_enum_variant))]
#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum Request {
    GetAccount { key: Pubkey },
    GetLastId,
    GetTransactionCount,
    GetSignature { signature: Signature },
    GetFinality,
}

impl Request {
    /// Verify the request is valid.
    pub fn verify(&self) -> bool {
        true
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Response {
    Account {
        key: Pubkey,
        account: Option<Account>,
    },
    LastId {
        id: Hash,
    },
    TransactionCount {
        transaction_count: u64,
    },
    SignatureStatus {
        signature_status: bool,
    },
    Finality {
        time: usize,
    },
}

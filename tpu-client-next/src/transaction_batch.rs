//! This module holds [`TransactionBatch`] structure.

use solana_sdk::timing::timestamp;

/// Batch of generated transactions timestamp is used to discard batches which
/// are too old to have valid blockhash.
#[derive(Clone, PartialEq)]
pub struct TransactionBatch {
    wired_transactions: Vec<WiredTransaction>,
    timestamp: u64,
}

type WiredTransaction = Vec<u8>;

impl IntoIterator for TransactionBatch {
    type Item = Vec<u8>;
    type IntoIter = std::vec::IntoIter<Self::Item>;
    fn into_iter(self) -> Self::IntoIter {
        self.wired_transactions.into_iter()
    }
}

impl TransactionBatch {
    pub fn new(wired_transactions: Vec<WiredTransaction>) -> Self {
        Self {
            wired_transactions,
            timestamp: timestamp(),
        }
    }
    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }
}

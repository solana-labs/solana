use {
    crate::immutable_deserialized_packet::ImmutableDeserializedPacket,
    solana_sdk::transaction::SanitizedTransaction, std::sync::Arc,
};

/// A unique identifier for a transaction batch.
/// The lower 64 bits are based on the order the transaction batch was scheduled.
/// The upper 64 bits are an optional priority field - currently unused.
#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct TransactionBatchId(u128);

impl TransactionBatchId {
    pub fn new(index: u64) -> Self {
        Self(index as u128)
    }
}

/// A unique identifier for a transaction.
/// The lower 64 bits are based on the order the transaction entered banking stage.
/// The upper 64 bits are the priority of the transaction.
#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct TransactionId(u128);

impl TransactionId {
    pub fn new(priority: u64, index: u64) -> Self {
        Self(((priority as u128) << 64) | (index as u128))
    }
}

/// Message: [Scheduler -> Worker]
/// Transactions to be consumed (executed, recorded, committed)
pub struct ConsumeWork {
    pub batch_id: TransactionBatchId,
    pub transaction_ids: Vec<TransactionId>,
    pub transactions: Vec<SanitizedTransaction>,
}

/// Message: [Worker -> Scheduler]
/// Transactions to be forwarded to the next leader(s)
pub struct ForwardWork {
    pub ids: Vec<TransactionId>,
    pub packets: Vec<Arc<ImmutableDeserializedPacket>>,
}

/// Message: [Worker -> Scheduler]
/// Processed transactions.
pub struct FinishedConsumeWork {
    pub work: ConsumeWork,
    pub retryable_indexes: Vec<usize>,
}

/// Message: [Worker -> Scheduler]
/// Forwarded transactions.
pub struct FinishedForwardWork {
    pub work: ForwardWork,
    pub successful: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_id_ordering() {
        let id1 = TransactionId::new(0, 0);
        let id2 = TransactionId::new(0, 1);
        let id3 = TransactionId::new(1, 0);
        let id4 = TransactionId::new(1, 1);
        assert!(id1 < id2);
        assert!(id1 < id3);
        assert!(id1 < id4);
        assert!(id2 < id3);
        assert!(id2 < id4);
        assert!(id3 < id4);
    }
}

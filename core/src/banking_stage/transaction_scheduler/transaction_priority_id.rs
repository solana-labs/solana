use crate::banking_stage::scheduler_messages::TransactionId;

/// A unique identifier tied with priority ordering for a transaction/packet:
///     - `id` has no effect on ordering
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct TransactionPriorityId {
    pub(crate) priority: u64,
    pub(crate) id: TransactionId,
}

impl TransactionPriorityId {
    pub(crate) fn new(priority: u64, id: TransactionId) -> Self {
        Self { priority, id }
    }
}

impl Ord for TransactionPriorityId {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.priority.cmp(&other.priority)
    }
}

impl PartialOrd for TransactionPriorityId {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

use {
    crate::banking_stage::scheduler_messages::TransactionId,
    prio_graph::TopLevelId,
    std::hash::{Hash, Hasher},
};

/// A unique identifier tied with priority ordering for a transaction/packet:
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TransactionPriorityId {
    pub(crate) priority: u64,
    pub(crate) id: TransactionId,
}

impl TransactionPriorityId {
    pub(crate) fn new(priority: u64, id: TransactionId) -> Self {
        Self { priority, id }
    }
}

impl Hash for TransactionPriorityId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state)
    }
}

impl TopLevelId<Self> for TransactionPriorityId {
    fn id(&self) -> Self {
        *self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_priority_id_ordering() {
        // Higher priority first
        {
            let id1 = TransactionPriorityId::new(1, TransactionId::new(1));
            let id2 = TransactionPriorityId::new(2, TransactionId::new(1));
            assert!(id1 < id2);
            assert!(id1 <= id2);
            assert!(id2 > id1);
            assert!(id2 >= id1);
        }

        // Equal priority then compare by id
        {
            let id1 = TransactionPriorityId::new(1, TransactionId::new(1));
            let id2 = TransactionPriorityId::new(1, TransactionId::new(2));
            assert!(id1 < id2);
            assert!(id1 <= id2);
            assert!(id2 > id1);
            assert!(id2 >= id1);
        }

        // Equal priority and id
        {
            let id1 = TransactionPriorityId::new(1, TransactionId::new(1));
            let id2 = TransactionPriorityId::new(1, TransactionId::new(1));
            assert_eq!(id1, id2);
            assert!(id1 >= id2);
            assert!(id1 <= id2);
            assert!(id2 >= id1);
            assert!(id2 <= id1);
        }
    }
}

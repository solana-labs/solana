use {
    super::transaction_priority_id::TransactionPriorityId,
    crate::banking_stage::scheduler_messages::TransactionId,
    min_max_heap::MinMaxHeap,
    solana_runtime::transaction_priority_details::TransactionPriorityDetails,
    solana_sdk::{slot_history::Slot, transaction::SanitizedTransaction},
    std::collections::HashMap,
};

#[allow(clippy::large_enum_variant)]
pub(crate) enum TransactionState {
    Unprocessed {
        transaction_ttl: SanitizedTransactionTTL,
        transaction_priority_details: TransactionPriorityDetails,
        forwarded: bool,
    },
    Pending {
        transaction_priority_details: TransactionPriorityDetails,
        forwarded: bool,
    },
}

impl TransactionState {
    fn new(
        transaction_ttl: SanitizedTransactionTTL,
        transaction_priority_details: TransactionPriorityDetails,
        forwarded: bool,
    ) -> Self {
        Self::Unprocessed {
            transaction_ttl,
            transaction_priority_details,
            forwarded,
        }
    }

    pub(crate) fn transaction_priority_details(&self) -> &TransactionPriorityDetails {
        match self {
            Self::Unprocessed {
                transaction_priority_details,
                ..
            } => transaction_priority_details,
            Self::Pending {
                transaction_priority_details,
                ..
            } => transaction_priority_details,
        }
    }

    pub(crate) fn priority(&self) -> u64 {
        self.transaction_priority_details().priority
    }

    pub(crate) fn forwarded(&self) -> bool {
        match self {
            Self::Unprocessed { forwarded, .. } => *forwarded,
            Self::Pending { forwarded, .. } => *forwarded,
        }
    }

    pub(crate) fn set_forwarded(&mut self) {
        match self {
            Self::Unprocessed { forwarded, .. } => *forwarded = true,
            Self::Pending { forwarded, .. } => *forwarded = true,
        }
    }

    fn transition_to_pending(&mut self) -> SanitizedTransactionTTL {
        match self.take() {
            TransactionState::Unprocessed {
                transaction_ttl,
                transaction_priority_details,
                forwarded,
            } => {
                *self = TransactionState::Pending {
                    transaction_priority_details,
                    forwarded,
                };
                transaction_ttl
            }
            TransactionState::Pending { .. } => {
                panic!("transaction already pending");
            }
        }
    }

    fn transition_to_unprocessed(&mut self, transaction_ttl: SanitizedTransactionTTL) {
        match self.take() {
            TransactionState::Unprocessed { .. } => panic!("already unprocessed"),
            TransactionState::Pending {
                transaction_priority_details,
                forwarded,
            } => {
                *self = Self::Unprocessed {
                    transaction_ttl,
                    transaction_priority_details,
                    forwarded,
                }
            }
        }
    }

    pub(crate) fn transaction_ttl(&self) -> &SanitizedTransactionTTL {
        match self {
            Self::Unprocessed {
                transaction_ttl, ..
            } => transaction_ttl,
            Self::Pending { .. } => panic!("transaction is pending"),
        }
    }

    /// Internal helper to transitioning between states.
    /// Replaces `self` with a dummy state that will immediately be overwritten in transition.
    fn take(&mut self) -> Self {
        core::mem::replace(
            self,
            Self::Pending {
                transaction_priority_details: TransactionPriorityDetails {
                    priority: 0,
                    compute_unit_limit: 0,
                },
                forwarded: false,
            },
        )
    }
}

pub(crate) struct SanitizedTransactionTTL {
    pub(crate) transaction: SanitizedTransaction,
    pub(crate) max_age_slot: Slot,
}

pub(crate) struct TransactionPacketContainer {
    priority_queue: MinMaxHeap<TransactionPriorityId>,
    id_to_transaction_state: HashMap<TransactionId, TransactionState>,
}

impl TransactionPacketContainer {
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            priority_queue: MinMaxHeap::with_capacity(capacity),
            id_to_transaction_state: HashMap::with_capacity(capacity),
        }
    }

    /// Returns true if the queue is empty.
    pub(crate) fn is_empty(&self) -> bool {
        self.priority_queue.is_empty()
    }

    /// Returns the remaining capacity of the queue
    pub(crate) fn remaining_queue_capacity(&self) -> usize {
        self.priority_queue.capacity() - self.priority_queue.len()
    }

    /// Get a non-consuming iterator over the top `n` transactions in the queue.
    pub(crate) fn take_top_n(
        &mut self,
        n: usize,
    ) -> impl Iterator<Item = TransactionPriorityId> + '_ {
        (0..n).map_while(|_| self.priority_queue.pop_max())
    }

    /// Serialize entire priority queue. `hold` indicates whether the priority queue should
    /// be drained or not.
    pub(crate) fn priority_ordered_ids(&mut self, hold: bool) -> Vec<TransactionPriorityId> {
        let priority_queue = if hold {
            self.priority_queue.clone()
        } else {
            let capacity = self.priority_queue.capacity();
            core::mem::replace(
                &mut self.priority_queue,
                MinMaxHeap::with_capacity(capacity),
            )
        };

        priority_queue.into_vec_desc()
    }

    /// Get transaction state by id.
    pub(crate) fn get_mut_transaction_state(
        &mut self,
        id: &TransactionId,
    ) -> Option<&mut TransactionState> {
        self.id_to_transaction_state.get_mut(id)
    }

    /// Get reference to `SanitizedTransactionTTL` by id.
    /// Panics if the transaction does not exist.
    pub(crate) fn get_transaction_ttl(
        &self,
        id: &TransactionId,
    ) -> Option<&SanitizedTransactionTTL> {
        self.id_to_transaction_state
            .get(id)
            .map(|state| state.transaction_ttl())
    }

    /// Take transaction by id.
    /// Panics if the transaction does not exist.
    pub(crate) fn take_transaction(&mut self, id: &TransactionId) -> SanitizedTransactionTTL {
        self.id_to_transaction_state
            .get_mut(id)
            .expect("transaction must exist")
            .transition_to_pending()
    }

    /// Insert a new transaction into the container's queues and maps.
    pub(crate) fn insert_new_transaction(
        &mut self,
        transaction_id: TransactionId,
        transaction_ttl: SanitizedTransactionTTL,
        transaction_priority_details: TransactionPriorityDetails,
    ) {
        let priority_id =
            TransactionPriorityId::new(transaction_priority_details.priority, transaction_id);
        if self.push_id_into_queue(priority_id) {
            self.id_to_transaction_state.insert(
                transaction_id,
                TransactionState::new(transaction_ttl, transaction_priority_details, false),
            );
        }
    }

    /// Retries a transaction - inserts transaction back into map (but not packet).
    pub(crate) fn retry_transaction(
        &mut self,
        transaction_id: TransactionId,
        transaction_ttl: SanitizedTransactionTTL,
    ) {
        let transaction_state = self
            .get_mut_transaction_state(&transaction_id)
            .expect("transaction must exist");
        let priority_id = TransactionPriorityId::new(transaction_state.priority(), transaction_id);
        transaction_state.transition_to_unprocessed(transaction_ttl);
        self.push_id_into_queue(priority_id);
    }

    /// Pushes a transaction id into the priority queue, without inserting the packet or transaction.
    /// Returns true if the id was successfully pushed into the priority queue
    pub(crate) fn push_id_into_queue(&mut self, priority_id: TransactionPriorityId) -> bool {
        if self.priority_queue.len() == self.priority_queue.capacity() {
            let popped_id = self.priority_queue.push_pop_min(priority_id);
            if popped_id == priority_id {
                return false;
            } else {
                self.remove_by_id(&popped_id.id);
            }
        } else {
            self.priority_queue.push(priority_id);
        }

        true
    }

    /// Remove packet and transaction by id.
    pub(crate) fn remove_by_id(&mut self, id: &TransactionId) {
        self.id_to_transaction_state.remove(id);
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::{
            compute_budget::ComputeBudgetInstruction, hash::Hash, message::Message,
            signature::Keypair, signer::Signer, system_instruction, transaction::Transaction,
        },
    };

    fn test_transaction(
        priority: u64,
    ) -> (SanitizedTransactionTTL, TransactionPriorityDetails) {
        let from_keypair = Keypair::new();
        let ixs = vec![
            system_instruction::transfer(
                &from_keypair.pubkey(),
                &solana_sdk::pubkey::new_rand(),
                1,
            ),
            ComputeBudgetInstruction::set_compute_unit_price(priority),
        ];
        let message = Message::new(&ixs, Some(&from_keypair.pubkey()));
        let tx = Transaction::new(&[&from_keypair], message, Hash::default());

        let transaction_ttl = SanitizedTransactionTTL {
            transaction: SanitizedTransaction::from_transaction_for_tests(tx),
            max_age_slot: Slot::MAX,
        };
        (transaction_ttl, TransactionPriorityDetails { priority, compute_unit_limit: 0 })
    }

    fn push_to_container(container: &mut TransactionPacketContainer, num: usize) {
        for id in 0..num as u64 {
            let priority = id;
            let (transaction_ttl, transaction_priority_details) = test_transaction(priority);
            container.insert_new_transaction(
                TransactionId::new(id),
                transaction_ttl,
                transaction_priority_details,
            );
        }
    }

    #[test]
    fn test_is_empty() {
        let mut container = TransactionPacketContainer::with_capacity(1);
        assert!(container.is_empty());

        push_to_container(&mut container, 1);
        assert!(!container.is_empty());
    }

    #[test]
    fn test_priority_queue_capacity() {
        let mut container = TransactionPacketContainer::with_capacity(1);
        push_to_container(&mut container, 5);

        assert_eq!(container.priority_queue.len(), 1);
        assert_eq!(container.id_to_transaction_state.len(), 1);
        assert_eq!(
            container
                .id_to_transaction_state
                .iter()
                .map(|ts| ts.1.priority())
                .next()
                .unwrap(),
            4
        );
    }

    #[test]
    fn test_take_top_n() {
        let mut container = TransactionPacketContainer::with_capacity(5);
        push_to_container(&mut container, 5);

        let taken = container.take_top_n(3).collect::<Vec<_>>();
        assert_eq!(
            taken,
            vec![
                TransactionPriorityId::new(4, TransactionId::new(4)),
                TransactionPriorityId::new(3, TransactionId::new(3)),
                TransactionPriorityId::new(2, TransactionId::new(2)),
            ]
        );
        assert_eq!(container.priority_queue.len(), 2);
    }

    #[test]
    fn test_remove_by_id() {
        let mut container = TransactionPacketContainer::with_capacity(5);
        push_to_container(&mut container, 5);

        container.remove_by_id(&TransactionId::new(3));
        assert_eq!(container.priority_queue.len(), 5); // remove_by_id does not remove from priority queue
        assert_eq!(container.id_to_transaction_state.len(), 4);

        container.remove_by_id(&TransactionId::new(7));
        assert_eq!(container.id_to_transaction_state.len(), 4);
    }

    #[test]
    fn test_push_id_into_queue() {
        let mut container = TransactionPacketContainer::with_capacity(1);
        assert!(container.push_id_into_queue(TransactionPriorityId::new(1, TransactionId::new(0))));
        assert_eq!(container.priority_queue.len(), 1);
        assert_eq!(container.id_to_transaction_state.len(), 0);

        assert!(container.push_id_into_queue(TransactionPriorityId::new(1, TransactionId::new(1))));
        assert_eq!(container.priority_queue.len(), 1);
        // should be dropped due to capacity
        assert!(!container.push_id_into_queue(TransactionPriorityId::new(0, TransactionId::new(2))));
        assert_eq!(container.priority_queue.len(), 1);
    }

    #[test]
    fn test_get_packet_entry_missing() {
        let mut container = TransactionPacketContainer::with_capacity(5);
        push_to_container(&mut container, 5);
        assert!(container
            .get_mut_transaction_state(&TransactionId::new(7))
            .is_none());
    }

    #[test]
    fn test_get_packet_entry() {
        let mut container = TransactionPacketContainer::with_capacity(5);
        push_to_container(&mut container, 5);
        assert!(container
            .get_mut_transaction_state(&TransactionId::new(3))
            .is_some());
    }

    #[test]
    fn test_get_transaction() {
        let mut container = TransactionPacketContainer::with_capacity(5);
        push_to_container(&mut container, 5);

        let existing_id = TransactionId::new(3);
        let non_existing_id = TransactionId::new(7);
        assert!(container.get_mut_transaction_state(&existing_id).is_some());
        assert!(container.get_mut_transaction_state(&existing_id).is_some());
        assert!(container.get_mut_transaction_state(&non_existing_id).is_none());
    }
}

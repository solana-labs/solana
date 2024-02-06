use {
    super::{
        transaction_priority_id::TransactionPriorityId,
        transaction_state::{SanitizedTransactionTTL, TransactionState},
    },
    crate::banking_stage::scheduler_messages::TransactionId,
    itertools::MinMaxResult,
    min_max_heap::MinMaxHeap,
    std::collections::HashMap,
};

/// This structure will hold `TransactionState` for the entirety of a
/// transaction's lifetime in the scheduler and BankingStage as a whole.
///
/// Transaction Lifetime:
/// 1. Received from `SigVerify` by `BankingStage`
/// 2. Inserted into `TransactionStateContainer` by `BankingStage`
/// 3. Popped in priority-order by scheduler, and transitioned to `Pending` state
/// 4. Processed by `ConsumeWorker`
///   a. If consumed, remove `Pending` state from the `TransactionStateContainer`
///   b. If retryable, transition back to `Unprocessed` state.
///      Re-insert to the queue, and return to step 3.
///
/// The structure is composed of two main components:
/// 1. A priority queue of wrapped `TransactionId`s, which are used to
///    order transactions by priority for selection by the scheduler.
/// 2. A map of `TransactionId` to `TransactionState`, which is used to
///    track the state of each transaction.
///
/// When `Pending`, the associated `TransactionId` is not in the queue, but
/// is still in the map.
/// The entry in the map should exist before insertion into the queue, and be
/// be removed only after the id is removed from the queue.
///
/// The container maintains a fixed capacity. If the queue is full when pushing
/// a new transaction, the lowest priority transaction will be dropped.
pub(crate) struct TransactionStateContainer {
    priority_queue: MinMaxHeap<TransactionPriorityId>,
    id_to_transaction_state: HashMap<TransactionId, TransactionState>,
}

impl TransactionStateContainer {
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

    /// Get the top transaction id in the priority queue.
    pub(crate) fn pop(&mut self) -> Option<TransactionPriorityId> {
        self.priority_queue.pop_max()
    }

    /// Get mutable transaction state by id.
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

    /// Take `SanitizedTransactionTTL` by id.
    /// This transitions the transaction to `Pending` state.
    /// Panics if the transaction does not exist.
    pub(crate) fn take_transaction(&mut self, id: &TransactionId) -> SanitizedTransactionTTL {
        self.id_to_transaction_state
            .get_mut(id)
            .expect("transaction must exist")
            .transition_to_pending()
    }

    /// Insert a new transaction into the container's queues and maps.
    /// Returns `true` if a packet was dropped due to capacity limits.
    pub(crate) fn insert_new_transaction(
        &mut self,
        transaction_id: TransactionId,
        transaction_ttl: SanitizedTransactionTTL,
        priority: u64,
        cost: u64,
    ) -> bool {
        let priority_id = TransactionPriorityId::new(priority, transaction_id);
        self.id_to_transaction_state.insert(
            transaction_id,
            TransactionState::new(transaction_ttl, priority, cost),
        );
        self.push_id_into_queue(priority_id)
    }

    /// Retries a transaction - inserts transaction back into map (but not packet).
    /// This transitions the transaction to `Unprocessed` state.
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

    /// Pushes a transaction id into the priority queue. If the queue is full, the lowest priority
    /// transaction will be dropped (removed from the queue and map).
    /// Returns `true` if a packet was dropped due to capacity limits.
    pub(crate) fn push_id_into_queue(&mut self, priority_id: TransactionPriorityId) -> bool {
        if self.remaining_queue_capacity() == 0 {
            let popped_id = self.priority_queue.push_pop_min(priority_id);
            self.remove_by_id(&popped_id.id);
            true
        } else {
            self.priority_queue.push(priority_id);
            false
        }
    }

    /// Remove transaction by id.
    pub(crate) fn remove_by_id(&mut self, id: &TransactionId) {
        self.id_to_transaction_state
            .remove(id)
            .expect("transaction must exist");
    }

    pub(crate) fn get_min_max_priority(&self) -> MinMaxResult<u64> {
        match self.priority_queue.peek_min() {
            Some(min) => match self.priority_queue.peek_max() {
                Some(max) => MinMaxResult::MinMax(min.priority, max.priority),
                None => MinMaxResult::OneElement(min.priority),
            },
            None => MinMaxResult::NoElements,
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::{
            compute_budget::ComputeBudgetInstruction,
            hash::Hash,
            message::Message,
            signature::Keypair,
            signer::Signer,
            slot_history::Slot,
            system_instruction,
            transaction::{SanitizedTransaction, Transaction},
        },
    };

    /// Returns (transaction_ttl, priority, cost)
    fn test_transaction(priority: u64) -> (SanitizedTransactionTTL, u64, u64) {
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
        let tx = SanitizedTransaction::from_transaction_for_tests(Transaction::new(
            &[&from_keypair],
            message,
            Hash::default(),
        ));
        let transaction_ttl = SanitizedTransactionTTL {
            transaction: tx,
            max_age_slot: Slot::MAX,
        };
        const TEST_TRANSACTION_COST: u64 = 5000;
        (transaction_ttl, priority, TEST_TRANSACTION_COST)
    }

    fn push_to_container(container: &mut TransactionStateContainer, num: usize) {
        for id in 0..num as u64 {
            let priority = id;
            let (transaction_ttl, priority, cost) = test_transaction(priority);
            container.insert_new_transaction(
                TransactionId::new(id),
                transaction_ttl,
                priority,
                cost,
            );
        }
    }

    #[test]
    fn test_is_empty() {
        let mut container = TransactionStateContainer::with_capacity(1);
        assert!(container.is_empty());

        push_to_container(&mut container, 1);
        assert!(!container.is_empty());
    }

    #[test]
    fn test_priority_queue_capacity() {
        let mut container = TransactionStateContainer::with_capacity(1);
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
    fn test_get_mut_transaction_state() {
        let mut container = TransactionStateContainer::with_capacity(5);
        push_to_container(&mut container, 5);

        let existing_id = TransactionId::new(3);
        let non_existing_id = TransactionId::new(7);
        assert!(container.get_mut_transaction_state(&existing_id).is_some());
        assert!(container.get_mut_transaction_state(&existing_id).is_some());
        assert!(container
            .get_mut_transaction_state(&non_existing_id)
            .is_none());
    }
}

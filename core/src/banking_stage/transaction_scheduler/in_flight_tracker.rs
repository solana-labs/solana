use {
    super::thread_aware_account_locks::ThreadId,
    crate::banking_stage::scheduler_messages::TransactionBatchId, std::collections::HashMap,
};

/// Tracks the number of transactions that are in flight for each thread.
pub struct InFlightTracker {
    num_in_flight_per_thread: Vec<usize>,
    batches: HashMap<TransactionBatchId, BatchEntry>,
}

struct BatchEntry {
    thread_id: ThreadId,
    num_transactions: usize,
}

impl InFlightTracker {
    pub fn new(num_threads: usize) -> Self {
        Self {
            num_in_flight_per_thread: vec![0; num_threads],
            batches: HashMap::new(),
        }
    }

    /// Returns the number of transactions that are in flight for each thread.
    pub fn num_in_flight_per_thread(&self) -> &[usize] {
        &self.num_in_flight_per_thread
    }

    /// Add a new batch with given `batch_id` and `num_transactions` to the
    /// thread with given `thread_id`.
    pub fn track_batch(
        &mut self,
        batch_id: TransactionBatchId,
        num_transactions: usize,
        thread_id: ThreadId,
    ) {
        self.num_in_flight_per_thread[thread_id] += num_transactions;
        assert!(
            self.batches
                .insert(
                    batch_id,
                    BatchEntry {
                        thread_id,
                        num_transactions,
                    }
                )
                .is_none(),
            "batch id {batch_id} is already being tracked"
        );
    }

    /// Stop tracking the batch with given `batch_id`.
    /// Removes the number of transactions for the scheduled thread.
    /// Returns the thread id that the batch was scheduled on.
    ///
    /// # Panics
    /// Panics if the batch id does not exist in the tracker.
    pub fn complete_batch(&mut self, batch_id: TransactionBatchId) -> ThreadId {
        let BatchEntry {
            thread_id,
            num_transactions,
        } = self
            .batches
            .remove(&batch_id)
            .expect("batch id {batch} is not being tracked");
        self.num_in_flight_per_thread[thread_id] -= num_transactions;

        thread_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic(expected = "is already being tracked")]
    fn test_in_flight_tracker_duplicate_batch() {
        let mut in_flight_tracker = InFlightTracker::new(2);

        let batch_id = TransactionBatchId::new(0);
        let batch_num_transactions = 2;

        in_flight_tracker.track_batch(batch_id, batch_num_transactions, 0);
        in_flight_tracker.track_batch(batch_id, batch_num_transactions, 1);
    }

    #[test]
    #[should_panic(expected = "is not being tracked")]
    fn test_in_flight_tracker_untracked_batch() {
        let mut in_flight_tracker = InFlightTracker::new(2);
        in_flight_tracker.complete_batch(TransactionBatchId::new(5));
    }

    #[test]
    fn test_in_flight_tracker() {
        let mut in_flight_tracker = InFlightTracker::new(2);

        let batch_id_0 = TransactionBatchId::new(0);
        let batch_id_1 = TransactionBatchId::new(1);

        // Add a batch with 2 transactions to thread 0.
        in_flight_tracker.track_batch(batch_id_0, 2, 0);
        assert_eq!(in_flight_tracker.num_in_flight_per_thread(), &[2, 0]);

        // Add a batch with 1 transaction to thread 1.
        in_flight_tracker.track_batch(batch_id_1, 1, 1);
        assert_eq!(in_flight_tracker.num_in_flight_per_thread(), &[2, 1]);

        // Complete the batch with 2 transactions on thread 0.
        in_flight_tracker.complete_batch(batch_id_0);
        assert_eq!(in_flight_tracker.num_in_flight_per_thread(), &[0, 1]);

        // Complete the batch with 1 transaction on thread 1.
        in_flight_tracker.complete_batch(batch_id_1);
        assert_eq!(in_flight_tracker.num_in_flight_per_thread(), &[0, 0]);
    }
}

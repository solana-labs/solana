use {
    super::{batch_id_generator::BatchIdGenerator, thread_aware_account_locks::ThreadId},
    crate::banking_stage::scheduler_messages::TransactionBatchId,
    std::collections::HashMap,
};

/// Tracks the number of transactions that are in flight for each thread.
pub struct InFlightTracker {
    num_in_flight_per_thread: Vec<usize>,
    cus_in_flight_per_thread: Vec<u64>,
    batches: HashMap<TransactionBatchId, BatchEntry>,
    batch_id_generator: BatchIdGenerator,
}

struct BatchEntry {
    thread_id: ThreadId,
    num_transactions: usize,
    total_cus: u64,
}

impl InFlightTracker {
    pub fn new(num_threads: usize) -> Self {
        Self {
            num_in_flight_per_thread: vec![0; num_threads],
            cus_in_flight_per_thread: vec![0; num_threads],
            batches: HashMap::new(),
            batch_id_generator: BatchIdGenerator::default(),
        }
    }

    /// Returns the number of transactions that are in flight for each thread.
    pub fn num_in_flight_per_thread(&self) -> &[usize] {
        &self.num_in_flight_per_thread
    }

    /// Returns the number of cus that are in flight for each thread.
    pub fn cus_in_flight_per_thread(&self) -> &[u64] {
        &self.cus_in_flight_per_thread
    }

    /// Tracks number of transactions and CUs in-flight for the `thread_id`.
    /// Returns a `TransactionBatchId` that can be used to stop tracking the batch
    /// when it is complete.
    pub fn track_batch(
        &mut self,
        num_transactions: usize,
        total_cus: u64,
        thread_id: ThreadId,
    ) -> TransactionBatchId {
        let batch_id = self.batch_id_generator.next();
        self.num_in_flight_per_thread[thread_id] += num_transactions;
        self.cus_in_flight_per_thread[thread_id] += total_cus;
        self.batches.insert(
            batch_id,
            BatchEntry {
                thread_id,
                num_transactions,
                total_cus,
            },
        );

        batch_id
    }

    /// Stop tracking the batch with given `batch_id`.
    /// Removes the number of transactions for the scheduled thread.
    /// Returns the thread id that the batch was scheduled on.
    ///
    /// # Panics
    /// Panics if the batch id does not exist in the tracker.
    pub fn complete_batch(&mut self, batch_id: TransactionBatchId) -> ThreadId {
        let Some(BatchEntry {
            thread_id,
            num_transactions,
            total_cus,
        }) = self.batches.remove(&batch_id)
        else {
            panic!("batch id {batch_id} is not being tracked");
        };
        self.num_in_flight_per_thread[thread_id] -= num_transactions;
        self.cus_in_flight_per_thread[thread_id] -= total_cus;

        thread_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic(expected = "is not being tracked")]
    fn test_in_flight_tracker_untracked_batch() {
        let mut in_flight_tracker = InFlightTracker::new(2);
        in_flight_tracker.complete_batch(TransactionBatchId::new(5));
    }

    #[test]
    fn test_in_flight_tracker() {
        let mut in_flight_tracker = InFlightTracker::new(2);

        // Add a batch with 2 transactions, 10 kCUs to thread 0.
        let batch_id_0 = in_flight_tracker.track_batch(2, 10_000, 0);
        assert_eq!(in_flight_tracker.num_in_flight_per_thread(), &[2, 0]);
        assert_eq!(in_flight_tracker.cus_in_flight_per_thread(), &[10_000, 0]);

        // Add a batch with 1 transaction, 15 kCUs to thread 1.
        let batch_id_1 = in_flight_tracker.track_batch(1, 15_000, 1);
        assert_eq!(in_flight_tracker.num_in_flight_per_thread(), &[2, 1]);
        assert_eq!(
            in_flight_tracker.cus_in_flight_per_thread(),
            &[10_000, 15_000]
        );

        in_flight_tracker.complete_batch(batch_id_0);
        assert_eq!(in_flight_tracker.num_in_flight_per_thread(), &[0, 1]);
        assert_eq!(in_flight_tracker.cus_in_flight_per_thread(), &[0, 15_000]);

        in_flight_tracker.complete_batch(batch_id_1);
        assert_eq!(in_flight_tracker.num_in_flight_per_thread(), &[0, 0]);
        assert_eq!(in_flight_tracker.cus_in_flight_per_thread(), &[0, 0]);
    }
}

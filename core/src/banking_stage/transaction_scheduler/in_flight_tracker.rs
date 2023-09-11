use {
    super::thread_aware_account_locks::ThreadId,
    crate::banking_stage::scheduler_messages::TransactionBatchId, std::collections::HashMap,
};

/// Tracks the number of transactions that are in flight for each thread.
pub struct InFlightTracker {
    num_in_flight_per_thread: Vec<usize>,
    batch_id_to_thread_id: HashMap<TransactionBatchId, ThreadId>,
}

impl InFlightTracker {
    pub fn new(num_threads: usize) -> Self {
        Self {
            num_in_flight_per_thread: vec![0; num_threads],
            batch_id_to_thread_id: HashMap::new(),
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
        self.batch_id_to_thread_id.insert(batch_id, thread_id);
    }

    /// Removes `num_transactions` from the thread which the batch was
    /// assigned to.
    pub fn complete_batch(
        &mut self,
        batch_id: TransactionBatchId,
        num_transactions: usize,
    ) -> ThreadId {
        let thread_id = self
            .batch_id_to_thread_id
            .remove(&batch_id)
            .expect("transaction batch id should exist in in-flight tracker");
        self.num_in_flight_per_thread[thread_id] -= num_transactions;

        thread_id
    }
}

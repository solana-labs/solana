use {
    super::{
        in_flight_tracker::InFlightTracker, scheduler_error::SchedulerError,
        thread_aware_account_locks::ThreadAwareAccountLocks,
        transaction_state_container::TransactionStateContainer,
    },
    crate::banking_stage::scheduler_messages::{ConsumeWork, FinishedConsumeWork},
    crossbeam_channel::{Receiver, Sender},
};

pub(crate) struct PrioGraphScheduler {
    in_flight_tracker: InFlightTracker,
    account_locks: ThreadAwareAccountLocks,
    consume_work_senders: Vec<Sender<ConsumeWork>>,
    consume_work_receiver: Receiver<FinishedConsumeWork>,
}

impl PrioGraphScheduler {
    pub(crate) fn new(
        consume_work_senders: Vec<Sender<ConsumeWork>>,
        consume_work_receiver: Receiver<FinishedConsumeWork>,
    ) -> Self {
        let num_threads = consume_work_senders.len();
        Self {
            in_flight_tracker: InFlightTracker::new(num_threads),
            account_locks: ThreadAwareAccountLocks::new(num_threads),
            consume_work_senders,
            consume_work_receiver,
        }
    }

    /// Schedule transactions from the given `TransactionStateContainer` to be consumed by the
    /// worker threads. Returns the number of transactions scheduled, or an error.
    ///
    /// Uses a `PrioGraph` to perform look-ahead during the scheduling of transactions.
    /// This, combined with internal tracking of threads' in-flight transactions, allows
    /// for load-balancing while prioritizing scheduling transactions onto threads that will
    /// not cause conflicts in the near future.
    pub(crate) fn schedule(
        &mut self,
        _container: &mut TransactionStateContainer,
    ) -> Result<usize, SchedulerError> {
        todo!()
    }
}

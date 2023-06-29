use {
    super::{
        batch_id_generator::BatchIdGenerator,
        central_scheduler_banking_stage::SchedulerError,
        in_flight_tracker::InFlightTracker,
        thread_aware_account_locks::{ThreadAwareAccountLocks, ThreadId, ThreadSet},
        transaction_packet_container::{SanitizedTransactionTTL, TransactionPacketContainer},
        transaction_priority_id::TransactionPriorityId,
    },
    crate::banking_stage::{
        consumer::TARGET_NUM_TRANSACTIONS_PER_BATCH,
        multi_iterator_scanner::{
            MultiIteratorScanner, PayloadAndAlreadyHandled, ProcessingDecision,
        },
        read_write_account_set::ReadWriteAccountSet,
        scheduler_messages::{ConsumeWork, TransactionBatchId, TransactionId},
    },
    crossbeam_channel::Sender,
    itertools::Itertools,
    solana_sdk::{slot_history::Slot, transaction::SanitizedTransaction},
};

const QUEUED_TRANSACTION_LIMIT: usize = 64 * 100;

/// Interface to perform scheduling for consuming transactions.
/// Using a multi-iterator approach.
pub struct MultiIteratorConsumeScheduler {
    in_flight_tracker: InFlightTracker,
    account_locks: ThreadAwareAccountLocks,
    batch_id_generator: BatchIdGenerator,
    consume_work_senders: Vec<Sender<ConsumeWork>>,
}

impl MultiIteratorConsumeScheduler {
    pub fn new(consume_work_senders: Vec<Sender<ConsumeWork>>) -> Self {
        let num_threads = consume_work_senders.len();
        Self {
            in_flight_tracker: InFlightTracker::new(num_threads),
            account_locks: ThreadAwareAccountLocks::new(num_threads),
            batch_id_generator: BatchIdGenerator::default(),
            consume_work_senders,
        }
    }

    pub(crate) fn schedule(
        &mut self,
        container: &mut TransactionPacketContainer,
    ) -> Result<(), SchedulerError> {
        let num_threads = self.consume_work_senders.len();
        let mut schedulable_threads = ThreadSet::any(num_threads);
        let outstanding = self.in_flight_tracker.num_in_flight_per_thread();
        for (thread_id, outstanding_count) in outstanding.iter().enumerate() {
            if *outstanding_count > QUEUED_TRANSACTION_LIMIT {
                schedulable_threads.remove(thread_id);
            }
        }

        let active_scheduler = ActiveMultiIteratorConsumeScheduler {
            in_flight_tracker: &mut self.in_flight_tracker,
            account_locks: &mut self.account_locks,
            batch_account_locks: ReadWriteAccountSet::default(),
            batches: Batches::new(num_threads),
            schedulable_threads,
            unschedulable_transactions: Vec::new(),
            priorty_order_guard: ReadWriteAccountSet::default(),
        };

        const MAX_TRANSACTIONS_PER_SCHEDULING_PASS: usize = 100_000;
        let target_scanner_batch_size: usize = TARGET_NUM_TRANSACTIONS_PER_BATCH * num_threads;
        let ids = container
            .take_top_n(MAX_TRANSACTIONS_PER_SCHEDULING_PASS)
            .collect_vec();

        let mut scanner = MultiIteratorScanner::new(
            &ids,
            target_scanner_batch_size,
            active_scheduler,
            |id, payload| payload.should_schedule(id, container),
        );

        while let Some((_ids, payload)) = scanner.iterate() {
            for thread_id in 0..num_threads {
                if !payload.batches.transactions.is_empty() {
                    let (ids, transactions, max_age_slots) = payload.batches.take_batch(thread_id);
                    let batch_id = self.batch_id_generator.next();
                    payload
                        .in_flight_tracker
                        .track_batch(batch_id, transactions.len(), thread_id);

                    let work = ConsumeWork {
                        batch_id,
                        ids,
                        transactions,
                        max_age_slots,
                    };
                    self.consume_work_senders[thread_id]
                        .send(work)
                        .map_err(|_| {
                            SchedulerError::DisconnectedSendChannel("consume work sender")
                        })?;
                }

                // clear account locks for the next iteration
                payload.batch_account_locks.clear();
            }
        }

        let PayloadAndAlreadyHandled {
            payload,
            already_handled,
        } = scanner.finalize();

        // Push unhandled or unschedulable (handled but not scheduled) back into the queue
        for id in ids
            .into_iter()
            .zip(already_handled.into_iter())
            .filter(|(_, already_handled)| !already_handled)
            .map(|(id, _)| id)
            .chain(payload.unschedulable_transactions.into_iter())
        {
            container.push_id_into_queue(id);
        }

        Ok(())
    }

    pub(crate) fn complete_batch(
        &mut self,
        batch_id: TransactionBatchId,
        transactions: &[SanitizedTransaction],
    ) {
        let thread_id = self
            .in_flight_tracker
            .complete_batch(batch_id, transactions.len());
        for transaction in transactions {
            let account_locks = transaction.get_account_locks_unchecked();
            self.account_locks.unlock_accounts(
                account_locks.writable.into_iter(),
                account_locks.readonly.into_iter(),
                thread_id,
            );
        }
    }
}

struct ActiveMultiIteratorConsumeScheduler<'a> {
    in_flight_tracker: &'a mut InFlightTracker,
    account_locks: &'a mut ThreadAwareAccountLocks,

    batch_account_locks: ReadWriteAccountSet,
    batches: Batches,
    schedulable_threads: ThreadSet,
    unschedulable_transactions: Vec<TransactionPriorityId>,

    /// grabs read/write locks in context of the scanner
    /// prevents low-priority txs being scheduled before high-priority ones
    priorty_order_guard: ReadWriteAccountSet,
}

struct Batches {
    ids: Vec<Vec<TransactionId>>,
    transactions: Vec<Vec<SanitizedTransaction>>,
    max_age_slots: Vec<Vec<Slot>>,
}

impl Batches {
    fn new(num_threads: usize) -> Self {
        Self {
            ids: vec![Vec::with_capacity(TARGET_NUM_TRANSACTIONS_PER_BATCH); num_threads],
            transactions: vec![Vec::with_capacity(TARGET_NUM_TRANSACTIONS_PER_BATCH); num_threads],
            max_age_slots: vec![Vec::with_capacity(TARGET_NUM_TRANSACTIONS_PER_BATCH); num_threads],
        }
    }

    fn take_batch(
        &mut self,
        thread_id: ThreadId,
    ) -> (Vec<TransactionId>, Vec<SanitizedTransaction>, Vec<Slot>) {
        (
            core::mem::replace(
                &mut self.ids[thread_id],
                Vec::with_capacity(TARGET_NUM_TRANSACTIONS_PER_BATCH),
            ),
            core::mem::replace(
                &mut self.transactions[thread_id],
                Vec::with_capacity(TARGET_NUM_TRANSACTIONS_PER_BATCH),
            ),
            core::mem::replace(
                &mut self.max_age_slots[thread_id],
                Vec::with_capacity(TARGET_NUM_TRANSACTIONS_PER_BATCH),
            ),
        )
    }
}

enum ConsumeSchedulingDecision {
    /// The transaction should be added to batch for thread `ThreadId`.
    Add(ThreadId),
    /// The transaction cannot be added to current batches, due to a lock conflict within batch(es).
    /// This transaction may be added to a future batch on this pass.
    Later,
    /// The transaction cannot be added to current batches, nor added to any future batches on this pass.
    NextPass,
}

impl<'a> ActiveMultiIteratorConsumeScheduler<'a> {
    fn should_schedule(
        &mut self,
        id: &TransactionPriorityId,
        container: &mut TransactionPacketContainer,
    ) -> ProcessingDecision {
        let transaction_ttl = container.get_transaction(&id.id);
        let SanitizedTransactionTTL { transaction, .. } = transaction_ttl;
        let scheduling_decision = self.make_scheduling_decision(transaction);
        match scheduling_decision {
            ConsumeSchedulingDecision::Add(thread_id) => {
                let transaction_ttl = container.take_transaction(&id.id);
                self.add_transaction_to_batch(id, transaction_ttl, thread_id);
                ProcessingDecision::Now
            }
            ConsumeSchedulingDecision::Later => ProcessingDecision::Later,
            ConsumeSchedulingDecision::NextPass => {
                self.unschedulable_transactions.push(*id);
                self.priorty_order_guard
                    .add_sanitized_message_account_locks(transaction.message());
                ProcessingDecision::Never
            }
        }
    }

    fn make_scheduling_decision(
        &mut self,
        transaction: &SanitizedTransaction,
    ) -> ConsumeSchedulingDecision {
        if !self
            .batch_account_locks
            .check_sanitized_message_account_locks(transaction.message())
        {
            return ConsumeSchedulingDecision::Later;
        }

        let transaction_locks = transaction.get_account_locks_unchecked();
        if let Some(thread_id) = self.account_locks.try_lock_accounts(
            transaction_locks.writable.into_iter(),
            transaction_locks.readonly.into_iter(),
            self.schedulable_threads,
            |thread_set| {
                Self::select_thread(
                    &self.batches.transactions,
                    self.in_flight_tracker.num_in_flight_per_thread(),
                    thread_set,
                )
            },
        ) {
            ConsumeSchedulingDecision::Add(thread_id)
        } else {
            ConsumeSchedulingDecision::NextPass
        }
    }

    fn add_transaction_to_batch(
        &mut self,
        id: &TransactionPriorityId,
        SanitizedTransactionTTL {
            transaction,
            max_age_slot,
        }: SanitizedTransactionTTL,
        thread_id: ThreadId,
    ) {
        self.batch_account_locks
            .add_sanitized_message_account_locks(transaction.message());
        self.batches.transactions[thread_id].push(transaction);
        self.batches.ids[thread_id].push(id.id);
        self.batches.max_age_slots[thread_id].push(max_age_slot);

        if self.batches.ids[thread_id].len()
            + self.in_flight_tracker.num_in_flight_per_thread()[thread_id]
            >= QUEUED_TRANSACTION_LIMIT
        {
            self.schedulable_threads.remove(thread_id);
        }
    }

    fn select_thread(
        batches_per_thread: &[Vec<SanitizedTransaction>],
        in_flight_per_thread: &[usize],
        thread_set: ThreadSet,
    ) -> ThreadId {
        thread_set
            .contained_threads_iter()
            .map(|thread_id| {
                (
                    thread_id,
                    batches_per_thread[thread_id].len() + in_flight_per_thread[thread_id],
                )
            })
            .min_by(|a, b| a.1.cmp(&b.1))
            .map(|(thread_id, _)| thread_id)
            .unwrap()
    }
}

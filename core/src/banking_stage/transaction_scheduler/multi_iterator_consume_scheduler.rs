use {
    super::{
        in_flight_tracker::InFlightTracker,
        scheduler_error::SchedulerError,
        thread_aware_account_locks::{ThreadAwareAccountLocks, ThreadId, ThreadSet},
        transaction_priority_id::TransactionPriorityId,
        transaction_state::SanitizedTransactionTTL,
        transaction_state_container::TransactionStateContainer,
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
    solana_sdk::{clock::Slot, transaction::SanitizedTransaction},
};

const QUEUED_TRANSACTION_LIMIT: usize = 64 * 100;

/// Interface to perform scheduling for consuming transactions.
/// Using a multi-iterator approach.
pub struct MultiIteratorConsumeScheduler {
    in_flight_tracker: InFlightTracker,
    account_locks: ThreadAwareAccountLocks,
    consume_work_senders: Vec<Sender<ConsumeWork>>,
}

impl MultiIteratorConsumeScheduler {
    pub fn new(consume_work_senders: Vec<Sender<ConsumeWork>>) -> Self {
        let num_threads = consume_work_senders.len();
        Self {
            in_flight_tracker: InFlightTracker::new(num_threads),
            account_locks: ThreadAwareAccountLocks::new(num_threads),
            consume_work_senders,
        }
    }

    pub(crate) fn schedule(
        &mut self,
        container: &mut TransactionStateContainer,
    ) -> Result<usize, SchedulerError> {
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
            batch_priorty_order_guard: ReadWriteAccountSet::default(),
            pass_priority_order_guard: ReadWriteAccountSet::default(),
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

        let mut num_scheduled = 0;
        while let Some((_ids, payload)) = scanner.iterate() {
            payload.batch_priorty_order_guard.clear();

            for thread_id in 0..num_threads {
                if !payload.batches.transactions.is_empty() {
                    let (ids, transactions, max_age_slots, total_cus) =
                        payload.batches.take_batch(thread_id);
                    let batch_id =
                        payload
                            .in_flight_tracker
                            .track_batch(ids.len(), total_cus, thread_id);

                    num_scheduled += ids.len();

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

        Ok(num_scheduled)
    }

    pub(crate) fn complete_batch(
        &mut self,
        batch_id: TransactionBatchId,
        transactions: &[SanitizedTransaction],
    ) {
        let thread_id = self.in_flight_tracker.complete_batch(batch_id);
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

    batch_priorty_order_guard: ReadWriteAccountSet,
    pass_priority_order_guard: ReadWriteAccountSet,
}

struct Batches {
    ids: Vec<Vec<TransactionId>>,
    transactions: Vec<Vec<SanitizedTransaction>>,
    max_age_slots: Vec<Vec<Slot>>,
    total_cus: Vec<u64>,
}

impl Batches {
    fn new(num_threads: usize) -> Self {
        Self {
            ids: vec![Vec::with_capacity(TARGET_NUM_TRANSACTIONS_PER_BATCH); num_threads],
            transactions: vec![Vec::with_capacity(TARGET_NUM_TRANSACTIONS_PER_BATCH); num_threads],
            max_age_slots: vec![Vec::with_capacity(TARGET_NUM_TRANSACTIONS_PER_BATCH); num_threads],
            total_cus: vec![0; num_threads],
        }
    }

    fn take_batch(
        &mut self,
        thread_id: ThreadId,
    ) -> (
        Vec<TransactionId>,
        Vec<SanitizedTransaction>,
        Vec<Slot>,
        u64,
    ) {
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
            core::mem::replace(&mut self.total_cus[thread_id], 0),
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
        container: &mut TransactionStateContainer,
    ) -> ProcessingDecision {
        let Some(transaction_state) = container.get_mut_transaction_state(&id.id) else {
            // Should not happen
            return ProcessingDecision::Never;
        };
        let scheduling_decision =
            self.make_scheduling_decision(&transaction_state.transaction_ttl().transaction);
        match scheduling_decision {
            ConsumeSchedulingDecision::Add(thread_id) => {
                let transaction_ttl = transaction_state.transition_to_pending();
                let cu_limit = transaction_state
                    .transaction_priority_details()
                    .compute_unit_limit;
                self.add_transaction_to_batch(id, transaction_ttl, cu_limit, thread_id);
                ProcessingDecision::Now
            }
            ConsumeSchedulingDecision::Later => {
                self.batch_priorty_order_guard
                    .add_sanitized_message_account_locks(
                        transaction_state.transaction_ttl().transaction.message(),
                    );
                ProcessingDecision::Later
            }
            ConsumeSchedulingDecision::NextPass => {
                self.unschedulable_transactions.push(*id);
                self.pass_priority_order_guard
                    .add_sanitized_message_account_locks(
                        transaction_state.transaction_ttl().transaction.message(),
                    );
                ProcessingDecision::Never
            }
        }
    }

    fn make_scheduling_decision(
        &mut self,
        transaction: &SanitizedTransaction,
    ) -> ConsumeSchedulingDecision {
        let message = transaction.message();

        // Check if this transaction conflicts with any transactions in the current batch.
        if !self
            .batch_account_locks
            .check_sanitized_message_account_locks(message)
        {
            return ConsumeSchedulingDecision::Later;
        }

        // Check if this transaction takes locks that conflict with any transactions
        // which were passed over due to above check.
        if !self
            .batch_priorty_order_guard
            .check_sanitized_message_account_locks(message)
        {
            return ConsumeSchedulingDecision::Later;
        }

        // Check if this transaction takes locks that conflict with any transactions
        // that were unschedulable due to below check.
        if !self
            .pass_priority_order_guard
            .check_sanitized_message_account_locks(message)
        {
            return ConsumeSchedulingDecision::NextPass;
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
        cu_limit: u64,
        thread_id: ThreadId,
    ) {
        self.batch_account_locks
            .add_sanitized_message_account_locks(transaction.message());
        self.batches.transactions[thread_id].push(transaction);
        self.batches.ids[thread_id].push(id.id);
        self.batches.max_age_slots[thread_id].push(max_age_slot);
        self.batches.total_cus[thread_id] += cu_limit;

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

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::banking_stage::consumer::TARGET_NUM_TRANSACTIONS_PER_BATCH,
        crossbeam_channel::{unbounded, Receiver},
        itertools::Itertools,
        solana_runtime::transaction_priority_details::TransactionPriorityDetails,
        solana_sdk::{
            compute_budget::ComputeBudgetInstruction, hash::Hash, message::Message, pubkey::Pubkey,
            signature::Keypair, signer::Signer, system_instruction, transaction::Transaction,
        },
        std::borrow::Borrow,
    };

    macro_rules! txid {
        ($value:expr) => {
            TransactionId::new($value)
        };
    }

    macro_rules! txids {
        ([$($element:expr),*]) => {
            vec![ $(txid!($element)),* ]
        };
    }

    fn create_test_frame(
        num_threads: usize,
    ) -> (MultiIteratorConsumeScheduler, Vec<Receiver<ConsumeWork>>) {
        let (consume_work_senders, consume_work_receivers) =
            (0..num_threads).map(|_| unbounded()).unzip();
        let scheduler = MultiIteratorConsumeScheduler::new(consume_work_senders);
        (scheduler, consume_work_receivers)
    }

    fn prioritized_tranfers(
        from_keypair: &Keypair,
        to_pubkeys: impl IntoIterator<Item = impl Borrow<Pubkey>>,
        lamports: u64,
        priority: u64,
    ) -> SanitizedTransaction {
        let to_pubkeys_lamports = to_pubkeys
            .into_iter()
            .map(|pubkey| *pubkey.borrow())
            .zip(std::iter::repeat(lamports))
            .collect_vec();
        let mut ixs =
            system_instruction::transfer_many(&from_keypair.pubkey(), &to_pubkeys_lamports);
        let prioritization = ComputeBudgetInstruction::set_compute_unit_price(priority);
        ixs.push(prioritization);
        let message = Message::new(&ixs, Some(&from_keypair.pubkey()));
        let tx = Transaction::new(&[from_keypair], message, Hash::default());
        SanitizedTransaction::from_transaction_for_tests(tx)
    }

    fn create_container(
        tx_infos: impl IntoIterator<
            Item = (
                impl Borrow<Keypair>,
                impl IntoIterator<Item = impl Borrow<Pubkey>>,
                u64,
                u64,
            ),
        >,
    ) -> TransactionStateContainer {
        let mut container = TransactionStateContainer::with_capacity(10 * 1024);
        for (index, (from_keypair, to_pubkeys, lamports, priority)) in
            tx_infos.into_iter().enumerate()
        {
            let id = TransactionId::new(index as u64);
            let transaction =
                prioritized_tranfers(from_keypair.borrow(), to_pubkeys, lamports, priority);
            let transaction_ttl = SanitizedTransactionTTL {
                transaction,
                max_age_slot: Slot::MAX,
            };
            container.insert_new_transaction(
                id,
                transaction_ttl,
                TransactionPriorityDetails {
                    priority,
                    compute_unit_limit: 1,
                },
            );
        }

        container
    }

    fn collect_work(
        receiver: &Receiver<ConsumeWork>,
    ) -> (Vec<ConsumeWork>, Vec<Vec<TransactionId>>) {
        receiver
            .try_iter()
            .map(|work| {
                let ids = work.ids.clone();
                (work, ids)
            })
            .unzip()
    }

    #[test]
    fn test_schedule_disconnected_channel() {
        let (mut scheduler, work_receivers) = create_test_frame(1);
        let mut container = create_container([(&Keypair::new(), &[Pubkey::new_unique()], 1, 1)]);

        drop(work_receivers); // explicitly drop receivers
        assert_matches!(
            scheduler.schedule(&mut container),
            Err(SchedulerError::DisconnectedSendChannel(_))
        );
    }

    #[test]
    fn test_schedule_single_threaded_no_conflicts() {
        let (mut scheduler, work_receivers) = create_test_frame(1);
        let mut container = create_container([
            (&Keypair::new(), &[Pubkey::new_unique()], 1, 1),
            (&Keypair::new(), &[Pubkey::new_unique()], 2, 2),
        ]);

        let num_scheduled = scheduler.schedule(&mut container).unwrap();
        assert_eq!(num_scheduled, 2);
        assert_eq!(collect_work(&work_receivers[0]).1, vec![txids!([1, 0])]);
    }

    #[test]
    fn test_schedule_single_threaded_conflict() {
        let (mut scheduler, work_receivers) = create_test_frame(1);
        let pubkey = Pubkey::new_unique();
        let mut container = create_container([
            (&Keypair::new(), &[pubkey], 1, 1),
            (&Keypair::new(), &[pubkey], 1, 2),
        ]);

        let num_scheduled = scheduler.schedule(&mut container).unwrap();
        assert_eq!(num_scheduled, 2);
        assert_eq!(
            collect_work(&work_receivers[0]).1,
            vec![txids!([1]), txids!([0])]
        );
    }

    #[test]
    fn test_schedule_consume_single_threaded_multi_batch() {
        let (mut scheduler, work_receivers) = create_test_frame(1);
        let mut container = create_container(
            (0..4 * TARGET_NUM_TRANSACTIONS_PER_BATCH)
                .map(|i| (Keypair::new(), [Pubkey::new_unique()], i as u64, 1)),
        );

        // expect 4 full batches to be scheduled
        let num_scheduled = scheduler.schedule(&mut container).unwrap();
        assert_eq!(num_scheduled, 4 * TARGET_NUM_TRANSACTIONS_PER_BATCH);

        let thread0_work_counts: Vec<_> = work_receivers[0]
            .try_iter()
            .map(|work| work.ids.len())
            .collect();
        assert_eq!(thread0_work_counts, [TARGET_NUM_TRANSACTIONS_PER_BATCH; 4]);
    }

    #[test]
    fn test_schedule_simple_thread_selection() {
        let (mut scheduler, work_receivers) = create_test_frame(2);
        let mut container =
            create_container((0..4).map(|i| (Keypair::new(), [Pubkey::new_unique()], 1, i)));

        let num_scheduled = scheduler.schedule(&mut container).unwrap();
        assert_eq!(num_scheduled, 4);
        assert_eq!(collect_work(&work_receivers[0]).1, [txids!([3, 1])]);
        assert_eq!(collect_work(&work_receivers[1]).1, [txids!([2, 0])]);
    }

    #[test]
    fn test_schedule_non_schedulable() {
        let (mut scheduler, work_receivers) = create_test_frame(2);

        let accounts = (0..4).map(|_| Keypair::new()).collect_vec();
        let mut container = create_container([
            (&accounts[0], &[accounts[1].pubkey()], 1, 2),
            (&accounts[2], &[accounts[3].pubkey()], 1, 1),
            (&accounts[1], &[accounts[2].pubkey()], 1, 0),
        ]);

        // high priority transactions [0, 1] do not conflict, and should be
        // scheduled to *different* threads.
        // low priority transaction [2] conflicts with both, and thus will
        // not be schedulable until one of the previous transactions is
        // completed.
        let num_scheduled = scheduler.schedule(&mut container).unwrap();
        assert_eq!(num_scheduled, 2);
        let (thread_0_work, thread_0_ids) = collect_work(&work_receivers[0]);
        assert_eq!(thread_0_ids, [txids!([0])]);
        assert_eq!(collect_work(&work_receivers[1]).1, [txids!([1])]);

        // Cannot schedule even on next pass because of lock conflicts
        let num_scheduled = scheduler.schedule(&mut container).unwrap();
        assert_eq!(num_scheduled, 0);

        // Complete batch on thread 0. Remaining tx can be scheduled onto thread 1
        scheduler.complete_batch(thread_0_work[0].batch_id, &thread_0_work[0].transactions);
        let num_scheduled = scheduler.schedule(&mut container).unwrap();
        assert_eq!(num_scheduled, 1);

        assert_eq!(collect_work(&work_receivers[1]).1, [txids!([2])]);
    }

    #[test]
    fn test_schedule_priority_guard() {
        let (mut scheduler, work_receivers) = create_test_frame(2);

        let accounts = (0..6).map(|_| Keypair::new()).collect_vec();
        let mut container = create_container([
            (&accounts[0], vec![accounts[1].pubkey()], 1, 3),
            (&accounts[2], vec![accounts[3].pubkey()], 1, 2),
            (
                &accounts[1],
                vec![accounts[2].pubkey(), accounts[4].pubkey()],
                1,
                1,
            ),
            (&accounts[4], vec![accounts[5].pubkey()], 1, 0),
        ]);

        // high priority transactions [0, 1] do not conflict, and should be
        // scheduled to *different* threads.
        // low priority transaction [2] conflicts with both, and thus will
        // not be schedulable until one of the previous transactions is
        // completed.
        // low priority transaction [3] does not conflict with any scheduled
        // transactions, but the priority guard should stop it from taking
        // a lock that transaction [2] needs.
        let num_scheduled = scheduler.schedule(&mut container).unwrap();
        assert_eq!(num_scheduled, 2);
        let (thread_0_work, thread_0_ids) = collect_work(&work_receivers[0]);
        assert_eq!(thread_0_ids, [txids!([0])]);
        assert_eq!(collect_work(&work_receivers[1]).1, [txids!([1])]);

        // Cannot schedule even on next pass because of lock conflicts
        let num_scheduled = scheduler.schedule(&mut container).unwrap();
        assert_eq!(num_scheduled, 0);

        // Complete batch on thread 0. Remaining txs can be scheduled onto thread 1
        scheduler.complete_batch(thread_0_work[0].batch_id, &thread_0_work[0].transactions);
        let num_scheduled = scheduler.schedule(&mut container).unwrap();
        assert_eq!(num_scheduled, 2);

        assert_eq!(
            collect_work(&work_receivers[1]).1,
            [txids!([2]), txids!([3])]
        );
    }

    #[test]
    fn test_schedule_queued_limit() {
        let (mut scheduler, _work_receivers) = create_test_frame(1);
        let mut container = create_container(
            (0..QUEUED_TRANSACTION_LIMIT + 4 * TARGET_NUM_TRANSACTIONS_PER_BATCH)
                .map(|i| (Keypair::new(), [Pubkey::new_unique()], 1, i as u64)),
        );

        // Even though no transactions conflict, we will only schedule up the queue limit
        let num_scheduled = scheduler.schedule(&mut container).unwrap();
        assert_eq!(num_scheduled, QUEUED_TRANSACTION_LIMIT);
    }
}

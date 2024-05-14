use {
    super::{
        in_flight_tracker::InFlightTracker,
        scheduler_error::SchedulerError,
        thread_aware_account_locks::{ThreadAwareAccountLocks, ThreadId, ThreadSet},
        transaction_state::SanitizedTransactionTTL,
        transaction_state_container::TransactionStateContainer,
    },
    crate::banking_stage::{
        consumer::TARGET_NUM_TRANSACTIONS_PER_BATCH,
        read_write_account_set::ReadWriteAccountSet,
        scheduler_messages::{ConsumeWork, FinishedConsumeWork, TransactionBatchId, TransactionId},
        transaction_scheduler::transaction_priority_id::TransactionPriorityId,
    },
    crossbeam_channel::{Receiver, Sender, TryRecvError},
    itertools::izip,
    prio_graph::{AccessKind, PrioGraph},
    solana_measure::measure_us,
    solana_sdk::{
        pubkey::Pubkey, saturating_add_assign, slot_history::Slot,
        transaction::SanitizedTransaction,
    },
};

pub(crate) struct PrioGraphScheduler {
    in_flight_tracker: InFlightTracker,
    account_locks: ThreadAwareAccountLocks,
    consume_work_senders: Vec<Sender<ConsumeWork>>,
    finished_consume_work_receiver: Receiver<FinishedConsumeWork>,
    look_ahead_window_size: usize,
}

impl PrioGraphScheduler {
    pub(crate) fn new(
        consume_work_senders: Vec<Sender<ConsumeWork>>,
        finished_consume_work_receiver: Receiver<FinishedConsumeWork>,
    ) -> Self {
        let num_threads = consume_work_senders.len();
        Self {
            in_flight_tracker: InFlightTracker::new(num_threads),
            account_locks: ThreadAwareAccountLocks::new(num_threads),
            consume_work_senders,
            finished_consume_work_receiver,
            look_ahead_window_size: 2048,
        }
    }

    /// Schedule transactions from the given `TransactionStateContainer` to be
    /// consumed by the worker threads. Returns summary of scheduling, or an
    /// error.
    /// `pre_graph_filter` is used to filter out transactions that should be
    /// skipped and dropped before insertion to the prio-graph. This fn should
    /// set `false` for transactions that should be dropped, and `true`
    /// otherwise.
    /// `pre_lock_filter` is used to filter out transactions after they have
    /// made it to the top of the prio-graph, and immediately before locks are
    /// checked and taken. This fn should return `true` for transactions that
    /// should be scheduled, and `false` otherwise.
    ///
    /// Uses a `PrioGraph` to perform look-ahead during the scheduling of transactions.
    /// This, combined with internal tracking of threads' in-flight transactions, allows
    /// for load-balancing while prioritizing scheduling transactions onto threads that will
    /// not cause conflicts in the near future.
    pub(crate) fn schedule(
        &mut self,
        container: &mut TransactionStateContainer,
        pre_graph_filter: impl Fn(&[&SanitizedTransaction], &mut [bool]),
        pre_lock_filter: impl Fn(&SanitizedTransaction) -> bool,
    ) -> Result<SchedulingSummary, SchedulerError> {
        let num_threads = self.consume_work_senders.len();
        let mut batches = Batches::new(num_threads);
        // Some transactions may be unschedulable due to multi-thread conflicts.
        // These transactions cannot be scheduled until some conflicting work is completed.
        // However, the scheduler should not allow other transactions that conflict with
        // these transactions to be scheduled before them.
        let mut unschedulable_ids = Vec::new();
        let mut blocking_locks = ReadWriteAccountSet::default();
        let mut prio_graph = PrioGraph::new(|id: &TransactionPriorityId, _graph_node| *id);

        // Track metrics on filter.
        let mut num_filtered_out: usize = 0;
        let mut total_filter_time_us: u64 = 0;

        let mut window_budget = self.look_ahead_window_size;
        let mut chunked_pops = |container: &mut TransactionStateContainer,
                                prio_graph: &mut PrioGraph<_, _, _, _>,
                                window_budget: &mut usize| {
            while *window_budget > 0 {
                const MAX_FILTER_CHUNK_SIZE: usize = 128;
                let mut filter_array = [true; MAX_FILTER_CHUNK_SIZE];
                let mut ids = Vec::with_capacity(MAX_FILTER_CHUNK_SIZE);
                let mut txs = Vec::with_capacity(MAX_FILTER_CHUNK_SIZE);

                let chunk_size = (*window_budget).min(MAX_FILTER_CHUNK_SIZE);
                for _ in 0..chunk_size {
                    if let Some(id) = container.pop() {
                        ids.push(id);
                    } else {
                        break;
                    }
                }
                *window_budget = window_budget.saturating_sub(chunk_size);

                ids.iter().for_each(|id| {
                    let transaction = container.get_transaction_ttl(&id.id).unwrap();
                    txs.push(&transaction.transaction);
                });

                let (_, filter_us) =
                    measure_us!(pre_graph_filter(&txs, &mut filter_array[..chunk_size]));
                saturating_add_assign!(total_filter_time_us, filter_us);

                for (id, filter_result) in ids.iter().zip(&filter_array[..chunk_size]) {
                    if *filter_result {
                        let transaction = container.get_transaction_ttl(&id.id).unwrap();
                        prio_graph.insert_transaction(
                            *id,
                            Self::get_transaction_account_access(transaction),
                        );
                    } else {
                        saturating_add_assign!(num_filtered_out, 1);
                        container.remove_by_id(&id.id);
                    }
                }

                if ids.len() != chunk_size {
                    break;
                }
            }
        };

        // Create the initial look-ahead window.
        // Check transactions against filter, remove from container if it fails.
        chunked_pops(container, &mut prio_graph, &mut window_budget);

        let mut unblock_this_batch =
            Vec::with_capacity(self.consume_work_senders.len() * TARGET_NUM_TRANSACTIONS_PER_BATCH);
        const MAX_TRANSACTIONS_PER_SCHEDULING_PASS: usize = 100_000;
        let mut num_scheduled: usize = 0;
        let mut num_sent: usize = 0;
        let mut num_unschedulable: usize = 0;
        while num_scheduled < MAX_TRANSACTIONS_PER_SCHEDULING_PASS {
            // If nothing is in the main-queue of the `PrioGraph` then there's nothing left to schedule.
            if prio_graph.is_empty() {
                break;
            }

            while let Some(id) = prio_graph.pop() {
                unblock_this_batch.push(id);

                // Should always be in the container, during initial testing phase panic.
                // Later, we can replace with a continue in case this does happen.
                let Some(transaction_state) = container.get_mut_transaction_state(&id.id) else {
                    panic!("transaction state must exist")
                };

                let transaction = &transaction_state.transaction_ttl().transaction;
                if !pre_lock_filter(transaction) {
                    container.remove_by_id(&id.id);
                    continue;
                }

                // Check if this transaction conflicts with any blocked transactions
                if !blocking_locks.check_locks(transaction.message()) {
                    blocking_locks.take_locks(transaction.message());
                    unschedulable_ids.push(id);
                    saturating_add_assign!(num_unschedulable, 1);
                    continue;
                }

                // Schedule the transaction if it can be.
                let transaction_locks = transaction.get_account_locks_unchecked();
                let Some(thread_id) = self.account_locks.try_lock_accounts(
                    transaction_locks.writable.into_iter(),
                    transaction_locks.readonly.into_iter(),
                    ThreadSet::any(num_threads),
                    |thread_set| {
                        Self::select_thread(
                            thread_set,
                            &batches.transactions,
                            self.in_flight_tracker.num_in_flight_per_thread(),
                        )
                    },
                ) else {
                    blocking_locks.take_locks(transaction.message());
                    unschedulable_ids.push(id);
                    saturating_add_assign!(num_unschedulable, 1);
                    continue;
                };

                saturating_add_assign!(num_scheduled, 1);

                let sanitized_transaction_ttl = transaction_state.transition_to_pending();
                let cost = transaction_state.cost();

                let SanitizedTransactionTTL {
                    transaction,
                    max_age_slot,
                } = sanitized_transaction_ttl;

                batches.transactions[thread_id].push(transaction);
                batches.ids[thread_id].push(id.id);
                batches.max_age_slots[thread_id].push(max_age_slot);
                saturating_add_assign!(batches.total_cus[thread_id], cost);

                // If target batch size is reached, send only this batch.
                if batches.ids[thread_id].len() >= TARGET_NUM_TRANSACTIONS_PER_BATCH {
                    saturating_add_assign!(num_sent, self.send_batch(&mut batches, thread_id)?);
                }

                if num_scheduled >= MAX_TRANSACTIONS_PER_SCHEDULING_PASS {
                    break;
                }
            }

            // Send all non-empty batches
            saturating_add_assign!(num_sent, self.send_batches(&mut batches)?);

            // Refresh window budget and do chunked pops
            saturating_add_assign!(window_budget, unblock_this_batch.len());
            chunked_pops(container, &mut prio_graph, &mut window_budget);

            // Unblock all transactions that were blocked by the transactions that were just sent.
            for id in unblock_this_batch.drain(..) {
                prio_graph.unblock(&id);
            }
        }

        // Send batches for any remaining transactions
        saturating_add_assign!(num_sent, self.send_batches(&mut batches)?);

        // Push unschedulable ids back into the container
        for id in unschedulable_ids {
            container.push_id_into_queue(id);
        }

        // Push remaining transactions back into the container
        while let Some((id, _)) = prio_graph.pop_and_unblock() {
            container.push_id_into_queue(id);
        }

        assert_eq!(
            num_scheduled, num_sent,
            "number of scheduled and sent transactions must match"
        );

        Ok(SchedulingSummary {
            num_scheduled,
            num_unschedulable,
            num_filtered_out,
            filter_time_us: total_filter_time_us,
        })
    }

    /// Receive completed batches of transactions without blocking.
    /// Returns (num_transactions, num_retryable_transactions) on success.
    pub fn receive_completed(
        &mut self,
        container: &mut TransactionStateContainer,
    ) -> Result<(usize, usize), SchedulerError> {
        let mut total_num_transactions: usize = 0;
        let mut total_num_retryable: usize = 0;
        loop {
            let (num_transactions, num_retryable) = self.try_receive_completed(container)?;
            if num_transactions == 0 {
                break;
            }
            saturating_add_assign!(total_num_transactions, num_transactions);
            saturating_add_assign!(total_num_retryable, num_retryable);
        }
        Ok((total_num_transactions, total_num_retryable))
    }

    /// Receive completed batches of transactions.
    /// Returns `Ok((num_transactions, num_retryable))` if a batch was received, `Ok((0, 0))` if no batch was received.
    fn try_receive_completed(
        &mut self,
        container: &mut TransactionStateContainer,
    ) -> Result<(usize, usize), SchedulerError> {
        match self.finished_consume_work_receiver.try_recv() {
            Ok(FinishedConsumeWork {
                work:
                    ConsumeWork {
                        batch_id,
                        ids,
                        transactions,
                        max_age_slots,
                    },
                retryable_indexes,
            }) => {
                let num_transactions = ids.len();
                let num_retryable = retryable_indexes.len();

                // Free the locks
                self.complete_batch(batch_id, &transactions);

                // Retryable transactions should be inserted back into the container
                let mut retryable_iter = retryable_indexes.into_iter().peekable();
                for (index, (id, transaction, max_age_slot)) in
                    izip!(ids, transactions, max_age_slots).enumerate()
                {
                    if let Some(retryable_index) = retryable_iter.peek() {
                        if *retryable_index == index {
                            container.retry_transaction(
                                id,
                                SanitizedTransactionTTL {
                                    transaction,
                                    max_age_slot,
                                },
                            );
                            retryable_iter.next();
                            continue;
                        }
                    }
                    container.remove_by_id(&id);
                }

                Ok((num_transactions, num_retryable))
            }
            Err(TryRecvError::Empty) => Ok((0, 0)),
            Err(TryRecvError::Disconnected) => Err(SchedulerError::DisconnectedRecvChannel(
                "finished consume work",
            )),
        }
    }

    /// Mark a given `TransactionBatchId` as completed.
    /// This will update the internal tracking, including account locks.
    fn complete_batch(
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

    /// Send all batches of transactions to the worker threads.
    /// Returns the number of transactions sent.
    fn send_batches(&mut self, batches: &mut Batches) -> Result<usize, SchedulerError> {
        (0..self.consume_work_senders.len())
            .map(|thread_index| self.send_batch(batches, thread_index))
            .sum()
    }

    /// Send a batch of transactions to the given thread's `ConsumeWork` channel.
    /// Returns the number of transactions sent.
    fn send_batch(
        &mut self,
        batches: &mut Batches,
        thread_index: usize,
    ) -> Result<usize, SchedulerError> {
        if batches.ids[thread_index].is_empty() {
            return Ok(0);
        }

        let (ids, transactions, max_age_slots, total_cus) = batches.take_batch(thread_index);

        let batch_id = self
            .in_flight_tracker
            .track_batch(ids.len(), total_cus, thread_index);

        let num_scheduled = ids.len();
        let work = ConsumeWork {
            batch_id,
            ids,
            transactions,
            max_age_slots,
        };
        self.consume_work_senders[thread_index]
            .send(work)
            .map_err(|_| SchedulerError::DisconnectedSendChannel("consume work sender"))?;

        Ok(num_scheduled)
    }

    /// Given the schedulable `thread_set`, select the thread with the least amount
    /// of work queued up.
    /// Currently, "work" is just defined as the number of transactions.
    ///
    /// If the `chain_thread` is available, this thread will be selected, regardless of
    /// load-balancing.
    ///
    /// Panics if the `thread_set` is empty. This should never happen, see comment
    /// on `ThreadAwareAccountLocks::try_lock_accounts`.
    fn select_thread(
        thread_set: ThreadSet,
        batches_per_thread: &[Vec<SanitizedTransaction>],
        in_flight_per_thread: &[usize],
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

    /// Gets accessed accounts (resources) for use in `PrioGraph`.
    fn get_transaction_account_access(
        transaction: &SanitizedTransactionTTL,
    ) -> impl Iterator<Item = (Pubkey, AccessKind)> + '_ {
        let message = transaction.transaction.message();
        message
            .account_keys()
            .iter()
            .enumerate()
            .map(|(index, key)| {
                if message.is_writable(index) {
                    (*key, AccessKind::Write)
                } else {
                    (*key, AccessKind::Read)
                }
            })
    }
}

/// Metrics from scheduling transactions.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SchedulingSummary {
    /// Number of transactions scheduled.
    pub num_scheduled: usize,
    /// Number of transactions that were not scheduled due to conflicts.
    pub num_unschedulable: usize,
    /// Number of transactions that were dropped due to filter.
    pub num_filtered_out: usize,
    /// Time spent filtering transactions
    pub filter_time_us: u64,
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

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::banking_stage::consumer::TARGET_NUM_TRANSACTIONS_PER_BATCH,
        crossbeam_channel::{unbounded, Receiver},
        itertools::Itertools,
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
    ) -> (
        PrioGraphScheduler,
        Vec<Receiver<ConsumeWork>>,
        Sender<FinishedConsumeWork>,
    ) {
        let (consume_work_senders, consume_work_receivers) =
            (0..num_threads).map(|_| unbounded()).unzip();
        let (finished_consume_work_sender, finished_consume_work_receiver) = unbounded();
        let scheduler =
            PrioGraphScheduler::new(consume_work_senders, finished_consume_work_receiver);
        (
            scheduler,
            consume_work_receivers,
            finished_consume_work_sender,
        )
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
        for (index, (from_keypair, to_pubkeys, lamports, compute_unit_price)) in
            tx_infos.into_iter().enumerate()
        {
            let id = TransactionId::new(index as u64);
            let transaction = prioritized_tranfers(
                from_keypair.borrow(),
                to_pubkeys,
                lamports,
                compute_unit_price,
            );
            let transaction_ttl = SanitizedTransactionTTL {
                transaction,
                max_age_slot: Slot::MAX,
            };
            const TEST_TRANSACTION_COST: u64 = 5000;
            container.insert_new_transaction(
                id,
                transaction_ttl,
                compute_unit_price,
                TEST_TRANSACTION_COST,
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

    fn test_pre_graph_filter(_txs: &[&SanitizedTransaction], results: &mut [bool]) {
        results.fill(true);
    }

    fn test_pre_lock_filter(_tx: &SanitizedTransaction) -> bool {
        true
    }

    #[test]
    fn test_schedule_disconnected_channel() {
        let (mut scheduler, work_receivers, _finished_work_sender) = create_test_frame(1);
        let mut container = create_container([(&Keypair::new(), &[Pubkey::new_unique()], 1, 1)]);

        drop(work_receivers); // explicitly drop receivers
        assert_matches!(
            scheduler.schedule(&mut container, test_pre_graph_filter, test_pre_lock_filter),
            Err(SchedulerError::DisconnectedSendChannel(_))
        );
    }

    #[test]
    fn test_schedule_single_threaded_no_conflicts() {
        let (mut scheduler, work_receivers, _finished_work_sender) = create_test_frame(1);
        let mut container = create_container([
            (&Keypair::new(), &[Pubkey::new_unique()], 1, 1),
            (&Keypair::new(), &[Pubkey::new_unique()], 2, 2),
        ]);

        let scheduling_summary = scheduler
            .schedule(&mut container, test_pre_graph_filter, test_pre_lock_filter)
            .unwrap();
        assert_eq!(scheduling_summary.num_scheduled, 2);
        assert_eq!(scheduling_summary.num_unschedulable, 0);
        assert_eq!(collect_work(&work_receivers[0]).1, vec![txids!([1, 0])]);
    }

    #[test]
    fn test_schedule_single_threaded_conflict() {
        let (mut scheduler, work_receivers, _finished_work_sender) = create_test_frame(1);
        let pubkey = Pubkey::new_unique();
        let mut container = create_container([
            (&Keypair::new(), &[pubkey], 1, 1),
            (&Keypair::new(), &[pubkey], 1, 2),
        ]);

        let scheduling_summary = scheduler
            .schedule(&mut container, test_pre_graph_filter, test_pre_lock_filter)
            .unwrap();
        assert_eq!(scheduling_summary.num_scheduled, 2);
        assert_eq!(scheduling_summary.num_unschedulable, 0);
        assert_eq!(
            collect_work(&work_receivers[0]).1,
            vec![txids!([1]), txids!([0])]
        );
    }

    #[test]
    fn test_schedule_consume_single_threaded_multi_batch() {
        let (mut scheduler, work_receivers, _finished_work_sender) = create_test_frame(1);
        let mut container = create_container(
            (0..4 * TARGET_NUM_TRANSACTIONS_PER_BATCH)
                .map(|i| (Keypair::new(), [Pubkey::new_unique()], i as u64, 1)),
        );

        // expect 4 full batches to be scheduled
        let scheduling_summary = scheduler
            .schedule(&mut container, test_pre_graph_filter, test_pre_lock_filter)
            .unwrap();
        assert_eq!(
            scheduling_summary.num_scheduled,
            4 * TARGET_NUM_TRANSACTIONS_PER_BATCH
        );
        assert_eq!(scheduling_summary.num_unschedulable, 0);

        let thread0_work_counts: Vec<_> = work_receivers[0]
            .try_iter()
            .map(|work| work.ids.len())
            .collect();
        assert_eq!(thread0_work_counts, [TARGET_NUM_TRANSACTIONS_PER_BATCH; 4]);
    }

    #[test]
    fn test_schedule_simple_thread_selection() {
        let (mut scheduler, work_receivers, _finished_work_sender) = create_test_frame(2);
        let mut container =
            create_container((0..4).map(|i| (Keypair::new(), [Pubkey::new_unique()], 1, i)));

        let scheduling_summary = scheduler
            .schedule(&mut container, test_pre_graph_filter, test_pre_lock_filter)
            .unwrap();
        assert_eq!(scheduling_summary.num_scheduled, 4);
        assert_eq!(scheduling_summary.num_unschedulable, 0);
        assert_eq!(collect_work(&work_receivers[0]).1, [txids!([3, 1])]);
        assert_eq!(collect_work(&work_receivers[1]).1, [txids!([2, 0])]);
    }

    #[test]
    fn test_schedule_priority_guard() {
        let (mut scheduler, work_receivers, finished_work_sender) = create_test_frame(2);
        // intentionally shorten the look-ahead window to cause unschedulable conflicts
        scheduler.look_ahead_window_size = 2;

        let accounts = (0..8).map(|_| Keypair::new()).collect_vec();
        let mut container = create_container([
            (&accounts[0], &[accounts[1].pubkey()], 1, 6),
            (&accounts[2], &[accounts[3].pubkey()], 1, 5),
            (&accounts[4], &[accounts[5].pubkey()], 1, 4),
            (&accounts[6], &[accounts[7].pubkey()], 1, 3),
            (&accounts[1], &[accounts[2].pubkey()], 1, 2),
            (&accounts[2], &[accounts[3].pubkey()], 1, 1),
        ]);

        // The look-ahead window is intentionally shortened, high priority transactions
        // [0, 1, 2, 3] do not conflict, and are scheduled onto threads in a
        // round-robin fashion. This leads to transaction [4] being unschedulable due
        // to conflicts with [0] and [1], which were scheduled to different threads.
        // Transaction [5] is technically schedulable, onto thread 1 since it only
        // conflicts with transaction [1]. However, [5] will not be scheduled because
        // it conflicts with a higher-priority transaction [4] that is unschedulable.
        // The full prio-graph can be visualized as:
        // [0] \
        //      -> [4] -> [5]
        // [1] / ------/
        // [2]
        // [3]
        // Because the look-ahead window is shortened to a size of 4, the scheduler does
        // not have knowledge of the joining at transaction [4] until after [0] and [1]
        // have been scheduled.
        let scheduling_summary = scheduler
            .schedule(&mut container, test_pre_graph_filter, test_pre_lock_filter)
            .unwrap();
        assert_eq!(scheduling_summary.num_scheduled, 4);
        assert_eq!(scheduling_summary.num_unschedulable, 2);
        let (thread_0_work, thread_0_ids) = collect_work(&work_receivers[0]);
        assert_eq!(thread_0_ids, [txids!([0]), txids!([2])]);
        assert_eq!(
            collect_work(&work_receivers[1]).1,
            [txids!([1]), txids!([3])]
        );

        // Cannot schedule even on next pass because of lock conflicts
        let scheduling_summary = scheduler
            .schedule(&mut container, test_pre_graph_filter, test_pre_lock_filter)
            .unwrap();
        assert_eq!(scheduling_summary.num_scheduled, 0);
        assert_eq!(scheduling_summary.num_unschedulable, 2);

        // Complete batch on thread 0. Remaining txs can be scheduled onto thread 1
        finished_work_sender
            .send(FinishedConsumeWork {
                work: thread_0_work.into_iter().next().unwrap(),
                retryable_indexes: vec![],
            })
            .unwrap();
        scheduler.receive_completed(&mut container).unwrap();
        let scheduling_summary = scheduler
            .schedule(&mut container, test_pre_graph_filter, test_pre_lock_filter)
            .unwrap();
        assert_eq!(scheduling_summary.num_scheduled, 2);
        assert_eq!(scheduling_summary.num_unschedulable, 0);

        assert_eq!(
            collect_work(&work_receivers[1]).1,
            [txids!([4]), txids!([5])]
        );
    }

    #[test]
    fn test_schedule_pre_lock_filter() {
        let (mut scheduler, work_receivers, _finished_work_sender) = create_test_frame(1);
        let pubkey = Pubkey::new_unique();
        let keypair = Keypair::new();
        let mut container = create_container([
            (&Keypair::new(), &[pubkey], 1, 1),
            (&keypair, &[pubkey], 1, 2),
            (&Keypair::new(), &[pubkey], 1, 3),
        ]);

        // 2nd transaction should be filtered out and dropped before locking.
        let pre_lock_filter =
            |tx: &SanitizedTransaction| tx.message().fee_payer() != &keypair.pubkey();
        let scheduling_summary = scheduler
            .schedule(&mut container, test_pre_graph_filter, pre_lock_filter)
            .unwrap();
        assert_eq!(scheduling_summary.num_scheduled, 2);
        assert_eq!(scheduling_summary.num_unschedulable, 0);
        assert_eq!(
            collect_work(&work_receivers[0]).1,
            vec![txids!([2]), txids!([0])]
        );
    }
}

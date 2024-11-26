//! Control flow for BankingStage's transaction scheduler.
//!

use {
    super::{
        prio_graph_scheduler::PrioGraphScheduler,
        receive_and_buffer::ReceiveAndBuffer,
        scheduler_error::SchedulerError,
        scheduler_metrics::{
            SchedulerCountMetrics, SchedulerLeaderDetectionMetrics, SchedulerTimingMetrics,
        },
        transaction_state_container::StateContainer,
    },
    crate::banking_stage::{
        consume_worker::ConsumeWorkerMetrics,
        consumer::Consumer,
        decision_maker::{BufferedPacketsDecision, DecisionMaker},
        forwarder::Forwarder,
        ForwardOption, LikeClusterInfo, TOTAL_BUFFERED_PACKETS,
    },
    solana_measure::measure_us,
    solana_runtime::{bank::Bank, bank_forks::BankForks},
    solana_sdk::{
        self,
        clock::{FORWARD_TRANSACTIONS_TO_LEADER_AT_SLOT_OFFSET, MAX_PROCESSING_AGE},
        saturating_add_assign,
    },
    solana_svm::transaction_error_metrics::TransactionErrorMetrics,
    std::{
        sync::{Arc, RwLock},
        time::{Duration, Instant},
    },
};

/// Controls packet and transaction flow into scheduler, and scheduling execution.
pub(crate) struct SchedulerController<C: LikeClusterInfo, R: ReceiveAndBuffer> {
    /// Decision maker for determining what should be done with transactions.
    decision_maker: DecisionMaker,
    receive_and_buffer: R,
    bank_forks: Arc<RwLock<BankForks>>,
    /// Container for transaction state.
    /// Shared resource between `packet_receiver` and `scheduler`.
    container: R::Container,
    /// State for scheduling and communicating with worker threads.
    scheduler: PrioGraphScheduler<R::Transaction>,
    /// Metrics tracking time for leader bank detection.
    leader_detection_metrics: SchedulerLeaderDetectionMetrics,
    /// Metrics tracking counts on transactions in different states
    /// over an interval and during a leader slot.
    count_metrics: SchedulerCountMetrics,
    /// Metrics tracking time spent in difference code sections
    /// over an interval and during a leader slot.
    timing_metrics: SchedulerTimingMetrics,
    /// Metric report handles for the worker threads.
    worker_metrics: Vec<Arc<ConsumeWorkerMetrics>>,
    /// State for forwarding packets to the leader, if enabled.
    forwarder: Option<Forwarder<C>>,
}

impl<C: LikeClusterInfo, R: ReceiveAndBuffer> SchedulerController<C, R> {
    pub fn new(
        decision_maker: DecisionMaker,
        receive_and_buffer: R,
        bank_forks: Arc<RwLock<BankForks>>,
        scheduler: PrioGraphScheduler<R::Transaction>,
        worker_metrics: Vec<Arc<ConsumeWorkerMetrics>>,
        forwarder: Option<Forwarder<C>>,
    ) -> Self {
        Self {
            decision_maker,
            receive_and_buffer,
            bank_forks,
            container: R::Container::with_capacity(TOTAL_BUFFERED_PACKETS),
            scheduler,
            leader_detection_metrics: SchedulerLeaderDetectionMetrics::default(),
            count_metrics: SchedulerCountMetrics::default(),
            timing_metrics: SchedulerTimingMetrics::default(),
            worker_metrics,
            forwarder,
        }
    }

    pub fn run(mut self) -> Result<(), SchedulerError> {
        loop {
            // BufferedPacketsDecision is shared with legacy BankingStage, which will forward
            // packets. Initially, not renaming these decision variants but the actions taken
            // are different, since new BankingStage will not forward packets.
            // For `Forward` and `ForwardAndHold`, we want to receive packets but will not
            // forward them to the next leader. In this case, `ForwardAndHold` is
            // indistinguishable from `Hold`.
            //
            // `Forward` will drop packets from the buffer instead of forwarding.
            // During receiving, since packets would be dropped from buffer anyway, we can
            // bypass sanitization and buffering and immediately drop the packets.
            let (decision, decision_time_us) =
                measure_us!(self.decision_maker.make_consume_or_forward_decision());
            self.timing_metrics.update(|timing_metrics| {
                saturating_add_assign!(timing_metrics.decision_time_us, decision_time_us);
            });
            let new_leader_slot = decision.bank_start().map(|b| b.working_bank.slot());
            self.leader_detection_metrics
                .update_and_maybe_report(decision.bank_start());
            self.count_metrics
                .maybe_report_and_reset_slot(new_leader_slot);
            self.timing_metrics
                .maybe_report_and_reset_slot(new_leader_slot);

            self.process_transactions(&decision)?;
            self.receive_completed()?;
            if !self.receive_and_buffer_packets(&decision) {
                break;
            }
            // Report metrics only if there is data.
            // Reset intervals when appropriate, regardless of report.
            let should_report = self.count_metrics.interval_has_data();
            let priority_min_max = self.container.get_min_max_priority();
            self.count_metrics.update(|count_metrics| {
                count_metrics.update_priority_stats(priority_min_max);
            });
            self.count_metrics
                .maybe_report_and_reset_interval(should_report);
            self.timing_metrics
                .maybe_report_and_reset_interval(should_report);
            self.worker_metrics
                .iter()
                .for_each(|metrics| metrics.maybe_report_and_reset(new_leader_slot));
        }

        Ok(())
    }

    /// Process packets based on decision.
    fn process_transactions(
        &mut self,
        decision: &BufferedPacketsDecision,
    ) -> Result<(), SchedulerError> {
        let forwarding_enabled = self.forwarder.is_some();
        match decision {
            BufferedPacketsDecision::Consume(bank_start) => {
                let (scheduling_summary, schedule_time_us) = measure_us!(self.scheduler.schedule(
                    &mut self.container,
                    |txs, results| {
                        Self::pre_graph_filter(
                            txs,
                            results,
                            &bank_start.working_bank,
                            MAX_PROCESSING_AGE,
                        )
                    },
                    |_| true // no pre-lock filter for now
                )?);

                self.count_metrics.update(|count_metrics| {
                    saturating_add_assign!(
                        count_metrics.num_scheduled,
                        scheduling_summary.num_scheduled
                    );
                    saturating_add_assign!(
                        count_metrics.num_unschedulable,
                        scheduling_summary.num_unschedulable
                    );
                    saturating_add_assign!(
                        count_metrics.num_schedule_filtered_out,
                        scheduling_summary.num_filtered_out
                    );
                });

                self.timing_metrics.update(|timing_metrics| {
                    saturating_add_assign!(
                        timing_metrics.schedule_filter_time_us,
                        scheduling_summary.filter_time_us
                    );
                    saturating_add_assign!(timing_metrics.schedule_time_us, schedule_time_us);
                });
            }
            BufferedPacketsDecision::Forward => {
                if forwarding_enabled {
                    let (_, forward_time_us) = measure_us!(self.forward_packets(false));
                    self.timing_metrics.update(|timing_metrics| {
                        saturating_add_assign!(timing_metrics.forward_time_us, forward_time_us);
                    });
                } else {
                    let (_, clear_time_us) = measure_us!(self.clear_container());
                    self.timing_metrics.update(|timing_metrics| {
                        saturating_add_assign!(timing_metrics.clear_time_us, clear_time_us);
                    });
                }
            }
            BufferedPacketsDecision::ForwardAndHold => {
                if forwarding_enabled {
                    let (_, forward_time_us) = measure_us!(self.forward_packets(true));
                    self.timing_metrics.update(|timing_metrics| {
                        saturating_add_assign!(timing_metrics.forward_time_us, forward_time_us);
                    });
                } else {
                    let (_, clean_time_us) = measure_us!(self.clean_queue());
                    self.timing_metrics.update(|timing_metrics| {
                        saturating_add_assign!(timing_metrics.clean_time_us, clean_time_us);
                    });
                }
            }
            BufferedPacketsDecision::Hold => {}
        }

        Ok(())
    }

    fn pre_graph_filter(
        transactions: &[&R::Transaction],
        results: &mut [bool],
        bank: &Bank,
        max_age: usize,
    ) {
        let lock_results = vec![Ok(()); transactions.len()];
        let mut error_counters = TransactionErrorMetrics::default();
        let check_results = bank.check_transactions::<R::Transaction>(
            transactions,
            &lock_results,
            max_age,
            &mut error_counters,
        );

        for ((check_result, tx), result) in check_results
            .into_iter()
            .zip(transactions)
            .zip(results.iter_mut())
        {
            *result = check_result
                .and_then(|_| Consumer::check_fee_payer_unlocked(bank, *tx, &mut error_counters))
                .is_ok();
        }
    }

    /// Forward packets to the next leader.
    fn forward_packets(&mut self, hold: bool) {
        const MAX_FORWARDING_DURATION: Duration = Duration::from_millis(100);
        let start = Instant::now();
        let bank = self.bank_forks.read().unwrap().working_bank();
        let feature_set = &bank.feature_set;
        let forwarder = self.forwarder.as_mut().expect("forwarder must exist");

        // Pop from the container in chunks, filter using bank checks, then attempt to forward.
        // This doubles as a way to clean the queue as well as forwarding transactions.
        const CHUNK_SIZE: usize = 64;
        let mut num_forwarded: usize = 0;
        let mut ids_to_add_back = Vec::new();
        let mut max_time_reached = false;
        while !self.container.is_empty() {
            let mut filter_array = [true; CHUNK_SIZE];
            let mut ids = Vec::with_capacity(CHUNK_SIZE);
            let mut txs = Vec::with_capacity(CHUNK_SIZE);

            for _ in 0..CHUNK_SIZE {
                if let Some(id) = self.container.pop() {
                    ids.push(id);
                } else {
                    break;
                }
            }
            let chunk_size = ids.len();
            ids.iter().for_each(|id| {
                let transaction = self.container.get_transaction_ttl(&id.id).unwrap();
                txs.push(&transaction.transaction);
            });

            // use same filter we use for processing transactions:
            // age, already processed, fee-check.
            Self::pre_graph_filter(
                &txs,
                &mut filter_array,
                &bank,
                MAX_PROCESSING_AGE
                    .saturating_sub(FORWARD_TRANSACTIONS_TO_LEADER_AT_SLOT_OFFSET as usize),
            );

            for (id, filter_result) in ids.iter().zip(&filter_array[..chunk_size]) {
                if !*filter_result {
                    self.container.remove_by_id(&id.id);
                    continue;
                }

                ids_to_add_back.push(*id); // add back to the queue at end
                let state = self.container.get_mut_transaction_state(&id.id).unwrap();
                let sanitized_transaction = &state.transaction_ttl().transaction;
                let immutable_packet = state.packet().clone();

                // If not already forwarded and can be forwarded, add to forwardable packets.
                if state.should_forward()
                    && forwarder.try_add_packet(
                        sanitized_transaction,
                        immutable_packet,
                        feature_set,
                    )
                {
                    saturating_add_assign!(num_forwarded, 1);
                    state.mark_forwarded();
                }
            }

            if start.elapsed() >= MAX_FORWARDING_DURATION {
                max_time_reached = true;
                break;
            }
        }

        // Forward each batch of transactions
        forwarder.forward_batched_packets(&ForwardOption::ForwardTransaction);
        forwarder.clear_batches();

        // If we hit the time limit. Drop everything that was not checked/processed.
        // If we cannot run these simple checks in time, then we cannot run them during
        // leader slot.
        if max_time_reached {
            while let Some(id) = self.container.pop() {
                self.container.remove_by_id(&id.id);
            }
        }

        if hold {
            for priority_id in ids_to_add_back {
                self.container.push_id_into_queue(priority_id);
            }
        } else {
            for priority_id in ids_to_add_back {
                self.container.remove_by_id(&priority_id.id);
            }
        }

        self.count_metrics.update(|count_metrics| {
            saturating_add_assign!(count_metrics.num_forwarded, num_forwarded);
        });
    }

    /// Clears the transaction state container.
    /// This only clears pending transactions, and does **not** clear in-flight transactions.
    fn clear_container(&mut self) {
        let mut num_dropped_on_clear: usize = 0;
        while let Some(id) = self.container.pop() {
            self.container.remove_by_id(&id.id);
            saturating_add_assign!(num_dropped_on_clear, 1);
        }

        self.count_metrics.update(|count_metrics| {
            saturating_add_assign!(count_metrics.num_dropped_on_clear, num_dropped_on_clear);
        });
    }

    /// Clean unprocessable transactions from the queue. These will be transactions that are
    /// expired, already processed, or are no longer sanitizable.
    /// This only clears pending transactions, and does **not** clear in-flight transactions.
    fn clean_queue(&mut self) {
        // Clean up any transactions that have already been processed, are too old, or do not have
        // valid nonce accounts.
        const MAX_TRANSACTION_CHECKS: usize = 10_000;
        let mut transaction_ids = Vec::with_capacity(MAX_TRANSACTION_CHECKS);

        while let Some(id) = self.container.pop() {
            transaction_ids.push(id);
        }

        let bank = self.bank_forks.read().unwrap().working_bank();

        const CHUNK_SIZE: usize = 128;
        let mut error_counters = TransactionErrorMetrics::default();
        let mut num_dropped_on_age_and_status: usize = 0;
        for chunk in transaction_ids.chunks(CHUNK_SIZE) {
            let lock_results = vec![Ok(()); chunk.len()];
            let sanitized_txs: Vec<_> = chunk
                .iter()
                .map(|id| {
                    &self
                        .container
                        .get_transaction_ttl(&id.id)
                        .expect("transaction must exist")
                        .transaction
                })
                .collect();

            let check_results = bank.check_transactions::<R::Transaction>(
                &sanitized_txs,
                &lock_results,
                MAX_PROCESSING_AGE,
                &mut error_counters,
            );

            for (result, id) in check_results.into_iter().zip(chunk.iter()) {
                if result.is_err() {
                    saturating_add_assign!(num_dropped_on_age_and_status, 1);
                    self.container.remove_by_id(&id.id);
                } else {
                    self.container.push_id_into_queue(*id);
                }
            }
        }

        self.count_metrics.update(|count_metrics| {
            saturating_add_assign!(
                count_metrics.num_dropped_on_age_and_status,
                num_dropped_on_age_and_status
            );
        });
    }

    /// Receives completed transactions from the workers and updates metrics.
    fn receive_completed(&mut self) -> Result<(), SchedulerError> {
        let ((num_transactions, num_retryable), receive_completed_time_us) =
            measure_us!(self.scheduler.receive_completed(&mut self.container)?);

        self.count_metrics.update(|count_metrics| {
            saturating_add_assign!(count_metrics.num_finished, num_transactions);
            saturating_add_assign!(count_metrics.num_retryable, num_retryable);
        });
        self.timing_metrics.update(|timing_metrics| {
            saturating_add_assign!(
                timing_metrics.receive_completed_time_us,
                receive_completed_time_us
            );
        });

        Ok(())
    }

    /// Returns whether the packet receiver is still connected.
    fn receive_and_buffer_packets(&mut self, decision: &BufferedPacketsDecision) -> bool {
        self.receive_and_buffer.receive_and_buffer_packets(
            &mut self.container,
            &mut self.timing_metrics,
            &mut self.count_metrics,
            decision,
        )
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            banking_stage::{
                consumer::TARGET_NUM_TRANSACTIONS_PER_BATCH,
                packet_deserializer::PacketDeserializer,
                scheduler_messages::{ConsumeWork, FinishedConsumeWork, TransactionBatchId},
                tests::create_slow_genesis_config,
                transaction_scheduler::receive_and_buffer::SanitizedTransactionReceiveAndBuffer,
            },
            banking_trace::BankingPacketBatch,
            sigverify::SigverifyTracerPacketStats,
        },
        crossbeam_channel::{unbounded, Receiver, Sender},
        itertools::Itertools,
        solana_gossip::cluster_info::ClusterInfo,
        solana_ledger::{
            blockstore::Blockstore, genesis_utils::GenesisConfigInfo,
            get_tmp_ledger_path_auto_delete, leader_schedule_cache::LeaderScheduleCache,
        },
        solana_perf::packet::{to_packet_batches, PacketBatch, NUM_PACKETS},
        solana_poh::poh_recorder::{PohRecorder, Record, WorkingBankEntry},
        solana_runtime::bank::Bank,
        solana_runtime_transaction::runtime_transaction::RuntimeTransaction,
        solana_sdk::{
            compute_budget::ComputeBudgetInstruction,
            fee_calculator::FeeRateGovernor,
            hash::Hash,
            message::Message,
            poh_config::PohConfig,
            pubkey::Pubkey,
            signature::Keypair,
            signer::Signer,
            system_instruction, system_transaction,
            transaction::{SanitizedTransaction, Transaction},
        },
        std::sync::{atomic::AtomicBool, Arc, RwLock},
        tempfile::TempDir,
    };

    fn create_channels<T>(num: usize) -> (Vec<Sender<T>>, Vec<Receiver<T>>) {
        (0..num).map(|_| unbounded()).unzip()
    }

    // Helper struct to create tests that hold channels, files, etc.
    // such that our tests can be more easily set up and run.
    struct TestFrame {
        bank: Arc<Bank>,
        mint_keypair: Keypair,
        _ledger_path: TempDir,
        _entry_receiver: Receiver<WorkingBankEntry>,
        _record_receiver: Receiver<Record>,
        poh_recorder: Arc<RwLock<PohRecorder>>,
        banking_packet_sender: Sender<Arc<(Vec<PacketBatch>, Option<SigverifyTracerPacketStats>)>>,

        consume_work_receivers:
            Vec<Receiver<ConsumeWork<RuntimeTransaction<SanitizedTransaction>>>>,
        finished_consume_work_sender:
            Sender<FinishedConsumeWork<RuntimeTransaction<SanitizedTransaction>>>,
    }

    #[allow(clippy::type_complexity)]
    fn create_test_frame(
        num_threads: usize,
    ) -> (
        TestFrame,
        SchedulerController<Arc<ClusterInfo>, SanitizedTransactionReceiveAndBuffer>,
    ) {
        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair,
            ..
        } = create_slow_genesis_config(u64::MAX);
        genesis_config.fee_rate_governor = FeeRateGovernor::new(5000, 0);
        let (bank, bank_forks) = Bank::new_no_wallclock_throttle_for_tests(&genesis_config);

        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path())
            .expect("Expected to be able to open database ledger");
        let (poh_recorder, entry_receiver, record_receiver) = PohRecorder::new(
            bank.tick_height(),
            bank.last_blockhash(),
            bank.clone(),
            Some((4, 4)),
            bank.ticks_per_slot(),
            Arc::new(blockstore),
            &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
            &PohConfig::default(),
            Arc::new(AtomicBool::default()),
        );
        let poh_recorder = Arc::new(RwLock::new(poh_recorder));
        let decision_maker = DecisionMaker::new(Pubkey::new_unique(), poh_recorder.clone());

        let (banking_packet_sender, banking_packet_receiver) = unbounded();
        let packet_deserializer = PacketDeserializer::new(banking_packet_receiver);

        let (consume_work_senders, consume_work_receivers) = create_channels(num_threads);
        let (finished_consume_work_sender, finished_consume_work_receiver) = unbounded();

        let test_frame = TestFrame {
            bank,
            mint_keypair,
            _ledger_path: ledger_path,
            _entry_receiver: entry_receiver,
            _record_receiver: record_receiver,
            poh_recorder,
            banking_packet_sender,
            consume_work_receivers,
            finished_consume_work_sender,
        };

        let receive_and_buffer = SanitizedTransactionReceiveAndBuffer::new(
            packet_deserializer,
            bank_forks.clone(),
            false,
        );

        let scheduler_controller = SchedulerController::new(
            decision_maker,
            receive_and_buffer,
            bank_forks,
            PrioGraphScheduler::new(consume_work_senders, finished_consume_work_receiver),
            vec![], // no actual workers with metrics to report, this can be empty
            None,
        );

        (test_frame, scheduler_controller)
    }

    fn create_and_fund_prioritized_transfer(
        bank: &Bank,
        mint_keypair: &Keypair,
        from_keypair: &Keypair,
        to_pubkey: &Pubkey,
        lamports: u64,
        compute_unit_price: u64,
        recent_blockhash: Hash,
    ) -> Transaction {
        // Fund the sending key, so that the transaction does not get filtered by the fee-payer check.
        {
            let transfer = system_transaction::transfer(
                mint_keypair,
                &from_keypair.pubkey(),
                500_000, // just some amount that will always be enough
                bank.last_blockhash(),
            );
            bank.process_transaction(&transfer).unwrap();
        }

        let transfer = system_instruction::transfer(&from_keypair.pubkey(), to_pubkey, lamports);
        let prioritization = ComputeBudgetInstruction::set_compute_unit_price(compute_unit_price);
        let message = Message::new(&[transfer, prioritization], Some(&from_keypair.pubkey()));
        Transaction::new(&vec![from_keypair], message, recent_blockhash)
    }

    fn to_banking_packet_batch(txs: &[Transaction]) -> BankingPacketBatch {
        let packet_batch = to_packet_batches(txs, NUM_PACKETS);
        Arc::new((packet_batch, None))
    }

    // Helper function to let test receive and then schedule packets.
    // The order of operations here is convenient for testing, but does not
    // match the order of operations in the actual scheduler.
    // The actual scheduler will process immediately after the decision,
    // in order to keep the decision as recent as possible for processing.
    // In the tests, the decision will not become stale, so it is more convenient
    // to receive first and then schedule.
    fn test_receive_then_schedule(
        scheduler_controller: &mut SchedulerController<
            Arc<ClusterInfo>,
            SanitizedTransactionReceiveAndBuffer,
        >,
    ) {
        let decision = scheduler_controller
            .decision_maker
            .make_consume_or_forward_decision();
        assert!(matches!(decision, BufferedPacketsDecision::Consume(_)));
        assert!(scheduler_controller.receive_completed().is_ok());
        assert!(scheduler_controller.receive_and_buffer_packets(&decision));
        assert!(scheduler_controller.process_transactions(&decision).is_ok());
    }

    #[test]
    #[should_panic(expected = "batch id 0 is not being tracked")]
    fn test_unexpected_batch_id() {
        let (test_frame, scheduler_controller) = create_test_frame(1);
        let TestFrame {
            finished_consume_work_sender,
            ..
        } = &test_frame;

        finished_consume_work_sender
            .send(FinishedConsumeWork {
                work: ConsumeWork {
                    batch_id: TransactionBatchId::new(0),
                    ids: vec![],
                    transactions: vec![],
                    max_ages: vec![],
                },
                retryable_indexes: vec![],
            })
            .unwrap();

        scheduler_controller.run().unwrap();
    }

    #[test]
    fn test_schedule_consume_single_threaded_no_conflicts() {
        let (test_frame, mut scheduler_controller) = create_test_frame(1);
        let TestFrame {
            bank,
            mint_keypair,
            poh_recorder,
            banking_packet_sender,
            consume_work_receivers,
            ..
        } = &test_frame;

        poh_recorder
            .write()
            .unwrap()
            .set_bank_for_test(bank.clone());

        // Send packet batch to the scheduler - should do nothing until we become the leader.
        let tx1 = create_and_fund_prioritized_transfer(
            bank,
            mint_keypair,
            &Keypair::new(),
            &Pubkey::new_unique(),
            1,
            1000,
            bank.last_blockhash(),
        );
        let tx2 = create_and_fund_prioritized_transfer(
            bank,
            mint_keypair,
            &Keypair::new(),
            &Pubkey::new_unique(),
            1,
            2000,
            bank.last_blockhash(),
        );
        let tx1_hash = tx1.message().hash();
        let tx2_hash = tx2.message().hash();

        let txs = vec![tx1, tx2];
        banking_packet_sender
            .send(to_banking_packet_batch(&txs))
            .unwrap();

        test_receive_then_schedule(&mut scheduler_controller);
        let consume_work = consume_work_receivers[0].try_recv().unwrap();
        assert_eq!(consume_work.ids.len(), 2);
        assert_eq!(consume_work.transactions.len(), 2);
        let message_hashes = consume_work
            .transactions
            .iter()
            .map(|tx| tx.message_hash())
            .collect_vec();
        assert_eq!(message_hashes, vec![&tx2_hash, &tx1_hash]);
    }

    #[test]
    fn test_schedule_consume_single_threaded_conflict() {
        let (test_frame, mut scheduler_controller) = create_test_frame(1);
        let TestFrame {
            bank,
            mint_keypair,
            poh_recorder,
            banking_packet_sender,
            consume_work_receivers,
            ..
        } = &test_frame;

        poh_recorder
            .write()
            .unwrap()
            .set_bank_for_test(bank.clone());

        let pk = Pubkey::new_unique();
        let tx1 = create_and_fund_prioritized_transfer(
            bank,
            mint_keypair,
            &Keypair::new(),
            &pk,
            1,
            1000,
            bank.last_blockhash(),
        );
        let tx2 = create_and_fund_prioritized_transfer(
            bank,
            mint_keypair,
            &Keypair::new(),
            &pk,
            1,
            2000,
            bank.last_blockhash(),
        );
        let tx1_hash = tx1.message().hash();
        let tx2_hash = tx2.message().hash();

        let txs = vec![tx1, tx2];
        banking_packet_sender
            .send(to_banking_packet_batch(&txs))
            .unwrap();

        // We expect 2 batches to be scheduled
        test_receive_then_schedule(&mut scheduler_controller);
        let consume_works = (0..2)
            .map(|_| consume_work_receivers[0].try_recv().unwrap())
            .collect_vec();

        let num_txs_per_batch = consume_works.iter().map(|cw| cw.ids.len()).collect_vec();
        let message_hashes = consume_works
            .iter()
            .flat_map(|cw| cw.transactions.iter().map(|tx| tx.message_hash()))
            .collect_vec();
        assert_eq!(num_txs_per_batch, vec![1; 2]);
        assert_eq!(message_hashes, vec![&tx2_hash, &tx1_hash]);
    }

    #[test]
    fn test_schedule_consume_single_threaded_multi_batch() {
        let (test_frame, mut scheduler_controller) = create_test_frame(1);
        let TestFrame {
            bank,
            mint_keypair,
            poh_recorder,
            banking_packet_sender,
            consume_work_receivers,
            ..
        } = &test_frame;

        poh_recorder
            .write()
            .unwrap()
            .set_bank_for_test(bank.clone());

        // Send multiple batches - all get scheduled
        let txs1 = (0..2 * TARGET_NUM_TRANSACTIONS_PER_BATCH)
            .map(|i| {
                create_and_fund_prioritized_transfer(
                    bank,
                    mint_keypair,
                    &Keypair::new(),
                    &Pubkey::new_unique(),
                    i as u64,
                    1,
                    bank.last_blockhash(),
                )
            })
            .collect_vec();
        let txs2 = (0..2 * TARGET_NUM_TRANSACTIONS_PER_BATCH)
            .map(|i| {
                create_and_fund_prioritized_transfer(
                    bank,
                    mint_keypair,
                    &Keypair::new(),
                    &Pubkey::new_unique(),
                    i as u64,
                    2,
                    bank.last_blockhash(),
                )
            })
            .collect_vec();

        banking_packet_sender
            .send(to_banking_packet_batch(&txs1))
            .unwrap();
        banking_packet_sender
            .send(to_banking_packet_batch(&txs2))
            .unwrap();

        // We expect 4 batches to be scheduled
        test_receive_then_schedule(&mut scheduler_controller);
        let consume_works = (0..4)
            .map(|_| consume_work_receivers[0].try_recv().unwrap())
            .collect_vec();

        assert_eq!(
            consume_works.iter().map(|cw| cw.ids.len()).collect_vec(),
            vec![TARGET_NUM_TRANSACTIONS_PER_BATCH; 4]
        );
    }

    #[test]
    fn test_schedule_consume_simple_thread_selection() {
        let (test_frame, mut scheduler_controller) = create_test_frame(2);
        let TestFrame {
            bank,
            mint_keypair,
            poh_recorder,
            banking_packet_sender,
            consume_work_receivers,
            ..
        } = &test_frame;

        poh_recorder
            .write()
            .unwrap()
            .set_bank_for_test(bank.clone());

        // Send 4 transactions w/o conflicts. 2 should be scheduled on each thread
        let txs = (0..4)
            .map(|i| {
                create_and_fund_prioritized_transfer(
                    bank,
                    mint_keypair,
                    &Keypair::new(),
                    &Pubkey::new_unique(),
                    1,
                    i * 10,
                    bank.last_blockhash(),
                )
            })
            .collect_vec();
        banking_packet_sender
            .send(to_banking_packet_batch(&txs))
            .unwrap();

        // Priority Expectation:
        // Thread 0: [3, 1]
        // Thread 1: [2, 0]
        let t0_expected = [3, 1]
            .into_iter()
            .map(|i| txs[i].message().hash())
            .collect_vec();
        let t1_expected = [2, 0]
            .into_iter()
            .map(|i| txs[i].message().hash())
            .collect_vec();

        test_receive_then_schedule(&mut scheduler_controller);
        let t0_actual = consume_work_receivers[0]
            .try_recv()
            .unwrap()
            .transactions
            .iter()
            .map(|tx| *tx.message_hash())
            .collect_vec();
        let t1_actual = consume_work_receivers[1]
            .try_recv()
            .unwrap()
            .transactions
            .iter()
            .map(|tx| *tx.message_hash())
            .collect_vec();

        assert_eq!(t0_actual, t0_expected);
        assert_eq!(t1_actual, t1_expected);
    }

    #[test]
    fn test_schedule_consume_retryable() {
        let (test_frame, mut scheduler_controller) = create_test_frame(1);
        let TestFrame {
            bank,
            mint_keypair,
            poh_recorder,
            banking_packet_sender,
            consume_work_receivers,
            finished_consume_work_sender,
            ..
        } = &test_frame;

        poh_recorder
            .write()
            .unwrap()
            .set_bank_for_test(bank.clone());

        // Send packet batch to the scheduler - should do nothing until we become the leader.
        let tx1 = create_and_fund_prioritized_transfer(
            bank,
            mint_keypair,
            &Keypair::new(),
            &Pubkey::new_unique(),
            1,
            1000,
            bank.last_blockhash(),
        );
        let tx2 = create_and_fund_prioritized_transfer(
            bank,
            mint_keypair,
            &Keypair::new(),
            &Pubkey::new_unique(),
            1,
            2000,
            bank.last_blockhash(),
        );
        let tx1_hash = tx1.message().hash();
        let tx2_hash = tx2.message().hash();

        let txs = vec![tx1, tx2];
        banking_packet_sender
            .send(to_banking_packet_batch(&txs))
            .unwrap();

        test_receive_then_schedule(&mut scheduler_controller);
        let consume_work = consume_work_receivers[0].try_recv().unwrap();
        assert_eq!(consume_work.ids.len(), 2);
        assert_eq!(consume_work.transactions.len(), 2);
        let message_hashes = consume_work
            .transactions
            .iter()
            .map(|tx| tx.message_hash())
            .collect_vec();
        assert_eq!(message_hashes, vec![&tx2_hash, &tx1_hash]);

        // Complete the batch - marking the second transaction as retryable
        finished_consume_work_sender
            .send(FinishedConsumeWork {
                work: consume_work,
                retryable_indexes: vec![1],
            })
            .unwrap();

        // Transaction should be rescheduled
        test_receive_then_schedule(&mut scheduler_controller);
        let consume_work = consume_work_receivers[0].try_recv().unwrap();
        assert_eq!(consume_work.ids.len(), 1);
        assert_eq!(consume_work.transactions.len(), 1);
        let message_hashes = consume_work
            .transactions
            .iter()
            .map(|tx| tx.message_hash())
            .collect_vec();
        assert_eq!(message_hashes, vec![&tx1_hash]);
    }
}

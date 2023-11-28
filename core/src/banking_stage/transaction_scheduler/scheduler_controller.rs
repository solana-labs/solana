//! Control flow for BankingStage's transaction scheduler.
//!

use {
    super::{
        prio_graph_scheduler::PrioGraphScheduler, scheduler_error::SchedulerError,
        transaction_id_generator::TransactionIdGenerator,
        transaction_state::SanitizedTransactionTTL,
        transaction_state_container::TransactionStateContainer,
    },
    crate::banking_stage::{
        consume_worker::ConsumeWorkerMetrics,
        decision_maker::{BufferedPacketsDecision, DecisionMaker},
        immutable_deserialized_packet::ImmutableDeserializedPacket,
        packet_deserializer::PacketDeserializer,
        TOTAL_BUFFERED_PACKETS,
    },
    crossbeam_channel::RecvTimeoutError,
    solana_accounts_db::transaction_error_metrics::TransactionErrorMetrics,
    solana_measure::measure_us,
    solana_runtime::bank_forks::BankForks,
    solana_sdk::{
        clock::MAX_PROCESSING_AGE, saturating_add_assign, timing::AtomicInterval,
        transaction::SanitizedTransaction,
    },
    std::{
        sync::{Arc, RwLock},
        time::Duration,
    },
};

/// Controls packet and transaction flow into scheduler, and scheduling execution.
pub(crate) struct SchedulerController {
    /// Decision maker for determining what should be done with transactions.
    decision_maker: DecisionMaker,
    /// Packet/Transaction ingress.
    packet_receiver: PacketDeserializer,
    bank_forks: Arc<RwLock<BankForks>>,
    /// Generates unique IDs for incoming transactions.
    transaction_id_generator: TransactionIdGenerator,
    /// Container for transaction state.
    /// Shared resource between `packet_receiver` and `scheduler`.
    container: TransactionStateContainer,
    /// State for scheduling and communicating with worker threads.
    scheduler: PrioGraphScheduler,
    /// Metrics tracking counts on transactions in different states.
    count_metrics: SchedulerCountMetrics,
    /// Metrics tracking time spent in different code sections.
    timing_metrics: SchedulerTimingMetrics,
    /// Metric report handles for the worker threads.
    worker_metrics: Vec<Arc<ConsumeWorkerMetrics>>,
}

impl SchedulerController {
    pub fn new(
        decision_maker: DecisionMaker,
        packet_deserializer: PacketDeserializer,
        bank_forks: Arc<RwLock<BankForks>>,
        scheduler: PrioGraphScheduler,
        worker_metrics: Vec<Arc<ConsumeWorkerMetrics>>,
    ) -> Self {
        Self {
            decision_maker,
            packet_receiver: packet_deserializer,
            bank_forks,
            transaction_id_generator: TransactionIdGenerator::default(),
            container: TransactionStateContainer::with_capacity(TOTAL_BUFFERED_PACKETS),
            scheduler,
            count_metrics: SchedulerCountMetrics::default(),
            timing_metrics: SchedulerTimingMetrics::default(),
            worker_metrics,
        }
    }

    pub fn run(mut self) -> Result<(), SchedulerError> {
        loop {
            // BufferedPacketsDecision is shared with legacy BankingStage, which will forward
            // packets. Initially, not renaming these decision variants but the actions taken
            // are different, since new BankingStage will not forward packets.
            // For `Forward` and `ForwardAndHold`, we want to receive packets but will not
            // forward them to the next leader. In this case, `ForwardAndHold` is
            // indistiguishable from `Hold`.
            //
            // `Forward` will drop packets from the buffer instead of forwarding.
            // During receiving, since packets would be dropped from buffer anyway, we can
            // bypass sanitization and buffering and immediately drop the packets.
            let (decision, decision_time_us) =
                measure_us!(self.decision_maker.make_consume_or_forward_decision());
            saturating_add_assign!(self.timing_metrics.decision_time_us, decision_time_us);

            self.process_transactions(&decision)?;
            self.receive_completed()?;
            if !self.receive_and_buffer_packets(&decision) {
                break;
            }

            // Report metrics only if there is data.
            // Reset intervals when appropriate, regardless of report.
            let should_report = self.count_metrics.has_data();
            self.count_metrics.maybe_report_and_reset(should_report);
            self.timing_metrics.maybe_report_and_reset(should_report);
            self.worker_metrics
                .iter()
                .for_each(|metrics| metrics.maybe_report_and_reset());
        }

        Ok(())
    }

    /// Process packets based on decision.
    fn process_transactions(
        &mut self,
        decision: &BufferedPacketsDecision,
    ) -> Result<(), SchedulerError> {
        match decision {
            BufferedPacketsDecision::Consume(_bank_start) => {
                let (scheduling_summary, schedule_time_us) =
                    measure_us!(self.scheduler.schedule(&mut self.container)?);
                saturating_add_assign!(
                    self.count_metrics.num_scheduled,
                    scheduling_summary.num_scheduled
                );
                saturating_add_assign!(
                    self.count_metrics.num_unschedulable,
                    scheduling_summary.num_unschedulable
                );
                saturating_add_assign!(self.timing_metrics.schedule_time_us, schedule_time_us);
            }
            BufferedPacketsDecision::Forward => {
                let (_, clear_time_us) = measure_us!(self.clear_container());
                saturating_add_assign!(self.timing_metrics.clear_time_us, clear_time_us);
            }
            BufferedPacketsDecision::ForwardAndHold => {
                let (_, clean_time_us) = measure_us!(self.clean_queue());
                saturating_add_assign!(self.timing_metrics.clean_time_us, clean_time_us);
            }
            BufferedPacketsDecision::Hold => {}
        }

        Ok(())
    }

    /// Clears the transaction state container.
    /// This only clears pending transactions, and does **not** clear in-flight transactions.
    fn clear_container(&mut self) {
        while let Some(id) = self.container.pop() {
            self.container.remove_by_id(&id.id);
            saturating_add_assign!(self.count_metrics.num_dropped_on_clear, 1);
        }
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

            let check_results = bank.check_transactions(
                &sanitized_txs,
                &lock_results,
                MAX_PROCESSING_AGE,
                &mut error_counters,
            );

            for ((result, _nonce), id) in check_results.into_iter().zip(chunk.iter()) {
                if result.is_err() {
                    saturating_add_assign!(self.count_metrics.num_dropped_on_age_and_status, 1);
                    self.container.remove_by_id(&id.id);
                }
            }
        }
    }

    /// Receives completed transactions from the workers and updates metrics.
    fn receive_completed(&mut self) -> Result<(), SchedulerError> {
        let ((num_transactions, num_retryable), receive_completed_time_us) =
            measure_us!(self.scheduler.receive_completed(&mut self.container)?);
        saturating_add_assign!(self.count_metrics.num_finished, num_transactions);
        saturating_add_assign!(self.count_metrics.num_retryable, num_retryable);
        saturating_add_assign!(
            self.timing_metrics.receive_completed_time_us,
            receive_completed_time_us
        );
        Ok(())
    }

    /// Returns whether the packet receiver is still connected.
    fn receive_and_buffer_packets(&mut self, decision: &BufferedPacketsDecision) -> bool {
        let remaining_queue_capacity = self.container.remaining_queue_capacity();

        const MAX_PACKET_RECEIVE_TIME: Duration = Duration::from_millis(100);
        let (recv_timeout, should_buffer) = match decision {
            BufferedPacketsDecision::Consume(_) => (
                if self.container.is_empty() {
                    MAX_PACKET_RECEIVE_TIME
                } else {
                    Duration::ZERO
                },
                true,
            ),
            BufferedPacketsDecision::Forward => (MAX_PACKET_RECEIVE_TIME, false),
            BufferedPacketsDecision::ForwardAndHold | BufferedPacketsDecision::Hold => {
                (MAX_PACKET_RECEIVE_TIME, true)
            }
        };

        let (received_packet_results, receive_time_us) = measure_us!(self
            .packet_receiver
            .receive_packets(recv_timeout, remaining_queue_capacity));
        saturating_add_assign!(self.timing_metrics.receive_time_us, receive_time_us);

        match received_packet_results {
            Ok(receive_packet_results) => {
                let num_received_packets = receive_packet_results.deserialized_packets.len();
                saturating_add_assign!(self.count_metrics.num_received, num_received_packets);
                if should_buffer {
                    let (_, buffer_time_us) = measure_us!(
                        self.buffer_packets(receive_packet_results.deserialized_packets)
                    );
                    saturating_add_assign!(self.timing_metrics.buffer_time_us, buffer_time_us);
                } else {
                    saturating_add_assign!(
                        self.count_metrics.num_dropped_on_receive,
                        num_received_packets
                    );
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return false,
        }

        true
    }

    fn buffer_packets(&mut self, packets: Vec<ImmutableDeserializedPacket>) {
        // Sanitize packets, generate IDs, and insert into the container.
        let bank = self.bank_forks.read().unwrap().working_bank();
        let last_slot_in_epoch = bank.epoch_schedule().get_last_slot_in_epoch(bank.epoch());
        let transaction_account_lock_limit = bank.get_transaction_account_lock_limit();
        let feature_set = &bank.feature_set;
        let vote_only = bank.vote_only_bank();
        for packet in packets {
            let Some(transaction) =
                packet.build_sanitized_transaction(feature_set, vote_only, bank.as_ref())
            else {
                saturating_add_assign!(self.count_metrics.num_dropped_on_sanitization, 1);
                continue;
            };

            // Check transaction does not have too many or duplicate locks.
            // If it does, transaction is not valid and should be dropped here.
            if SanitizedTransaction::validate_account_locks(
                transaction.message(),
                transaction_account_lock_limit,
            )
            .is_err()
            {
                saturating_add_assign!(self.count_metrics.num_dropped_on_validate_locks, 1);
                continue;
            }

            let transaction_id = self.transaction_id_generator.next();
            let transaction_ttl = SanitizedTransactionTTL {
                transaction,
                max_age_slot: last_slot_in_epoch,
            };
            let transaction_priority_details = packet.priority_details();
            if self.container.insert_new_transaction(
                transaction_id,
                transaction_ttl,
                transaction_priority_details,
            ) {
                saturating_add_assign!(self.count_metrics.num_dropped_on_capacity, 1);
            }
            saturating_add_assign!(self.count_metrics.num_buffered, 1);
        }
    }
}

#[derive(Default)]
struct SchedulerCountMetrics {
    interval: AtomicInterval,

    /// Number of packets received.
    num_received: usize,
    /// Number of packets buffered.
    num_buffered: usize,

    /// Number of transactions scheduled.
    num_scheduled: usize,
    /// Number of transactions that were unschedulable.
    num_unschedulable: usize,
    /// Number of completed transactions received from workers.
    num_finished: usize,
    /// Number of transactions that were retryable.
    num_retryable: usize,

    /// Number of transactions that were immediately dropped on receive.
    num_dropped_on_receive: usize,
    /// Number of transactions that were dropped due to sanitization failure.
    num_dropped_on_sanitization: usize,
    /// Number of transactions that were dropped due to failed lock validation.
    num_dropped_on_validate_locks: usize,
    /// Number of transactions that were dropped due to clearing.
    num_dropped_on_clear: usize,
    /// Number of transactions that were dropped due to age and status checks.
    num_dropped_on_age_and_status: usize,
    /// Number of transactions that were dropped due to exceeded capacity.
    num_dropped_on_capacity: usize,
}

impl SchedulerCountMetrics {
    fn maybe_report_and_reset(&mut self, should_report: bool) {
        const REPORT_INTERVAL_MS: u64 = 1000;
        if self.interval.should_update(REPORT_INTERVAL_MS) {
            if should_report {
                self.report();
            }
            self.reset();
        }
    }

    fn report(&self) {
        datapoint_info!(
            "banking_stage_scheduler_counts",
            ("num_received", self.num_received, i64),
            ("num_buffered", self.num_buffered, i64),
            ("num_scheduled", self.num_scheduled, i64),
            ("num_unschedulable", self.num_unschedulable, i64),
            ("num_finished", self.num_finished, i64),
            ("num_retryable", self.num_retryable, i64),
            ("num_dropped_on_receive", self.num_dropped_on_receive, i64),
            (
                "num_dropped_on_sanitization",
                self.num_dropped_on_sanitization,
                i64
            ),
            (
                "num_dropped_on_validate_locks",
                self.num_dropped_on_validate_locks,
                i64
            ),
            ("num_dropped_on_clear", self.num_dropped_on_clear, i64),
            (
                "num_dropped_on_age_and_status",
                self.num_dropped_on_age_and_status,
                i64
            ),
            ("num_dropped_on_capacity", self.num_dropped_on_capacity, i64)
        );
    }

    fn has_data(&self) -> bool {
        self.num_received != 0
            || self.num_buffered != 0
            || self.num_scheduled != 0
            || self.num_unschedulable != 0
            || self.num_finished != 0
            || self.num_retryable != 0
            || self.num_dropped_on_receive != 0
            || self.num_dropped_on_sanitization != 0
            || self.num_dropped_on_validate_locks != 0
            || self.num_dropped_on_clear != 0
            || self.num_dropped_on_age_and_status != 0
            || self.num_dropped_on_capacity != 0
    }

    fn reset(&mut self) {
        self.num_received = 0;
        self.num_buffered = 0;
        self.num_scheduled = 0;
        self.num_unschedulable = 0;
        self.num_finished = 0;
        self.num_retryable = 0;
        self.num_dropped_on_receive = 0;
        self.num_dropped_on_sanitization = 0;
        self.num_dropped_on_validate_locks = 0;
        self.num_dropped_on_clear = 0;
        self.num_dropped_on_age_and_status = 0;
        self.num_dropped_on_capacity = 0;
    }
}

#[derive(Default)]
struct SchedulerTimingMetrics {
    interval: AtomicInterval,
    /// Time spent making processing decisions.
    decision_time_us: u64,
    /// Time spent receiving packets.
    receive_time_us: u64,
    /// Time spent buffering packets.
    buffer_time_us: u64,
    /// Time spent scheduling transactions.
    schedule_time_us: u64,
    /// Time spent clearing transactions from the container.
    clear_time_us: u64,
    /// Time spent cleaning expired or processed transactions from the container.
    clean_time_us: u64,
    /// Time spent receiving completed transactions.
    receive_completed_time_us: u64,
}

impl SchedulerTimingMetrics {
    fn maybe_report_and_reset(&mut self, should_report: bool) {
        const REPORT_INTERVAL_MS: u64 = 1000;
        if self.interval.should_update(REPORT_INTERVAL_MS) {
            if should_report {
                self.report();
            }
            self.reset();
        }
    }

    fn report(&self) {
        datapoint_info!(
            "banking_stage_scheduler_timing",
            ("decision_time_us", self.decision_time_us, i64),
            ("receive_time_us", self.receive_time_us, i64),
            ("buffer_time_us", self.buffer_time_us, i64),
            ("schedule_time_us", self.schedule_time_us, i64),
            ("clear_time_us", self.clear_time_us, i64),
            ("clean_time_us", self.clean_time_us, i64),
            (
                "receive_completed_time_us",
                self.receive_completed_time_us,
                i64
            )
        );
    }

    fn reset(&mut self) {
        self.decision_time_us = 0;
        self.receive_time_us = 0;
        self.buffer_time_us = 0;
        self.schedule_time_us = 0;
        self.clear_time_us = 0;
        self.clean_time_us = 0;
        self.receive_completed_time_us = 0;
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            banking_stage::{
                consumer::TARGET_NUM_TRANSACTIONS_PER_BATCH,
                scheduler_messages::{ConsumeWork, FinishedConsumeWork, TransactionBatchId},
                tests::create_slow_genesis_config,
            },
            banking_trace::BankingPacketBatch,
            sigverify::SigverifyTracerPacketStats,
        },
        crossbeam_channel::{unbounded, Receiver, Sender},
        itertools::Itertools,
        solana_ledger::{
            blockstore::Blockstore, genesis_utils::GenesisConfigInfo,
            get_tmp_ledger_path_auto_delete, leader_schedule_cache::LeaderScheduleCache,
        },
        solana_perf::packet::{to_packet_batches, PacketBatch, NUM_PACKETS},
        solana_poh::poh_recorder::{PohRecorder, Record, WorkingBankEntry},
        solana_runtime::{bank::Bank, bank_forks::BankForks},
        solana_sdk::{
            compute_budget::ComputeBudgetInstruction, hash::Hash, message::Message,
            poh_config::PohConfig, pubkey::Pubkey, signature::Keypair, signer::Signer,
            system_instruction, transaction::Transaction,
        },
        std::sync::{atomic::AtomicBool, Arc, RwLock},
        tempfile::TempDir,
    };

    const TEST_TIMEOUT: Duration = Duration::from_millis(1000);

    fn create_channels<T>(num: usize) -> (Vec<Sender<T>>, Vec<Receiver<T>>) {
        (0..num).map(|_| unbounded()).unzip()
    }

    // Helper struct to create tests that hold channels, files, etc.
    // such that our tests can be more easily set up and run.
    struct TestFrame {
        bank: Arc<Bank>,
        _ledger_path: TempDir,
        _entry_receiver: Receiver<WorkingBankEntry>,
        _record_receiver: Receiver<Record>,
        poh_recorder: Arc<RwLock<PohRecorder>>,
        banking_packet_sender: Sender<Arc<(Vec<PacketBatch>, Option<SigverifyTracerPacketStats>)>>,

        consume_work_receivers: Vec<Receiver<ConsumeWork>>,
        finished_consume_work_sender: Sender<FinishedConsumeWork>,
    }

    fn create_test_frame(num_threads: usize) -> (TestFrame, SchedulerController) {
        let GenesisConfigInfo { genesis_config, .. } = create_slow_genesis_config(10_000);
        let bank = Bank::new_no_wallclock_throttle_for_tests(&genesis_config);
        let bank_forks = BankForks::new_rw_arc(bank);
        let bank = bank_forks.read().unwrap().working_bank();

        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path())
            .expect("Expected to be able to open database ledger");
        let (poh_recorder, entry_receiver, record_receiver) = PohRecorder::new(
            bank.tick_height(),
            bank.last_blockhash(),
            bank.clone(),
            Some((4, 4)),
            bank.ticks_per_slot(),
            &Pubkey::new_unique(),
            Arc::new(blockstore),
            &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
            &PohConfig::default(),
            Arc::new(AtomicBool::default()),
        );
        let poh_recorder = Arc::new(RwLock::new(poh_recorder));
        let decision_maker = DecisionMaker::new(Pubkey::new_unique(), poh_recorder.clone());

        let (banking_packet_sender, banking_packet_receiver) = unbounded();
        let packet_deserializer =
            PacketDeserializer::new(banking_packet_receiver, bank_forks.clone());

        let (consume_work_senders, consume_work_receivers) = create_channels(num_threads);
        let (finished_consume_work_sender, finished_consume_work_receiver) = unbounded();

        let test_frame = TestFrame {
            bank,
            _ledger_path: ledger_path,
            _entry_receiver: entry_receiver,
            _record_receiver: record_receiver,
            poh_recorder,
            banking_packet_sender,
            consume_work_receivers,
            finished_consume_work_sender,
        };
        let scheduler_controller = SchedulerController::new(
            decision_maker,
            packet_deserializer,
            bank_forks,
            PrioGraphScheduler::new(consume_work_senders, finished_consume_work_receiver),
            vec![], // no actual workers with metrics to report, this can be empty
        );

        (test_frame, scheduler_controller)
    }

    fn prioritized_tranfer(
        from_keypair: &Keypair,
        to_pubkey: &Pubkey,
        lamports: u64,
        priority: u64,
        recent_blockhash: Hash,
    ) -> Transaction {
        let transfer = system_instruction::transfer(&from_keypair.pubkey(), to_pubkey, lamports);
        let prioritization = ComputeBudgetInstruction::set_compute_unit_price(priority);
        let message = Message::new(&[transfer, prioritization], Some(&from_keypair.pubkey()));
        Transaction::new(&vec![from_keypair], message, recent_blockhash)
    }

    fn to_banking_packet_batch(txs: &[Transaction]) -> BankingPacketBatch {
        let packet_batch = to_packet_batches(txs, NUM_PACKETS);
        Arc::new((packet_batch, None))
    }

    #[test]
    #[should_panic(expected = "batch id 0 is not being tracked")]
    fn test_unexpected_batch_id() {
        let (test_frame, central_scheduler_banking_stage) = create_test_frame(1);
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
                    max_age_slots: vec![],
                },
                retryable_indexes: vec![],
            })
            .unwrap();

        central_scheduler_banking_stage.run().unwrap();
    }

    #[test]
    fn test_schedule_consume_single_threaded_no_conflicts() {
        let (test_frame, central_scheduler_banking_stage) = create_test_frame(1);
        let TestFrame {
            bank,
            poh_recorder,
            banking_packet_sender,
            consume_work_receivers,
            ..
        } = &test_frame;

        poh_recorder
            .write()
            .unwrap()
            .set_bank_for_test(bank.clone());
        let scheduler_thread = std::thread::spawn(move || central_scheduler_banking_stage.run());

        // Send packet batch to the scheduler - should do nothing until we become the leader.
        let tx1 = prioritized_tranfer(
            &Keypair::new(),
            &Pubkey::new_unique(),
            1,
            1,
            bank.last_blockhash(),
        );
        let tx2 = prioritized_tranfer(
            &Keypair::new(),
            &Pubkey::new_unique(),
            1,
            2,
            bank.last_blockhash(),
        );
        let tx1_hash = tx1.message().hash();
        let tx2_hash = tx2.message().hash();

        let txs = vec![tx1, tx2];
        banking_packet_sender
            .send(to_banking_packet_batch(&txs))
            .unwrap();

        let consume_work = consume_work_receivers[0]
            .recv_timeout(TEST_TIMEOUT)
            .unwrap();
        assert_eq!(consume_work.ids.len(), 2);
        assert_eq!(consume_work.transactions.len(), 2);
        let message_hashes = consume_work
            .transactions
            .iter()
            .map(|tx| tx.message_hash())
            .collect_vec();
        assert_eq!(message_hashes, vec![&tx2_hash, &tx1_hash]);

        drop(test_frame);
        let _ = scheduler_thread.join();
    }

    #[test]
    fn test_schedule_consume_single_threaded_conflict() {
        let (test_frame, central_scheduler_banking_stage) = create_test_frame(1);
        let TestFrame {
            bank,
            poh_recorder,
            banking_packet_sender,
            consume_work_receivers,
            ..
        } = &test_frame;

        poh_recorder
            .write()
            .unwrap()
            .set_bank_for_test(bank.clone());
        let scheduler_thread = std::thread::spawn(move || central_scheduler_banking_stage.run());

        let pk = Pubkey::new_unique();
        let tx1 = prioritized_tranfer(&Keypair::new(), &pk, 1, 1, bank.last_blockhash());
        let tx2 = prioritized_tranfer(&Keypair::new(), &pk, 1, 2, bank.last_blockhash());
        let tx1_hash = tx1.message().hash();
        let tx2_hash = tx2.message().hash();

        let txs = vec![tx1, tx2];
        banking_packet_sender
            .send(to_banking_packet_batch(&txs))
            .unwrap();

        // We expect 2 batches to be scheduled
        let consume_works = (0..2)
            .map(|_| {
                consume_work_receivers[0]
                    .recv_timeout(TEST_TIMEOUT)
                    .unwrap()
            })
            .collect_vec();

        let num_txs_per_batch = consume_works.iter().map(|cw| cw.ids.len()).collect_vec();
        let message_hashes = consume_works
            .iter()
            .flat_map(|cw| cw.transactions.iter().map(|tx| tx.message_hash()))
            .collect_vec();
        assert_eq!(num_txs_per_batch, vec![1; 2]);
        assert_eq!(message_hashes, vec![&tx2_hash, &tx1_hash]);

        drop(test_frame);
        let _ = scheduler_thread.join();
    }

    #[test]
    fn test_schedule_consume_single_threaded_multi_batch() {
        let (test_frame, central_scheduler_banking_stage) = create_test_frame(1);
        let TestFrame {
            bank,
            poh_recorder,
            banking_packet_sender,
            consume_work_receivers,
            ..
        } = &test_frame;

        let scheduler_thread = std::thread::spawn(move || central_scheduler_banking_stage.run());
        poh_recorder
            .write()
            .unwrap()
            .set_bank_for_test(bank.clone());

        // Send multiple batches - all get scheduled
        let txs1 = (0..2 * TARGET_NUM_TRANSACTIONS_PER_BATCH)
            .map(|i| {
                prioritized_tranfer(
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
                prioritized_tranfer(
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
        let consume_works = (0..4)
            .map(|_| {
                consume_work_receivers[0]
                    .recv_timeout(TEST_TIMEOUT)
                    .unwrap()
            })
            .collect_vec();

        assert_eq!(
            consume_works.iter().map(|cw| cw.ids.len()).collect_vec(),
            vec![TARGET_NUM_TRANSACTIONS_PER_BATCH; 4]
        );

        drop(test_frame);
        let _ = scheduler_thread.join();
    }

    #[test]
    fn test_schedule_consume_simple_thread_selection() {
        let (test_frame, central_scheduler_banking_stage) = create_test_frame(2);
        let TestFrame {
            bank,
            poh_recorder,
            banking_packet_sender,
            consume_work_receivers,
            ..
        } = &test_frame;

        poh_recorder
            .write()
            .unwrap()
            .set_bank_for_test(bank.clone());
        let scheduler_thread = std::thread::spawn(move || central_scheduler_banking_stage.run());

        // Send 4 transactions w/o conflicts. 2 should be scheduled on each thread
        let txs = (0..4)
            .map(|i| {
                prioritized_tranfer(
                    &Keypair::new(),
                    &Pubkey::new_unique(),
                    1,
                    i,
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
        let t0_actual = consume_work_receivers[0]
            .recv_timeout(TEST_TIMEOUT)
            .unwrap()
            .transactions
            .iter()
            .map(|tx| *tx.message_hash())
            .collect_vec();
        let t1_actual = consume_work_receivers[1]
            .recv_timeout(TEST_TIMEOUT)
            .unwrap()
            .transactions
            .iter()
            .map(|tx| *tx.message_hash())
            .collect_vec();

        assert_eq!(t0_actual, t0_expected);
        assert_eq!(t1_actual, t1_expected);

        drop(test_frame);
        let _ = scheduler_thread.join();
    }

    #[test]
    fn test_schedule_consume_retryable() {
        let (test_frame, central_scheduler_banking_stage) = create_test_frame(1);
        let TestFrame {
            bank,
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
        let scheduler_thread = std::thread::spawn(move || central_scheduler_banking_stage.run());

        // Send packet batch to the scheduler - should do nothing until we become the leader.
        let tx1 = prioritized_tranfer(
            &Keypair::new(),
            &Pubkey::new_unique(),
            1,
            1,
            bank.last_blockhash(),
        );
        let tx2 = prioritized_tranfer(
            &Keypair::new(),
            &Pubkey::new_unique(),
            1,
            2,
            bank.last_blockhash(),
        );
        let tx1_hash = tx1.message().hash();
        let tx2_hash = tx2.message().hash();

        let txs = vec![tx1, tx2];
        banking_packet_sender
            .send(to_banking_packet_batch(&txs))
            .unwrap();

        let consume_work = consume_work_receivers[0]
            .recv_timeout(TEST_TIMEOUT)
            .unwrap();
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
        let consume_work = consume_work_receivers[0]
            .recv_timeout(TEST_TIMEOUT)
            .unwrap();
        assert_eq!(consume_work.ids.len(), 1);
        assert_eq!(consume_work.transactions.len(), 1);
        let message_hashes = consume_work
            .transactions
            .iter()
            .map(|tx| tx.message_hash())
            .collect_vec();
        assert_eq!(message_hashes, vec![&tx1_hash]);

        drop(test_frame);
        let _ = scheduler_thread.join();
    }
}

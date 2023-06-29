use {
    super::{
        multi_iterator_consume_scheduler::MultiIteratorConsumeScheduler,
        multi_iterator_forward_scheduler::MultiIteratorForwardScheduler,
        transaction_id_generator::TransactionIdGenerator,
        transaction_packet_container::{SanitizedTransactionTTL, TransactionPacketContainer},
    },
    crate::banking_stage::{
        decision_maker::{BufferedPacketsDecision, DecisionMaker},
        packet_deserializer::{PacketDeserializer, ReceivePacketResults},
        scheduler_messages::{ConsumeWork, FinishedConsumeWork, FinishedForwardWork, ForwardWork},
    },
    crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TryRecvError},
    itertools::izip,
    solana_runtime::bank_forks::BankForks,
    solana_sdk::{clock::MAX_PROCESSING_AGE, transaction::SanitizedTransaction},
    std::{
        sync::{Arc, RwLock},
        time::Duration,
    },
    thiserror::Error,
};

#[derive(Debug, Error)]
pub enum SchedulerError {
    #[error("Receiving channel disconnected: {0}")]
    DisconnectedReceiveChannel(&'static str),
    #[error("Sending channel disconnected: {0}")]
    DisconnectedSendChannel(&'static str),
}

pub struct CentralSchedulerBankingStage {
    /// Makes decision about whether to consume, forward, or do nothing with packets.
    decision_maker: DecisionMaker,

    /// BankForks for getting working bank for sanitization
    bank_forks: Arc<RwLock<BankForks>>,
    /// Receiver for packets from sigverify
    packet_deserializer: PacketDeserializer,
    /// Generator for transaction ids
    transaction_id_generator: TransactionIdGenerator,
    /// Tracks all transactions/packets within scheduler
    container: TransactionPacketContainer,

    /// Scheduler for consuming transactions
    consume_scheduler: MultiIteratorConsumeScheduler,
    /// Scheduler for forwarding transactions
    forward_scheduler: MultiIteratorForwardScheduler,

    /// Receives finished consume work from consume worker(s)
    finished_consume_work_receiver: Receiver<FinishedConsumeWork>,
    /// Receives finished forward work from forward worker(s)
    finished_forward_work_receiver: Receiver<FinishedForwardWork>,
}

impl CentralSchedulerBankingStage {
    pub fn new(
        decision_maker: DecisionMaker,
        bank_forks: Arc<RwLock<BankForks>>,
        consume_work_senders: Vec<Sender<ConsumeWork>>,
        finished_consume_work_receiver: Receiver<FinishedConsumeWork>,
        forward_work_sender: Sender<ForwardWork>,
        finished_forward_work_receiver: Receiver<FinishedForwardWork>,
        packet_deserializer: PacketDeserializer,
    ) -> Self {
        Self {
            decision_maker,
            bank_forks,
            packet_deserializer,
            transaction_id_generator: TransactionIdGenerator::default(),
            container: TransactionPacketContainer::with_capacity(700_000),
            consume_scheduler: MultiIteratorConsumeScheduler::new(consume_work_senders),
            forward_scheduler: MultiIteratorForwardScheduler::new(forward_work_sender),
            finished_consume_work_receiver,
            finished_forward_work_receiver,
        }
    }

    pub fn run(mut self) -> Result<(), SchedulerError> {
        loop {
            // If there are queued transactions/packets, make a decision about what to do with them
            // and schedule work accordingly
            if !self.container.is_empty() {
                let decision = self.decision_maker.make_consume_or_forward_decision();
                match decision {
                    BufferedPacketsDecision::Consume(_) => {
                        self.consume_scheduler.schedule(&mut self.container)?
                    }
                    BufferedPacketsDecision::Forward => self.schedule_forward(false)?,
                    BufferedPacketsDecision::ForwardAndHold => self.schedule_forward(true)?,
                    BufferedPacketsDecision::Hold => {}
                }
            }

            self.receive_and_buffer_packets()?;
            self.receive_and_process_finished_work()?;
        }
    }

    fn schedule_forward(&mut self, hold: bool) -> Result<(), SchedulerError> {
        let bank = self.bank_forks.read().unwrap().root_bank();
        self.forward_scheduler
            .schedule(&bank, &mut self.container, hold)
    }

    fn receive_and_buffer_packets(&mut self) -> Result<(), SchedulerError> {
        const EMPTY_RECEIVE_TIMEOUT: Duration = Duration::from_millis(100);
        const NON_EMPTY_RECEIVE_TIMEOUT: Duration = Duration::from_millis(0);
        let timeout = if self.container.is_empty() {
            EMPTY_RECEIVE_TIMEOUT
        } else {
            NON_EMPTY_RECEIVE_TIMEOUT
        };

        let remaining_capacity = self.container.remaining_queue_capacity();
        let receive_packet_results = self
            .packet_deserializer
            .receive_packets(timeout, remaining_capacity);

        match receive_packet_results {
            Ok(receive_packet_results) => self.sanitize_and_buffer(receive_packet_results),
            Err(RecvTimeoutError::Disconnected) => {
                return Err(SchedulerError::DisconnectedReceiveChannel(
                    "packet deserializer",
                ));
            }
            Err(RecvTimeoutError::Timeout) => {}
        }

        Ok(())
    }

    fn sanitize_and_buffer(&mut self, receive_packet_results: ReceivePacketResults) {
        let bank = self.bank_forks.read().unwrap().working_bank();
        let tx_account_lock_limit = bank.get_transaction_account_lock_limit();
        let last_slot_in_epoch = bank.epoch_schedule().get_last_slot_in_epoch(bank.epoch());
        let r_blockhash = bank.read_blockhash_queue().unwrap();

        for (packet, transaction) in receive_packet_results
            .deserialized_packets
            .into_iter()
            .filter_map(|packet| {
                packet
                    .build_sanitized_transaction(
                        &bank.feature_set,
                        bank.vote_only_bank(),
                        bank.as_ref(),
                    )
                    .map(|tx| (packet, tx))
            })
            .filter(|(_, transaction)| {
                SanitizedTransaction::validate_account_locks(
                    transaction.message(),
                    tx_account_lock_limit,
                )
                .is_ok()
            })
            .filter_map(|(packet, transaction)| {
                r_blockhash
                    .get_hash_age(transaction.message().recent_blockhash())
                    .filter(|age| *age <= MAX_PROCESSING_AGE as u64)
                    .map(|_| (packet, transaction))
            })
        {
            let transaction_ttl = SanitizedTransactionTTL {
                transaction,
                max_age_slot: last_slot_in_epoch,
            };

            let transaction_id = self.transaction_id_generator.next();
            self.container
                .insert_new_transaction(transaction_id, packet, transaction_ttl);
        }
    }

    fn receive_and_process_finished_work(&mut self) -> Result<(), SchedulerError> {
        self.receive_and_process_finished_consume_work()?;
        self.receive_and_process_finished_forward_work()
    }

    fn receive_and_process_finished_consume_work(&mut self) -> Result<(), SchedulerError> {
        loop {
            match self.finished_consume_work_receiver.try_recv() {
                Ok(finished_consume_work) => {
                    self.process_finished_consume_work(finished_consume_work)
                }
                Err(TryRecvError::Empty) => return Ok(()),
                Err(TryRecvError::Disconnected) => {
                    return Err(SchedulerError::DisconnectedReceiveChannel(
                        "finished consume work receiver",
                    ));
                }
            }
        }
    }

    fn process_finished_consume_work(
        &mut self,
        FinishedConsumeWork {
            work:
                ConsumeWork {
                    batch_id,
                    ids,
                    transactions,
                    max_age_slots,
                },
            retryable_indexes,
        }: FinishedConsumeWork,
    ) {
        self.consume_scheduler
            .complete_batch(batch_id, &transactions);

        let mut retryable_id_iter = retryable_indexes.into_iter().peekable();
        for (index, (id, transaction, max_age_slot)) in
            izip!(ids, transactions, max_age_slots).enumerate()
        {
            match retryable_id_iter.peek() {
                Some(retryable_index) if index == *retryable_index => {
                    self.container
                        .retry_transaction(id, transaction, max_age_slot);
                    retryable_id_iter.next(); // advance the iterator
                }
                _ => {}
            }
        }
    }

    fn receive_and_process_finished_forward_work(&mut self) -> Result<(), SchedulerError> {
        loop {
            match self.finished_forward_work_receiver.try_recv() {
                Ok(finished_forward_work) => {
                    self.process_finished_forward_work(finished_forward_work)
                }
                Err(TryRecvError::Empty) => return Ok(()),
                Err(TryRecvError::Disconnected) => {
                    return Err(SchedulerError::DisconnectedReceiveChannel(
                        "finished forward work receiver",
                    ));
                }
            }
        }
    }

    fn process_finished_forward_work(
        &mut self,
        FinishedForwardWork {
            work: ForwardWork { ids, packets: _ },
            successful,
        }: FinishedForwardWork,
    ) {
        if successful {
            for id in ids {
                if let Some(deserialized_packet) = self.container.get_mut_packet(&id) {
                    deserialized_packet.forwarded = true;
                } else {
                    // If a packet is not in the map, then it was forwarded *without* holding
                    // and this can return early without iterating over the remaining ids.
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            banking_stage::{
                consumer::TARGET_NUM_TRANSACTIONS_PER_BATCH,
                scheduler_messages::TransactionBatchId, tests::create_slow_genesis_config,
            },
            banking_trace::BankingPacketBatch,
            sigverify::SigverifyTracerPacketStats,
        },
        crossbeam_channel::unbounded,
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
        ledger_path: TempDir,
        entry_receiver: Receiver<WorkingBankEntry>,
        record_receiver: Receiver<Record>,
        poh_recorder: Arc<RwLock<PohRecorder>>,
        banking_packet_sender: Sender<Arc<(Vec<PacketBatch>, Option<SigverifyTracerPacketStats>)>>,

        consume_work_receivers: Vec<Receiver<ConsumeWork>>,
        finished_consume_work_sender: Sender<FinishedConsumeWork>,
        forward_work_receiver: Receiver<ForwardWork>,
        finished_forward_work_sender: Sender<FinishedForwardWork>,
    }

    fn create_test_frame(num_threads: usize) -> (TestFrame, CentralSchedulerBankingStage) {
        let GenesisConfigInfo { genesis_config, .. } = create_slow_genesis_config(10_000);
        let bank = Bank::new_no_wallclock_throttle_for_tests(&genesis_config);
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
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
        let (forward_work_sender, forward_work_receiver) = unbounded();
        let (finished_forward_work_sender, finished_forward_work_receiver) = unbounded();

        let test_frame = TestFrame {
            bank,
            ledger_path,
            entry_receiver,
            record_receiver,
            poh_recorder,
            banking_packet_sender,
            consume_work_receivers,
            finished_consume_work_sender,
            forward_work_receiver,
            finished_forward_work_sender,
        };
        let central_scheduler_banking_stage = CentralSchedulerBankingStage::new(
            decision_maker,
            bank_forks,
            consume_work_senders,
            finished_consume_work_receiver,
            forward_work_sender,
            finished_forward_work_receiver,
            packet_deserializer,
        );

        (test_frame, central_scheduler_banking_stage)
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
    #[should_panic(expected = "transaction batch id should exist in in-flight tracker")]
    fn test_unexpected_batch_id() {
        let (test_frame, mut central_scheduler_banking_stage) = create_test_frame(1);
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

        central_scheduler_banking_stage
            .receive_and_process_finished_work()
            .unwrap();
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

        // set bank
        poh_recorder.write().unwrap().set_bank(bank.clone(), false);
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
    fn test_schedule_consume_single_threaded_no_conflicts_in_progress() {
        let (test_frame, central_scheduler_banking_stage) = create_test_frame(1);
        let TestFrame {
            bank,
            poh_recorder,
            banking_packet_sender,
            consume_work_receivers,
            ..
        } = &test_frame;

        let scheduler_thread = std::thread::spawn(move || central_scheduler_banking_stage.run());

        // set bank before sending packets - should still be scheduled even while already leader
        poh_recorder.write().unwrap().set_bank(bank.clone(), false);

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
        // transactions appear in priority order - even though there are no conflicts
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

        let scheduler_thread = std::thread::spawn(move || central_scheduler_banking_stage.run());
        poh_recorder.write().unwrap().set_bank(bank.clone(), false);

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
        poh_recorder.write().unwrap().set_bank(bank.clone(), false);

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
        let scheduler_thread = std::thread::spawn(move || central_scheduler_banking_stage.run());
        poh_recorder.write().unwrap().set_bank(bank.clone(), false);

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
    fn test_schedule_consume_non_schedulable() {
        let (test_frame, central_scheduler_banking_stage) = create_test_frame(2);
        let TestFrame {
            bank,
            poh_recorder,
            banking_packet_sender,
            consume_work_receivers,
            finished_consume_work_sender,
            ..
        } = &test_frame;
        let scheduler_thread = std::thread::spawn(move || central_scheduler_banking_stage.run());
        poh_recorder.write().unwrap().set_bank(bank.clone(), false);

        let accounts = (0..4).map(|_| Keypair::new()).collect_vec();

        // high priority transactions [0, 1] do not conflict, and should be
        // scheduled to *different* threads.
        // low priority transaction [2] conflicts with both, and thus will
        // not be schedulable until one of the previous transactions is
        // completed.
        let txs = vec![
            prioritized_tranfer(
                &accounts[0],
                &accounts[1].pubkey(),
                1,
                2,
                bank.last_blockhash(),
            ),
            prioritized_tranfer(
                &accounts[2],
                &accounts[3].pubkey(),
                1,
                1,
                bank.last_blockhash(),
            ),
            prioritized_tranfer(
                &accounts[1],
                &accounts[2].pubkey(),
                1,
                0,
                bank.last_blockhash(),
            ),
        ];
        banking_packet_sender
            .send(to_banking_packet_batch(&txs))
            .unwrap();

        // Initial batches expectation:
        // Thread 0: [3, 1]
        // Thread 1: [2, 0]
        let t0_expected = [0]
            .into_iter()
            .map(|i| txs[i].message().hash())
            .collect_vec();
        let t1_expected = [1]
            .into_iter()
            .map(|i| txs[i].message().hash())
            .collect_vec();
        let t0_work = consume_work_receivers[0]
            .recv_timeout(TEST_TIMEOUT)
            .unwrap();
        let t1_work = consume_work_receivers[1]
            .recv_timeout(TEST_TIMEOUT)
            .unwrap();

        let t0_actual = t0_work
            .transactions
            .iter()
            .map(|tx| *tx.message_hash())
            .collect_vec();
        let t1_actual = t1_work
            .transactions
            .iter()
            .map(|tx| *tx.message_hash())
            .collect_vec();

        assert_eq!(t0_actual, t0_expected);
        assert_eq!(t1_actual, t1_expected);

        // Complete t1's batch - t0 should not be schedulable
        finished_consume_work_sender
            .send(FinishedConsumeWork {
                work: t1_work,
                retryable_indexes: vec![],
            })
            .unwrap();

        // t0 should not be scheduled for the remaining transaction
        let remaining_expected = [2]
            .into_iter()
            .map(|i| txs[i].message().hash())
            .collect_vec();
        let remaining_actual = consume_work_receivers[0]
            .recv_timeout(TEST_TIMEOUT)
            .unwrap()
            .transactions
            .iter()
            .map(|tx| *tx.message_hash())
            .collect_vec();
        assert_eq!(remaining_actual, remaining_expected);

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

        // set bank
        poh_recorder.write().unwrap().set_bank(bank.clone(), false);
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

    #[test]
    fn test_schedule_forward() {
        let (test_frame, central_scheduler_banking_stage) = create_test_frame(1);
        let TestFrame {
            bank,
            banking_packet_sender,
            forward_work_receiver,
            finished_forward_work_sender,
            ..
        } = &test_frame;

        let scheduler_thread = std::thread::spawn(move || central_scheduler_banking_stage.run());

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
        let forward_works = (0..4)
            .map(|_| forward_work_receiver.recv_timeout(TEST_TIMEOUT).unwrap())
            .collect_vec();

        assert_eq!(
            forward_works.iter().map(|cw| cw.ids.len()).collect_vec(),
            vec![TARGET_NUM_TRANSACTIONS_PER_BATCH; 4]
        );
        for forward_work in forward_works.into_iter() {
            finished_forward_work_sender
                .send(FinishedForwardWork {
                    work: forward_work,
                    successful: true,
                })
                .unwrap();
        }

        drop(test_frame);
        let _ = scheduler_thread.join();
    }

    #[test]
    fn test_schedule_forward_conflicts() {
        let (test_frame, central_scheduler_banking_stage) = create_test_frame(1);
        let TestFrame {
            bank,
            banking_packet_sender,
            forward_work_receiver,
            ..
        } = &test_frame;

        let scheduler_thread = std::thread::spawn(move || central_scheduler_banking_stage.run());

        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();

        let txs = vec![
            prioritized_tranfer(&keypair1, &keypair2.pubkey(), 1, 2, bank.last_blockhash()),
            prioritized_tranfer(&keypair2, &keypair1.pubkey(), 1, 1, bank.last_blockhash()),
        ];

        banking_packet_sender
            .send(to_banking_packet_batch(&txs))
            .unwrap();

        // We expect 2 batches to be scheduled since the transactions conflict
        let forward_works = (0..2)
            .map(|_| forward_work_receiver.recv_timeout(TEST_TIMEOUT).unwrap())
            .collect_vec();

        let expected_hashes = txs.iter().map(|tx| tx.message().hash()).collect_vec();
        let actual_hashes = forward_works
            .iter()
            .flat_map(|fw| fw.packets.iter())
            .map(|p| *p.message_hash())
            .collect_vec();
        assert_eq!(expected_hashes, actual_hashes,);

        drop(test_frame);
        let _ = scheduler_thread.join();
    }
}

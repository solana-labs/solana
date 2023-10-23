use {
    super::{
        consumer::{Consumer, ExecuteAndCommitTransactionsOutput, ProcessTransactionBatchOutput},
        scheduler_messages::{ConsumeWork, FinishedConsumeWork},
    },
    crossbeam_channel::{Receiver, RecvError, SendError, Sender},
    solana_poh::leader_bank_notifier::LeaderBankNotifier,
    solana_runtime::bank::Bank,
    std::{sync::Arc, time::Duration},
    thiserror::Error,
};

#[derive(Debug, Error)]
pub enum ConsumeWorkerError {
    #[error("Failed to receive work from scheduler: {0}")]
    Recv(#[from] RecvError),
    #[error("Failed to send finalized consume work to scheduler: {0}")]
    Send(#[from] SendError<FinishedConsumeWork>),
}

pub(crate) struct ConsumeWorker {
    consume_receiver: Receiver<ConsumeWork>,
    consumer: Consumer,
    consumed_sender: Sender<FinishedConsumeWork>,

    leader_bank_notifier: Arc<LeaderBankNotifier>,
}

#[allow(dead_code)]
impl ConsumeWorker {
    pub fn new(
        consume_receiver: Receiver<ConsumeWork>,
        consumer: Consumer,
        consumed_sender: Sender<FinishedConsumeWork>,
        leader_bank_notifier: Arc<LeaderBankNotifier>,
    ) -> Self {
        Self {
            consume_receiver,
            consumer,
            consumed_sender,
            leader_bank_notifier,
        }
    }

    pub fn run(self) -> Result<(), ConsumeWorkerError> {
        loop {
            let work = self.consume_receiver.recv()?;
            self.consume_loop(work)?;
        }
    }

    fn consume_loop(&self, work: ConsumeWork) -> Result<(), ConsumeWorkerError> {
        let Some(mut bank) = self.get_consume_bank() else {
            return self.retry_drain(work);
        };

        for work in try_drain_iter(work, &self.consume_receiver) {
            if bank.is_complete() {
                if let Some(new_bank) = self.get_consume_bank() {
                    bank = new_bank;
                } else {
                    return self.retry_drain(work);
                }
            }
            self.consume(&bank, work)?;
        }

        Ok(())
    }

    /// Consume a single batch.
    fn consume(&self, bank: &Arc<Bank>, work: ConsumeWork) -> Result<(), ConsumeWorkerError> {
        let ProcessTransactionBatchOutput {
            execute_and_commit_transactions_output:
                ExecuteAndCommitTransactionsOutput {
                    retryable_transaction_indexes,
                    ..
                },
            ..
        } = self.consumer.process_and_record_aged_transactions(
            bank,
            &work.transactions,
            &work.max_age_slots,
        );

        self.consumed_sender.send(FinishedConsumeWork {
            work,
            retryable_indexes: retryable_transaction_indexes,
        })?;
        Ok(())
    }

    /// Try to get a bank for consuming.
    fn get_consume_bank(&self) -> Option<Arc<Bank>> {
        self.leader_bank_notifier
            .get_or_wait_for_in_progress(Duration::from_millis(50))
            .upgrade()
    }

    /// Retry current batch and all outstanding batches.
    fn retry_drain(&self, work: ConsumeWork) -> Result<(), ConsumeWorkerError> {
        for work in try_drain_iter(work, &self.consume_receiver) {
            self.retry(work)?;
        }
        Ok(())
    }

    /// Send transactions back to scheduler as retryable.
    fn retry(&self, work: ConsumeWork) -> Result<(), ConsumeWorkerError> {
        let retryable_indexes = (0..work.transactions.len()).collect();
        self.consumed_sender.send(FinishedConsumeWork {
            work,
            retryable_indexes,
        })?;
        Ok(())
    }
}

/// Helper function to create an non-blocking iterator over work in the receiver,
/// starting with the given work item.
fn try_drain_iter<T>(work: T, receiver: &Receiver<T>) -> impl Iterator<Item = T> + '_ {
    std::iter::once(work).chain(receiver.try_iter())
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::banking_stage::{
            committer::Committer,
            qos_service::QosService,
            scheduler_messages::{TransactionBatchId, TransactionId},
            tests::{create_slow_genesis_config, sanitize_transactions, simulate_poh},
        },
        crossbeam_channel::unbounded,
        solana_ledger::{
            blockstore::Blockstore, genesis_utils::GenesisConfigInfo,
            get_tmp_ledger_path_auto_delete, leader_schedule_cache::LeaderScheduleCache,
        },
        solana_poh::poh_recorder::{PohRecorder, WorkingBankEntry},
        solana_runtime::{bank_forks::BankForks, prioritization_fee_cache::PrioritizationFeeCache},
        solana_sdk::{
            genesis_config::GenesisConfig, poh_config::PohConfig, pubkey::Pubkey,
            signature::Keypair, system_transaction,
        },
        solana_vote::vote_sender_types::ReplayVoteReceiver,
        std::{
            sync::{atomic::AtomicBool, RwLock},
            thread::JoinHandle,
        },
        tempfile::TempDir,
    };

    // Helper struct to create tests that hold channels, files, etc.
    // such that our tests can be more easily set up and run.
    struct TestFrame {
        mint_keypair: Keypair,
        genesis_config: GenesisConfig,
        bank: Arc<Bank>,
        _ledger_path: TempDir,
        _entry_receiver: Receiver<WorkingBankEntry>,
        poh_recorder: Arc<RwLock<PohRecorder>>,
        _poh_simulator: JoinHandle<()>,
        _replay_vote_receiver: ReplayVoteReceiver,

        consume_sender: Sender<ConsumeWork>,
        consumed_receiver: Receiver<FinishedConsumeWork>,
    }

    fn setup_test_frame() -> (TestFrame, ConsumeWorker) {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_slow_genesis_config(10_000);
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
        let recorder = poh_recorder.new_recorder();
        let poh_recorder = Arc::new(RwLock::new(poh_recorder));
        let poh_simulator = simulate_poh(record_receiver, &poh_recorder);

        let (replay_vote_sender, replay_vote_receiver) = unbounded();
        let committer = Committer::new(
            None,
            replay_vote_sender,
            Arc::new(PrioritizationFeeCache::new(0u64)),
        );
        let consumer = Consumer::new(committer, recorder, QosService::new(1), None);

        let (consume_sender, consume_receiver) = unbounded();
        let (consumed_sender, consumed_receiver) = unbounded();
        let worker = ConsumeWorker::new(
            consume_receiver,
            consumer,
            consumed_sender,
            poh_recorder.read().unwrap().new_leader_bank_notifier(),
        );

        (
            TestFrame {
                mint_keypair,
                genesis_config,
                bank,
                _ledger_path: ledger_path,
                _entry_receiver: entry_receiver,
                poh_recorder,
                _poh_simulator: poh_simulator,
                _replay_vote_receiver: replay_vote_receiver,
                consume_sender,
                consumed_receiver,
            },
            worker,
        )
    }

    #[test]
    fn test_worker_consume_no_bank() {
        let (test_frame, worker) = setup_test_frame();
        let TestFrame {
            mint_keypair,
            genesis_config,
            bank,
            consume_sender,
            consumed_receiver,
            ..
        } = &test_frame;
        let worker_thread = std::thread::spawn(move || worker.run());

        let pubkey1 = Pubkey::new_unique();

        let transactions = sanitize_transactions(vec![system_transaction::transfer(
            mint_keypair,
            &pubkey1,
            1,
            genesis_config.hash(),
        )]);
        let bid = TransactionBatchId::new(0);
        let id = TransactionId::new(0);
        let work = ConsumeWork {
            batch_id: bid,
            ids: vec![id],
            transactions,
            max_age_slots: vec![bank.slot()],
        };
        consume_sender.send(work).unwrap();
        let consumed = consumed_receiver.recv().unwrap();
        assert_eq!(consumed.work.batch_id, bid);
        assert_eq!(consumed.work.ids, vec![id]);
        assert_eq!(consumed.work.max_age_slots, vec![bank.slot()]);
        assert_eq!(consumed.retryable_indexes, vec![0]);

        drop(test_frame);
        let _ = worker_thread.join().unwrap();
    }

    #[test]
    fn test_worker_consume_simple() {
        let (test_frame, worker) = setup_test_frame();
        let TestFrame {
            mint_keypair,
            genesis_config,
            bank,
            poh_recorder,
            consume_sender,
            consumed_receiver,
            ..
        } = &test_frame;
        let worker_thread = std::thread::spawn(move || worker.run());
        poh_recorder.write().unwrap().set_bank(bank.clone(), false);

        let pubkey1 = Pubkey::new_unique();

        let transactions = sanitize_transactions(vec![system_transaction::transfer(
            mint_keypair,
            &pubkey1,
            1,
            genesis_config.hash(),
        )]);
        let bid = TransactionBatchId::new(0);
        let id = TransactionId::new(0);
        let work = ConsumeWork {
            batch_id: bid,
            ids: vec![id],
            transactions,
            max_age_slots: vec![bank.slot()],
        };
        consume_sender.send(work).unwrap();
        let consumed = consumed_receiver.recv().unwrap();
        assert_eq!(consumed.work.batch_id, bid);
        assert_eq!(consumed.work.ids, vec![id]);
        assert_eq!(consumed.work.max_age_slots, vec![bank.slot()]);
        assert_eq!(consumed.retryable_indexes, Vec::<usize>::new());

        drop(test_frame);
        let _ = worker_thread.join().unwrap();
    }

    #[test]
    fn test_worker_consume_self_conflicting() {
        let (test_frame, worker) = setup_test_frame();
        let TestFrame {
            mint_keypair,
            genesis_config,
            bank,
            poh_recorder,
            consume_sender,
            consumed_receiver,
            ..
        } = &test_frame;
        let worker_thread = std::thread::spawn(move || worker.run());
        poh_recorder.write().unwrap().set_bank(bank.clone(), false);

        let pubkey1 = Pubkey::new_unique();
        let pubkey2 = Pubkey::new_unique();

        let txs = sanitize_transactions(vec![
            system_transaction::transfer(mint_keypair, &pubkey1, 2, genesis_config.hash()),
            system_transaction::transfer(mint_keypair, &pubkey2, 2, genesis_config.hash()),
        ]);

        let bid = TransactionBatchId::new(0);
        let id1 = TransactionId::new(1);
        let id2 = TransactionId::new(0);
        consume_sender
            .send(ConsumeWork {
                batch_id: bid,
                ids: vec![id1, id2],
                transactions: txs,
                max_age_slots: vec![bank.slot(), bank.slot()],
            })
            .unwrap();

        let consumed = consumed_receiver.recv().unwrap();
        assert_eq!(consumed.work.batch_id, bid);
        assert_eq!(consumed.work.ids, vec![id1, id2]);
        assert_eq!(consumed.work.max_age_slots, vec![bank.slot(), bank.slot()]);
        assert_eq!(consumed.retryable_indexes, vec![1]); // id2 is retryable since lock conflict

        drop(test_frame);
        let _ = worker_thread.join().unwrap();
    }

    #[test]
    fn test_worker_consume_multiple_messages() {
        let (test_frame, worker) = setup_test_frame();
        let TestFrame {
            mint_keypair,
            genesis_config,
            bank,
            poh_recorder,
            consume_sender,
            consumed_receiver,
            ..
        } = &test_frame;
        let worker_thread = std::thread::spawn(move || worker.run());
        poh_recorder.write().unwrap().set_bank(bank.clone(), false);

        let pubkey1 = Pubkey::new_unique();
        let pubkey2 = Pubkey::new_unique();

        let txs1 = sanitize_transactions(vec![system_transaction::transfer(
            mint_keypair,
            &pubkey1,
            2,
            genesis_config.hash(),
        )]);
        let txs2 = sanitize_transactions(vec![system_transaction::transfer(
            mint_keypair,
            &pubkey2,
            2,
            genesis_config.hash(),
        )]);

        let bid1 = TransactionBatchId::new(0);
        let bid2 = TransactionBatchId::new(1);
        let id1 = TransactionId::new(1);
        let id2 = TransactionId::new(0);
        consume_sender
            .send(ConsumeWork {
                batch_id: bid1,
                ids: vec![id1],
                transactions: txs1,
                max_age_slots: vec![bank.slot()],
            })
            .unwrap();

        consume_sender
            .send(ConsumeWork {
                batch_id: bid2,
                ids: vec![id2],
                transactions: txs2,
                max_age_slots: vec![bank.slot()],
            })
            .unwrap();
        let consumed = consumed_receiver.recv().unwrap();
        assert_eq!(consumed.work.batch_id, bid1);
        assert_eq!(consumed.work.ids, vec![id1]);
        assert_eq!(consumed.work.max_age_slots, vec![bank.slot()]);
        assert_eq!(consumed.retryable_indexes, Vec::<usize>::new());

        let consumed = consumed_receiver.recv().unwrap();
        assert_eq!(consumed.work.batch_id, bid2);
        assert_eq!(consumed.work.ids, vec![id2]);
        assert_eq!(consumed.work.max_age_slots, vec![bank.slot()]);
        assert_eq!(consumed.retryable_indexes, Vec::<usize>::new());

        drop(test_frame);
        let _ = worker_thread.join().unwrap();
    }
}

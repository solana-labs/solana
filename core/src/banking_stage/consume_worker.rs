use {
    super::{
        consumer::{Consumer, ExecuteAndCommitTransactionsOutput, ProcessTransactionBatchOutput},
        leader_slot_timing_metrics::LeaderExecuteAndCommitTimings,
        scheduler_messages::{ConsumeWork, FinishedConsumeWork},
    },
    crossbeam_channel::{Receiver, RecvError, SendError, Sender},
    solana_measure::measure_us,
    solana_poh::leader_bank_notifier::LeaderBankNotifier,
    solana_runtime::bank::Bank,
    solana_sdk::timing::AtomicInterval,
    solana_svm::transaction_error_metrics::TransactionErrorMetrics,
    std::{
        sync::{
            atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
            Arc,
        },
        time::Duration,
    },
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
    metrics: Arc<ConsumeWorkerMetrics>,
}

impl ConsumeWorker {
    pub fn new(
        id: u32,
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
            metrics: Arc::new(ConsumeWorkerMetrics::new(id)),
        }
    }

    pub fn metrics_handle(&self) -> Arc<ConsumeWorkerMetrics> {
        self.metrics.clone()
    }

    pub fn run(self) -> Result<(), ConsumeWorkerError> {
        loop {
            let work = self.consume_receiver.recv()?;
            self.consume_loop(work)?;
        }
    }

    fn consume_loop(&self, work: ConsumeWork) -> Result<(), ConsumeWorkerError> {
        let (maybe_consume_bank, get_bank_us) = measure_us!(self.get_consume_bank());
        let Some(mut bank) = maybe_consume_bank else {
            self.metrics
                .timing_metrics
                .wait_for_bank_failure_us
                .fetch_add(get_bank_us, Ordering::Relaxed);
            return self.retry_drain(work);
        };
        self.metrics
            .timing_metrics
            .wait_for_bank_success_us
            .fetch_add(get_bank_us, Ordering::Relaxed);

        for work in try_drain_iter(work, &self.consume_receiver) {
            if bank.is_complete() {
                let (maybe_new_bank, get_bank_us) = measure_us!(self.get_consume_bank());
                if let Some(new_bank) = maybe_new_bank {
                    self.metrics
                        .timing_metrics
                        .wait_for_bank_success_us
                        .fetch_add(get_bank_us, Ordering::Relaxed);
                    bank = new_bank;
                } else {
                    self.metrics
                        .timing_metrics
                        .wait_for_bank_failure_us
                        .fetch_add(get_bank_us, Ordering::Relaxed);
                    return self.retry_drain(work);
                }
            }
            self.consume(&bank, work)?;
        }

        Ok(())
    }

    /// Consume a single batch.
    fn consume(&self, bank: &Arc<Bank>, work: ConsumeWork) -> Result<(), ConsumeWorkerError> {
        let output = self.consumer.process_and_record_aged_transactions(
            bank,
            &work.transactions,
            &work.max_age_slots,
        );

        self.metrics.update_for_consume(&output);
        self.metrics.has_data.store(true, Ordering::Relaxed);

        self.consumed_sender.send(FinishedConsumeWork {
            work,
            retryable_indexes: output
                .execute_and_commit_transactions_output
                .retryable_transaction_indexes,
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
        let retryable_indexes: Vec<_> = (0..work.transactions.len()).collect();
        let num_retryable = retryable_indexes.len();
        self.metrics
            .count_metrics
            .retryable_transaction_count
            .fetch_add(num_retryable, Ordering::Relaxed);
        self.metrics
            .count_metrics
            .retryable_expired_bank_count
            .fetch_add(num_retryable, Ordering::Relaxed);
        self.metrics.has_data.store(true, Ordering::Relaxed);
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

/// Metrics tracking number of packets processed by the consume worker.
/// These are atomic, and intended to be reported by the scheduling thread
/// since the consume worker thread is sleeping unless there is work to be
/// done.
pub(crate) struct ConsumeWorkerMetrics {
    id: String,
    interval: AtomicInterval,
    has_data: AtomicBool,

    count_metrics: ConsumeWorkerCountMetrics,
    error_metrics: ConsumeWorkerTransactionErrorMetrics,
    timing_metrics: ConsumeWorkerTimingMetrics,
}

impl ConsumeWorkerMetrics {
    /// Report and reset metrics iff the interval has elapsed and the worker did some work.
    pub fn maybe_report_and_reset(&self) {
        const REPORT_INTERVAL_MS: u64 = 1000;
        if self.interval.should_update(REPORT_INTERVAL_MS)
            && self.has_data.swap(false, Ordering::Relaxed)
        {
            self.count_metrics.report_and_reset(&self.id);
            self.timing_metrics.report_and_reset(&self.id);
            self.error_metrics.report_and_reset(&self.id);
        }
    }

    fn new(id: u32) -> Self {
        Self {
            id: id.to_string(),
            interval: AtomicInterval::default(),
            has_data: AtomicBool::new(false),
            count_metrics: ConsumeWorkerCountMetrics::default(),
            error_metrics: ConsumeWorkerTransactionErrorMetrics::default(),
            timing_metrics: ConsumeWorkerTimingMetrics::default(),
        }
    }

    fn update_for_consume(
        &self,
        ProcessTransactionBatchOutput {
            cost_model_throttled_transactions_count,
            cost_model_us,
            execute_and_commit_transactions_output,
        }: &ProcessTransactionBatchOutput,
    ) {
        self.count_metrics
            .cost_model_throttled_transactions_count
            .fetch_add(*cost_model_throttled_transactions_count, Ordering::Relaxed);
        self.timing_metrics
            .cost_model_us
            .fetch_add(*cost_model_us, Ordering::Relaxed);
        self.update_on_execute_and_commit_transactions_output(
            execute_and_commit_transactions_output,
        );
    }

    fn update_on_execute_and_commit_transactions_output(
        &self,
        ExecuteAndCommitTransactionsOutput {
            transaction_counts,
            retryable_transaction_indexes,
            execute_and_commit_timings,
            error_counters,
            min_prioritization_fees,
            max_prioritization_fees,
            ..
        }: &ExecuteAndCommitTransactionsOutput,
    ) {
        self.count_metrics
            .transactions_attempted_execution_count
            .fetch_add(
                transaction_counts.attempted_execution_count,
                Ordering::Relaxed,
            );
        self.count_metrics
            .executed_transactions_count
            .fetch_add(transaction_counts.executed_count, Ordering::Relaxed);
        self.count_metrics
            .executed_with_successful_result_count
            .fetch_add(
                transaction_counts.executed_with_successful_result_count,
                Ordering::Relaxed,
            );
        self.count_metrics
            .retryable_transaction_count
            .fetch_add(retryable_transaction_indexes.len(), Ordering::Relaxed);
        let min_prioritization_fees = self
            .count_metrics
            .min_prioritization_fees
            .fetch_min(*min_prioritization_fees, Ordering::Relaxed);
        let max_prioritization_fees = self
            .count_metrics
            .max_prioritization_fees
            .fetch_max(*max_prioritization_fees, Ordering::Relaxed);
        self.count_metrics
            .min_prioritization_fees
            .swap(min_prioritization_fees, Ordering::Relaxed);
        self.count_metrics
            .max_prioritization_fees
            .swap(max_prioritization_fees, Ordering::Relaxed);
        self.update_on_execute_and_commit_timings(execute_and_commit_timings);
        self.update_on_error_counters(error_counters);
    }

    fn update_on_execute_and_commit_timings(
        &self,
        LeaderExecuteAndCommitTimings {
            collect_balances_us,
            load_execute_us,
            freeze_lock_us,
            last_blockhash_us,
            record_us,
            commit_us,
            find_and_send_votes_us,
            ..
        }: &LeaderExecuteAndCommitTimings,
    ) {
        self.timing_metrics
            .collect_balances_us
            .fetch_add(*collect_balances_us, Ordering::Relaxed);
        self.timing_metrics
            .load_execute_us
            .fetch_add(*load_execute_us, Ordering::Relaxed);
        self.timing_metrics
            .freeze_lock_us
            .fetch_add(*freeze_lock_us, Ordering::Relaxed);
        self.timing_metrics
            .last_blockhash_us
            .fetch_add(*last_blockhash_us, Ordering::Relaxed);
        self.timing_metrics
            .record_us
            .fetch_add(*record_us, Ordering::Relaxed);
        self.timing_metrics
            .commit_us
            .fetch_add(*commit_us, Ordering::Relaxed);
        self.timing_metrics
            .find_and_send_votes_us
            .fetch_add(*find_and_send_votes_us, Ordering::Relaxed);
    }

    fn update_on_error_counters(
        &self,
        TransactionErrorMetrics {
            total,
            account_in_use,
            too_many_account_locks,
            account_loaded_twice,
            account_not_found,
            blockhash_not_found,
            blockhash_too_old,
            call_chain_too_deep,
            already_processed,
            instruction_error,
            insufficient_funds,
            invalid_account_for_fee,
            invalid_account_index,
            invalid_program_for_execution,
            invalid_compute_budget,
            not_allowed_during_cluster_maintenance,
            invalid_writable_account,
            invalid_rent_paying_account,
            would_exceed_max_block_cost_limit,
            would_exceed_max_account_cost_limit,
            would_exceed_max_vote_cost_limit,
            would_exceed_account_data_block_limit,
            max_loaded_accounts_data_size_exceeded,
            program_execution_temporarily_restricted,
        }: &TransactionErrorMetrics,
    ) {
        self.error_metrics
            .total
            .fetch_add(*total, Ordering::Relaxed);
        self.error_metrics
            .account_in_use
            .fetch_add(*account_in_use, Ordering::Relaxed);
        self.error_metrics
            .too_many_account_locks
            .fetch_add(*too_many_account_locks, Ordering::Relaxed);
        self.error_metrics
            .account_loaded_twice
            .fetch_add(*account_loaded_twice, Ordering::Relaxed);
        self.error_metrics
            .account_not_found
            .fetch_add(*account_not_found, Ordering::Relaxed);
        self.error_metrics
            .blockhash_not_found
            .fetch_add(*blockhash_not_found, Ordering::Relaxed);
        self.error_metrics
            .blockhash_too_old
            .fetch_add(*blockhash_too_old, Ordering::Relaxed);
        self.error_metrics
            .call_chain_too_deep
            .fetch_add(*call_chain_too_deep, Ordering::Relaxed);
        self.error_metrics
            .already_processed
            .fetch_add(*already_processed, Ordering::Relaxed);
        self.error_metrics
            .instruction_error
            .fetch_add(*instruction_error, Ordering::Relaxed);
        self.error_metrics
            .insufficient_funds
            .fetch_add(*insufficient_funds, Ordering::Relaxed);
        self.error_metrics
            .invalid_account_for_fee
            .fetch_add(*invalid_account_for_fee, Ordering::Relaxed);
        self.error_metrics
            .invalid_account_index
            .fetch_add(*invalid_account_index, Ordering::Relaxed);
        self.error_metrics
            .invalid_program_for_execution
            .fetch_add(*invalid_program_for_execution, Ordering::Relaxed);
        self.error_metrics
            .invalid_compute_budget
            .fetch_add(*invalid_compute_budget, Ordering::Relaxed);
        self.error_metrics
            .not_allowed_during_cluster_maintenance
            .fetch_add(*not_allowed_during_cluster_maintenance, Ordering::Relaxed);
        self.error_metrics
            .invalid_writable_account
            .fetch_add(*invalid_writable_account, Ordering::Relaxed);
        self.error_metrics
            .invalid_rent_paying_account
            .fetch_add(*invalid_rent_paying_account, Ordering::Relaxed);
        self.error_metrics
            .would_exceed_max_block_cost_limit
            .fetch_add(*would_exceed_max_block_cost_limit, Ordering::Relaxed);
        self.error_metrics
            .would_exceed_max_account_cost_limit
            .fetch_add(*would_exceed_max_account_cost_limit, Ordering::Relaxed);
        self.error_metrics
            .would_exceed_max_vote_cost_limit
            .fetch_add(*would_exceed_max_vote_cost_limit, Ordering::Relaxed);
        self.error_metrics
            .would_exceed_account_data_block_limit
            .fetch_add(*would_exceed_account_data_block_limit, Ordering::Relaxed);
        self.error_metrics
            .max_loaded_accounts_data_size_exceeded
            .fetch_add(*max_loaded_accounts_data_size_exceeded, Ordering::Relaxed);
        self.error_metrics
            .program_execution_temporarily_restricted
            .fetch_add(*program_execution_temporarily_restricted, Ordering::Relaxed);
    }
}

struct ConsumeWorkerCountMetrics {
    transactions_attempted_execution_count: AtomicU64,
    executed_transactions_count: AtomicU64,
    executed_with_successful_result_count: AtomicU64,
    retryable_transaction_count: AtomicUsize,
    retryable_expired_bank_count: AtomicUsize,
    cost_model_throttled_transactions_count: AtomicU64,
    min_prioritization_fees: AtomicU64,
    max_prioritization_fees: AtomicU64,
}

impl Default for ConsumeWorkerCountMetrics {
    fn default() -> Self {
        Self {
            transactions_attempted_execution_count: AtomicU64::default(),
            executed_transactions_count: AtomicU64::default(),
            executed_with_successful_result_count: AtomicU64::default(),
            retryable_transaction_count: AtomicUsize::default(),
            retryable_expired_bank_count: AtomicUsize::default(),
            cost_model_throttled_transactions_count: AtomicU64::default(),
            min_prioritization_fees: AtomicU64::new(u64::MAX),
            max_prioritization_fees: AtomicU64::default(),
        }
    }
}

impl ConsumeWorkerCountMetrics {
    fn report_and_reset(&self, id: &str) {
        datapoint_info!(
            "banking_stage_worker_counts",
            "id" => id,
            (
                "transactions_attempted_execution_count",
                self.transactions_attempted_execution_count
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "executed_transactions_count",
                self.executed_transactions_count.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "executed_with_successful_result_count",
                self.executed_with_successful_result_count
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "retryable_transaction_count",
                self.retryable_transaction_count.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "retryable_expired_bank_count",
                self.retryable_expired_bank_count.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "cost_model_throttled_transactions_count",
                self.cost_model_throttled_transactions_count
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "min_prioritization_fees",
                self.min_prioritization_fees
                    .swap(u64::MAX, Ordering::Relaxed),
                i64
            ),
            (
                "max_prioritization_fees",
                self.max_prioritization_fees.swap(0, Ordering::Relaxed),
                i64
            ),
        );
    }
}

#[derive(Default)]
struct ConsumeWorkerTimingMetrics {
    cost_model_us: AtomicU64,
    collect_balances_us: AtomicU64,
    load_execute_us: AtomicU64,
    freeze_lock_us: AtomicU64,
    last_blockhash_us: AtomicU64,
    record_us: AtomicU64,
    commit_us: AtomicU64,
    find_and_send_votes_us: AtomicU64,
    wait_for_bank_success_us: AtomicU64,
    wait_for_bank_failure_us: AtomicU64,
}

impl ConsumeWorkerTimingMetrics {
    fn report_and_reset(&self, id: &str) {
        datapoint_info!(
            "banking_stage_worker_timing",
            "id" => id,
            (
                "cost_model_us",
                self.cost_model_us.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "collect_balances_us",
                self.collect_balances_us.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "load_execute_us",
                self.load_execute_us.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "freeze_lock_us",
                self.freeze_lock_us.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "last_blockhash_us",
                self.last_blockhash_us.swap(0, Ordering::Relaxed),
                i64
            ),
            ("record_us", self.record_us.swap(0, Ordering::Relaxed), i64),
            ("commit_us", self.commit_us.swap(0, Ordering::Relaxed), i64),
            (
                "find_and_send_votes_us",
                self.find_and_send_votes_us.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "wait_for_bank_success_us",
                self.wait_for_bank_success_us.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "wait_for_bank_failure_us",
                self.wait_for_bank_failure_us.swap(0, Ordering::Relaxed),
                i64
            ),
        );
    }
}

#[derive(Default)]
struct ConsumeWorkerTransactionErrorMetrics {
    total: AtomicUsize,
    account_in_use: AtomicUsize,
    too_many_account_locks: AtomicUsize,
    account_loaded_twice: AtomicUsize,
    account_not_found: AtomicUsize,
    blockhash_not_found: AtomicUsize,
    blockhash_too_old: AtomicUsize,
    call_chain_too_deep: AtomicUsize,
    already_processed: AtomicUsize,
    instruction_error: AtomicUsize,
    insufficient_funds: AtomicUsize,
    invalid_account_for_fee: AtomicUsize,
    invalid_account_index: AtomicUsize,
    invalid_program_for_execution: AtomicUsize,
    invalid_compute_budget: AtomicUsize,
    not_allowed_during_cluster_maintenance: AtomicUsize,
    invalid_writable_account: AtomicUsize,
    invalid_rent_paying_account: AtomicUsize,
    would_exceed_max_block_cost_limit: AtomicUsize,
    would_exceed_max_account_cost_limit: AtomicUsize,
    would_exceed_max_vote_cost_limit: AtomicUsize,
    would_exceed_account_data_block_limit: AtomicUsize,
    max_loaded_accounts_data_size_exceeded: AtomicUsize,
    program_execution_temporarily_restricted: AtomicUsize,
}

impl ConsumeWorkerTransactionErrorMetrics {
    fn report_and_reset(&self, id: &str) {
        datapoint_info!(
            "banking_stage_worker_error_metrics",
            "id" => id,
            ("total", self.total.swap(0, Ordering::Relaxed), i64),
            (
                "account_in_use",
                self.account_in_use.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "too_many_account_locks",
                self.too_many_account_locks.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "account_loaded_twice",
                self.account_loaded_twice.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "account_not_found",
                self.account_not_found.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "blockhash_not_found",
                self.blockhash_not_found.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "blockhash_too_old",
                self.blockhash_too_old.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "call_chain_too_deep",
                self.call_chain_too_deep.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "already_processed",
                self.already_processed.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "instruction_error",
                self.instruction_error.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "insufficient_funds",
                self.insufficient_funds.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "invalid_account_for_fee",
                self.invalid_account_for_fee.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "invalid_account_index",
                self.invalid_account_index.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "invalid_program_for_execution",
                self.invalid_program_for_execution
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "invalid_compute_budget",
                self.invalid_compute_budget
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "not_allowed_during_cluster_maintenance",
                self.not_allowed_during_cluster_maintenance
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "invalid_writable_account",
                self.invalid_writable_account.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "invalid_rent_paying_account",
                self.invalid_rent_paying_account.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "would_exceed_max_block_cost_limit",
                self.would_exceed_max_block_cost_limit
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "would_exceed_max_account_cost_limit",
                self.would_exceed_max_account_cost_limit
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "would_exceed_max_vote_cost_limit",
                self.would_exceed_max_vote_cost_limit
                    .swap(0, Ordering::Relaxed),
                i64
            ),
        );
    }
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
        solana_runtime::{
            bank_forks::BankForks, prioritization_fee_cache::PrioritizationFeeCache,
            vote_sender_types::ReplayVoteReceiver,
        },
        solana_sdk::{
            genesis_config::GenesisConfig, poh_config::PohConfig, pubkey::Pubkey,
            signature::Keypair, system_transaction,
        },
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
        _bank_forks: Arc<RwLock<BankForks>>,
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
            0,
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
                _bank_forks: bank_forks,
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
        poh_recorder
            .write()
            .unwrap()
            .set_bank_for_test(bank.clone());

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
        poh_recorder
            .write()
            .unwrap()
            .set_bank_for_test(bank.clone());

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
        poh_recorder
            .write()
            .unwrap()
            .set_bank_for_test(bank.clone());

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

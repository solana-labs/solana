//! Quality of service for block producer.
//! Provides logic and functions to allow a Leader to prioritize
//! how transactions are included in blocks, and optimize those blocks.
//!
use {
    crate::banking_stage::BatchedTransactionCostDetails,
    crossbeam_channel::{unbounded, Receiver, Sender},
    solana_measure::measure::Measure,
    solana_runtime::{
        bank::Bank,
        cost_model::{CostModel, TransactionCost},
        cost_tracker::CostTrackerError,
    },
    solana_sdk::{
        clock::Slot,
        transaction::{self, SanitizedTransaction, TransactionError},
    },
    std::{
        sync::{
            atomic::{AtomicBool, AtomicU64, Ordering},
            Arc, RwLock,
        },
        thread::{self, Builder, JoinHandle},
        time::Duration,
    },
};

pub enum QosMetrics {
    BlockBatchUpdate { bank: Arc<Bank> },
}

// QosService is local to each banking thread, each instance of QosService provides services to
// one banking thread.
// It hosts a private thread for async metrics reporting, tagged with banking thredas ID. Banking
// threda calls `report_metrics(&bank)` at end of `process_and_record_tramsaction()`, or any time
// it wants, QosService sends `&bank` to reporting thread via channel, signalling stats to be
// reported if new bank slot has changed.
//
pub struct QosService {
    // cost_model instance is owned by validator, shared between replay_stage and
    // banking_stage. replay_stage writes the latest on-chain program timings to
    // it; banking_stage's qos_service reads that information to calculate
    // transaction cost, hence RwLock wrapped.
    cost_model: Arc<RwLock<CostModel>>,
    // QosService hosts metrics object and a private reporting thread, as well as sender to
    // communicate with thread.
    report_sender: Sender<QosMetrics>,
    metrics: Arc<QosServiceMetrics>,
    // metrics reporting runs on a private thread
    reporting_thread: Option<JoinHandle<()>>,
    running_flag: Arc<AtomicBool>,
}

impl Drop for QosService {
    fn drop(&mut self) {
        self.running_flag.store(false, Ordering::Relaxed);
        self.reporting_thread
            .take()
            .unwrap()
            .join()
            .expect("qos service metrics reporting thread failed to join");
    }
}

impl QosService {
    pub fn new(cost_model: Arc<RwLock<CostModel>>, id: u32) -> Self {
        let (report_sender, report_receiver) = unbounded();
        let running_flag = Arc::new(AtomicBool::new(true));
        let metrics = Arc::new(QosServiceMetrics::new(id));

        let running_flag_clone = running_flag.clone();
        let metrics_clone = metrics.clone();
        let reporting_thread = Some(
            Builder::new()
                .name("solana-qos-service-metrics-repoting".to_string())
                .spawn(move || {
                    Self::reporting_loop(running_flag_clone, metrics_clone, report_receiver);
                })
                .unwrap(),
        );

        Self {
            cost_model,
            metrics,
            reporting_thread,
            running_flag,
            report_sender,
        }
    }

    // invoke cost_model to calculate cost for the given list of transactions
    pub fn compute_transaction_costs<'a>(
        &self,
        transactions: impl Iterator<Item = &'a SanitizedTransaction>,
    ) -> Vec<TransactionCost> {
        let mut compute_cost_time = Measure::start("compute_cost_time");
        let cost_model = self.cost_model.read().unwrap();
        let txs_costs: Vec<_> = transactions
            .map(|tx| {
                let cost = cost_model.calculate_cost(tx);
                debug!(
                    "transaction {:?}, cost {:?}, cost sum {}",
                    tx,
                    cost,
                    cost.sum()
                );
                cost
            })
            .collect();
        compute_cost_time.stop();
        self.metrics
            .compute_cost_time
            .fetch_add(compute_cost_time.as_us(), Ordering::Relaxed);
        self.metrics
            .compute_cost_count
            .fetch_add(txs_costs.len() as u64, Ordering::Relaxed);
        txs_costs
    }

    // Given a list of transactions and their costs, this function returns a corresponding
    // list of Results that indicate if a transaction is selected to be included in the current block,
    pub fn select_transactions_per_cost<'a>(
        &self,
        transactions: impl Iterator<Item = &'a SanitizedTransaction>,
        transactions_costs: impl Iterator<Item = &'a TransactionCost>,
        bank: &Arc<Bank>,
    ) -> Vec<transaction::Result<()>> {
        let mut cost_tracking_time = Measure::start("cost_tracking_time");
        let mut cost_tracker = bank.write_cost_tracker().unwrap();
        let select_results = transactions
            .zip(transactions_costs)
            .map(|(tx, cost)| match cost_tracker.try_add(tx, cost) {
                Ok(current_block_cost) => {
                    debug!("slot {:?}, transaction {:?}, cost {:?}, fit into current block, current block cost {}", bank.slot(), tx, cost, current_block_cost);
                    self.metrics.selected_txs_count.fetch_add(1, Ordering::Relaxed);
                    Ok(())
                },
                Err(e) => {
                    debug!("slot {:?}, transaction {:?}, cost {:?}, not fit into current block, '{:?}'", bank.slot(), tx, cost, e);
                    match e {
                        CostTrackerError::WouldExceedBlockMaxLimit => {
                            self.metrics.retried_txs_per_block_limit_count.fetch_add(1, Ordering::Relaxed);
                            Err(TransactionError::WouldExceedMaxBlockCostLimit)
                        }
                        CostTrackerError::WouldExceedVoteMaxLimit => {
                            self.metrics.retried_txs_per_vote_limit_count.fetch_add(1, Ordering::Relaxed);
                            Err(TransactionError::WouldExceedMaxVoteCostLimit)
                        }
                        CostTrackerError::WouldExceedAccountMaxLimit => {
                            self.metrics.retried_txs_per_account_limit_count.fetch_add(1, Ordering::Relaxed);
                            Err(TransactionError::WouldExceedMaxAccountCostLimit)
                        }
                        CostTrackerError::WouldExceedAccountDataMaxLimit => {
                            self.metrics.retried_txs_per_account_data_limit_count.fetch_add(1, Ordering::Relaxed);
                            Err(TransactionError::WouldExceedMaxAccountDataCostLimit)
                        }
                    }
                }
            })
            .collect();
        cost_tracking_time.stop();
        self.metrics
            .cost_tracking_time
            .fetch_add(cost_tracking_time.as_us(), Ordering::Relaxed);
        select_results
    }

    // metrics are reported by bank slot
    pub fn report_metrics(&self, bank: Arc<Bank>) {
        self.report_sender
            .send(QosMetrics::BlockBatchUpdate { bank })
            .unwrap_or_else(|err| warn!("qos service report metrics failed: {:?}", err));
    }

    // metrics accumulating apis
    pub fn accumulate_tpu_ingested_packets_count(&self, count: u64) {
        self.metrics
            .tpu_ingested_packets_count
            .fetch_add(count, Ordering::Relaxed);
    }

    pub fn accumulate_tpu_buffered_packets_count(&self, count: u64) {
        self.metrics
            .tpu_buffered_packets_count
            .fetch_add(count, Ordering::Relaxed);
    }

    pub fn accumulated_verified_txs_count(&self, count: u64) {
        self.metrics
            .verified_txs_count
            .fetch_add(count, Ordering::Relaxed);
    }

    pub fn accumulated_processed_txs_count(&self, count: u64) {
        self.metrics
            .processed_txs_count
            .fetch_add(count, Ordering::Relaxed);
    }

    pub fn accumulated_retryable_txs_count(&self, count: u64) {
        self.metrics
            .retryable_txs_count
            .fetch_add(count, Ordering::Relaxed);
    }

    pub fn accumulate_estimated_transaction_costs(
        &self,
        cost_details: &BatchedTransactionCostDetails,
    ) {
        self.metrics
            .estimated_signature_cu
            .fetch_add(cost_details.batched_signature_cost, Ordering::Relaxed);
        self.metrics
            .estimated_write_lock_cu
            .fetch_add(cost_details.batched_write_lock_cost, Ordering::Relaxed);
        self.metrics
            .estimated_data_bytes_cu
            .fetch_add(cost_details.batched_data_bytes_cost, Ordering::Relaxed);
        self.metrics
            .estimated_execute_cu
            .fetch_add(cost_details.batched_execute_cost, Ordering::Relaxed);
    }

    pub fn accumulate_actual_execute_cu(&self, units: u64) {
        self.metrics
            .actual_execute_cu
            .fetch_add(units, Ordering::Relaxed);
    }

    pub fn accumulate_actual_execute_time(&self, micro_sec: u64) {
        self.metrics
            .actual_execute_time_us
            .fetch_add(micro_sec, Ordering::Relaxed);
    }

    fn reporting_loop(
        running_flag: Arc<AtomicBool>,
        metrics: Arc<QosServiceMetrics>,
        report_receiver: Receiver<QosMetrics>,
    ) {
        while running_flag.load(Ordering::Relaxed) {
            for qos_metrics in report_receiver.try_iter() {
                match qos_metrics {
                    QosMetrics::BlockBatchUpdate { bank } => {
                        metrics.report(bank.slot());
                    }
                }
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}

#[derive(Default)]
struct QosServiceMetrics {
    // banking_stage creates one QosService instance per working threads, that is uniquely
    // identified by id. This field allows to categorize metrics for gossip votes, TPU votes
    // and other transactions.
    id: u32,

    // aggregate metrics per slot
    slot: AtomicU64,

    // accumulated number of live packets TPU received from verified receiver for processing.
    tpu_ingested_packets_count: AtomicU64,

    // accumulated number of live packets TPU put into buffer due to no active bank.
    tpu_buffered_packets_count: AtomicU64,

    // accumulated number of verified txs, which excludes unsanitized transactions and
    // non-vote transactions when in vote-only mode from ingested packets
    verified_txs_count: AtomicU64,

    // accumulated number of transactions been processed, includes those landed and those to be
    // returned (due to AccountInUse, and other QoS related reasons)
    processed_txs_count: AtomicU64,

    // accumulated number of transactions buffered for retry, often due to AccountInUse and QoS
    // reasons, includes retried_txs_per_block_limit_count and retried_txs_per_account_limit_count
    retryable_txs_count: AtomicU64,

    // accumulated time in micro-sec spent in computing transaction cost. It is the main performance
    // overhead introduced by cost_model
    compute_cost_time: AtomicU64,

    // total nummber of transactions in the reporting period to be computed for theit cost. It is
    // usually the number of sanitized transactions leader receives.
    compute_cost_count: AtomicU64,

    // acumulated time in micro-sec spent in tracking each bank's cost. It is the second part of
    // overhead introduced
    cost_tracking_time: AtomicU64,

    // number of transactions to be included in blocks
    selected_txs_count: AtomicU64,

    // number of transactions to be queued for retry due to its potential to breach block limit
    retried_txs_per_block_limit_count: AtomicU64,

    // number of transactions to be queued for retry due to its potential to breach vote limit
    retried_txs_per_vote_limit_count: AtomicU64,

    // number of transactions to be queued for retry due to its potential to breach writable
    // account limit
    retried_txs_per_account_limit_count: AtomicU64,

    // number of transactions to be queued for retry due to its account data limits
    retried_txs_per_account_data_limit_count: AtomicU64,

    // accumulated estimated signature Compute Unites to be packed into block
    estimated_signature_cu: AtomicU64,

    // accumulated estimated write locks Compute Units to be packed into block
    estimated_write_lock_cu: AtomicU64,

    // accumulated estimated instructino data Compute Units to be packed into block
    estimated_data_bytes_cu: AtomicU64,

    // accumulated estimated program Compute Units to be packed into block
    estimated_execute_cu: AtomicU64,

    // accumulated actual program Compute Units that have been packed into block
    actual_execute_cu: AtomicU64,

    // accumulated actual program execute micro-sec that have been packed into block
    actual_execute_time_us: AtomicU64,
}

impl QosServiceMetrics {
    pub fn new(id: u32) -> Self {
        QosServiceMetrics {
            id,
            ..QosServiceMetrics::default()
        }
    }

    pub fn report(&self, bank_slot: Slot) {
        if bank_slot != self.slot.load(Ordering::Relaxed) {
            datapoint_info!(
                "qos-service-stats",
                ("id", self.id as i64, i64),
                ("bank_slot", bank_slot as i64, i64),
                (
                    "tpu_ingested_packets_count",
                    self.tpu_ingested_packets_count.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "tpu_buffered_packets_count",
                    self.tpu_buffered_packets_count.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "verified_txs_count",
                    self.verified_txs_count.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "processed_txs_count",
                    self.processed_txs_count.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "retryable_txs_count",
                    self.retryable_txs_count.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "compute_cost_time",
                    self.compute_cost_time.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "compute_cost_count",
                    self.compute_cost_count.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "cost_tracking_time",
                    self.cost_tracking_time.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "selected_txs_count",
                    self.selected_txs_count.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "retried_txs_per_block_limit_count",
                    self.retried_txs_per_block_limit_count
                        .swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "retried_txs_per_vote_limit_count",
                    self.retried_txs_per_vote_limit_count
                        .swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "retried_txs_per_account_limit_count",
                    self.retried_txs_per_account_limit_count
                        .swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "retried_txs_per_account_data_limit_count",
                    self.retried_txs_per_account_data_limit_count
                        .swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "estimated_signature_cu",
                    self.estimated_signature_cu.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "estimated_write_lock_cu",
                    self.estimated_write_lock_cu.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "estimated_data_bytes_cu",
                    self.estimated_data_bytes_cu.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "estimated_execute_cu",
                    self.estimated_execute_cu.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "actual_execute_cu",
                    self.actual_execute_cu.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "actual_execute_time_us",
                    self.actual_execute_time_us.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
            );
            self.slot.store(bank_slot, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        itertools::Itertools,
        solana_runtime::{
            bank::Bank,
            genesis_utils::{create_genesis_config, GenesisConfigInfo},
        },
        solana_sdk::{
            hash::Hash,
            signature::{Keypair, Signer},
            system_transaction,
        },
        solana_vote_program::vote_transaction,
    };

    #[test]
    fn test_compute_transaction_costs() {
        solana_logger::setup();

        // make a vec of txs
        let keypair = Keypair::new();
        let transfer_tx = SanitizedTransaction::from_transaction_for_tests(
            system_transaction::transfer(&keypair, &keypair.pubkey(), 1, Hash::default()),
        );
        let vote_tx = SanitizedTransaction::from_transaction_for_tests(
            vote_transaction::new_vote_transaction(
                vec![42],
                Hash::default(),
                Hash::default(),
                &keypair,
                &keypair,
                &keypair,
                None,
            ),
        );
        let txs = vec![transfer_tx.clone(), vote_tx.clone(), vote_tx, transfer_tx];

        let cost_model = Arc::new(RwLock::new(CostModel::default()));
        let qos_service = QosService::new(cost_model.clone(), 1);
        let txs_costs = qos_service.compute_transaction_costs(txs.iter());

        // verify the size of txs_costs and its contents
        assert_eq!(txs_costs.len(), txs.len());
        txs_costs
            .iter()
            .enumerate()
            .map(|(index, cost)| {
                assert_eq!(
                    cost.sum(),
                    cost_model.read().unwrap().calculate_cost(&txs[index]).sum()
                );
            })
            .collect_vec();
    }

    #[test]
    fn test_select_transactions_per_cost() {
        solana_logger::setup();
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10);
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));
        let cost_model = Arc::new(RwLock::new(CostModel::default()));

        let keypair = Keypair::new();
        let transfer_tx = SanitizedTransaction::from_transaction_for_tests(
            system_transaction::transfer(&keypair, &keypair.pubkey(), 1, Hash::default()),
        );
        let vote_tx = SanitizedTransaction::from_transaction_for_tests(
            vote_transaction::new_vote_transaction(
                vec![42],
                Hash::default(),
                Hash::default(),
                &keypair,
                &keypair,
                &keypair,
                None,
            ),
        );
        let transfer_tx_cost = cost_model
            .read()
            .unwrap()
            .calculate_cost(&transfer_tx)
            .sum();
        let vote_tx_cost = cost_model.read().unwrap().calculate_cost(&vote_tx).sum();

        // make a vec of txs
        let txs = vec![transfer_tx.clone(), vote_tx.clone(), transfer_tx, vote_tx];

        let qos_service = QosService::new(cost_model, 1);
        let txs_costs = qos_service.compute_transaction_costs(txs.iter());

        // set cost tracker limit to fit 1 transfer tx and 1 vote tx
        let cost_limit = transfer_tx_cost + vote_tx_cost;
        bank.write_cost_tracker()
            .unwrap()
            .set_limits(cost_limit, cost_limit, cost_limit);
        let results = qos_service.select_transactions_per_cost(txs.iter(), txs_costs.iter(), &bank);

        // verify that first transfer tx and first vote are allowed
        assert_eq!(results.len(), txs.len());
        assert!(results[0].is_ok());
        assert!(results[1].is_ok());
        assert!(results[2].is_err());
        assert!(results[3].is_err());
    }
}

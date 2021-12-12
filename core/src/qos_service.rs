//! Quality of service for block producer.
//! Provides logic and functions to allow a Leader to prioritize
//! how transactions are included in blocks, and optimize those blocks.
//!
use {
    solana_measure::measure::Measure,
    solana_runtime::{
        bank::Bank,
        cost_model::{CostModel, TransactionCost},
        cost_tracker::CostTrackerError,
    },
    solana_sdk::{
        timing::AtomicInterval,
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

pub struct QosService {
    // cost_model instance is owned by validator, shared between replay_stage and
    // banking_stage. replay_stage writes the latest on-chain program timings to
    // it; banking_stage's qos_service reads that information to calculate
    // transaction cost, hence RwLock wrapped.
    cost_model: Arc<RwLock<CostModel>>,
    metrics: Arc<QosServiceMetrics>,
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
    pub fn new(cost_model: Arc<RwLock<CostModel>>) -> Self {
        Self::new_with_reporting_duration(cost_model, 1000u64)
    }

    pub fn new_with_reporting_duration(
        cost_model: Arc<RwLock<CostModel>>,
        reporting_duration_ms: u64,
    ) -> Self {
        let running_flag = Arc::new(AtomicBool::new(true));
        let metrics = Arc::new(QosServiceMetrics::default());

        let running_flag_clone = running_flag.clone();
        let metrics_clone = metrics.clone();
        let reporting_thread = Some(
            Builder::new()
                .name("solana-qos-service-metrics-repoting".to_string())
                .spawn(move || {
                    Self::reporting_loop(running_flag_clone, metrics_clone, reporting_duration_ms);
                })
                .unwrap(),
        );
        Self {
            cost_model,
            metrics,
            reporting_thread,
            running_flag,
        }
    }

    pub fn compute_transaction_costs<'a>(
        &self,
        transactions: impl Iterator<Item = &'a SanitizedTransaction>,
        demote_program_write_locks: bool,
    ) -> Vec<TransactionCost> {
        let mut compute_cost_time = Measure::start("compute_cost_time");
        let cost_model = self.cost_model.read().unwrap();
        let txs_costs: Vec<_> = transactions
            .map(|tx| {
                let cost = cost_model.calculate_cost(tx, demote_program_write_locks);
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
                        CostTrackerError::WouldExceedAccountMaxLimit => {
                            self.metrics.retried_txs_per_account_limit_count.fetch_add(1, Ordering::Relaxed);
                            Err(TransactionError::WouldExceedMaxAccountCostLimit)
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

    fn reporting_loop(
        running_flag: Arc<AtomicBool>,
        metrics: Arc<QosServiceMetrics>,
        reporting_duration_ms: u64,
    ) {
        while running_flag.load(Ordering::Relaxed) {
            metrics.report(reporting_duration_ms);
            thread::sleep(Duration::from_millis(100));
        }
    }
}

#[derive(Default)]
struct QosServiceMetrics {
    last_report: AtomicInterval,
    compute_cost_time: AtomicU64,
    compute_cost_count: AtomicU64,
    cost_tracking_time: AtomicU64,
    selected_txs_count: AtomicU64,
    retried_txs_per_block_limit_count: AtomicU64,
    retried_txs_per_account_limit_count: AtomicU64,
}

impl QosServiceMetrics {
    pub fn report(&self, report_interval_ms: u64) {
        if self.last_report.should_update(report_interval_ms) {
            datapoint_info!(
                "qos-service-stats",
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
                    "retried_txs_per_account_limit_count",
                    self.retried_txs_per_account_limit_count
                        .swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
            );
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
        let qos_service = QosService::new(cost_model.clone());
        let txs_costs = qos_service.compute_transaction_costs(txs.iter(), false);

        // verify the size of txs_costs and its contents
        assert_eq!(txs_costs.len(), txs.len());
        txs_costs
            .iter()
            .enumerate()
            .map(|(index, cost)| {
                assert_eq!(
                    cost.sum(),
                    cost_model
                        .read()
                        .unwrap()
                        .calculate_cost(&txs[index], false)
                        .sum()
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
            .calculate_cost(&transfer_tx, false)
            .sum();

        // make a vec of txs
        let txs = vec![transfer_tx.clone(), vote_tx.clone(), transfer_tx, vote_tx];

        let qos_service = QosService::new(cost_model);
        let txs_costs = qos_service.compute_transaction_costs(txs.iter(), false);

        // set cost tracker limit to fit 1 transfer tx, vote tx bypasses limit check
        let cost_limit = transfer_tx_cost;
        bank.write_cost_tracker()
            .unwrap()
            .set_limits(cost_limit, cost_limit);
        let results = qos_service.select_transactions_per_cost(txs.iter(), txs_costs.iter(), &bank);

        // verify that first transfer tx and all votes are allowed
        assert_eq!(results.len(), txs.len());
        assert!(results[0].is_ok());
        assert!(results[1].is_ok());
        assert!(results[2].is_err());
        assert!(results[3].is_ok());
    }

    #[test]
    fn test_async_report_metrics() {
        solana_logger::setup();
        //solana_logger::setup_with_default("solana=info");

        // make a vec of txs
        let txs_count = 128usize;
        let keypair = Keypair::new();
        let transfer_tx = SanitizedTransaction::from_transaction_for_tests(
            system_transaction::transfer(&keypair, &keypair.pubkey(), 1, Hash::default()),
        );
        let mut txs_1 = Vec::with_capacity(txs_count);
        let mut txs_2 = Vec::with_capacity(txs_count);
        for _i in 0..txs_count {
            txs_1.push(transfer_tx.clone());
            txs_2.push(transfer_tx.clone());
        }

        // set reporting duration to long enough so the stats wouldn't reset during testing
        let ten_min = 600_000u64;
        let cost_model = Arc::new(RwLock::new(CostModel::default()));
        let qos_service = Arc::new(QosService::new_with_reporting_duration(cost_model, ten_min));
        let qos_service_1 = qos_service.clone();
        let qos_service_2 = qos_service.clone();

        let th_1 = Builder::new()
            .name("test-producer-1".to_string())
            .spawn(move || {
                debug!("thread 1 starts with {} txs", txs_1.len());
                let tx_costs = qos_service_1.compute_transaction_costs(txs_1.iter(), false);
                assert_eq!(txs_count, tx_costs.len());
                debug!(
                    "thread 1 done, generated {} count, see service count as {}",
                    txs_count,
                    qos_service_1
                        .metrics
                        .compute_cost_count
                        .load(Ordering::Relaxed)
                );
            })
            .unwrap();

        let th_2 = Builder::new()
            .name("test-producer-2".to_string())
            .spawn(move || {
                debug!("thread 2 starts with {} txs", txs_2.len());
                let tx_costs = qos_service_2.compute_transaction_costs(txs_2.iter(), false);
                assert_eq!(txs_count, tx_costs.len());
                debug!(
                    "thread 2 done, generated {} count, see service count as {}",
                    txs_count,
                    qos_service_2
                        .metrics
                        .compute_cost_count
                        .load(Ordering::Relaxed)
                );
            })
            .unwrap();

        th_1.join().expect("qos service 1 panicked");
        th_2.join().expect("qos service 2 panicked");

        debug!(
            "all threads joined. count {}",
            qos_service
                .metrics
                .compute_cost_count
                .load(Ordering::Relaxed)
        );

        assert_eq!(
            txs_count as u64 * 2,
            qos_service
                .metrics
                .compute_cost_count
                .load(Ordering::Relaxed)
        );
    }
}

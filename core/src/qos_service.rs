//! Producer quality of service
//!
//!
use solana_measure::measure::Measure;
use solana_runtime::{
    bank::is_simple_vote_transaction,
    bank::Bank,
    cost_model::{CostModel, TransactionCost},
    cost_tracker::CostTrackerError,
};
use solana_sdk::{
    timing::AtomicInterval,
    transaction::{self, SanitizedTransaction, TransactionError},
};
use std::{
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    sync::{Arc, RwLock},
    thread::{self, Builder, JoinHandle},
    time::Duration,
};

pub struct QosService {
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
        let running_flag = Arc::new(AtomicBool::new(true));
        let metrics = Arc::new(QosServiceMetrics::default());

        let running_flag_clone = running_flag.clone();
        let metrics_clone = metrics.clone();
        let reporting_thread = Some(
            Builder::new()
                .name("solana-qos-service-metrics-repoting".to_string())
                .spawn(move || {
                    Self::reporting_loop(running_flag_clone, metrics_clone);
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

    pub fn compute_transactions_costs<'a>(
        &self,
        transactions: impl Iterator<Item = &'a SanitizedTransaction>,
        bank: &Arc<Bank>,
    ) -> Vec<TransactionCost> {
        let mut compute_cost_time = Measure::start("compute_cost_time");
        let cost_model = self.cost_model.read().unwrap();
        let txs_costs: Vec<_> = transactions
            .map(|tx| {
                // calculate deterministic tx cost
                let mut cost = cost_model.calculate_cost(tx, bank.demote_program_write_locks());
                // calculate cost weight
                cost.cost_weight = Self::calculate_cost_weight(
                    tx,
                    bank.read_cost_tracker()
                        .unwrap()
                        .get_fee_payer_transactions_count(tx.message().fee_payer()),
                );
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

    // Given a list of txs and their cost, this function returns a corresponding
    // list of Results indicate if tx is selected to be included in current block,
    // Reasons a tx not being included are:
    // - it'd exceed max block cost limit
    // - it'd exceed a writable account max cost limit
    // - it's fee_account does not have enough balance, tx will be dropped.
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
            .map(|(tx, cost)| {
                if Self::fee_payer_has_enough_balance(tx, cost, bank) {
                    match cost_tracker.try_add(tx, cost) {
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
                    }
                } else {
                    self.metrics.dropped_txs_insufficient_funds.fetch_add(1, Ordering::Relaxed);
                    Err(TransactionError::InsufficientFundsForFee)
                }
            })
            .collect();
        cost_tracking_time.stop();
        self.metrics
            .cost_tracking_time
            .fetch_add(cost_tracking_time.as_us(), Ordering::Relaxed);
        select_results
    }

    fn calculate_cost_weight(
        transaction: &SanitizedTransaction,
        fee_payer_txs_count: Option<&u64>,
    ) -> u32 {
        #[allow(clippy::if_same_then_else)]
        if is_simple_vote_transaction(transaction) {
            // vote has zero cost weight, so it bypasses block cost limit checking
            0u32
        } else if let Some(_count) = fee_payer_txs_count {
            // TODO - use brackets or exponential function to calc weight from count,
            // for debugging purpose, let's return 1 for now
            1u32
        } else {
            1u32
        }
    }

    fn fee_payer_has_enough_balance(
        tx: &SanitizedTransaction,
        tx_cost: &TransactionCost,
        bank: &Arc<Bank>,
    ) -> bool {
        // roughly define the compute-unit to lamports convertion ratio, a fee governor feature,
        // for now by Signature cost - defined in block_cost_limit.rs, and signature lamports -
        // defined in fee_calculator.rs
        const LAMPORTS_TO_COMPUTE_COST_RATIO: u64 = 10_000u64 / (40u64 * 130u64);
        let fee_payer = tx.message().fee_payer();
        let fee_payer_balance = bank.get_balance(fee_payer);
        let required_balance =
            tx_cost.sum() * tx_cost.cost_weight as u64 * LAMPORTS_TO_COMPUTE_COST_RATIO;
        debug!(
            "tx {:?}, fee_payer {:?}, fee_payer_balance {:?}, required_balance {:?},",
            tx, fee_payer, fee_payer_balance, required_balance
        );
        fee_payer_balance > required_balance
    }

    fn reporting_loop(running_flag: Arc<AtomicBool>, metrics: Arc<QosServiceMetrics>) {
        while running_flag.load(Ordering::Relaxed) {
            // hardcode to report every 1000ms
            metrics.report(1000u64);
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
    dropped_txs_insufficient_funds: AtomicU64,
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
                (
                    "dropped_txs_insufficient_funds",
                    self.dropped_txs_insufficient_funds
                        .swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use itertools::Itertools;
    use solana_runtime::{
        bank::Bank,
        genesis_utils::{create_genesis_config, GenesisConfigInfo},
    };
    use solana_sdk::{
        hash::Hash,
        signature::{Keypair, Signer},
        system_transaction,
    };
    use solana_vote_program::vote_transaction;

    fn test_setup() -> (Keypair, Hash, Arc<Bank>) {
        solana_logger::setup();
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(10);
        let bank = Arc::new(Bank::new_no_wallclock_throttle_for_tests(&genesis_config));
        let start_hash = bank.last_blockhash();
        (mint_keypair, start_hash, bank)
    }

    #[test]
    fn test_compute_transactions_costs() {
        let (mint_keypair, start_hash, bank) = test_setup();

        // make a vec of txs
        let transfer_tx = SanitizedTransaction::from_transaction_for_tests(
            system_transaction::transfer(&mint_keypair, &mint_keypair.pubkey(), 1, start_hash),
        );
        let vote_tx = SanitizedTransaction::from_transaction_for_tests(
            vote_transaction::new_vote_transaction(
                vec![42],
                Hash::default(),
                Hash::default(),
                &Keypair::new(),
                &Keypair::new(),
                &Keypair::new(),
                None,
            ),
        );
        let txs = vec![transfer_tx.clone(), vote_tx.clone(), vote_tx, transfer_tx];

        let cost_model = Arc::new(RwLock::new(CostModel::default()));
        let qos_service = QosService::new(cost_model.clone());
        let txs_costs = qos_service.compute_transactions_costs(txs.iter(), &bank);

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
        let (mint_keypair, start_hash, bank) = test_setup();
        let cost_model = Arc::new(RwLock::new(CostModel::default()));

        let transfer_tx = SanitizedTransaction::from_transaction_for_tests(
            system_transaction::transfer(&mint_keypair, &mint_keypair.pubkey(), 1, start_hash),
        );
        let vote_tx = SanitizedTransaction::from_transaction_for_tests(
            vote_transaction::new_vote_transaction(
                vec![42],
                Hash::default(),
                Hash::default(),
                &Keypair::new(),
                &Keypair::new(),
                &Keypair::new(),
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
        let txs_costs = qos_service.compute_transactions_costs(txs.iter(), &bank);

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
        let (mint_keypair, start_hash, bank) = test_setup();

        // make a vec of txs
        let txs_count = 1024usize;
        let transfer_tx = SanitizedTransaction::from_transaction_for_tests(
            system_transaction::transfer(&mint_keypair, &mint_keypair.pubkey(), 1, start_hash),
        );
        let mut txs_1 = Vec::with_capacity(txs_count);
        let mut txs_2 = Vec::with_capacity(txs_count);
        for _i in 0..txs_count {
            txs_1.push(transfer_tx.clone());
            txs_2.push(transfer_tx.clone());
        }

        let cost_model = Arc::new(RwLock::new(CostModel::default()));
        let qos_service = Arc::new(QosService::new(cost_model));
        let qos_service_1 = qos_service.clone();
        let qos_service_2 = qos_service.clone();
        let bank_clone = bank.clone();

        let th_1 = thread::spawn(move || {
            qos_service_1.compute_transactions_costs(txs_1.iter(), &bank_clone);
        });

        let th_2 = thread::spawn(move || {
            qos_service_2.compute_transactions_costs(txs_2.iter(), &bank);
        });

        th_1.join().expect("qos service 1 faield to join");
        th_2.join().expect("qos service 2 faield to join");

        assert_eq!(
            txs_count as u64 * 2,
            qos_service
                .metrics
                .compute_cost_count
                .load(Ordering::Relaxed)
        );
    }

    #[test]
    fn test_calculate_cost_weight() {
        let (mint_keypair, start_hash, _bank) = test_setup();

        let simple_transaction = SanitizedTransaction::from_transaction_for_tests(
            system_transaction::transfer(&mint_keypair, &mint_keypair.pubkey(), 2, start_hash),
        );
        let vote_transaction = SanitizedTransaction::from_transaction_for_tests(
            vote_transaction::new_vote_transaction(
                vec![42],
                Hash::default(),
                Hash::default(),
                &Keypair::new(),
                &Keypair::new(),
                &Keypair::new(),
                None,
            ),
        );

        // For now, vote always has zero weight, everything else is neutral
        assert_eq!(
            0u32,
            QosService::calculate_cost_weight(&vote_transaction, None)
        );
        assert_eq!(
            0u32,
            QosService::calculate_cost_weight(&vote_transaction, Some(&10u64))
        );
        assert_eq!(
            1u32,
            QosService::calculate_cost_weight(&simple_transaction, None)
        );
        assert_eq!(
            1u32,
            QosService::calculate_cost_weight(&simple_transaction, Some(&10u64))
        );
    }
}

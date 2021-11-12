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
    solana_sdk::transaction::{self, SanitizedTransaction, TransactionError},
    std::sync::{Arc, RwLock},
};

#[derive(Default)]
pub struct QosServiceStats {
    compute_cost_time: u64,
    cost_tracking_time: u64,
    selected_txs_count: u64,
    retried_txs_per_block_limit_count: u64,
    retried_txs_per_account_limit_count: u64,
}

impl QosServiceStats {
    pub fn report(&mut self) {
        datapoint_info!(
            "qos-service-stats",
            ("compute_cost_time", self.compute_cost_time, i64),
            ("cost_tracking_time", self.cost_tracking_time, i64),
            ("selected_txs_count", self.selected_txs_count, i64),
            (
                "retried_txs_per_block_limit_count",
                self.retried_txs_per_block_limit_count,
                i64
            ),
            (
                "retried_txs_per_account_limit_count",
                self.retried_txs_per_account_limit_count,
                i64
            ),
        );
    }
}

pub struct QosService {
    cost_model: Arc<RwLock<CostModel>>,
}

impl QosService {
    pub fn new(cost_model: Arc<RwLock<CostModel>>) -> Self {
        Self { cost_model }
    }

    pub fn compute_transaction_costs<'a>(
        &self,
        transactions: impl Iterator<Item = &'a SanitizedTransaction>,
        demote_program_write_locks: bool,
        stats: &mut QosServiceStats,
    ) -> Vec<TransactionCost> {
        let mut compute_cost_time = Measure::start("compute_cost_time");
        let cost_model = self.cost_model.read().unwrap();
        let txs_costs = transactions
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
        stats.compute_cost_time += compute_cost_time.as_us();
        txs_costs
    }

    // Given a list of transactions and their costs, this function returns a corresponding
    // list of Results that indicate if a transaction is selected to be included in the current block,
    pub fn select_transactions_per_cost<'a>(
        &self,
        transactions: impl Iterator<Item = &'a SanitizedTransaction>,
        transactions_costs: impl Iterator<Item = &'a TransactionCost>,
        bank: &Arc<Bank>,
        stats: &mut QosServiceStats,
    ) -> Vec<transaction::Result<()>> {
        let mut cost_tracking_time = Measure::start("cost_tracking_time");
        let mut cost_tracker = bank.write_cost_tracker().unwrap();
        let select_results = transactions
            .zip(transactions_costs)
            .map(|(tx, cost)| match cost_tracker.try_add(tx, cost) {
                Ok(current_block_cost) => {
                    debug!("slot {:?}, transaction {:?}, cost {:?}, fit into current block, current block cost {}", bank.slot(), tx, cost, current_block_cost);
                    stats.selected_txs_count += 1;
                    Ok(())
                },
                Err(e) => {
                    debug!("slot {:?}, transaction {:?}, cost {:?}, not fit into current block, '{:?}'", bank.slot(), tx, cost, e);
                    match e {
                        CostTrackerError::WouldExceedBlockMaxLimit => {
                            stats.retried_txs_per_block_limit_count += 1;
                            Err(TransactionError::WouldExceedMaxBlockCostLimit)
                        }
                        CostTrackerError::WouldExceedAccountMaxLimit => {
                            stats.retried_txs_per_account_limit_count += 1;
                            Err(TransactionError::WouldExceedMaxAccountCostLimit)
                        }
                    }
                }
            })
            .collect();
        cost_tracking_time.stop();
        stats.cost_tracking_time += cost_tracking_time.as_us();
        select_results
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
    fn test_compute_transactions_costs() {
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
        let txs_costs = qos_service.compute_transaction_costs(
            txs.iter(),
            false,
            &mut QosServiceStats::default(),
        );

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
        let txs_costs = qos_service.compute_transaction_costs(
            txs.iter(),
            false,
            &mut QosServiceStats::default(),
        );

        // set cost tracker limit to fit 1 transfer tx, vote tx bypasses limit check
        let cost_limit = transfer_tx_cost;
        bank.write_cost_tracker()
            .unwrap()
            .set_limits(cost_limit, cost_limit);
        let results = qos_service.select_transactions_per_cost(
            txs.iter(),
            txs_costs.iter(),
            &bank,
            &mut QosServiceStats::default(),
        );

        // verify that first transfer tx and all votes are allowed
        assert_eq!(results.len(), txs.len());
        assert!(results[0].is_ok());
        assert!(results[1].is_ok());
        assert!(results[2].is_err());
        assert!(results[3].is_ok());
    }
}

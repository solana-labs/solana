use {
    super::leader_slot_timing_metrics::LeaderExecuteAndCommitTimings,
    itertools::Itertools,
    solana_accounts_db::{
        accounts::TransactionLoadResult,
        transaction_results::{TransactionExecutionResult, TransactionResults},
    },
    solana_ledger::{
        blockstore_processor::TransactionStatusSender, token_balances::collect_token_balances,
    },
    solana_measure::measure_us,
    solana_runtime::{
        bank::{Bank, CommitTransactionCounts, TransactionBalancesSet},
        bank_utils,
        prioritization_fee_cache::PrioritizationFeeCache,
        transaction_batch::TransactionBatch,
    },
    solana_sdk::{pubkey::Pubkey, saturating_add_assign},
    solana_transaction_status::{
        token_balances::TransactionTokenBalancesSet, TransactionTokenBalance,
    },
    solana_vote::vote_sender_types::ReplayVoteSender,
    std::{collections::HashMap, sync::Arc},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommitTransactionDetails {
    Committed {
        // actual compute units consumed for executing the committed transaction
        executed_units: u64,
        // actual micro-second elapsed for executing the committed transaction
        executed_us: u64,
        // compute units to add to executed_units to compensate for under priced CUs
        adjust_units: u64,
    },
    NotCommitted,
}

#[derive(Default)]
pub(super) struct PreBalanceInfo {
    pub native: Vec<Vec<u64>>,
    pub token: Vec<Vec<TransactionTokenBalance>>,
    pub mint_decimals: HashMap<Pubkey, u8>,
}

#[derive(Clone)]
pub struct Committer {
    transaction_status_sender: Option<TransactionStatusSender>,
    replay_vote_sender: ReplayVoteSender,
    prioritization_fee_cache: Arc<PrioritizationFeeCache>,
}

impl Committer {
    pub fn new(
        transaction_status_sender: Option<TransactionStatusSender>,
        replay_vote_sender: ReplayVoteSender,
        prioritization_fee_cache: Arc<PrioritizationFeeCache>,
    ) -> Self {
        Self {
            transaction_status_sender,
            replay_vote_sender,
            prioritization_fee_cache,
        }
    }

    pub(super) fn transaction_status_sender_enabled(&self) -> bool {
        self.transaction_status_sender.is_some()
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn commit_transactions(
        &self,
        batch: &TransactionBatch,
        loaded_transactions: &mut [TransactionLoadResult],
        execution_results: Vec<TransactionExecutionResult>,
        starting_transaction_index: Option<usize>,
        bank: &Arc<Bank>,
        pre_balance_info: &mut PreBalanceInfo,
        execute_and_commit_timings: &mut LeaderExecuteAndCommitTimings,
        signature_count: u64,
        executed_transactions_count: usize,
        executed_non_vote_transactions_count: usize,
        executed_with_successful_result_count: usize,
    ) -> (u64, Vec<CommitTransactionDetails>) {
        let (last_blockhash, lamports_per_signature) =
            bank.last_blockhash_and_lamports_per_signature();

        let executed_transactions = execution_results
            .iter()
            .zip(batch.sanitized_transactions())
            .filter_map(|(execution_result, tx)| execution_result.was_executed().then_some(tx))
            .collect_vec();

        let (tx_results, commit_time_us) = measure_us!(bank.commit_transactions(
            batch.sanitized_transactions(),
            loaded_transactions,
            execution_results,
            last_blockhash,
            lamports_per_signature,
            CommitTransactionCounts {
                committed_transactions_count: executed_transactions_count as u64,
                committed_non_vote_transactions_count: executed_non_vote_transactions_count as u64,
                committed_with_failure_result_count: executed_transactions_count
                    .saturating_sub(executed_with_successful_result_count)
                    as u64,
                signature_count,
            },
            &mut execute_and_commit_timings.execute_timings,
        ));
        execute_and_commit_timings.commit_us = commit_time_us;

        let commit_transaction_statuses = tx_results
            .execution_results
            .iter()
            .map(|execution_result| match execution_result.details() {
                Some(details) => CommitTransactionDetails::Committed {
                    executed_units: details.executed_units,
                    executed_us: details.executed_us,
                    adjust_units: Self::adjust_executed_units_for_potential_underpricing(
                        details.executed_units,
                        details.executed_us,
                    ),
                },
                None => CommitTransactionDetails::NotCommitted,
            })
            .collect();

        let ((), find_and_send_votes_us) = measure_us!({
            bank_utils::find_and_send_votes(
                batch.sanitized_transactions(),
                &tx_results,
                Some(&self.replay_vote_sender),
            );
            self.collect_balances_and_send_status_batch(
                tx_results,
                bank,
                batch,
                pre_balance_info,
                starting_transaction_index,
            );
            self.prioritization_fee_cache
                .update(bank, executed_transactions.into_iter());
        });
        execute_and_commit_timings.find_and_send_votes_us = find_and_send_votes_us;
        (commit_time_us, commit_transaction_statuses)
    }

    fn collect_balances_and_send_status_batch(
        &self,
        tx_results: TransactionResults,
        bank: &Arc<Bank>,
        batch: &TransactionBatch,
        pre_balance_info: &mut PreBalanceInfo,
        starting_transaction_index: Option<usize>,
    ) {
        if let Some(transaction_status_sender) = &self.transaction_status_sender {
            let txs = batch.sanitized_transactions().to_vec();
            let post_balances = bank.collect_balances(batch);
            let post_token_balances =
                collect_token_balances(bank, batch, &mut pre_balance_info.mint_decimals);
            let mut transaction_index = starting_transaction_index.unwrap_or_default();
            let batch_transaction_indexes: Vec<_> = tx_results
                .execution_results
                .iter()
                .map(|result| {
                    if result.was_executed() {
                        let this_transaction_index = transaction_index;
                        saturating_add_assign!(transaction_index, 1);
                        this_transaction_index
                    } else {
                        0
                    }
                })
                .collect();
            transaction_status_sender.send_transaction_status_batch(
                bank.clone(),
                txs,
                tx_results.execution_results,
                TransactionBalancesSet::new(
                    std::mem::take(&mut pre_balance_info.native),
                    post_balances,
                ),
                TransactionTokenBalancesSet::new(
                    std::mem::take(&mut pre_balance_info.token),
                    post_token_balances,
                ),
                tx_results.rent_debits,
                batch_transaction_indexes,
            );
        }
    }

    // If transaction's actual CU/us ratio is below cluster average COMPUTE_UNIT_TO_US_RATIO,
    // it is likely has been under priced. To prevent extending replay time significantly,
    // we can pad additional CUs to transaction's actual CUs during packing to compensate
    // additional executing time it needs.
    // adjustment is u64 for now, meaning only add more CUs when transactions are under priced,
    // but not to reduce CU if transactions are over priced.
    fn adjust_executed_units_for_potential_underpricing(
        executed_units: u64,
        executed_us: u64,
    ) -> u64 {
        // Some transactions that only have builtin instructions, or vote transactions, do not
        // consume compute units yet. Let's not to adjust for these type transactions.
        if executed_units == 0 {
            return 0;
        }

        // "actual executed units" is consistent across cluster, but "adjustment" is only based
        // on current leader node. Add a 50% taper to reduce local variance.
        const TAPER: u64 = 2;
        solana_cost_model::block_cost_limits::COMPUTE_UNIT_TO_US_RATIO
            .saturating_mul(executed_us)
            .saturating_sub(executed_units)
            .saturating_div(TAPER)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adjust_executed_units_for_potential_underpricing() {
        solana_logger::setup();
        use solana_cost_model::block_cost_limits::COMPUTE_UNIT_TO_US_RATIO;

        let executed_cu = 70;

        // no adjust for over-pricing
        assert_eq!(
            0,
            Committer::adjust_executed_units_for_potential_underpricing(
                executed_cu,
                executed_cu / (COMPUTE_UNIT_TO_US_RATIO + 10)
            )
        );

        // adjust for under pricing
        let slow_execution_time = executed_cu / (COMPUTE_UNIT_TO_US_RATIO - 10);
        let expected_adjustment = ((COMPUTE_UNIT_TO_US_RATIO - executed_cu / slow_execution_time)
            * slow_execution_time)
            / 2;
        assert_eq!(
            expected_adjustment,
            Committer::adjust_executed_units_for_potential_underpricing(
                executed_cu,
                slow_execution_time
            )
        );

        // handle zeros
        assert_eq!(
            0,
            Committer::adjust_executed_units_for_potential_underpricing(0, 0)
        );

        // no adjustment for those consumed 0 CU (builtins, votes etc)
        assert_eq!(
            0,
            Committer::adjust_executed_units_for_potential_underpricing(0, u64::MAX)
        );

        // the case of extreme underpricing
        assert_eq!(
            u64::MAX / 2, // tapered in half
            Committer::adjust_executed_units_for_potential_underpricing(1, u64::MAX)
        );

        // No adjustment if executed_units is already MAX
        assert_eq!(
            0,
            Committer::adjust_executed_units_for_potential_underpricing(u64::MAX, u64::MAX)
        );
        assert_eq!(
            0,
            Committer::adjust_executed_units_for_potential_underpricing(u64::MAX, 0)
        );
    }
}

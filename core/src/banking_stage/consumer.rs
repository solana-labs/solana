use {
    super::{
        committer::{CommitTransactionDetails, Committer, PreBalanceInfo},
        immutable_deserialized_packet::ImmutableDeserializedPacket,
        leader_slot_metrics::{
            LeaderSlotMetricsTracker, ProcessTransactionsCounts, ProcessTransactionsSummary,
        },
        leader_slot_timing_metrics::LeaderExecuteAndCommitTimings,
        qos_service::QosService,
        unprocessed_transaction_storage::{ConsumeScannerPayload, UnprocessedTransactionStorage},
        BankingStageStats,
    },
    itertools::Itertools,
    solana_ledger::token_balances::collect_token_balances,
    solana_measure::{measure::Measure, measure_us},
    solana_poh::poh_recorder::{
        BankStart, PohRecorderError, RecordTransactionsSummary, RecordTransactionsTimings,
        TransactionRecorder,
    },
    solana_runtime::{
        bank::{Bank, LoadAndExecuteTransactionsOutput},
        transaction_batch::TransactionBatch,
    },
    solana_runtime_transaction::instructions_processor::process_compute_budget_instructions,
    solana_sdk::{
        clock::{Slot, FORWARD_TRANSACTIONS_TO_LEADER_AT_SLOT_OFFSET, MAX_PROCESSING_AGE},
        feature_set,
        fee::FeeBudgetLimits,
        message::SanitizedMessage,
        saturating_add_assign,
        timing::timestamp,
        transaction::{self, AddressLoader, SanitizedTransaction, TransactionError},
    },
    solana_svm::{
        account_loader::{validate_fee_payer, TransactionCheckResult},
        transaction_error_metrics::TransactionErrorMetrics,
        transaction_processor::{ExecutionRecordingConfig, TransactionProcessingConfig},
    },
    solana_timings::ExecuteTimings,
    std::{
        sync::{atomic::Ordering, Arc},
        time::Instant,
    },
};

/// Consumer will create chunks of transactions from buffer with up to this size.
pub const TARGET_NUM_TRANSACTIONS_PER_BATCH: usize = 64;

pub struct ProcessTransactionBatchOutput {
    // The number of transactions filtered out by the cost model
    pub(crate) cost_model_throttled_transactions_count: u64,
    // Amount of time spent running the cost model
    pub(crate) cost_model_us: u64,
    pub execute_and_commit_transactions_output: ExecuteAndCommitTransactionsOutput,
}

pub struct ExecuteAndCommitTransactionsOutput {
    // Transactions counts reported to `ConsumeWorkerMetrics` and then
    // accumulated later for `LeaderSlotMetrics`
    pub(crate) transaction_counts: ExecuteAndCommitTransactionsCounts,
    // Transactions that either were not executed, or were executed and failed to be committed due
    // to the block ending.
    pub(crate) retryable_transaction_indexes: Vec<usize>,
    // A result that indicates whether transactions were successfully
    // committed into the Poh stream.
    pub commit_transactions_result: Result<Vec<CommitTransactionDetails>, PohRecorderError>,
    pub(crate) execute_and_commit_timings: LeaderExecuteAndCommitTimings,
    pub(crate) error_counters: TransactionErrorMetrics,
    pub(crate) min_prioritization_fees: u64,
    pub(crate) max_prioritization_fees: u64,
}

#[derive(Debug, Default, PartialEq)]
pub struct ExecuteAndCommitTransactionsCounts {
    // Total number of transactions that were passed as candidates for execution
    pub(crate) attempted_execution_count: u64,
    // The number of transactions of that were executed. See description of in `ProcessTransactionsSummary`
    // for possible outcomes of execution.
    pub(crate) executed_count: u64,
    // Total number of the executed transactions that returned success/not
    // an error.
    pub(crate) executed_with_successful_result_count: u64,
}

pub struct Consumer {
    committer: Committer,
    transaction_recorder: TransactionRecorder,
    qos_service: QosService,
    log_messages_bytes_limit: Option<usize>,
}

impl Consumer {
    pub fn new(
        committer: Committer,
        transaction_recorder: TransactionRecorder,
        qos_service: QosService,
        log_messages_bytes_limit: Option<usize>,
    ) -> Self {
        Self {
            committer,
            transaction_recorder,
            qos_service,
            log_messages_bytes_limit,
        }
    }

    pub fn consume_buffered_packets(
        &self,
        bank_start: &BankStart,
        unprocessed_transaction_storage: &mut UnprocessedTransactionStorage,
        banking_stage_stats: &BankingStageStats,
        slot_metrics_tracker: &mut LeaderSlotMetricsTracker,
    ) {
        let mut rebuffered_packet_count = 0;
        let mut consumed_buffered_packets_count = 0;
        let mut proc_start = Measure::start("consume_buffered_process");
        let num_packets_to_process = unprocessed_transaction_storage.len();

        let reached_end_of_slot = unprocessed_transaction_storage.process_packets(
            bank_start.working_bank.clone(),
            banking_stage_stats,
            slot_metrics_tracker,
            |packets_to_process, payload| {
                self.do_process_packets(
                    bank_start,
                    payload,
                    banking_stage_stats,
                    &mut consumed_buffered_packets_count,
                    &mut rebuffered_packet_count,
                    packets_to_process,
                )
            },
        );

        if reached_end_of_slot {
            slot_metrics_tracker.set_end_of_slot_unprocessed_buffer_len(
                unprocessed_transaction_storage.len() as u64,
            );
        }

        proc_start.stop();
        debug!(
            "@{:?} done processing buffered batches: {} time: {:?}ms tx count: {} tx/s: {}",
            timestamp(),
            num_packets_to_process,
            proc_start.as_ms(),
            consumed_buffered_packets_count,
            (consumed_buffered_packets_count as f32) / (proc_start.as_s())
        );

        banking_stage_stats
            .consume_buffered_packets_elapsed
            .fetch_add(proc_start.as_us(), Ordering::Relaxed);
        banking_stage_stats
            .rebuffered_packets_count
            .fetch_add(rebuffered_packet_count, Ordering::Relaxed);
        banking_stage_stats
            .consumed_buffered_packets_count
            .fetch_add(consumed_buffered_packets_count, Ordering::Relaxed);
    }

    fn do_process_packets(
        &self,
        bank_start: &BankStart,
        payload: &mut ConsumeScannerPayload,
        banking_stage_stats: &BankingStageStats,
        consumed_buffered_packets_count: &mut usize,
        rebuffered_packet_count: &mut usize,
        packets_to_process: &[Arc<ImmutableDeserializedPacket>],
    ) -> Option<Vec<usize>> {
        if payload.reached_end_of_slot {
            return None;
        }

        let packets_to_process_len = packets_to_process.len();
        let (process_transactions_summary, process_packets_transactions_us) = measure_us!(self
            .process_packets_transactions(
                &bank_start.working_bank,
                &bank_start.bank_creation_time,
                &payload.sanitized_transactions,
                banking_stage_stats,
                payload.slot_metrics_tracker,
            ));
        payload
            .slot_metrics_tracker
            .increment_process_packets_transactions_us(process_packets_transactions_us);

        // Clear payload for next iteration
        payload.sanitized_transactions.clear();
        payload.account_locks.clear();

        let ProcessTransactionsSummary {
            reached_max_poh_height,
            retryable_transaction_indexes,
            ..
        } = process_transactions_summary;

        if reached_max_poh_height || !bank_start.should_working_bank_still_be_processing_txs() {
            payload.reached_end_of_slot = true;
        }

        // The difference between all transactions passed to execution and the ones that
        // are retryable were the ones that were either:
        // 1) Committed into the block
        // 2) Dropped without being committed because they had some fatal error (too old,
        // duplicate signature, etc.)
        //
        // Note: This assumes that every packet deserializes into one transaction!
        *consumed_buffered_packets_count +=
            packets_to_process_len.saturating_sub(retryable_transaction_indexes.len());

        // Out of the buffered packets just retried, collect any still unprocessed
        // transactions in this batch for forwarding
        *rebuffered_packet_count += retryable_transaction_indexes.len();

        payload
            .slot_metrics_tracker
            .increment_retryable_packets_count(retryable_transaction_indexes.len() as u64);

        Some(retryable_transaction_indexes)
    }

    fn process_packets_transactions(
        &self,
        bank: &Arc<Bank>,
        bank_creation_time: &Instant,
        sanitized_transactions: &[SanitizedTransaction],
        banking_stage_stats: &BankingStageStats,
        slot_metrics_tracker: &mut LeaderSlotMetricsTracker,
    ) -> ProcessTransactionsSummary {
        let (mut process_transactions_summary, process_transactions_us) = measure_us!(
            self.process_transactions(bank, bank_creation_time, sanitized_transactions)
        );
        slot_metrics_tracker.increment_process_transactions_us(process_transactions_us);
        banking_stage_stats
            .transaction_processing_elapsed
            .fetch_add(process_transactions_us, Ordering::Relaxed);

        let ProcessTransactionsSummary {
            ref retryable_transaction_indexes,
            ref error_counters,
            ..
        } = process_transactions_summary;

        slot_metrics_tracker.accumulate_process_transactions_summary(&process_transactions_summary);
        slot_metrics_tracker.accumulate_transaction_errors(error_counters);

        // Filter out the retryable transactions that are too old
        let (filtered_retryable_transaction_indexes, filter_retryable_packets_us) =
            measure_us!(Self::filter_pending_packets_from_pending_txs(
                bank,
                sanitized_transactions,
                retryable_transaction_indexes,
            ));
        slot_metrics_tracker.increment_filter_retryable_packets_us(filter_retryable_packets_us);
        banking_stage_stats
            .filter_pending_packets_elapsed
            .fetch_add(filter_retryable_packets_us, Ordering::Relaxed);

        let retryable_packets_filtered_count = retryable_transaction_indexes
            .len()
            .saturating_sub(filtered_retryable_transaction_indexes.len());
        slot_metrics_tracker
            .increment_retryable_packets_filtered_count(retryable_packets_filtered_count as u64);

        banking_stage_stats
            .dropped_forward_packets_count
            .fetch_add(retryable_packets_filtered_count, Ordering::Relaxed);

        process_transactions_summary.retryable_transaction_indexes =
            filtered_retryable_transaction_indexes;
        process_transactions_summary
    }

    /// Sends transactions to the bank.
    ///
    /// Returns the number of transactions successfully processed by the bank, which may be less
    /// than the total number if max PoH height was reached and the bank halted
    fn process_transactions(
        &self,
        bank: &Arc<Bank>,
        bank_creation_time: &Instant,
        transactions: &[SanitizedTransaction],
    ) -> ProcessTransactionsSummary {
        let mut chunk_start = 0;
        let mut all_retryable_tx_indexes = vec![];
        let mut total_transaction_counts = ProcessTransactionsCounts::default();
        let mut total_cost_model_throttled_transactions_count: u64 = 0;
        let mut total_cost_model_us: u64 = 0;
        let mut total_execute_and_commit_timings = LeaderExecuteAndCommitTimings::default();
        let mut total_error_counters = TransactionErrorMetrics::default();
        let mut reached_max_poh_height = false;
        let mut overall_min_prioritization_fees: u64 = u64::MAX;
        let mut overall_max_prioritization_fees: u64 = 0;
        while chunk_start != transactions.len() {
            let chunk_end = std::cmp::min(
                transactions.len(),
                chunk_start + TARGET_NUM_TRANSACTIONS_PER_BATCH,
            );
            let process_transaction_batch_output = self.process_and_record_transactions(
                bank,
                &transactions[chunk_start..chunk_end],
                chunk_start,
            );

            let ProcessTransactionBatchOutput {
                cost_model_throttled_transactions_count: new_cost_model_throttled_transactions_count,
                cost_model_us: new_cost_model_us,
                execute_and_commit_transactions_output,
            } = process_transaction_batch_output;
            saturating_add_assign!(
                total_cost_model_throttled_transactions_count,
                new_cost_model_throttled_transactions_count
            );
            saturating_add_assign!(total_cost_model_us, new_cost_model_us);

            let ExecuteAndCommitTransactionsOutput {
                transaction_counts: new_transaction_counts,
                retryable_transaction_indexes: new_retryable_transaction_indexes,
                commit_transactions_result: new_commit_transactions_result,
                execute_and_commit_timings: new_execute_and_commit_timings,
                error_counters: new_error_counters,
                min_prioritization_fees,
                max_prioritization_fees,
                ..
            } = execute_and_commit_transactions_output;

            total_execute_and_commit_timings.accumulate(&new_execute_and_commit_timings);
            total_error_counters.accumulate(&new_error_counters);
            total_transaction_counts.accumulate(
                &new_transaction_counts,
                new_commit_transactions_result.is_ok(),
            );

            overall_min_prioritization_fees =
                std::cmp::min(overall_min_prioritization_fees, min_prioritization_fees);
            overall_max_prioritization_fees =
                std::cmp::min(overall_max_prioritization_fees, max_prioritization_fees);

            // Add the retryable txs (transactions that errored in a way that warrants a retry)
            // to the list of unprocessed txs.
            all_retryable_tx_indexes.extend_from_slice(&new_retryable_transaction_indexes);

            let should_bank_still_be_processing_txs =
                Bank::should_bank_still_be_processing_txs(bank_creation_time, bank.ns_per_slot);
            match (
                new_commit_transactions_result,
                should_bank_still_be_processing_txs,
            ) {
                (Err(PohRecorderError::MaxHeightReached), _) | (_, false) => {
                    info!(
                        "process transactions: max height reached slot: {} height: {}",
                        bank.slot(),
                        bank.tick_height()
                    );
                    // process_and_record_transactions has returned all retryable errors in
                    // transactions[chunk_start..chunk_end], so we just need to push the remaining
                    // transactions into the unprocessed queue.
                    all_retryable_tx_indexes.extend(chunk_end..transactions.len());
                    reached_max_poh_height = true;
                    break;
                }
                _ => (),
            }
            // Don't exit early on any other type of error, continue processing...
            chunk_start = chunk_end;
        }

        ProcessTransactionsSummary {
            reached_max_poh_height,
            transaction_counts: total_transaction_counts,
            retryable_transaction_indexes: all_retryable_tx_indexes,
            cost_model_throttled_transactions_count: total_cost_model_throttled_transactions_count,
            cost_model_us: total_cost_model_us,
            execute_and_commit_timings: total_execute_and_commit_timings,
            error_counters: total_error_counters,
            min_prioritization_fees: overall_min_prioritization_fees,
            max_prioritization_fees: overall_max_prioritization_fees,
        }
    }

    pub fn process_and_record_transactions(
        &self,
        bank: &Arc<Bank>,
        txs: &[SanitizedTransaction],
        chunk_offset: usize,
    ) -> ProcessTransactionBatchOutput {
        let mut error_counters = TransactionErrorMetrics::default();
        let pre_results = vec![Ok(()); txs.len()];
        let check_results =
            bank.check_transactions(txs, &pre_results, MAX_PROCESSING_AGE, &mut error_counters);
        // If checks passed, verify pre-compiles and continue processing on success.
        let check_results: Vec<_> = txs
            .iter()
            .zip(check_results)
            .map(|(tx, result)| match result {
                Ok(_) => tx.verify_precompiles(&bank.feature_set),
                Err(err) => Err(err),
            })
            .collect();
        let mut output = self.process_and_record_transactions_with_pre_results(
            bank,
            txs,
            chunk_offset,
            check_results.into_iter(),
        );

        // Accumulate error counters from the initial checks into final results
        output
            .execute_and_commit_transactions_output
            .error_counters
            .accumulate(&error_counters);
        output
    }

    pub fn process_and_record_aged_transactions(
        &self,
        bank: &Arc<Bank>,
        txs: &[SanitizedTransaction],
        max_slot_ages: &[Slot],
    ) -> ProcessTransactionBatchOutput {
        // Verify pre-compiles.
        // Need to filter out transactions since they were sanitized earlier.
        // This means that the transaction may cross and epoch boundary (not allowed),
        //  or account lookup tables may have been closed.
        let pre_results = txs.iter().zip(max_slot_ages).map(|(tx, max_slot_age)| {
            if *max_slot_age < bank.slot() {
                // Pre-compiles are verified here.
                // Attempt re-sanitization after epoch-cross.
                // Re-sanitized transaction should be equal to the original transaction,
                // but whether it will pass sanitization needs to be checked.
                let resanitized_tx =
                    bank.fully_verify_transaction(tx.to_versioned_transaction())?;
                if resanitized_tx != *tx {
                    // Sanitization before/after epoch give different transaction data - do not execute.
                    return Err(TransactionError::ResanitizationNeeded);
                }
            } else {
                // Verify pre-compiles.
                tx.verify_precompiles(&bank.feature_set)?;
                // Any transaction executed between sanitization time and now may have closed the lookup table(s).
                // Above re-sanitization already loads addresses, so don't need to re-check in that case.
                let lookup_tables = tx.message().message_address_table_lookups();
                if !lookup_tables.is_empty() {
                    bank.load_addresses(lookup_tables)?;
                }
            }
            Ok(())
        });
        self.process_and_record_transactions_with_pre_results(bank, txs, 0, pre_results)
    }

    fn process_and_record_transactions_with_pre_results(
        &self,
        bank: &Arc<Bank>,
        txs: &[SanitizedTransaction],
        chunk_offset: usize,
        pre_results: impl Iterator<Item = Result<(), TransactionError>>,
    ) -> ProcessTransactionBatchOutput {
        let (
            (transaction_qos_cost_results, cost_model_throttled_transactions_count),
            cost_model_us,
        ) = measure_us!(self.qos_service.select_and_accumulate_transaction_costs(
            bank,
            txs,
            pre_results
        ));

        // Only lock accounts for those transactions are selected for the block;
        // Once accounts are locked, other threads cannot encode transactions that will modify the
        // same account state
        let (batch, lock_us) = measure_us!(bank.prepare_sanitized_batch_with_results(
            txs,
            transaction_qos_cost_results.iter().map(|r| match r {
                Ok(_cost) => Ok(()),
                Err(err) => Err(err.clone()),
            })
        ));

        // retryable_txs includes AccountInUse, WouldExceedMaxBlockCostLimit
        // WouldExceedMaxAccountCostLimit, WouldExceedMaxVoteCostLimit
        // and WouldExceedMaxAccountDataCostLimit
        let mut execute_and_commit_transactions_output =
            self.execute_and_commit_transactions_locked(bank, &batch);

        // Once the accounts are new transactions can enter the pipeline to process them
        let (_, unlock_us) = measure_us!(drop(batch));

        let ExecuteAndCommitTransactionsOutput {
            ref mut retryable_transaction_indexes,
            ref execute_and_commit_timings,
            ref commit_transactions_result,
            ..
        } = execute_and_commit_transactions_output;

        // Costs of all transactions are added to the cost_tracker before processing.
        // To ensure accurate tracking of compute units, transactions that ultimately
        // were not included in the block should have their cost removed, the rest
        // should update with their actually consumed units.
        QosService::remove_or_update_costs(
            transaction_qos_cost_results.iter(),
            commit_transactions_result.as_ref().ok(),
            bank,
        );

        retryable_transaction_indexes
            .iter_mut()
            .for_each(|x| *x += chunk_offset);

        let (cu, us) =
            Self::accumulate_execute_units_and_time(&execute_and_commit_timings.execute_timings);
        self.qos_service.accumulate_actual_execute_cu(cu);
        self.qos_service.accumulate_actual_execute_time(us);

        // reports qos service stats for this batch
        self.qos_service.report_metrics(bank.slot());

        debug!(
            "bank: {} lock: {}us unlock: {}us txs_len: {}",
            bank.slot(),
            lock_us,
            unlock_us,
            txs.len(),
        );

        ProcessTransactionBatchOutput {
            cost_model_throttled_transactions_count,
            cost_model_us,
            execute_and_commit_transactions_output,
        }
    }

    fn execute_and_commit_transactions_locked(
        &self,
        bank: &Arc<Bank>,
        batch: &TransactionBatch,
    ) -> ExecuteAndCommitTransactionsOutput {
        let transaction_status_sender_enabled = self.committer.transaction_status_sender_enabled();
        let mut execute_and_commit_timings = LeaderExecuteAndCommitTimings::default();

        let mut pre_balance_info = PreBalanceInfo::default();
        let (_, collect_balances_us) = measure_us!({
            // If the extra meta-data services are enabled for RPC, collect the
            // pre-balances for native and token programs.
            if transaction_status_sender_enabled {
                pre_balance_info.native = bank.collect_balances(batch);
                pre_balance_info.token =
                    collect_token_balances(bank, batch, &mut pre_balance_info.mint_decimals)
            }
        });
        execute_and_commit_timings.collect_balances_us = collect_balances_us;

        let min_max = batch
            .sanitized_transactions()
            .iter()
            .filter_map(|transaction| {
                process_compute_budget_instructions(
                    transaction.message().program_instructions_iter(),
                )
                .ok()
                .map(|limits| limits.compute_unit_price)
            })
            .minmax();
        let (min_prioritization_fees, max_prioritization_fees) =
            min_max.into_option().unwrap_or_default();

        let mut error_counters = TransactionErrorMetrics::default();
        let mut retryable_transaction_indexes: Vec<_> = batch
            .lock_results()
            .iter()
            .enumerate()
            .filter_map(|(index, res)| match res {
                // following are retryable errors
                Err(TransactionError::AccountInUse) => {
                    error_counters.account_in_use += 1;
                    Some(index)
                }
                Err(TransactionError::WouldExceedMaxBlockCostLimit) => {
                    error_counters.would_exceed_max_block_cost_limit += 1;
                    Some(index)
                }
                Err(TransactionError::WouldExceedMaxVoteCostLimit) => {
                    error_counters.would_exceed_max_vote_cost_limit += 1;
                    Some(index)
                }
                Err(TransactionError::WouldExceedMaxAccountCostLimit) => {
                    error_counters.would_exceed_max_account_cost_limit += 1;
                    Some(index)
                }
                Err(TransactionError::WouldExceedAccountDataBlockLimit) => {
                    error_counters.would_exceed_account_data_block_limit += 1;
                    Some(index)
                }
                // following are non-retryable errors
                Err(TransactionError::TooManyAccountLocks) => {
                    error_counters.too_many_account_locks += 1;
                    None
                }
                Err(_) => None,
                Ok(_) => None,
            })
            .collect();

        let (load_and_execute_transactions_output, load_execute_us) = measure_us!(bank
            .load_and_execute_transactions(
                batch,
                MAX_PROCESSING_AGE,
                &mut execute_and_commit_timings.execute_timings,
                &mut error_counters,
                TransactionProcessingConfig {
                    account_overrides: None,
                    check_program_modification_slot: bank.check_program_modification_slot(),
                    compute_budget: bank.compute_budget(),
                    log_messages_bytes_limit: self.log_messages_bytes_limit,
                    limit_to_load_programs: true,
                    recording_config: ExecutionRecordingConfig::new_single_setting(
                        transaction_status_sender_enabled
                    ),
                    transaction_account_lock_limit: Some(bank.get_transaction_account_lock_limit()),
                }
            ));
        execute_and_commit_timings.load_execute_us = load_execute_us;

        let LoadAndExecuteTransactionsOutput {
            execution_results,
            execution_counts,
        } = load_and_execute_transactions_output;

        let transaction_counts = ExecuteAndCommitTransactionsCounts {
            executed_count: execution_counts.executed_transactions_count,
            executed_with_successful_result_count: execution_counts.executed_successfully_count,
            attempted_execution_count: execution_results.len() as u64,
        };

        let (executed_transactions, execution_results_to_transactions_us) =
            measure_us!(execution_results
                .iter()
                .zip(batch.sanitized_transactions())
                .filter_map(|(execution_result, tx)| {
                    if execution_result.was_executed() {
                        Some(tx.to_versioned_transaction())
                    } else {
                        None
                    }
                })
                .collect_vec());

        let (freeze_lock, freeze_lock_us) = measure_us!(bank.freeze_lock());
        execute_and_commit_timings.freeze_lock_us = freeze_lock_us;

        // In order to avoid a race condition, leaders must get the last
        // blockhash *before* recording transactions because recording
        // transactions will only succeed if the block max tick height hasn't
        // been reached yet. If they get the last blockhash *after* recording
        // transactions, the block max tick height could have already been
        // reached and the blockhash queue could have already been updated with
        // a new blockhash.
        let ((last_blockhash, lamports_per_signature), last_blockhash_us) =
            measure_us!(bank.last_blockhash_and_lamports_per_signature());
        execute_and_commit_timings.last_blockhash_us = last_blockhash_us;

        let (record_transactions_summary, record_us) = measure_us!(self
            .transaction_recorder
            .record_transactions(bank.slot(), executed_transactions));
        execute_and_commit_timings.record_us = record_us;

        let RecordTransactionsSummary {
            result: record_transactions_result,
            record_transactions_timings,
            starting_transaction_index,
        } = record_transactions_summary;
        execute_and_commit_timings.record_transactions_timings = RecordTransactionsTimings {
            execution_results_to_transactions_us,
            ..record_transactions_timings
        };

        if let Err(recorder_err) = record_transactions_result {
            retryable_transaction_indexes.extend(execution_results.iter().enumerate().filter_map(
                |(index, execution_result)| execution_result.was_executed().then_some(index),
            ));

            return ExecuteAndCommitTransactionsOutput {
                transaction_counts,
                retryable_transaction_indexes,
                commit_transactions_result: Err(recorder_err),
                execute_and_commit_timings,
                error_counters,
                min_prioritization_fees,
                max_prioritization_fees,
            };
        }

        let (commit_time_us, commit_transaction_statuses) =
            if execution_counts.executed_transactions_count != 0 {
                self.committer.commit_transactions(
                    batch,
                    execution_results,
                    last_blockhash,
                    lamports_per_signature,
                    starting_transaction_index,
                    bank,
                    &mut pre_balance_info,
                    &mut execute_and_commit_timings,
                    &execution_counts,
                )
            } else {
                (
                    0,
                    vec![CommitTransactionDetails::NotCommitted; execution_results.len()],
                )
            };

        drop(freeze_lock);

        debug!(
            "bank: {} process_and_record_locked: {}us record: {}us commit: {}us txs_len: {}",
            bank.slot(),
            load_execute_us,
            record_us,
            commit_time_us,
            batch.sanitized_transactions().len(),
        );

        debug!(
            "execute_and_commit_transactions_locked: {:?}",
            execute_and_commit_timings.execute_timings,
        );

        debug_assert_eq!(
            transaction_counts.attempted_execution_count,
            commit_transaction_statuses.len() as u64,
        );

        ExecuteAndCommitTransactionsOutput {
            transaction_counts,
            retryable_transaction_indexes,
            commit_transactions_result: Ok(commit_transaction_statuses),
            execute_and_commit_timings,
            error_counters,
            min_prioritization_fees,
            max_prioritization_fees,
        }
    }

    pub fn check_fee_payer_unlocked(
        bank: &Bank,
        message: &SanitizedMessage,
        error_counters: &mut TransactionErrorMetrics,
    ) -> Result<(), TransactionError> {
        let fee_payer = message.fee_payer();
        let fee_budget_limits = FeeBudgetLimits::from(process_compute_budget_instructions(
            message.program_instructions_iter(),
        )?);
        let fee = solana_fee::calculate_fee(
            message,
            bank.get_lamports_per_signature() == 0,
            bank.fee_structure().lamports_per_signature,
            fee_budget_limits.prioritization_fee,
            bank.feature_set
                .is_active(&feature_set::remove_rounding_in_fee_calculation::id()),
        );
        let (mut fee_payer_account, _slot) = bank
            .rc
            .accounts
            .accounts_db
            .load_with_fixed_root(&bank.ancestors, fee_payer)
            .ok_or(TransactionError::AccountNotFound)?;

        validate_fee_payer(
            fee_payer,
            &mut fee_payer_account,
            0,
            error_counters,
            bank.rent_collector(),
            fee,
        )
    }

    fn accumulate_execute_units_and_time(execute_timings: &ExecuteTimings) -> (u64, u64) {
        execute_timings.details.per_program_timings.values().fold(
            (0, 0),
            |(units, times), program_timings| {
                (
                    units
                        .saturating_add(program_timings.accumulated_units)
                        .saturating_add(program_timings.total_errored_units),
                    times.saturating_add(program_timings.accumulated_us),
                )
            },
        )
    }

    /// This function filters pending packets that are still valid
    /// # Arguments
    /// * `transactions` - a batch of transactions deserialized from packets
    /// * `pending_indexes` - identifies which indexes in the `transactions` list are still pending
    fn filter_pending_packets_from_pending_txs(
        bank: &Bank,
        transactions: &[SanitizedTransaction],
        pending_indexes: &[usize],
    ) -> Vec<usize> {
        let filter =
            Self::prepare_filter_for_pending_transactions(transactions.len(), pending_indexes);

        let results = bank.check_transactions_with_forwarding_delay(
            transactions,
            &filter,
            FORWARD_TRANSACTIONS_TO_LEADER_AT_SLOT_OFFSET,
        );

        Self::filter_valid_transaction_indexes(&results)
    }

    /// This function creates a filter of transaction results with Ok() for every pending
    /// transaction. The non-pending transactions are marked with TransactionError
    fn prepare_filter_for_pending_transactions(
        transactions_len: usize,
        pending_tx_indexes: &[usize],
    ) -> Vec<transaction::Result<()>> {
        let mut mask = vec![Err(TransactionError::BlockhashNotFound); transactions_len];
        pending_tx_indexes.iter().for_each(|x| mask[*x] = Ok(()));
        mask
    }

    /// This function returns a vector containing index of all valid transactions. A valid
    /// transaction has result Ok() as the value
    fn filter_valid_transaction_indexes(valid_txs: &[TransactionCheckResult]) -> Vec<usize> {
        valid_txs
            .iter()
            .enumerate()
            .filter_map(|(index, res)| res.as_ref().ok().map(|_| index))
            .collect_vec()
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::banking_stage::{
            immutable_deserialized_packet::DeserializedPacketError,
            tests::{create_slow_genesis_config, sanitize_transactions, simulate_poh},
            unprocessed_packet_batches::{DeserializedPacket, UnprocessedPacketBatches},
            unprocessed_transaction_storage::ThreadType,
        },
        crossbeam_channel::{unbounded, Receiver},
        solana_cost_model::{cost_model::CostModel, transaction_cost::TransactionCost},
        solana_entry::entry::{next_entry, next_versioned_entry},
        solana_ledger::{
            blockstore::{entries_to_test_shreds, Blockstore},
            blockstore_processor::TransactionStatusSender,
            genesis_utils::GenesisConfigInfo,
            get_tmp_ledger_path_auto_delete,
            leader_schedule_cache::LeaderScheduleCache,
        },
        solana_perf::packet::Packet,
        solana_poh::poh_recorder::{PohRecorder, Record, WorkingBankEntry},
        solana_rpc::transaction_status_service::TransactionStatusService,
        solana_runtime::{bank_forks::BankForks, prioritization_fee_cache::PrioritizationFeeCache},
        solana_sdk::{
            account::AccountSharedData,
            account_utils::StateMut,
            address_lookup_table::{
                self,
                state::{AddressLookupTable, LookupTableMeta},
            },
            compute_budget,
            fee_calculator::FeeCalculator,
            hash::Hash,
            instruction::InstructionError,
            message::{
                v0::{self, MessageAddressTableLookup},
                Message, MessageHeader, VersionedMessage,
            },
            nonce::{self, state::DurableNonce},
            nonce_account::verify_nonce_account,
            poh_config::PohConfig,
            pubkey::Pubkey,
            reserved_account_keys::ReservedAccountKeys,
            signature::Keypair,
            signer::Signer,
            system_instruction, system_program, system_transaction,
            transaction::{MessageHash, Transaction, VersionedTransaction},
        },
        solana_svm::account_loader::CheckedTransactionDetails,
        solana_timings::ProgramTiming,
        solana_transaction_status::{TransactionStatusMeta, VersionedTransactionWithStatusMeta},
        std::{
            borrow::Cow,
            path::Path,
            sync::{
                atomic::{AtomicBool, AtomicU64},
                RwLock,
            },
            thread::{Builder, JoinHandle},
            time::Duration,
        },
    };

    fn execute_transactions_with_dummy_poh_service(
        bank: Arc<Bank>,
        transactions: Vec<Transaction>,
    ) -> ProcessTransactionsSummary {
        let transactions = sanitize_transactions(transactions);
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path())
            .expect("Expected to be able to open database ledger");
        let (poh_recorder, _entry_receiver, record_receiver) = PohRecorder::new(
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

        poh_recorder
            .write()
            .unwrap()
            .set_bank_for_test(bank.clone());

        let poh_simulator = simulate_poh(record_receiver, &poh_recorder);

        let (replay_vote_sender, _replay_vote_receiver) = unbounded();
        let committer = Committer::new(
            None,
            replay_vote_sender,
            Arc::new(PrioritizationFeeCache::new(0u64)),
        );
        let consumer = Consumer::new(committer, recorder, QosService::new(1), None);
        let process_transactions_summary =
            consumer.process_transactions(&bank, &Instant::now(), &transactions);

        poh_recorder
            .read()
            .unwrap()
            .is_exited
            .store(true, Ordering::Relaxed);
        let _ = poh_simulator.join();

        process_transactions_summary
    }

    fn generate_new_address_lookup_table(
        authority: Option<Pubkey>,
        num_addresses: usize,
    ) -> AddressLookupTable<'static> {
        let mut addresses = Vec::with_capacity(num_addresses);
        addresses.resize_with(num_addresses, Pubkey::new_unique);
        AddressLookupTable {
            meta: LookupTableMeta {
                authority,
                ..LookupTableMeta::default()
            },
            addresses: Cow::Owned(addresses),
        }
    }

    fn store_nonce_account(
        bank: &Bank,
        account_address: Pubkey,
        nonce_state: nonce::State,
    ) -> AccountSharedData {
        let mut account = AccountSharedData::new(1, nonce::State::size(), &system_program::id());
        account
            .set_state(&nonce::state::Versions::new(nonce_state))
            .unwrap();
        bank.store_account(&account_address, &account);

        account
    }

    fn store_address_lookup_table(
        bank: &Bank,
        account_address: Pubkey,
        address_lookup_table: AddressLookupTable<'static>,
    ) -> AccountSharedData {
        let data = address_lookup_table.serialize_for_tests().unwrap();
        let mut account =
            AccountSharedData::new(1, data.len(), &address_lookup_table::program::id());
        account.set_data(data);
        bank.store_account(&account_address, &account);

        account
    }

    #[allow(clippy::type_complexity)]
    fn setup_conflicting_transactions(
        ledger_path: &Path,
    ) -> (
        Vec<Transaction>,
        Arc<Bank>,
        Arc<RwLock<BankForks>>,
        Arc<RwLock<PohRecorder>>,
        Receiver<WorkingBankEntry>,
        GenesisConfigInfo,
        JoinHandle<()>,
    ) {
        Blockstore::destroy(ledger_path).unwrap();
        let genesis_config_info = create_slow_genesis_config(100_000_000);
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = &genesis_config_info;
        let blockstore =
            Blockstore::open(ledger_path).expect("Expected to be able to open database ledger");
        let (bank, bank_forks) = Bank::new_no_wallclock_throttle_for_tests(genesis_config);
        let exit = Arc::new(AtomicBool::default());
        let (poh_recorder, entry_receiver, record_receiver) = PohRecorder::new(
            bank.tick_height(),
            bank.last_blockhash(),
            bank.clone(),
            Some((4, 4)),
            bank.ticks_per_slot(),
            Arc::new(blockstore),
            &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
            &PohConfig::default(),
            exit,
        );
        let poh_recorder = Arc::new(RwLock::new(poh_recorder));

        // Set up unparallelizable conflicting transactions
        let pubkey0 = solana_sdk::pubkey::new_rand();
        let pubkey1 = solana_sdk::pubkey::new_rand();
        let pubkey2 = solana_sdk::pubkey::new_rand();
        let transactions = vec![
            system_transaction::transfer(mint_keypair, &pubkey0, 1, genesis_config.hash()),
            system_transaction::transfer(mint_keypair, &pubkey1, 1, genesis_config.hash()),
            system_transaction::transfer(mint_keypair, &pubkey2, 1, genesis_config.hash()),
        ];
        let poh_simulator = simulate_poh(record_receiver, &poh_recorder);

        (
            transactions,
            bank,
            bank_forks,
            poh_recorder,
            entry_receiver,
            genesis_config_info,
            poh_simulator,
        )
    }

    fn transactions_to_deserialized_packets(
        transactions: &[Transaction],
    ) -> Result<Vec<DeserializedPacket>, DeserializedPacketError> {
        transactions
            .iter()
            .map(|transaction| {
                let packet = Packet::from_data(None, transaction)?;
                DeserializedPacket::new(packet)
            })
            .collect()
    }

    #[test]
    fn test_bank_process_and_record_transactions() {
        solana_logger::setup();
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_slow_genesis_config(10_000);
        let (bank, _bank_forks) = Bank::new_no_wallclock_throttle_for_tests(&genesis_config);
        let pubkey = solana_sdk::pubkey::new_rand();

        let transactions = sanitize_transactions(vec![system_transaction::transfer(
            &mint_keypair,
            &pubkey,
            1,
            genesis_config.hash(),
        )]);

        let ledger_path = get_tmp_ledger_path_auto_delete!();
        {
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

            poh_recorder
                .write()
                .unwrap()
                .set_bank_for_test(bank.clone());
            let (replay_vote_sender, _replay_vote_receiver) = unbounded();
            let committer = Committer::new(
                None,
                replay_vote_sender,
                Arc::new(PrioritizationFeeCache::new(0u64)),
            );
            let consumer = Consumer::new(committer, recorder, QosService::new(1), None);

            let process_transactions_batch_output =
                consumer.process_and_record_transactions(&bank, &transactions, 0);

            let ExecuteAndCommitTransactionsOutput {
                transaction_counts,
                commit_transactions_result,
                ..
            } = process_transactions_batch_output.execute_and_commit_transactions_output;

            assert_eq!(
                transaction_counts,
                ExecuteAndCommitTransactionsCounts {
                    attempted_execution_count: 1,
                    executed_count: 1,
                    executed_with_successful_result_count: 1,
                }
            );
            assert!(commit_transactions_result.is_ok());

            // Tick up to max tick height
            while poh_recorder.read().unwrap().tick_height() != bank.max_tick_height() {
                poh_recorder.write().unwrap().tick();
            }

            let mut done = false;
            // read entries until I find mine, might be ticks...
            while let Ok((_bank, (entry, _tick_height))) = entry_receiver.recv() {
                if !entry.is_tick() {
                    trace!("got entry");
                    assert_eq!(entry.transactions.len(), transactions.len());
                    assert_eq!(bank.get_balance(&pubkey), 1);
                    done = true;
                }
                if done {
                    break;
                }
            }
            trace!("done ticking");

            assert!(done);

            let transactions = sanitize_transactions(vec![system_transaction::transfer(
                &mint_keypair,
                &pubkey,
                2,
                genesis_config.hash(),
            )]);

            let process_transactions_batch_output =
                consumer.process_and_record_transactions(&bank, &transactions, 0);

            let ExecuteAndCommitTransactionsOutput {
                transaction_counts,
                retryable_transaction_indexes,
                commit_transactions_result,
                ..
            } = process_transactions_batch_output.execute_and_commit_transactions_output;
            assert_eq!(
                transaction_counts,
                ExecuteAndCommitTransactionsCounts {
                    attempted_execution_count: 1,
                    // Transactions was still executed, just wasn't committed, so should be counted here.
                    executed_count: 1,
                    executed_with_successful_result_count: 1,
                }
            );
            assert_eq!(retryable_transaction_indexes, vec![0]);
            assert_matches!(
                commit_transactions_result,
                Err(PohRecorderError::MaxHeightReached)
            );

            poh_recorder
                .read()
                .unwrap()
                .is_exited
                .store(true, Ordering::Relaxed);
            let _ = poh_simulator.join();

            assert_eq!(bank.get_balance(&pubkey), 1);
        }
        Blockstore::destroy(ledger_path.path()).unwrap();
    }

    #[test]
    fn test_bank_nonce_update_blockhash_queried_before_transaction_record() {
        solana_logger::setup();
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_slow_genesis_config(10_000);
        let (bank, _bank_forks) = Bank::new_no_wallclock_throttle_for_tests(&genesis_config);
        let pubkey = Pubkey::new_unique();

        // setup nonce account with a durable nonce different from the current
        // bank so that it can be advanced in this bank
        let durable_nonce = DurableNonce::from_blockhash(&Hash::new_unique());
        let nonce_hash = *durable_nonce.as_hash();
        let nonce_pubkey = Pubkey::new_unique();
        let nonce_state = nonce::State::Initialized(nonce::state::Data {
            authority: mint_keypair.pubkey(),
            durable_nonce,
            fee_calculator: FeeCalculator::new(5000),
        });

        store_nonce_account(&bank, nonce_pubkey, nonce_state);

        // setup a valid nonce tx which will fail during execution
        let transactions = sanitize_transactions(vec![system_transaction::nonced_transfer(
            &mint_keypair,
            &pubkey,
            u64::MAX,
            &nonce_pubkey,
            &mint_keypair,
            nonce_hash,
        )]);

        let ledger_path = get_tmp_ledger_path_auto_delete!();
        {
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
                Arc::new(AtomicBool::new(false)),
            );
            let recorder = poh_recorder.new_recorder();
            let poh_recorder = Arc::new(RwLock::new(poh_recorder));

            fn poh_tick_before_returning_record_response(
                record_receiver: Receiver<Record>,
                poh_recorder: Arc<RwLock<PohRecorder>>,
            ) -> JoinHandle<()> {
                let is_exited = poh_recorder.read().unwrap().is_exited.clone();
                let tick_producer = Builder::new()
                    .name("solana-simulate_poh".to_string())
                    .spawn(move || loop {
                        let timeout = Duration::from_millis(10);
                        let record = record_receiver.recv_timeout(timeout);
                        if let Ok(record) = record {
                            let record_response = poh_recorder.write().unwrap().record(
                                record.slot,
                                record.mixin,
                                record.transactions,
                            );
                            poh_recorder.write().unwrap().tick();
                            if record.sender.send(record_response).is_err() {
                                panic!("Error returning mixin hash");
                            }
                        }
                        if is_exited.load(Ordering::Relaxed) {
                            break;
                        }
                    });
                tick_producer.unwrap()
            }

            // Simulate a race condition by setting up poh to do the last tick
            // right before returning the transaction record response so that
            // bank blockhash queue is updated before transactions are
            // committed.
            let poh_simulator =
                poh_tick_before_returning_record_response(record_receiver, poh_recorder.clone());

            poh_recorder
                .write()
                .unwrap()
                .set_bank_for_test(bank.clone());

            // Tick up to max tick height - 1 so that only one tick remains
            // before recording transactions to poh
            while poh_recorder.read().unwrap().tick_height() != bank.max_tick_height() - 1 {
                poh_recorder.write().unwrap().tick();
            }

            let (replay_vote_sender, _replay_vote_receiver) = unbounded();
            let committer = Committer::new(
                None,
                replay_vote_sender,
                Arc::new(PrioritizationFeeCache::new(0u64)),
            );
            let consumer = Consumer::new(committer, recorder, QosService::new(1), None);

            let process_transactions_batch_output =
                consumer.process_and_record_transactions(&bank, &transactions, 0);
            let ExecuteAndCommitTransactionsOutput {
                transaction_counts,
                commit_transactions_result,
                ..
            } = process_transactions_batch_output.execute_and_commit_transactions_output;

            assert_eq!(
                transaction_counts,
                ExecuteAndCommitTransactionsCounts {
                    attempted_execution_count: 1,
                    executed_count: 1,
                    executed_with_successful_result_count: 0,
                }
            );
            assert!(commit_transactions_result.is_ok());

            // Ensure that poh did the last tick after recording transactions
            assert_eq!(
                poh_recorder.read().unwrap().tick_height(),
                bank.max_tick_height()
            );

            let mut done = false;
            // read entries until I find mine, might be ticks...
            while let Ok((_bank, (entry, _tick_height))) = entry_receiver.recv() {
                if !entry.is_tick() {
                    assert_eq!(entry.transactions.len(), transactions.len());
                    done = true;
                    break;
                }
            }
            assert!(done);

            poh_recorder
                .read()
                .unwrap()
                .is_exited
                .store(true, Ordering::Relaxed);
            let _ = poh_simulator.join();

            // check that the nonce was advanced to the current bank's last blockhash
            // rather than the current bank's blockhash as would occur had the update
            // blockhash been queried _after_ transaction recording
            let expected_nonce = DurableNonce::from_blockhash(&genesis_config.hash());
            let expected_nonce_hash = expected_nonce.as_hash();
            let nonce_account = bank.get_account(&nonce_pubkey).unwrap();
            assert!(verify_nonce_account(&nonce_account, expected_nonce_hash).is_some());
        }
        Blockstore::destroy(ledger_path.path()).unwrap();
    }

    #[test]
    fn test_bank_process_and_record_transactions_all_unexecuted() {
        solana_logger::setup();
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_slow_genesis_config(10_000);
        let (bank, _bank_forks) = Bank::new_no_wallclock_throttle_for_tests(&genesis_config);
        let pubkey = solana_sdk::pubkey::new_rand();

        let transactions = {
            let mut tx =
                system_transaction::transfer(&mint_keypair, &pubkey, 1, genesis_config.hash());
            // Add duplicate account key
            tx.message.account_keys.push(pubkey);
            sanitize_transactions(vec![tx])
        };

        let ledger_path = get_tmp_ledger_path_auto_delete!();
        {
            let blockstore = Blockstore::open(ledger_path.path())
                .expect("Expected to be able to open database ledger");
            let (poh_recorder, _entry_receiver, record_receiver) = PohRecorder::new(
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

            poh_recorder
                .write()
                .unwrap()
                .set_bank_for_test(bank.clone());
            let (replay_vote_sender, _replay_vote_receiver) = unbounded();
            let committer = Committer::new(
                None,
                replay_vote_sender,
                Arc::new(PrioritizationFeeCache::new(0u64)),
            );
            let consumer = Consumer::new(committer, recorder, QosService::new(1), None);

            let process_transactions_batch_output =
                consumer.process_and_record_transactions(&bank, &transactions, 0);

            let ExecuteAndCommitTransactionsOutput {
                transaction_counts,
                commit_transactions_result,
                retryable_transaction_indexes,
                ..
            } = process_transactions_batch_output.execute_and_commit_transactions_output;

            assert_eq!(
                transaction_counts,
                ExecuteAndCommitTransactionsCounts {
                    attempted_execution_count: 1,
                    executed_count: 0,
                    executed_with_successful_result_count: 0,
                }
            );
            assert!(retryable_transaction_indexes.is_empty());
            assert_eq!(
                commit_transactions_result.ok(),
                Some(vec![CommitTransactionDetails::NotCommitted; 1])
            );

            poh_recorder
                .read()
                .unwrap()
                .is_exited
                .store(true, Ordering::Relaxed);
            let _ = poh_simulator.join();
        }
        Blockstore::destroy(ledger_path.path()).unwrap();
    }

    #[test]
    fn test_bank_process_and_record_transactions_cost_tracker() {
        solana_logger::setup();
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_slow_genesis_config(10_000);
        let mut bank = Bank::new_for_tests(&genesis_config);
        bank.ns_per_slot = u128::MAX;
        let (bank, _bank_forks) = bank.wrap_with_bank_forks_for_tests();
        let pubkey = solana_sdk::pubkey::new_rand();

        let ledger_path = get_tmp_ledger_path_auto_delete!();
        {
            let blockstore = Blockstore::open(ledger_path.path())
                .expect("Expected to be able to open database ledger");
            let (poh_recorder, _entry_receiver, record_receiver) = PohRecorder::new(
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

            poh_recorder
                .write()
                .unwrap()
                .set_bank_for_test(bank.clone());
            let (replay_vote_sender, _replay_vote_receiver) = unbounded();
            let committer = Committer::new(
                None,
                replay_vote_sender,
                Arc::new(PrioritizationFeeCache::new(0u64)),
            );
            let consumer = Consumer::new(committer, recorder, QosService::new(1), None);

            let get_block_cost = || bank.read_cost_tracker().unwrap().block_cost();
            let get_tx_count = || bank.read_cost_tracker().unwrap().transaction_count();
            assert_eq!(get_block_cost(), 0);
            assert_eq!(get_tx_count(), 0);

            //
            // TEST: cost tracker's block cost increases when successfully processing a tx
            //

            let transactions = sanitize_transactions(vec![system_transaction::transfer(
                &mint_keypair,
                &pubkey,
                1,
                genesis_config.hash(),
            )]);

            let process_transactions_batch_output =
                consumer.process_and_record_transactions(&bank, &transactions, 0);

            let ExecuteAndCommitTransactionsOutput {
                transaction_counts,
                commit_transactions_result,
                ..
            } = process_transactions_batch_output.execute_and_commit_transactions_output;
            assert_eq!(transaction_counts.executed_with_successful_result_count, 1);
            assert!(commit_transactions_result.is_ok());

            let block_cost = get_block_cost();
            assert_ne!(block_cost, 0);
            assert_eq!(get_tx_count(), 1);

            // TEST: it's expected that the allocation will execute but the transfer will not
            // because of a shared write-lock between mint_keypair. Ensure only the first transaction
            // takes compute units in the block
            let allocate_keypair = Keypair::new();
            let transactions = sanitize_transactions(vec![
                system_transaction::allocate(
                    &mint_keypair,
                    &allocate_keypair,
                    genesis_config.hash(),
                    100,
                ),
                // this one won't execute in process_and_record_transactions from shared account lock overlap
                system_transaction::transfer(&mint_keypair, &pubkey, 2, genesis_config.hash()),
            ]);

            let process_transactions_batch_output =
                consumer.process_and_record_transactions(&bank, &transactions, 0);

            let ExecuteAndCommitTransactionsOutput {
                transaction_counts,
                commit_transactions_result,
                retryable_transaction_indexes,
                ..
            } = process_transactions_batch_output.execute_and_commit_transactions_output;
            assert_eq!(transaction_counts.executed_with_successful_result_count, 1);
            assert!(commit_transactions_result.is_ok());

            // first one should have been committed, second one not committed due to AccountInUse error during
            // account locking
            let commit_transactions_result = commit_transactions_result.unwrap();
            assert_eq!(commit_transactions_result.len(), 2);
            assert_matches!(
                commit_transactions_result.first(),
                Some(CommitTransactionDetails::Committed { .. })
            );
            assert_matches!(
                commit_transactions_result.get(1),
                Some(CommitTransactionDetails::NotCommitted)
            );
            assert_eq!(retryable_transaction_indexes, vec![1]);

            let expected_block_cost = {
                let (actual_programs_execution_cost, actual_loaded_accounts_data_size_cost) =
                    match commit_transactions_result.first().unwrap() {
                        CommitTransactionDetails::Committed {
                            compute_units,
                            loaded_accounts_data_size,
                        } => (
                            *compute_units,
                            CostModel::calculate_loaded_accounts_data_size_cost(
                                *loaded_accounts_data_size,
                                &bank.feature_set,
                            ),
                        ),
                        CommitTransactionDetails::NotCommitted => {
                            unreachable!()
                        }
                    };

                let mut cost = CostModel::calculate_cost(&transactions[0], &bank.feature_set);
                if let TransactionCost::Transaction(ref mut usage_cost) = cost {
                    usage_cost.programs_execution_cost = actual_programs_execution_cost;
                    usage_cost.loaded_accounts_data_size_cost =
                        actual_loaded_accounts_data_size_cost;
                }

                block_cost + cost.sum()
            };

            assert_eq!(get_block_cost(), expected_block_cost);
            assert_eq!(get_tx_count(), 2);

            poh_recorder
                .read()
                .unwrap()
                .is_exited
                .store(true, Ordering::Relaxed);
            let _ = poh_simulator.join();
        }
        Blockstore::destroy(ledger_path.path()).unwrap();
    }

    #[test]
    fn test_bank_process_and_record_transactions_account_in_use() {
        solana_logger::setup();
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_slow_genesis_config(10_000);
        let (bank, _bank_forks) = Bank::new_no_wallclock_throttle_for_tests(&genesis_config);
        let pubkey = solana_sdk::pubkey::new_rand();
        let pubkey1 = solana_sdk::pubkey::new_rand();

        let transactions = sanitize_transactions(vec![
            system_transaction::transfer(&mint_keypair, &pubkey, 1, genesis_config.hash()),
            system_transaction::transfer(&mint_keypair, &pubkey1, 1, genesis_config.hash()),
        ]);

        let ledger_path = get_tmp_ledger_path_auto_delete!();
        {
            let blockstore = Blockstore::open(ledger_path.path())
                .expect("Expected to be able to open database ledger");
            let (poh_recorder, _entry_receiver, record_receiver) = PohRecorder::new(
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

            poh_recorder
                .write()
                .unwrap()
                .set_bank_for_test(bank.clone());

            let poh_simulator = simulate_poh(record_receiver, &poh_recorder);

            let (replay_vote_sender, _replay_vote_receiver) = unbounded();
            let committer = Committer::new(
                None,
                replay_vote_sender,
                Arc::new(PrioritizationFeeCache::new(0u64)),
            );
            let consumer = Consumer::new(committer, recorder, QosService::new(1), None);

            let process_transactions_batch_output =
                consumer.process_and_record_transactions(&bank, &transactions, 0);

            poh_recorder
                .read()
                .unwrap()
                .is_exited
                .store(true, Ordering::Relaxed);
            let _ = poh_simulator.join();

            let ExecuteAndCommitTransactionsOutput {
                transaction_counts,
                retryable_transaction_indexes,
                commit_transactions_result,
                ..
            } = process_transactions_batch_output.execute_and_commit_transactions_output;

            assert_eq!(
                transaction_counts,
                ExecuteAndCommitTransactionsCounts {
                    attempted_execution_count: 2,
                    executed_count: 1,
                    executed_with_successful_result_count: 1,
                }
            );
            assert_eq!(retryable_transaction_indexes, vec![1]);
            assert!(commit_transactions_result.is_ok());
        }
        Blockstore::destroy(ledger_path.path()).unwrap();
    }

    #[test]
    fn test_process_transactions_instruction_error() {
        solana_logger::setup();
        let lamports = 10_000;
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_slow_genesis_config(lamports);
        let (bank, _bank_forks) = Bank::new_no_wallclock_throttle_for_tests(&genesis_config);
        // set cost tracker limits to MAX so it will not filter out TXs
        bank.write_cost_tracker()
            .unwrap()
            .set_limits(u64::MAX, u64::MAX, u64::MAX);

        // Transfer more than the balance of the mint keypair, should cause a
        // InstructionError::InsufficientFunds that is then committed. Needs to be
        // MAX_NUM_TRANSACTIONS_PER_BATCH at least so it doesn't conflict on account locks
        // with the below transaction
        let mut transactions = vec![
            system_transaction::transfer(
                &mint_keypair,
                &Pubkey::new_unique(),
                lamports + 1,
                genesis_config.hash(),
            );
            TARGET_NUM_TRANSACTIONS_PER_BATCH
        ];

        // Make one transaction that will succeed.
        transactions.push(system_transaction::transfer(
            &mint_keypair,
            &Pubkey::new_unique(),
            1,
            genesis_config.hash(),
        ));

        let transactions_len = transactions.len();
        let ProcessTransactionsSummary {
            reached_max_poh_height,
            transaction_counts,
            retryable_transaction_indexes,
            ..
        } = execute_transactions_with_dummy_poh_service(bank, transactions);

        // All the transactions should have been replayed, but only 1 committed
        assert!(!reached_max_poh_height);
        assert_eq!(
            transaction_counts,
            ProcessTransactionsCounts {
                attempted_execution_count: transactions_len as u64,
                // Both transactions should have been committed, even though one was an error,
                // because InstructionErrors are committed
                committed_transactions_count: 2,
                committed_transactions_with_successful_result_count: 1,
                executed_but_failed_commit: 0,
            }
        );
        assert_eq!(
            retryable_transaction_indexes,
            (1..transactions_len - 1).collect::<Vec<usize>>()
        );
    }

    #[test]
    fn test_process_transactions_account_in_use() {
        solana_logger::setup();
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_slow_genesis_config(10_000);
        let (bank, _bank_forks) = Bank::new_no_wallclock_throttle_for_tests(&genesis_config);
        // set cost tracker limits to MAX so it will not filter out TXs
        bank.write_cost_tracker()
            .unwrap()
            .set_limits(u64::MAX, u64::MAX, u64::MAX);

        // Make all repetitive transactions that conflict on the `mint_keypair`, so only 1 should be executed
        let mut transactions = vec![
            system_transaction::transfer(
                &mint_keypair,
                &Pubkey::new_unique(),
                1,
                genesis_config.hash()
            );
            TARGET_NUM_TRANSACTIONS_PER_BATCH
        ];

        // Make one more in separate batch that also conflicts, but because it's in a separate batch, it
        // should be executed
        transactions.push(system_transaction::transfer(
            &mint_keypair,
            &Pubkey::new_unique(),
            1,
            genesis_config.hash(),
        ));

        let transactions_len = transactions.len();
        let ProcessTransactionsSummary {
            reached_max_poh_height,
            transaction_counts,
            retryable_transaction_indexes,
            ..
        } = execute_transactions_with_dummy_poh_service(bank, transactions);

        // All the transactions should have been replayed, but only 2 committed (first and last)
        assert!(!reached_max_poh_height);
        assert_eq!(
            transaction_counts,
            ProcessTransactionsCounts {
                attempted_execution_count: transactions_len as u64,
                committed_transactions_count: 2,
                committed_transactions_with_successful_result_count: 2,
                executed_but_failed_commit: 0,
            }
        );

        // Everything except first and last index of the transactions failed and are last retryable
        assert_eq!(
            retryable_transaction_indexes,
            (1..transactions_len - 1).collect::<Vec<usize>>()
        );
    }

    #[test]
    fn test_process_transactions_returns_unprocessed_txs() {
        solana_logger::setup();
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_slow_genesis_config(10_000);
        let (bank, _bank_forks) = Bank::new_no_wallclock_throttle_for_tests(&genesis_config);

        let pubkey = solana_sdk::pubkey::new_rand();

        let transactions = sanitize_transactions(vec![system_transaction::transfer(
            &mint_keypair,
            &pubkey,
            1,
            genesis_config.hash(),
        )]);

        let ledger_path = get_tmp_ledger_path_auto_delete!();
        {
            let blockstore = Blockstore::open(ledger_path.path())
                .expect("Expected to be able to open database ledger");
            let (poh_recorder, _entry_receiver, record_receiver) = PohRecorder::new(
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

            // Poh Recorder has no working bank, so should throw MaxHeightReached error on
            // record
            let recorder = poh_recorder.new_recorder();

            let poh_simulator = simulate_poh(record_receiver, &Arc::new(RwLock::new(poh_recorder)));

            let (replay_vote_sender, _replay_vote_receiver) = unbounded();
            let committer = Committer::new(
                None,
                replay_vote_sender,
                Arc::new(PrioritizationFeeCache::new(0u64)),
            );
            let consumer = Consumer::new(committer, recorder.clone(), QosService::new(1), None);

            let process_transactions_summary =
                consumer.process_transactions(&bank, &Instant::now(), &transactions);

            let ProcessTransactionsSummary {
                reached_max_poh_height,
                transaction_counts,
                mut retryable_transaction_indexes,
                ..
            } = process_transactions_summary;
            assert!(reached_max_poh_height);
            assert_eq!(
                transaction_counts,
                ProcessTransactionsCounts {
                    attempted_execution_count: 1,
                    // MaxHeightReached error does not commit, should be zero here
                    committed_transactions_count: 0,
                    committed_transactions_with_successful_result_count: 0,
                    executed_but_failed_commit: 1,
                }
            );

            retryable_transaction_indexes.sort_unstable();
            let expected: Vec<usize> = (0..transactions.len()).collect();
            assert_eq!(retryable_transaction_indexes, expected);

            recorder.is_exited.store(true, Ordering::Relaxed);
            let _ = poh_simulator.join();
        }

        Blockstore::destroy(ledger_path.path()).unwrap();
    }

    #[test]
    fn test_write_persist_transaction_status() {
        solana_logger::setup();
        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair,
            ..
        } = create_slow_genesis_config(solana_sdk::native_token::sol_to_lamports(1000.0));
        genesis_config.rent.lamports_per_byte_year = 50;
        genesis_config.rent.exemption_threshold = 2.0;
        let (bank, _bank_forks) = Bank::new_no_wallclock_throttle_for_tests(&genesis_config);
        let pubkey = solana_sdk::pubkey::new_rand();
        let pubkey1 = solana_sdk::pubkey::new_rand();
        let keypair1 = Keypair::new();

        let rent_exempt_amount = bank.get_minimum_balance_for_rent_exemption(0);

        let success_tx = system_transaction::transfer(
            &mint_keypair,
            &pubkey,
            rent_exempt_amount,
            genesis_config.hash(),
        );
        let success_signature = success_tx.signatures[0];
        let entry_1 = next_entry(&genesis_config.hash(), 1, vec![success_tx.clone()]);
        let ix_error_tx = system_transaction::transfer(
            &keypair1,
            &pubkey1,
            2 * rent_exempt_amount,
            genesis_config.hash(),
        );
        let ix_error_signature = ix_error_tx.signatures[0];
        let entry_2 = next_entry(&entry_1.hash, 1, vec![ix_error_tx.clone()]);
        let entries = vec![entry_1, entry_2];

        let transactions = sanitize_transactions(vec![success_tx, ix_error_tx]);
        bank.transfer(rent_exempt_amount, &mint_keypair, &keypair1.pubkey())
            .unwrap();

        let ledger_path = get_tmp_ledger_path_auto_delete!();
        {
            let blockstore = Blockstore::open(ledger_path.path())
                .expect("Expected to be able to open database ledger");
            let blockstore = Arc::new(blockstore);
            let (poh_recorder, _entry_receiver, record_receiver) = PohRecorder::new(
                bank.tick_height(),
                bank.last_blockhash(),
                bank.clone(),
                Some((4, 4)),
                bank.ticks_per_slot(),
                blockstore.clone(),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
                &PohConfig::default(),
                Arc::new(AtomicBool::default()),
            );
            let recorder = poh_recorder.new_recorder();
            let poh_recorder = Arc::new(RwLock::new(poh_recorder));

            let poh_simulator = simulate_poh(record_receiver, &poh_recorder);

            poh_recorder
                .write()
                .unwrap()
                .set_bank_for_test(bank.clone());

            let shreds = entries_to_test_shreds(
                &entries,
                bank.slot(),
                0,    // parent_slot
                true, // is_full_slot
                0,    // version
                true, // merkle_variant
            );
            blockstore.insert_shreds(shreds, None, false).unwrap();
            blockstore.set_roots(std::iter::once(&bank.slot())).unwrap();

            let (transaction_status_sender, transaction_status_receiver) = unbounded();
            let transaction_status_service = TransactionStatusService::new(
                transaction_status_receiver,
                Arc::new(AtomicU64::default()),
                true,
                None,
                blockstore.clone(),
                false,
                Arc::new(AtomicBool::new(false)),
            );

            let (replay_vote_sender, _replay_vote_receiver) = unbounded();
            let committer = Committer::new(
                Some(TransactionStatusSender {
                    sender: transaction_status_sender,
                }),
                replay_vote_sender,
                Arc::new(PrioritizationFeeCache::new(0u64)),
            );
            let consumer = Consumer::new(committer, recorder, QosService::new(1), None);

            let _ = consumer.process_and_record_transactions(&bank, &transactions, 0);

            drop(consumer); // drop/disconnect transaction_status_sender
            transaction_status_service.join().unwrap();

            let confirmed_block = blockstore.get_rooted_block(bank.slot(), false).unwrap();
            let actual_tx_results: Vec<_> = confirmed_block
                .transactions
                .into_iter()
                .map(|VersionedTransactionWithStatusMeta { transaction, meta }| {
                    (transaction.signatures[0], meta.status)
                })
                .collect();
            let expected_tx_results = vec![
                (success_signature, Ok(())),
                (
                    ix_error_signature,
                    Err(TransactionError::InstructionError(
                        0,
                        InstructionError::Custom(1),
                    )),
                ),
            ];
            assert_eq!(actual_tx_results, expected_tx_results);

            poh_recorder
                .read()
                .unwrap()
                .is_exited
                .store(true, Ordering::Relaxed);
            let _ = poh_simulator.join();
        }
        Blockstore::destroy(ledger_path.path()).unwrap();
    }

    #[test]
    fn test_write_persist_loaded_addresses() {
        solana_logger::setup();
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_slow_genesis_config(10_000);
        let (bank, bank_forks) = Bank::new_no_wallclock_throttle_for_tests(&genesis_config);
        let keypair = Keypair::new();

        let address_table_key = Pubkey::new_unique();
        let address_table_state = generate_new_address_lookup_table(None, 2);
        store_address_lookup_table(&bank, address_table_key, address_table_state);

        let new_bank = Bank::new_from_parent(bank, &Pubkey::new_unique(), 2);
        let bank = bank_forks
            .write()
            .unwrap()
            .insert(new_bank)
            .clone_without_scheduler();
        let message = VersionedMessage::V0(v0::Message {
            header: MessageHeader {
                num_required_signatures: 1,
                num_readonly_signed_accounts: 0,
                num_readonly_unsigned_accounts: 0,
            },
            recent_blockhash: genesis_config.hash(),
            account_keys: vec![keypair.pubkey()],
            address_table_lookups: vec![MessageAddressTableLookup {
                account_key: address_table_key,
                writable_indexes: vec![0],
                readonly_indexes: vec![1],
            }],
            instructions: vec![],
        });

        let tx = VersionedTransaction::try_new(message, &[&keypair]).unwrap();
        let sanitized_tx = SanitizedTransaction::try_create(
            tx.clone(),
            MessageHash::Compute,
            Some(false),
            bank.as_ref(),
            &ReservedAccountKeys::empty_key_set(),
        )
        .unwrap();

        let entry = next_versioned_entry(&genesis_config.hash(), 1, vec![tx]);
        let entries = vec![entry];

        bank.transfer(1, &mint_keypair, &keypair.pubkey()).unwrap();

        let ledger_path = get_tmp_ledger_path_auto_delete!();
        {
            let blockstore = Blockstore::open(ledger_path.path())
                .expect("Expected to be able to open database ledger");
            let blockstore = Arc::new(blockstore);
            let (poh_recorder, _entry_receiver, record_receiver) = PohRecorder::new(
                bank.tick_height(),
                bank.last_blockhash(),
                bank.clone(),
                Some((4, 4)),
                bank.ticks_per_slot(),
                blockstore.clone(),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
                &PohConfig::default(),
                Arc::new(AtomicBool::default()),
            );
            let recorder = poh_recorder.new_recorder();
            let poh_recorder = Arc::new(RwLock::new(poh_recorder));

            let poh_simulator = simulate_poh(record_receiver, &poh_recorder);

            poh_recorder
                .write()
                .unwrap()
                .set_bank_for_test(bank.clone());

            let shreds = entries_to_test_shreds(
                &entries,
                bank.slot(),
                0,    // parent_slot
                true, // is_full_slot
                0,    // version
                true, // merkle_variant
            );
            blockstore.insert_shreds(shreds, None, false).unwrap();
            blockstore.set_roots(std::iter::once(&bank.slot())).unwrap();

            let (transaction_status_sender, transaction_status_receiver) = unbounded();
            let transaction_status_service = TransactionStatusService::new(
                transaction_status_receiver,
                Arc::new(AtomicU64::default()),
                true,
                None,
                blockstore.clone(),
                false,
                Arc::new(AtomicBool::new(false)),
            );

            let (replay_vote_sender, _replay_vote_receiver) = unbounded();
            let committer = Committer::new(
                Some(TransactionStatusSender {
                    sender: transaction_status_sender,
                }),
                replay_vote_sender,
                Arc::new(PrioritizationFeeCache::new(0u64)),
            );
            let consumer = Consumer::new(committer, recorder, QosService::new(1), None);

            let _ = consumer.process_and_record_transactions(&bank, &[sanitized_tx.clone()], 0);

            drop(consumer); // drop/disconnect transaction_status_sender
            transaction_status_service.join().unwrap();

            let mut confirmed_block = blockstore.get_rooted_block(bank.slot(), false).unwrap();
            assert_eq!(confirmed_block.transactions.len(), 1);

            let recorded_meta = confirmed_block.transactions.pop().unwrap().meta;
            assert_eq!(
                recorded_meta,
                TransactionStatusMeta {
                    status: Ok(()),
                    pre_balances: vec![1, 0, 0],
                    post_balances: vec![1, 0, 0],
                    pre_token_balances: Some(vec![]),
                    post_token_balances: Some(vec![]),
                    rewards: Some(vec![]),
                    loaded_addresses: sanitized_tx.get_loaded_addresses(),
                    compute_units_consumed: Some(0),
                    ..TransactionStatusMeta::default()
                }
            );
            poh_recorder
                .read()
                .unwrap()
                .is_exited
                .store(true, Ordering::Relaxed);
            let _ = poh_simulator.join();
        }
        Blockstore::destroy(ledger_path.path()).unwrap();
    }

    #[test]
    fn test_consume_buffered_packets() {
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        {
            let (transactions, bank, _bank_forks, poh_recorder, _entry_receiver, _, poh_simulator) =
                setup_conflicting_transactions(ledger_path.path());
            let recorder: TransactionRecorder = poh_recorder.read().unwrap().new_recorder();
            let num_conflicting_transactions = transactions.len();
            let deserialized_packets = transactions_to_deserialized_packets(&transactions).unwrap();
            assert_eq!(deserialized_packets.len(), num_conflicting_transactions);
            let mut buffered_packet_batches =
                UnprocessedTransactionStorage::new_transaction_storage(
                    UnprocessedPacketBatches::from_iter(
                        deserialized_packets,
                        num_conflicting_transactions,
                    ),
                    ThreadType::Transactions,
                );

            let (replay_vote_sender, _replay_vote_receiver) = unbounded();
            let committer = Committer::new(
                None,
                replay_vote_sender,
                Arc::new(PrioritizationFeeCache::new(0u64)),
            );
            let consumer = Consumer::new(committer, recorder, QosService::new(1), None);

            // When the working bank in poh_recorder is None, no packets should be processed (consume will not be called)
            assert!(!poh_recorder.read().unwrap().has_bank());
            assert_eq!(buffered_packet_batches.len(), num_conflicting_transactions);
            // When the working bank in poh_recorder is Some, all packets should be processed.
            // Multi-Iterator will process them 1-by-1 if all txs are conflicting.
            poh_recorder.write().unwrap().set_bank_for_test(bank);
            let bank_start = poh_recorder.read().unwrap().bank_start().unwrap();
            let banking_stage_stats = BankingStageStats::default();
            consumer.consume_buffered_packets(
                &bank_start,
                &mut buffered_packet_batches,
                &banking_stage_stats,
                &mut LeaderSlotMetricsTracker::new(0),
            );

            // Check that all packets were processed without retrying
            assert!(buffered_packet_batches.is_empty());
            assert_eq!(
                banking_stage_stats
                    .consumed_buffered_packets_count
                    .load(Ordering::Relaxed),
                num_conflicting_transactions
            );
            assert_eq!(
                banking_stage_stats
                    .rebuffered_packets_count
                    .load(Ordering::Relaxed),
                0
            );
            // Use bank to check the number of entries (batches)
            assert_eq!(bank_start.working_bank.transactions_per_entry_max(), 1);
            assert_eq!(
                bank_start.working_bank.transaction_entries_count(),
                num_conflicting_transactions as u64
            );

            poh_recorder
                .read()
                .unwrap()
                .is_exited
                .store(true, Ordering::Relaxed);
            let _ = poh_simulator.join();
        }
        Blockstore::destroy(ledger_path.path()).unwrap();
    }

    #[test]
    fn test_consume_buffered_packets_sanitization_error() {
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        {
            let (
                mut transactions,
                bank,
                _bank_forks,
                poh_recorder,
                _entry_receiver,
                _,
                poh_simulator,
            ) = setup_conflicting_transactions(ledger_path.path());
            let duplicate_account_key = transactions[0].message.account_keys[0];
            transactions[0]
                .message
                .account_keys
                .push(duplicate_account_key); // corrupt transaction
            let recorder = poh_recorder.read().unwrap().new_recorder();
            let num_conflicting_transactions = transactions.len();
            let deserialized_packets = transactions_to_deserialized_packets(&transactions).unwrap();
            assert_eq!(deserialized_packets.len(), num_conflicting_transactions);
            let mut buffered_packet_batches =
                UnprocessedTransactionStorage::new_transaction_storage(
                    UnprocessedPacketBatches::from_iter(
                        deserialized_packets,
                        num_conflicting_transactions,
                    ),
                    ThreadType::Transactions,
                );

            let (replay_vote_sender, _replay_vote_receiver) = unbounded();
            let committer = Committer::new(
                None,
                replay_vote_sender,
                Arc::new(PrioritizationFeeCache::new(0u64)),
            );
            let consumer = Consumer::new(committer, recorder, QosService::new(1), None);

            // When the working bank in poh_recorder is None, no packets should be processed
            assert!(!poh_recorder.read().unwrap().has_bank());
            assert_eq!(buffered_packet_batches.len(), num_conflicting_transactions);
            // When the working bank in poh_recorder is Some, all packets should be processed.
            // Multi-Iterator will process them 1-by-1 if all txs are conflicting.
            poh_recorder.write().unwrap().set_bank_for_test(bank);
            let bank_start = poh_recorder.read().unwrap().bank_start().unwrap();
            consumer.consume_buffered_packets(
                &bank_start,
                &mut buffered_packet_batches,
                &BankingStageStats::default(),
                &mut LeaderSlotMetricsTracker::new(0),
            );
            assert!(buffered_packet_batches.is_empty());
            poh_recorder
                .read()
                .unwrap()
                .is_exited
                .store(true, Ordering::Relaxed);
            let _ = poh_simulator.join();
        }
        Blockstore::destroy(ledger_path.path()).unwrap();
    }

    #[test]
    fn test_consume_buffered_packets_retryable() {
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        {
            let (transactions, bank, _bank_forks, poh_recorder, _entry_receiver, _, poh_simulator) =
                setup_conflicting_transactions(ledger_path.path());
            let recorder = poh_recorder.read().unwrap().new_recorder();
            let num_conflicting_transactions = transactions.len();
            let deserialized_packets = transactions_to_deserialized_packets(&transactions).unwrap();
            assert_eq!(deserialized_packets.len(), num_conflicting_transactions);
            let retryable_packet = deserialized_packets[0].clone();
            let mut buffered_packet_batches =
                UnprocessedTransactionStorage::new_transaction_storage(
                    UnprocessedPacketBatches::from_iter(
                        deserialized_packets,
                        num_conflicting_transactions,
                    ),
                    ThreadType::Transactions,
                );

            let (replay_vote_sender, _replay_vote_receiver) = unbounded();
            let committer = Committer::new(
                None,
                replay_vote_sender,
                Arc::new(PrioritizationFeeCache::new(0u64)),
            );
            let consumer = Consumer::new(committer, recorder, QosService::new(1), None);

            // When the working bank in poh_recorder is None, no packets should be processed (consume will not be called)
            assert!(!poh_recorder.read().unwrap().has_bank());
            assert_eq!(buffered_packet_batches.len(), num_conflicting_transactions);
            // When the working bank in poh_recorder is Some, all packets should be processed
            // except except for retryable errors. Manually take the lock of a transaction to
            // simulate another thread processing a transaction with that lock.
            poh_recorder
                .write()
                .unwrap()
                .set_bank_for_test(bank.clone());
            let bank_start = poh_recorder.read().unwrap().bank_start().unwrap();

            let lock_account = transactions[0].message.account_keys[1];
            let manual_lock_tx =
                SanitizedTransaction::from_transaction_for_tests(system_transaction::transfer(
                    &Keypair::new(),
                    &lock_account,
                    1,
                    bank.last_blockhash(),
                ));
            let _ = bank_start.working_bank.accounts().lock_accounts(
                std::iter::once(&manual_lock_tx),
                bank_start.working_bank.get_transaction_account_lock_limit(),
            );

            let banking_stage_stats = BankingStageStats::default();
            consumer.consume_buffered_packets(
                &bank_start,
                &mut buffered_packet_batches,
                &banking_stage_stats,
                &mut LeaderSlotMetricsTracker::new(0),
            );

            // Check that all but 1 transaction was processed. And that it was rebuffered.
            assert_eq!(buffered_packet_batches.len(), 1);
            assert_eq!(
                buffered_packet_batches.iter().next().unwrap(),
                &retryable_packet
            );
            assert_eq!(
                banking_stage_stats
                    .consumed_buffered_packets_count
                    .load(Ordering::Relaxed),
                num_conflicting_transactions - 1,
            );
            assert_eq!(
                banking_stage_stats
                    .rebuffered_packets_count
                    .load(Ordering::Relaxed),
                1
            );
            // Use bank to check the number of entries (batches)
            assert_eq!(bank_start.working_bank.transactions_per_entry_max(), 1);
            assert_eq!(
                bank_start.working_bank.transaction_entries_count(),
                num_conflicting_transactions as u64 - 1
            );

            poh_recorder
                .read()
                .unwrap()
                .is_exited
                .store(true, Ordering::Relaxed);
            let _ = poh_simulator.join();
        }
        Blockstore::destroy(ledger_path.path()).unwrap();
    }

    #[test]
    fn test_consume_buffered_packets_batch_priority_guard() {
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        {
            let (
                _,
                bank,
                _bank_forks,
                poh_recorder,
                _entry_receiver,
                genesis_config_info,
                poh_simulator,
            ) = setup_conflicting_transactions(ledger_path.path());
            let recorder = poh_recorder.read().unwrap().new_recorder();

            // Setup transactions:
            // [(AB), (BC), (CD)]
            // (AB) and (BC) are conflicting, and cannot go into the same batch.
            // (AB) and (CD) are not conflict. However, (CD) should not be able to take locks needed by (BC).
            let keypair_a = Keypair::new();
            let keypair_b = Keypair::new();
            let keypair_c = Keypair::new();
            let keypair_d = Keypair::new();
            for keypair in &[&keypair_a, &keypair_b, &keypair_c, &keypair_d] {
                bank.transfer(5_000, &genesis_config_info.mint_keypair, &keypair.pubkey())
                    .unwrap();
            }

            let make_prioritized_transfer =
                |from: &Keypair, to, lamports, priority| -> Transaction {
                    let ixs = vec![
                        system_instruction::transfer(&from.pubkey(), to, lamports),
                        compute_budget::ComputeBudgetInstruction::set_compute_unit_price(priority),
                    ];
                    let message = Message::new(&ixs, Some(&from.pubkey()));
                    Transaction::new(&[from], message, bank.last_blockhash())
                };

            let transactions = vec![
                make_prioritized_transfer(&keypair_a, &keypair_b.pubkey(), 1, 3),
                make_prioritized_transfer(&keypair_b, &keypair_c.pubkey(), 1, 2),
                make_prioritized_transfer(&keypair_c, &keypair_d.pubkey(), 1, 1),
            ];

            let num_conflicting_transactions = transactions.len();
            let deserialized_packets = transactions_to_deserialized_packets(&transactions).unwrap();
            assert_eq!(deserialized_packets.len(), num_conflicting_transactions);
            let mut buffered_packet_batches =
                UnprocessedTransactionStorage::new_transaction_storage(
                    UnprocessedPacketBatches::from_iter(
                        deserialized_packets,
                        num_conflicting_transactions,
                    ),
                    ThreadType::Transactions,
                );

            let (replay_vote_sender, _replay_vote_receiver) = unbounded();
            let committer = Committer::new(
                None,
                replay_vote_sender,
                Arc::new(PrioritizationFeeCache::new(0u64)),
            );
            let consumer = Consumer::new(committer, recorder, QosService::new(1), None);

            // When the working bank in poh_recorder is None, no packets should be processed (consume will not be called)
            assert!(!poh_recorder.read().unwrap().has_bank());
            assert_eq!(buffered_packet_batches.len(), num_conflicting_transactions);
            // When the working bank in poh_recorder is Some, all packets should be processed.
            // Multi-Iterator will process them 1-by-1 if all txs are conflicting.
            poh_recorder.write().unwrap().set_bank_for_test(bank);
            let bank_start = poh_recorder.read().unwrap().bank_start().unwrap();
            let banking_stage_stats = BankingStageStats::default();
            consumer.consume_buffered_packets(
                &bank_start,
                &mut buffered_packet_batches,
                &banking_stage_stats,
                &mut LeaderSlotMetricsTracker::new(0),
            );

            // Check that all packets were processed without retrying
            assert!(buffered_packet_batches.is_empty());
            assert_eq!(
                banking_stage_stats
                    .consumed_buffered_packets_count
                    .load(Ordering::Relaxed),
                num_conflicting_transactions
            );
            assert_eq!(
                banking_stage_stats
                    .rebuffered_packets_count
                    .load(Ordering::Relaxed),
                0
            );
            // Use bank to check the number of entries (batches)
            assert_eq!(bank_start.working_bank.transactions_per_entry_max(), 1);
            assert_eq!(
                bank_start.working_bank.transaction_entries_count(),
                4 + num_conflicting_transactions as u64 // 4 for funding transfers
            );

            poh_recorder
                .read()
                .unwrap()
                .is_exited
                .store(true, Ordering::Relaxed);
            let _ = poh_simulator.join();
        }
        Blockstore::destroy(ledger_path.path()).unwrap();
    }

    #[test]
    fn test_accumulate_execute_units_and_time() {
        let mut execute_timings = ExecuteTimings::default();
        let mut expected_units = 0;
        let mut expected_us = 0;

        for n in 0..10 {
            execute_timings.details.per_program_timings.insert(
                Pubkey::new_unique(),
                ProgramTiming {
                    accumulated_us: n * 100,
                    accumulated_units: n * 1000,
                    count: n as u32,
                    errored_txs_compute_consumed: vec![],
                    total_errored_units: 0,
                },
            );
            expected_us += n * 100;
            expected_units += n * 1000;
        }

        let (units, us) = Consumer::accumulate_execute_units_and_time(&execute_timings);

        assert_eq!(expected_units, units);
        assert_eq!(expected_us, us);
    }

    #[test]
    fn test_bank_prepare_filter_for_pending_transaction() {
        assert_eq!(
            Consumer::prepare_filter_for_pending_transactions(6, &[2, 4, 5]),
            vec![
                Err(TransactionError::BlockhashNotFound),
                Err(TransactionError::BlockhashNotFound),
                Ok(()),
                Err(TransactionError::BlockhashNotFound),
                Ok(()),
                Ok(())
            ]
        );

        assert_eq!(
            Consumer::prepare_filter_for_pending_transactions(6, &[0, 2, 3]),
            vec![
                Ok(()),
                Err(TransactionError::BlockhashNotFound),
                Ok(()),
                Ok(()),
                Err(TransactionError::BlockhashNotFound),
                Err(TransactionError::BlockhashNotFound),
            ]
        );
    }

    #[test]
    fn test_bank_filter_valid_transaction_indexes() {
        assert_eq!(
            Consumer::filter_valid_transaction_indexes(&[
                Err(TransactionError::BlockhashNotFound),
                Err(TransactionError::BlockhashNotFound),
                Ok(CheckedTransactionDetails {
                    nonce: None,
                    lamports_per_signature: 0
                }),
                Err(TransactionError::BlockhashNotFound),
                Ok(CheckedTransactionDetails {
                    nonce: None,
                    lamports_per_signature: 0
                }),
                Ok(CheckedTransactionDetails {
                    nonce: None,
                    lamports_per_signature: 0
                }),
            ]),
            [2, 4, 5]
        );

        assert_eq!(
            Consumer::filter_valid_transaction_indexes(&[
                Ok(CheckedTransactionDetails {
                    nonce: None,
                    lamports_per_signature: 0,
                }),
                Err(TransactionError::BlockhashNotFound),
                Err(TransactionError::BlockhashNotFound),
                Ok(CheckedTransactionDetails {
                    nonce: None,
                    lamports_per_signature: 0,
                }),
                Ok(CheckedTransactionDetails {
                    nonce: None,
                    lamports_per_signature: 0,
                }),
                Ok(CheckedTransactionDetails {
                    nonce: None,
                    lamports_per_signature: 0,
                }),
            ]),
            [0, 3, 4, 5]
        );
    }
}

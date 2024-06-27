#[cfg(feature = "dev-context-only-utils")]
use qualifier_attr::qualifiers;
use {
    crate::{
        account_loader::{
            collect_rent_from_account, load_accounts, validate_fee_payer,
            CheckedTransactionDetails, LoadedTransaction, TransactionCheckResult,
            TransactionLoadResult, TransactionValidationResult, ValidatedTransactionDetails,
        },
        account_overrides::AccountOverrides,
        message_processor::MessageProcessor,
        program_loader::{get_program_modification_slot, load_program_with_pubkey},
        rollback_accounts::RollbackAccounts,
        transaction_account_state_info::TransactionAccountStateInfo,
        transaction_error_metrics::TransactionErrorMetrics,
        transaction_processing_callback::TransactionProcessingCallback,
        transaction_results::{TransactionExecutionDetails, TransactionExecutionResult},
    },
    log::debug,
    percentage::Percentage,
    solana_bpf_loader_program::syscalls::create_program_runtime_environment_v1,
    solana_compute_budget::{
        compute_budget::ComputeBudget,
        compute_budget_processor::process_compute_budget_instructions,
    },
    solana_loader_v4_program::create_program_runtime_environment_v2,
    solana_measure::{measure, measure::Measure},
    solana_program_runtime::{
        invoke_context::{EnvironmentConfig, InvokeContext},
        loaded_programs::{
            ForkGraph, ProgramCache, ProgramCacheEntry, ProgramCacheForTxBatch,
            ProgramCacheMatchCriteria, ProgramRuntimeEnvironments,
        },
        log_collector::LogCollector,
        sysvar_cache::SysvarCache,
        timings::{ExecuteTimingType, ExecuteTimings},
    },
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount, PROGRAM_OWNERS},
        clock::{Epoch, Slot},
        feature_set::{
            include_loaded_accounts_data_size_in_fee_calculation,
            remove_rounding_in_fee_calculation, FeatureSet,
        },
        fee::{FeeBudgetLimits, FeeStructure},
        hash::Hash,
        inner_instruction::{InnerInstruction, InnerInstructionsList},
        instruction::{CompiledInstruction, TRANSACTION_LEVEL_STACK_HEIGHT},
        message::SanitizedMessage,
        pubkey::Pubkey,
        rent_collector::RentCollector,
        saturating_add_assign,
        transaction::{self, SanitizedTransaction, TransactionError},
        transaction_context::{ExecutionRecord, TransactionContext},
    },
    solana_type_overrides::sync::{atomic::Ordering, Arc, RwLock, RwLockReadGuard},
    solana_vote::vote_account::VoteAccountsHashMap,
    std::{
        cell::RefCell,
        collections::{hash_map::Entry, HashMap, HashSet},
        fmt::{Debug, Formatter},
        rc::Rc,
    },
};

/// A list of log messages emitted during a transaction
pub type TransactionLogMessages = Vec<String>;

/// The output of the transaction batch processor's
/// `load_and_execute_sanitized_transactions` method.
pub struct LoadAndExecuteSanitizedTransactionsOutput {
    /// Error metrics for transactions that were processed.
    pub error_metrics: TransactionErrorMetrics,
    /// Timings for transaction batch execution.
    pub execute_timings: ExecuteTimings,
    // Vector of results indicating whether a transaction was executed or could not
    // be executed. Note executed transactions can still have failed!
    pub execution_results: Vec<TransactionExecutionResult>,
    // Vector of loaded transactions from transactions that were processed.
    pub loaded_transactions: Vec<TransactionLoadResult>,
}

/// Configuration of the recording capabilities for transaction execution
#[derive(Copy, Clone, Default)]
pub struct ExecutionRecordingConfig {
    pub enable_cpi_recording: bool,
    pub enable_log_recording: bool,
    pub enable_return_data_recording: bool,
}

impl ExecutionRecordingConfig {
    pub fn new_single_setting(option: bool) -> Self {
        ExecutionRecordingConfig {
            enable_return_data_recording: option,
            enable_log_recording: option,
            enable_cpi_recording: option,
        }
    }
}

/// Configurations for processing transactions.
#[derive(Default)]
pub struct TransactionProcessingConfig<'a> {
    /// Encapsulates overridden accounts, typically used for transaction
    /// simulation.
    pub account_overrides: Option<&'a AccountOverrides>,
    /// Whether or not to check a program's modification slot when replenishing
    /// a program cache instance.
    pub check_program_modification_slot: bool,
    /// The compute budget to use for transaction execution.
    pub compute_budget: Option<ComputeBudget>,
    /// The maximum number of bytes that log messages can consume.
    pub log_messages_bytes_limit: Option<usize>,
    /// Whether to limit the number of programs loaded for the transaction
    /// batch.
    pub limit_to_load_programs: bool,
    /// Recording capabilities for transaction execution.
    pub recording_config: ExecutionRecordingConfig,
    /// The max number of accounts that a transaction may lock.
    pub transaction_account_lock_limit: Option<usize>,
}

/// Runtime environment for transaction batch processing.
#[derive(Default)]
pub struct TransactionProcessingEnvironment<'a> {
    /// The blockhash to use for the transaction batch.
    pub blockhash: Hash,
    /// The total stake for the current epoch.
    pub epoch_total_stake: Option<u64>,
    /// The vote accounts for the current epoch.
    pub epoch_vote_accounts: Option<&'a VoteAccountsHashMap>,
    /// Runtime feature set to use for the transaction batch.
    pub feature_set: Arc<FeatureSet>,
    /// Fee structure to use for assessing transaction fees.
    pub fee_structure: Option<&'a FeeStructure>,
    /// Lamports per signature to charge per transaction.
    pub lamports_per_signature: u64,
    /// Rent collector to use for the transaction batch.
    pub rent_collector: Option<&'a RentCollector>,
}

#[cfg_attr(feature = "frozen-abi", derive(AbiExample))]
pub struct TransactionBatchProcessor<FG: ForkGraph> {
    /// Bank slot (i.e. block)
    slot: Slot,

    /// Bank epoch
    epoch: Epoch,

    /// SysvarCache is a collection of system variables that are
    /// accessible from on chain programs. It is passed to SVM from
    /// client code (e.g. Bank) and forwarded to the MessageProcessor.
    sysvar_cache: RwLock<SysvarCache>,

    /// Programs required for transaction batch processing
    pub program_cache: Arc<RwLock<ProgramCache<FG>>>,

    /// Builtin program ids
    pub builtin_program_ids: RwLock<HashSet<Pubkey>>,
}

impl<FG: ForkGraph> Debug for TransactionBatchProcessor<FG> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransactionBatchProcessor")
            .field("slot", &self.slot)
            .field("epoch", &self.epoch)
            .field("sysvar_cache", &self.sysvar_cache)
            .field("program_cache", &self.program_cache)
            .finish()
    }
}

impl<FG: ForkGraph> Default for TransactionBatchProcessor<FG> {
    fn default() -> Self {
        Self {
            slot: Slot::default(),
            epoch: Epoch::default(),
            sysvar_cache: RwLock::<SysvarCache>::default(),
            program_cache: Arc::new(RwLock::new(ProgramCache::new(
                Slot::default(),
                Epoch::default(),
            ))),
            builtin_program_ids: RwLock::new(HashSet::new()),
        }
    }
}

impl<FG: ForkGraph> TransactionBatchProcessor<FG> {
    pub fn new(slot: Slot, epoch: Epoch, builtin_program_ids: HashSet<Pubkey>) -> Self {
        Self {
            slot,
            epoch,
            sysvar_cache: RwLock::<SysvarCache>::default(),
            program_cache: Arc::new(RwLock::new(ProgramCache::new(slot, epoch))),
            builtin_program_ids: RwLock::new(builtin_program_ids),
        }
    }

    pub fn new_from(&self, slot: Slot, epoch: Epoch) -> Self {
        Self {
            slot,
            epoch,
            sysvar_cache: RwLock::<SysvarCache>::default(),
            program_cache: self.program_cache.clone(),
            builtin_program_ids: RwLock::new(self.builtin_program_ids.read().unwrap().clone()),
        }
    }

    /// Returns the current environments depending on the given epoch
    /// Returns None if the call could result in a deadlock
    #[cfg_attr(feature = "dev-context-only-utils", qualifiers(pub))]
    pub fn get_environments_for_epoch(&self, epoch: Epoch) -> Option<ProgramRuntimeEnvironments> {
        self.program_cache
            .try_read()
            .ok()
            .map(|cache| cache.get_environments_for_epoch(epoch))
    }

    pub fn sysvar_cache(&self) -> RwLockReadGuard<SysvarCache> {
        self.sysvar_cache.read().unwrap()
    }

    /// Main entrypoint to the SVM.
    pub fn load_and_execute_sanitized_transactions<CB: TransactionProcessingCallback>(
        &self,
        callbacks: &CB,
        sanitized_txs: &[SanitizedTransaction],
        check_results: Vec<TransactionCheckResult>,
        environment: &TransactionProcessingEnvironment,
        config: &TransactionProcessingConfig,
    ) -> LoadAndExecuteSanitizedTransactionsOutput {
        // Initialize metrics.
        let mut error_metrics = TransactionErrorMetrics::default();
        let mut execute_timings = ExecuteTimings::default();

        let (validation_results, validate_fees_time) = measure!(self.validate_fees(
            callbacks,
            sanitized_txs,
            check_results,
            &environment.feature_set,
            environment
                .fee_structure
                .unwrap_or(&FeeStructure::default()),
            environment
                .rent_collector
                .unwrap_or(&RentCollector::default()),
            &mut error_metrics
        ));

        let mut program_cache_time = Measure::start("program_cache");
        let mut program_accounts_map = Self::filter_executable_program_accounts(
            callbacks,
            sanitized_txs,
            &validation_results,
            PROGRAM_OWNERS,
        );
        for builtin_program in self.builtin_program_ids.read().unwrap().iter() {
            program_accounts_map.insert(*builtin_program, 0);
        }

        let program_cache_for_tx_batch = Rc::new(RefCell::new(self.replenish_program_cache(
            callbacks,
            &program_accounts_map,
            config.check_program_modification_slot,
            config.limit_to_load_programs,
        )));

        if program_cache_for_tx_batch.borrow().hit_max_limit {
            const ERROR: TransactionError = TransactionError::ProgramCacheHitMaxLimit;
            let loaded_transactions = vec![Err(ERROR); sanitized_txs.len()];
            let execution_results =
                vec![TransactionExecutionResult::NotExecuted(ERROR); sanitized_txs.len()];
            return LoadAndExecuteSanitizedTransactionsOutput {
                error_metrics,
                execute_timings,
                execution_results,
                loaded_transactions,
            };
        }
        program_cache_time.stop();

        let mut load_time = Measure::start("accounts_load");
        let mut loaded_transactions = load_accounts(
            callbacks,
            sanitized_txs,
            validation_results,
            &mut error_metrics,
            config.account_overrides,
            &environment.feature_set,
            environment
                .rent_collector
                .unwrap_or(&RentCollector::default()),
            &program_cache_for_tx_batch.borrow(),
        );
        load_time.stop();

        let mut execution_time = Measure::start("execution_time");

        let execution_results: Vec<TransactionExecutionResult> = loaded_transactions
            .iter_mut()
            .zip(sanitized_txs.iter())
            .map(|(load_result, tx)| match load_result {
                Err(e) => TransactionExecutionResult::NotExecuted(e.clone()),
                Ok(loaded_transaction) => {
                    let result = self.execute_loaded_transaction(
                        tx,
                        loaded_transaction,
                        &mut execute_timings,
                        &mut error_metrics,
                        &mut program_cache_for_tx_batch.borrow_mut(),
                        environment,
                        config,
                    );

                    if let TransactionExecutionResult::Executed {
                        details,
                        programs_modified_by_tx,
                    } = &result
                    {
                        // Update batch specific cache of the loaded programs with the modifications
                        // made by the transaction, if it executed successfully.
                        if details.status.is_ok() {
                            program_cache_for_tx_batch
                                .borrow_mut()
                                .merge(programs_modified_by_tx);
                        }
                    }

                    result
                }
            })
            .collect();

        execution_time.stop();

        // Skip eviction when there's no chance this particular tx batch has increased the size of
        // ProgramCache entries. Note that loaded_missing is deliberately defined, so that there's
        // still at least one other batch, which will evict the program cache, even after the
        // occurrences of cooperative loading.
        if program_cache_for_tx_batch.borrow().loaded_missing
            || program_cache_for_tx_batch.borrow().merged_modified
        {
            const SHRINK_LOADED_PROGRAMS_TO_PERCENTAGE: u8 = 90;
            self.program_cache
                .write()
                .unwrap()
                .evict_using_2s_random_selection(
                    Percentage::from(SHRINK_LOADED_PROGRAMS_TO_PERCENTAGE),
                    self.slot,
                );
        }

        debug!(
            "load: {}us execute: {}us txs_len={}",
            load_time.as_us(),
            execution_time.as_us(),
            sanitized_txs.len(),
        );

        execute_timings.saturating_add_in_place(
            ExecuteTimingType::ValidateFeesUs,
            validate_fees_time.as_us(),
        );
        execute_timings.saturating_add_in_place(
            ExecuteTimingType::ProgramCacheUs,
            program_cache_time.as_us(),
        );
        execute_timings.saturating_add_in_place(ExecuteTimingType::LoadUs, load_time.as_us());
        execute_timings
            .saturating_add_in_place(ExecuteTimingType::ExecuteUs, execution_time.as_us());

        LoadAndExecuteSanitizedTransactionsOutput {
            error_metrics,
            execute_timings,
            execution_results,
            loaded_transactions,
        }
    }

    fn validate_fees<CB: TransactionProcessingCallback>(
        &self,
        callbacks: &CB,
        sanitized_txs: &[impl core::borrow::Borrow<SanitizedTransaction>],
        check_results: Vec<TransactionCheckResult>,
        feature_set: &FeatureSet,
        fee_structure: &FeeStructure,
        rent_collector: &RentCollector,
        error_counters: &mut TransactionErrorMetrics,
    ) -> Vec<TransactionValidationResult> {
        sanitized_txs
            .iter()
            .zip(check_results)
            .map(|(sanitized_tx, check_result)| {
                check_result.and_then(|checked_details| {
                    let message = sanitized_tx.borrow().message();
                    self.validate_transaction_fee_payer(
                        callbacks,
                        message,
                        checked_details,
                        feature_set,
                        fee_structure,
                        rent_collector,
                        error_counters,
                    )
                })
            })
            .collect()
    }

    // Loads transaction fee payer, collects rent if necessary, then calculates
    // transaction fees, and deducts them from the fee payer balance. If the
    // account is not found or has insufficient funds, an error is returned.
    fn validate_transaction_fee_payer<CB: TransactionProcessingCallback>(
        &self,
        callbacks: &CB,
        message: &SanitizedMessage,
        checked_details: CheckedTransactionDetails,
        feature_set: &FeatureSet,
        fee_structure: &FeeStructure,
        rent_collector: &RentCollector,
        error_counters: &mut TransactionErrorMetrics,
    ) -> transaction::Result<ValidatedTransactionDetails> {
        let compute_budget_limits = process_compute_budget_instructions(
            message.program_instructions_iter(),
        )
        .map_err(|err| {
            error_counters.invalid_compute_budget += 1;
            err
        })?;

        let fee_payer_address = message.fee_payer();
        let Some(mut fee_payer_account) = callbacks.get_account_shared_data(fee_payer_address)
        else {
            error_counters.account_not_found += 1;
            return Err(TransactionError::AccountNotFound);
        };

        let fee_payer_loaded_rent_epoch = fee_payer_account.rent_epoch();
        let fee_payer_rent_debit = collect_rent_from_account(
            feature_set,
            rent_collector,
            fee_payer_address,
            &mut fee_payer_account,
        )
        .rent_amount;

        let CheckedTransactionDetails {
            nonce,
            lamports_per_signature,
        } = checked_details;

        let fee_budget_limits = FeeBudgetLimits::from(compute_budget_limits);
        let fee_details = fee_structure.calculate_fee_details(
            message,
            lamports_per_signature,
            &fee_budget_limits,
            feature_set.is_active(&include_loaded_accounts_data_size_in_fee_calculation::id()),
            feature_set.is_active(&remove_rounding_in_fee_calculation::id()),
        );

        let fee_payer_index = 0;
        validate_fee_payer(
            fee_payer_address,
            &mut fee_payer_account,
            fee_payer_index,
            error_counters,
            rent_collector,
            fee_details.total_fee(),
        )?;

        // Capture fee-subtracted fee payer account and original nonce account state
        // to rollback to if transaction execution fails.
        let rollback_accounts = RollbackAccounts::new(
            nonce,
            *fee_payer_address,
            fee_payer_account.clone(),
            fee_payer_rent_debit,
            fee_payer_loaded_rent_epoch,
        );

        Ok(ValidatedTransactionDetails {
            fee_details,
            fee_payer_account,
            fee_payer_rent_debit,
            rollback_accounts,
            compute_budget_limits,
        })
    }

    /// Returns a map from executable program accounts (all accounts owned by any loader)
    /// to their usage counters, for the transactions with a valid blockhash or nonce.
    fn filter_executable_program_accounts<CB: TransactionProcessingCallback>(
        callbacks: &CB,
        txs: &[SanitizedTransaction],
        validation_results: &[TransactionValidationResult],
        program_owners: &[Pubkey],
    ) -> HashMap<Pubkey, u64> {
        let mut result: HashMap<Pubkey, u64> = HashMap::new();
        validation_results.iter().zip(txs).for_each(|etx| {
            if let (Ok(_), tx) = etx {
                tx.message()
                    .account_keys()
                    .iter()
                    .for_each(|key| match result.entry(*key) {
                        Entry::Occupied(mut entry) => {
                            let count = entry.get_mut();
                            saturating_add_assign!(*count, 1);
                        }
                        Entry::Vacant(entry) => {
                            if callbacks
                                .account_matches_owners(key, program_owners)
                                .is_some()
                            {
                                entry.insert(1);
                            }
                        }
                    });
            }
        });
        result
    }

    fn replenish_program_cache<CB: TransactionProcessingCallback>(
        &self,
        callback: &CB,
        program_accounts_map: &HashMap<Pubkey, u64>,
        check_program_modification_slot: bool,
        limit_to_load_programs: bool,
    ) -> ProgramCacheForTxBatch {
        let mut missing_programs: Vec<(Pubkey, (ProgramCacheMatchCriteria, u64))> =
            program_accounts_map
                .iter()
                .map(|(pubkey, count)| {
                    let match_criteria = if check_program_modification_slot {
                        get_program_modification_slot(callback, pubkey)
                            .map_or(ProgramCacheMatchCriteria::Tombstone, |slot| {
                                ProgramCacheMatchCriteria::DeployedOnOrAfterSlot(slot)
                            })
                    } else {
                        ProgramCacheMatchCriteria::NoCriteria
                    };
                    (*pubkey, (match_criteria, *count))
                })
                .collect();

        let mut loaded_programs_for_txs = None;
        loop {
            let (program_to_store, task_cookie, task_waiter) = {
                // Lock the global cache.
                let program_cache = self.program_cache.read().unwrap();
                // Initialize our local cache.
                let is_first_round = loaded_programs_for_txs.is_none();
                if is_first_round {
                    loaded_programs_for_txs = Some(ProgramCacheForTxBatch::new_from_cache(
                        self.slot,
                        self.epoch,
                        &program_cache,
                    ));
                }
                // Figure out which program needs to be loaded next.
                let program_to_load = program_cache.extract(
                    &mut missing_programs,
                    loaded_programs_for_txs.as_mut().unwrap(),
                    is_first_round,
                );

                let program_to_store = program_to_load.map(|(key, count)| {
                    // Load, verify and compile one program.
                    let program = load_program_with_pubkey(
                        callback,
                        &program_cache.get_environments_for_epoch(self.epoch),
                        &key,
                        self.slot,
                        false,
                    )
                    .expect("called load_program_with_pubkey() with nonexistent account");
                    program.tx_usage_counter.store(count, Ordering::Relaxed);
                    (key, program)
                });

                let task_waiter = Arc::clone(&program_cache.loading_task_waiter);
                (program_to_store, task_waiter.cookie(), task_waiter)
                // Unlock the global cache again.
            };

            if let Some((key, program)) = program_to_store {
                let mut program_cache = self.program_cache.write().unwrap();
                // Submit our last completed loading task.
                if program_cache.finish_cooperative_loading_task(self.slot, key, program)
                    && limit_to_load_programs
                {
                    // This branch is taken when there is an error in assigning a program to a
                    // cache slot. It is not possible to mock this error for SVM unit
                    // tests purposes.
                    let mut ret = ProgramCacheForTxBatch::new_from_cache(
                        self.slot,
                        self.epoch,
                        &program_cache,
                    );
                    ret.hit_max_limit = true;
                    return ret;
                }
            } else if missing_programs.is_empty() {
                break;
            } else {
                // Sleep until the next finish_cooperative_loading_task() call.
                // Once a task completes we'll wake up and try to load the
                // missing programs inside the tx batch again.
                let _new_cookie = task_waiter.wait(task_cookie);
            }
        }

        loaded_programs_for_txs.unwrap()
    }

    pub fn prepare_program_cache_for_upcoming_feature_set<CB: TransactionProcessingCallback>(
        &self,
        callbacks: &CB,
        upcoming_feature_set: &FeatureSet,
        compute_budget: &ComputeBudget,
        slot_index: u64,
        slots_in_epoch: u64,
    ) {
        // Recompile loaded programs one at a time before the next epoch hits
        let slots_in_recompilation_phase =
            (solana_program_runtime::loaded_programs::MAX_LOADED_ENTRY_COUNT as u64)
                .min(slots_in_epoch)
                .checked_div(2)
                .unwrap();
        let mut program_cache = self.program_cache.write().unwrap();
        if program_cache.upcoming_environments.is_some() {
            if let Some((key, program_to_recompile)) = program_cache.programs_to_recompile.pop() {
                let effective_epoch = program_cache.latest_root_epoch.saturating_add(1);
                drop(program_cache);
                let environments_for_epoch = self
                    .program_cache
                    .read()
                    .unwrap()
                    .get_environments_for_epoch(effective_epoch);
                if let Some(recompiled) = load_program_with_pubkey(
                    callbacks,
                    &environments_for_epoch,
                    &key,
                    self.slot,
                    false,
                ) {
                    recompiled.tx_usage_counter.fetch_add(
                        program_to_recompile
                            .tx_usage_counter
                            .load(Ordering::Relaxed),
                        Ordering::Relaxed,
                    );
                    recompiled.ix_usage_counter.fetch_add(
                        program_to_recompile
                            .ix_usage_counter
                            .load(Ordering::Relaxed),
                        Ordering::Relaxed,
                    );
                    let mut program_cache = self.program_cache.write().unwrap();
                    program_cache.assign_program(key, recompiled);
                }
            }
        } else if self.epoch != program_cache.latest_root_epoch
            || slot_index.saturating_add(slots_in_recompilation_phase) >= slots_in_epoch
        {
            // Anticipate the upcoming program runtime environment for the next epoch,
            // so we can try to recompile loaded programs before the feature transition hits.
            drop(program_cache);
            let mut program_cache = self.program_cache.write().unwrap();
            let program_runtime_environment_v1 = create_program_runtime_environment_v1(
                upcoming_feature_set,
                compute_budget,
                false, /* deployment */
                false, /* debugging_features */
            )
            .unwrap();
            let program_runtime_environment_v2 = create_program_runtime_environment_v2(
                compute_budget,
                false, /* debugging_features */
            );
            let mut upcoming_environments = program_cache.environments.clone();
            let changed_program_runtime_v1 =
                *upcoming_environments.program_runtime_v1 != program_runtime_environment_v1;
            let changed_program_runtime_v2 =
                *upcoming_environments.program_runtime_v2 != program_runtime_environment_v2;
            if changed_program_runtime_v1 {
                upcoming_environments.program_runtime_v1 = Arc::new(program_runtime_environment_v1);
            }
            if changed_program_runtime_v2 {
                upcoming_environments.program_runtime_v2 = Arc::new(program_runtime_environment_v2);
            }
            program_cache.upcoming_environments = Some(upcoming_environments);
            program_cache.programs_to_recompile = program_cache
                .get_flattened_entries(changed_program_runtime_v1, changed_program_runtime_v2);
            program_cache
                .programs_to_recompile
                .sort_by_cached_key(|(_id, program)| program.decayed_usage_counter(self.slot));
        }
    }

    /// Execute a transaction using the provided loaded accounts and update
    /// the executors cache if the transaction was successful.
    #[allow(clippy::too_many_arguments)]
    fn execute_loaded_transaction(
        &self,
        tx: &SanitizedTransaction,
        loaded_transaction: &mut LoadedTransaction,
        execute_timings: &mut ExecuteTimings,
        error_metrics: &mut TransactionErrorMetrics,
        program_cache_for_tx_batch: &mut ProgramCacheForTxBatch,
        environment: &TransactionProcessingEnvironment,
        config: &TransactionProcessingConfig,
    ) -> TransactionExecutionResult {
        let transaction_accounts = std::mem::take(&mut loaded_transaction.accounts);

        fn transaction_accounts_lamports_sum(
            accounts: &[(Pubkey, AccountSharedData)],
            message: &SanitizedMessage,
        ) -> Option<u128> {
            let mut lamports_sum = 0u128;
            for i in 0..message.account_keys().len() {
                let (_, account) = accounts.get(i)?;
                lamports_sum = lamports_sum.checked_add(u128::from(account.lamports()))?;
            }
            Some(lamports_sum)
        }

        let rent = environment
            .rent_collector
            .map(|rent_collector| rent_collector.rent.clone())
            .unwrap_or_default();

        let lamports_before_tx =
            transaction_accounts_lamports_sum(&transaction_accounts, tx.message()).unwrap_or(0);

        let compute_budget = config
            .compute_budget
            .unwrap_or_else(|| ComputeBudget::from(loaded_transaction.compute_budget_limits));

        let mut transaction_context = TransactionContext::new(
            transaction_accounts,
            rent.clone(),
            compute_budget.max_instruction_stack_depth,
            compute_budget.max_instruction_trace_length,
        );
        #[cfg(debug_assertions)]
        transaction_context.set_signature(tx.signature());

        let pre_account_state_info =
            TransactionAccountStateInfo::new(&rent, &transaction_context, tx.message());

        let log_collector = if config.recording_config.enable_log_recording {
            match config.log_messages_bytes_limit {
                None => Some(LogCollector::new_ref()),
                Some(log_messages_bytes_limit) => Some(LogCollector::new_ref_with_limit(Some(
                    log_messages_bytes_limit,
                ))),
            }
        } else {
            None
        };

        let blockhash = environment.blockhash;
        let lamports_per_signature = environment.lamports_per_signature;

        let mut executed_units = 0u64;
        let sysvar_cache = &self.sysvar_cache.read().unwrap();

        let mut invoke_context = InvokeContext::new(
            &mut transaction_context,
            program_cache_for_tx_batch,
            EnvironmentConfig::new(
                blockhash,
                environment.epoch_total_stake,
                environment.epoch_vote_accounts,
                Arc::clone(&environment.feature_set),
                lamports_per_signature,
                sysvar_cache,
            ),
            log_collector.clone(),
            compute_budget,
        );

        let mut process_message_time = Measure::start("process_message_time");
        let process_result = MessageProcessor::process_message(
            tx.message(),
            &loaded_transaction.program_indices,
            &mut invoke_context,
            execute_timings,
            &mut executed_units,
        );
        process_message_time.stop();

        drop(invoke_context);

        saturating_add_assign!(
            execute_timings.execute_accessories.process_message_us,
            process_message_time.as_us()
        );

        let mut status = process_result
            .and_then(|info| {
                let post_account_state_info =
                    TransactionAccountStateInfo::new(&rent, &transaction_context, tx.message());
                TransactionAccountStateInfo::verify_changes(
                    &pre_account_state_info,
                    &post_account_state_info,
                    &transaction_context,
                )
                .map(|_| info)
            })
            .map_err(|err| {
                match err {
                    TransactionError::InvalidRentPayingAccount
                    | TransactionError::InsufficientFundsForRent { .. } => {
                        error_metrics.invalid_rent_paying_account += 1;
                    }
                    TransactionError::InvalidAccountIndex => {
                        error_metrics.invalid_account_index += 1;
                    }
                    _ => {
                        error_metrics.instruction_error += 1;
                    }
                }
                err
            });

        let log_messages: Option<TransactionLogMessages> =
            log_collector.and_then(|log_collector| {
                Rc::try_unwrap(log_collector)
                    .map(|log_collector| log_collector.into_inner().into_messages())
                    .ok()
            });

        let inner_instructions = if config.recording_config.enable_cpi_recording {
            Some(Self::inner_instructions_list_from_instruction_trace(
                &transaction_context,
            ))
        } else {
            None
        };

        let ExecutionRecord {
            accounts,
            return_data,
            touched_account_count,
            accounts_resize_delta: accounts_data_len_delta,
        } = transaction_context.into();

        if status.is_ok()
            && transaction_accounts_lamports_sum(&accounts, tx.message())
                .filter(|lamports_after_tx| lamports_before_tx == *lamports_after_tx)
                .is_none()
        {
            status = Err(TransactionError::UnbalancedTransaction);
        }
        let status = status.map(|_| ());

        loaded_transaction.accounts = accounts;
        saturating_add_assign!(
            execute_timings.execute_accounts_details.total_account_count,
            loaded_transaction.accounts.len() as u64
        );
        saturating_add_assign!(
            execute_timings
                .execute_accounts_details
                .changed_account_count,
            touched_account_count
        );

        let return_data = if config.recording_config.enable_return_data_recording
            && !return_data.data.is_empty()
        {
            Some(return_data)
        } else {
            None
        };

        TransactionExecutionResult::Executed {
            details: TransactionExecutionDetails {
                status,
                log_messages,
                inner_instructions,
                fee_details: loaded_transaction.fee_details,
                return_data,
                executed_units,
                accounts_data_len_delta,
            },
            programs_modified_by_tx: program_cache_for_tx_batch.drain_modified_entries(),
        }
    }

    /// Extract the InnerInstructionsList from a TransactionContext
    fn inner_instructions_list_from_instruction_trace(
        transaction_context: &TransactionContext,
    ) -> InnerInstructionsList {
        debug_assert!(transaction_context
            .get_instruction_context_at_index_in_trace(0)
            .map(|instruction_context| instruction_context.get_stack_height()
                == TRANSACTION_LEVEL_STACK_HEIGHT)
            .unwrap_or(true));
        let mut outer_instructions = Vec::new();
        for index_in_trace in 0..transaction_context.get_instruction_trace_length() {
            if let Ok(instruction_context) =
                transaction_context.get_instruction_context_at_index_in_trace(index_in_trace)
            {
                let stack_height = instruction_context.get_stack_height();
                if stack_height == TRANSACTION_LEVEL_STACK_HEIGHT {
                    outer_instructions.push(Vec::new());
                } else if let Some(inner_instructions) = outer_instructions.last_mut() {
                    let stack_height = u8::try_from(stack_height).unwrap_or(u8::MAX);
                    let instruction = CompiledInstruction::new_from_raw_parts(
                        instruction_context
                            .get_index_of_program_account_in_transaction(
                                instruction_context
                                    .get_number_of_program_accounts()
                                    .saturating_sub(1),
                            )
                            .unwrap_or_default() as u8,
                        instruction_context.get_instruction_data().to_vec(),
                        (0..instruction_context.get_number_of_instruction_accounts())
                            .map(|instruction_account_index| {
                                instruction_context
                                    .get_index_of_instruction_account_in_transaction(
                                        instruction_account_index,
                                    )
                                    .unwrap_or_default() as u8
                            })
                            .collect(),
                    );
                    inner_instructions.push(InnerInstruction {
                        instruction,
                        stack_height,
                    });
                } else {
                    debug_assert!(false);
                }
            } else {
                debug_assert!(false);
            }
        }
        outer_instructions
    }

    pub fn fill_missing_sysvar_cache_entries<CB: TransactionProcessingCallback>(
        &self,
        callbacks: &CB,
    ) {
        let mut sysvar_cache = self.sysvar_cache.write().unwrap();
        sysvar_cache.fill_missing_entries(|pubkey, set_sysvar| {
            if let Some(account) = callbacks.get_account_shared_data(pubkey) {
                set_sysvar(account.data());
            }
        });
    }

    pub fn reset_sysvar_cache(&self) {
        let mut sysvar_cache = self.sysvar_cache.write().unwrap();
        sysvar_cache.reset();
    }

    pub fn get_sysvar_cache_for_tests(&self) -> SysvarCache {
        self.sysvar_cache.read().unwrap().clone()
    }

    /// Add a built-in program
    pub fn add_builtin<CB: TransactionProcessingCallback>(
        &self,
        callbacks: &CB,
        program_id: Pubkey,
        name: &str,
        builtin: ProgramCacheEntry,
    ) {
        debug!("Adding program {} under {:?}", name, program_id);
        callbacks.add_builtin_account(name, &program_id);
        self.builtin_program_ids.write().unwrap().insert(program_id);
        self.program_cache
            .write()
            .unwrap()
            .assign_program(program_id, Arc::new(builtin));
        debug!("Added program {} under {:?}", name, program_id);
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            account_loader::ValidatedTransactionDetails, nonce_info::NoncePartial,
            rollback_accounts::RollbackAccounts,
        },
        solana_compute_budget::compute_budget_processor::ComputeBudgetLimits,
        solana_program_runtime::loaded_programs::{BlockRelation, ProgramCacheEntryType},
        solana_sdk::{
            account::{create_account_shared_data_for_test, WritableAccount},
            bpf_loader,
            bpf_loader_upgradeable::{self, UpgradeableLoaderState},
            compute_budget::ComputeBudgetInstruction,
            epoch_schedule::EpochSchedule,
            feature_set::FeatureSet,
            fee::FeeDetails,
            fee_calculator::FeeCalculator,
            hash::Hash,
            message::{LegacyMessage, Message, MessageHeader},
            nonce,
            rent_collector::{RentCollector, RENT_EXEMPT_RENT_EPOCH},
            rent_debits::RentDebits,
            reserved_account_keys::ReservedAccountKeys,
            signature::{Keypair, Signature},
            system_program,
            sysvar::{self, rent::Rent},
            transaction::{SanitizedTransaction, Transaction, TransactionError},
            transaction_context::TransactionContext,
        },
        std::{
            env,
            fs::{self, File},
            io::Read,
            thread,
        },
    };

    fn new_unchecked_sanitized_message(message: Message) -> SanitizedMessage {
        SanitizedMessage::Legacy(LegacyMessage::new(
            message,
            &ReservedAccountKeys::empty_key_set(),
        ))
    }

    struct TestForkGraph {}

    impl ForkGraph for TestForkGraph {
        fn relationship(&self, _a: Slot, _b: Slot) -> BlockRelation {
            BlockRelation::Unknown
        }
    }

    #[derive(Default, Clone)]
    pub struct MockBankCallback {
        pub account_shared_data: Arc<RwLock<HashMap<Pubkey, AccountSharedData>>>,
    }

    impl TransactionProcessingCallback for MockBankCallback {
        fn account_matches_owners(&self, account: &Pubkey, owners: &[Pubkey]) -> Option<usize> {
            if let Some(data) = self.account_shared_data.read().unwrap().get(account) {
                if data.lamports() == 0 {
                    None
                } else {
                    owners.iter().position(|entry| data.owner() == entry)
                }
            } else {
                None
            }
        }

        fn get_account_shared_data(&self, pubkey: &Pubkey) -> Option<AccountSharedData> {
            self.account_shared_data
                .read()
                .unwrap()
                .get(pubkey)
                .cloned()
        }

        fn add_builtin_account(&self, name: &str, program_id: &Pubkey) {
            let mut account_data = AccountSharedData::default();
            account_data.set_data(name.as_bytes().to_vec());
            self.account_shared_data
                .write()
                .unwrap()
                .insert(*program_id, account_data);
        }
    }

    #[test]
    fn test_inner_instructions_list_from_instruction_trace() {
        let instruction_trace = [1, 2, 1, 1, 2, 3, 2];
        let mut transaction_context =
            TransactionContext::new(vec![], Rent::default(), 3, instruction_trace.len());
        for (index_in_trace, stack_height) in instruction_trace.into_iter().enumerate() {
            while stack_height <= transaction_context.get_instruction_context_stack_height() {
                transaction_context.pop().unwrap();
            }
            if stack_height > transaction_context.get_instruction_context_stack_height() {
                transaction_context
                    .get_next_instruction_context()
                    .unwrap()
                    .configure(&[], &[], &[index_in_trace as u8]);
                transaction_context.push().unwrap();
            }
        }
        let inner_instructions =
            TransactionBatchProcessor::<TestForkGraph>::inner_instructions_list_from_instruction_trace(
                &transaction_context,
            );

        assert_eq!(
            inner_instructions,
            vec![
                vec![InnerInstruction {
                    instruction: CompiledInstruction::new_from_raw_parts(0, vec![1], vec![]),
                    stack_height: 2,
                }],
                vec![],
                vec![
                    InnerInstruction {
                        instruction: CompiledInstruction::new_from_raw_parts(0, vec![4], vec![]),
                        stack_height: 2,
                    },
                    InnerInstruction {
                        instruction: CompiledInstruction::new_from_raw_parts(0, vec![5], vec![]),
                        stack_height: 3,
                    },
                    InnerInstruction {
                        instruction: CompiledInstruction::new_from_raw_parts(0, vec![6], vec![]),
                        stack_height: 2,
                    },
                ]
            ]
        );
    }

    #[test]
    fn test_execute_loaded_transaction_recordings() {
        // Setting all the arguments correctly is too burdensome for testing
        // execute_loaded_transaction separately.This function will be tested in an integration
        // test with load_and_execute_sanitized_transactions
        let message = Message {
            account_keys: vec![Pubkey::new_from_array([0; 32])],
            header: MessageHeader::default(),
            instructions: vec![CompiledInstruction {
                program_id_index: 0,
                accounts: vec![],
                data: vec![],
            }],
            recent_blockhash: Hash::default(),
        };

        let sanitized_message = new_unchecked_sanitized_message(message);
        let mut program_cache_for_tx_batch = ProgramCacheForTxBatch::default();
        let batch_processor = TransactionBatchProcessor::<TestForkGraph>::default();

        let sanitized_transaction = SanitizedTransaction::new_for_tests(
            sanitized_message,
            vec![Signature::new_unique()],
            false,
        );

        let mut loaded_transaction = LoadedTransaction {
            accounts: vec![(Pubkey::new_unique(), AccountSharedData::default())],
            program_indices: vec![vec![0]],
            fee_details: FeeDetails::default(),
            rollback_accounts: RollbackAccounts::default(),
            compute_budget_limits: ComputeBudgetLimits::default(),
            rent: 0,
            rent_debits: RentDebits::default(),
            loaded_accounts_data_size: 32,
        };

        let processing_environment = TransactionProcessingEnvironment::default();

        let mut processing_config = TransactionProcessingConfig::default();
        processing_config.recording_config.enable_log_recording = true;

        let result = batch_processor.execute_loaded_transaction(
            &sanitized_transaction,
            &mut loaded_transaction,
            &mut ExecuteTimings::default(),
            &mut TransactionErrorMetrics::default(),
            &mut program_cache_for_tx_batch,
            &processing_environment,
            &processing_config,
        );

        let TransactionExecutionResult::Executed {
            details: TransactionExecutionDetails { log_messages, .. },
            ..
        } = result
        else {
            panic!("Unexpected result")
        };
        assert!(log_messages.is_some());

        processing_config.log_messages_bytes_limit = Some(2);

        let result = batch_processor.execute_loaded_transaction(
            &sanitized_transaction,
            &mut loaded_transaction,
            &mut ExecuteTimings::default(),
            &mut TransactionErrorMetrics::default(),
            &mut program_cache_for_tx_batch,
            &processing_environment,
            &processing_config,
        );

        let TransactionExecutionResult::Executed {
            details:
                TransactionExecutionDetails {
                    log_messages,
                    inner_instructions,
                    ..
                },
            ..
        } = result
        else {
            panic!("Unexpected result")
        };
        assert!(log_messages.is_some());
        assert!(inner_instructions.is_none());

        processing_config.recording_config.enable_log_recording = false;
        processing_config.recording_config.enable_cpi_recording = true;
        processing_config.log_messages_bytes_limit = None;

        let result = batch_processor.execute_loaded_transaction(
            &sanitized_transaction,
            &mut loaded_transaction,
            &mut ExecuteTimings::default(),
            &mut TransactionErrorMetrics::default(),
            &mut program_cache_for_tx_batch,
            &processing_environment,
            &processing_config,
        );

        let TransactionExecutionResult::Executed {
            details:
                TransactionExecutionDetails {
                    log_messages,
                    inner_instructions,
                    ..
                },
            ..
        } = result
        else {
            panic!("Unexpected result")
        };
        assert!(log_messages.is_none());
        assert!(inner_instructions.is_some());
    }

    #[test]
    fn test_execute_loaded_transaction_error_metrics() {
        // Setting all the arguments correctly is too burdensome for testing
        // execute_loaded_transaction separately.This function will be tested in an integration
        // test with load_and_execute_sanitized_transactions
        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        let message = Message {
            account_keys: vec![key1, key2],
            header: MessageHeader::default(),
            instructions: vec![CompiledInstruction {
                program_id_index: 0,
                accounts: vec![2],
                data: vec![],
            }],
            recent_blockhash: Hash::default(),
        };

        let sanitized_message = new_unchecked_sanitized_message(message);
        let mut program_cache_for_tx_batch = ProgramCacheForTxBatch::default();
        let batch_processor = TransactionBatchProcessor::<TestForkGraph>::default();

        let sanitized_transaction = SanitizedTransaction::new_for_tests(
            sanitized_message,
            vec![Signature::new_unique()],
            false,
        );

        let mut account_data = AccountSharedData::default();
        account_data.set_owner(bpf_loader::id());
        let mut loaded_transaction = LoadedTransaction {
            accounts: vec![
                (key1, AccountSharedData::default()),
                (key2, AccountSharedData::default()),
            ],
            program_indices: vec![vec![0]],
            fee_details: FeeDetails::default(),
            rollback_accounts: RollbackAccounts::default(),
            compute_budget_limits: ComputeBudgetLimits::default(),
            rent: 0,
            rent_debits: RentDebits::default(),
            loaded_accounts_data_size: 0,
        };

        let processing_config = TransactionProcessingConfig {
            recording_config: ExecutionRecordingConfig::new_single_setting(false),
            ..Default::default()
        };
        let mut error_metrics = TransactionErrorMetrics::new();

        let _ = batch_processor.execute_loaded_transaction(
            &sanitized_transaction,
            &mut loaded_transaction,
            &mut ExecuteTimings::default(),
            &mut error_metrics,
            &mut program_cache_for_tx_batch,
            &TransactionProcessingEnvironment::default(),
            &processing_config,
        );

        assert_eq!(error_metrics.instruction_error, 1);
    }

    #[test]
    #[should_panic = "called load_program_with_pubkey() with nonexistent account"]
    fn test_replenish_program_cache_with_nonexistent_accounts() {
        let mock_bank = MockBankCallback::default();
        let batch_processor = TransactionBatchProcessor::<TestForkGraph>::default();
        batch_processor.program_cache.write().unwrap().fork_graph =
            Some(Arc::new(RwLock::new(TestForkGraph {})));
        let key = Pubkey::new_unique();

        let mut account_maps: HashMap<Pubkey, u64> = HashMap::new();
        account_maps.insert(key, 4);

        batch_processor.replenish_program_cache(&mock_bank, &account_maps, false, true);
    }

    #[test]
    fn test_replenish_program_cache() {
        let mock_bank = MockBankCallback::default();
        let batch_processor = TransactionBatchProcessor::<TestForkGraph>::default();
        batch_processor.program_cache.write().unwrap().fork_graph =
            Some(Arc::new(RwLock::new(TestForkGraph {})));
        let key = Pubkey::new_unique();

        let mut account_data = AccountSharedData::default();
        account_data.set_owner(bpf_loader::id());
        mock_bank
            .account_shared_data
            .write()
            .unwrap()
            .insert(key, account_data);

        let mut account_maps: HashMap<Pubkey, u64> = HashMap::new();
        account_maps.insert(key, 4);

        for limit_to_load_programs in [false, true] {
            let result = batch_processor.replenish_program_cache(
                &mock_bank,
                &account_maps,
                false,
                limit_to_load_programs,
            );
            assert!(!result.hit_max_limit);
            let program = result.find(&key).unwrap();
            assert!(matches!(
                program.program,
                ProgramCacheEntryType::FailedVerification(_)
            ));
        }
    }

    #[test]
    fn test_filter_executable_program_accounts() {
        let mock_bank = MockBankCallback::default();
        let key1 = Pubkey::new_unique();
        let owner1 = Pubkey::new_unique();

        let mut data = AccountSharedData::default();
        data.set_owner(owner1);
        data.set_lamports(93);
        mock_bank
            .account_shared_data
            .write()
            .unwrap()
            .insert(key1, data);

        let message = Message {
            account_keys: vec![key1],
            header: MessageHeader::default(),
            instructions: vec![CompiledInstruction {
                program_id_index: 0,
                accounts: vec![],
                data: vec![],
            }],
            recent_blockhash: Hash::default(),
        };

        let sanitized_message = new_unchecked_sanitized_message(message);

        let sanitized_transaction_1 = SanitizedTransaction::new_for_tests(
            sanitized_message,
            vec![Signature::new_unique()],
            false,
        );

        let key2 = Pubkey::new_unique();
        let owner2 = Pubkey::new_unique();

        let mut account_data = AccountSharedData::default();
        account_data.set_owner(owner2);
        account_data.set_lamports(90);
        mock_bank
            .account_shared_data
            .write()
            .unwrap()
            .insert(key2, account_data);

        let message = Message {
            account_keys: vec![key1, key2],
            header: MessageHeader::default(),
            instructions: vec![CompiledInstruction {
                program_id_index: 0,
                accounts: vec![],
                data: vec![],
            }],
            recent_blockhash: Hash::default(),
        };

        let sanitized_message = new_unchecked_sanitized_message(message);

        let sanitized_transaction_2 = SanitizedTransaction::new_for_tests(
            sanitized_message,
            vec![Signature::new_unique()],
            false,
        );

        let transactions = vec![
            sanitized_transaction_1.clone(),
            sanitized_transaction_2.clone(),
            sanitized_transaction_1,
        ];
        let validation_results = vec![
            Ok(ValidatedTransactionDetails::default()),
            Ok(ValidatedTransactionDetails::default()),
            Err(TransactionError::ProgramAccountNotFound),
        ];
        let owners = vec![owner1, owner2];

        let result = TransactionBatchProcessor::<TestForkGraph>::filter_executable_program_accounts(
            &mock_bank,
            &transactions,
            &validation_results,
            &owners,
        );

        assert_eq!(result.len(), 2);
        assert_eq!(result[&key1], 2);
        assert_eq!(result[&key2], 1);
    }

    #[test]
    fn test_filter_executable_program_accounts_no_errors() {
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();

        let non_program_pubkey1 = Pubkey::new_unique();
        let non_program_pubkey2 = Pubkey::new_unique();
        let program1_pubkey = Pubkey::new_unique();
        let program2_pubkey = Pubkey::new_unique();
        let account1_pubkey = Pubkey::new_unique();
        let account2_pubkey = Pubkey::new_unique();
        let account3_pubkey = Pubkey::new_unique();
        let account4_pubkey = Pubkey::new_unique();

        let account5_pubkey = Pubkey::new_unique();

        let bank = MockBankCallback::default();
        bank.account_shared_data.write().unwrap().insert(
            non_program_pubkey1,
            AccountSharedData::new(1, 10, &account5_pubkey),
        );
        bank.account_shared_data.write().unwrap().insert(
            non_program_pubkey2,
            AccountSharedData::new(1, 10, &account5_pubkey),
        );
        bank.account_shared_data.write().unwrap().insert(
            program1_pubkey,
            AccountSharedData::new(40, 1, &account5_pubkey),
        );
        bank.account_shared_data.write().unwrap().insert(
            program2_pubkey,
            AccountSharedData::new(40, 1, &account5_pubkey),
        );
        bank.account_shared_data.write().unwrap().insert(
            account1_pubkey,
            AccountSharedData::new(1, 10, &non_program_pubkey1),
        );
        bank.account_shared_data.write().unwrap().insert(
            account2_pubkey,
            AccountSharedData::new(1, 10, &non_program_pubkey2),
        );
        bank.account_shared_data.write().unwrap().insert(
            account3_pubkey,
            AccountSharedData::new(40, 1, &program1_pubkey),
        );
        bank.account_shared_data.write().unwrap().insert(
            account4_pubkey,
            AccountSharedData::new(40, 1, &program2_pubkey),
        );

        let tx1 = Transaction::new_with_compiled_instructions(
            &[&keypair1],
            &[non_program_pubkey1],
            Hash::new_unique(),
            vec![account1_pubkey, account2_pubkey, account3_pubkey],
            vec![CompiledInstruction::new(1, &(), vec![0])],
        );
        let sanitized_tx1 = SanitizedTransaction::from_transaction_for_tests(tx1);

        let tx2 = Transaction::new_with_compiled_instructions(
            &[&keypair2],
            &[non_program_pubkey2],
            Hash::new_unique(),
            vec![account4_pubkey, account3_pubkey, account2_pubkey],
            vec![CompiledInstruction::new(1, &(), vec![0])],
        );
        let sanitized_tx2 = SanitizedTransaction::from_transaction_for_tests(tx2);

        let owners = &[program1_pubkey, program2_pubkey];
        let programs =
            TransactionBatchProcessor::<TestForkGraph>::filter_executable_program_accounts(
                &bank,
                &[sanitized_tx1, sanitized_tx2],
                &[
                    Ok(ValidatedTransactionDetails::default()),
                    Ok(ValidatedTransactionDetails::default()),
                ],
                owners,
            );

        // The result should contain only account3_pubkey, and account4_pubkey as the program accounts
        assert_eq!(programs.len(), 2);
        assert_eq!(
            programs
                .get(&account3_pubkey)
                .expect("failed to find the program account"),
            &2
        );
        assert_eq!(
            programs
                .get(&account4_pubkey)
                .expect("failed to find the program account"),
            &1
        );
    }

    #[test]
    fn test_filter_executable_program_accounts_invalid_blockhash() {
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();

        let non_program_pubkey1 = Pubkey::new_unique();
        let non_program_pubkey2 = Pubkey::new_unique();
        let program1_pubkey = Pubkey::new_unique();
        let program2_pubkey = Pubkey::new_unique();
        let account1_pubkey = Pubkey::new_unique();
        let account2_pubkey = Pubkey::new_unique();
        let account3_pubkey = Pubkey::new_unique();
        let account4_pubkey = Pubkey::new_unique();

        let account5_pubkey = Pubkey::new_unique();

        let bank = MockBankCallback::default();
        bank.account_shared_data.write().unwrap().insert(
            non_program_pubkey1,
            AccountSharedData::new(1, 10, &account5_pubkey),
        );
        bank.account_shared_data.write().unwrap().insert(
            non_program_pubkey2,
            AccountSharedData::new(1, 10, &account5_pubkey),
        );
        bank.account_shared_data.write().unwrap().insert(
            program1_pubkey,
            AccountSharedData::new(40, 1, &account5_pubkey),
        );
        bank.account_shared_data.write().unwrap().insert(
            program2_pubkey,
            AccountSharedData::new(40, 1, &account5_pubkey),
        );
        bank.account_shared_data.write().unwrap().insert(
            account1_pubkey,
            AccountSharedData::new(1, 10, &non_program_pubkey1),
        );
        bank.account_shared_data.write().unwrap().insert(
            account2_pubkey,
            AccountSharedData::new(1, 10, &non_program_pubkey2),
        );
        bank.account_shared_data.write().unwrap().insert(
            account3_pubkey,
            AccountSharedData::new(40, 1, &program1_pubkey),
        );
        bank.account_shared_data.write().unwrap().insert(
            account4_pubkey,
            AccountSharedData::new(40, 1, &program2_pubkey),
        );

        let tx1 = Transaction::new_with_compiled_instructions(
            &[&keypair1],
            &[non_program_pubkey1],
            Hash::new_unique(),
            vec![account1_pubkey, account2_pubkey, account3_pubkey],
            vec![CompiledInstruction::new(1, &(), vec![0])],
        );
        let sanitized_tx1 = SanitizedTransaction::from_transaction_for_tests(tx1);

        let tx2 = Transaction::new_with_compiled_instructions(
            &[&keypair2],
            &[non_program_pubkey2],
            Hash::new_unique(),
            vec![account4_pubkey, account3_pubkey, account2_pubkey],
            vec![CompiledInstruction::new(1, &(), vec![0])],
        );
        // Let's not register blockhash from tx2. This should cause the tx2 to fail
        let sanitized_tx2 = SanitizedTransaction::from_transaction_for_tests(tx2);

        let owners = &[program1_pubkey, program2_pubkey];
        let validation_results = vec![
            Ok(ValidatedTransactionDetails::default()),
            Err(TransactionError::BlockhashNotFound),
        ];
        let programs =
            TransactionBatchProcessor::<TestForkGraph>::filter_executable_program_accounts(
                &bank,
                &[sanitized_tx1, sanitized_tx2],
                &validation_results,
                owners,
            );

        // The result should contain only account3_pubkey as the program accounts
        assert_eq!(programs.len(), 1);
        assert_eq!(
            programs
                .get(&account3_pubkey)
                .expect("failed to find the program account"),
            &1
        );
    }

    #[test]
    #[allow(deprecated)]
    fn test_sysvar_cache_initialization1() {
        let mock_bank = MockBankCallback::default();

        let clock = sysvar::clock::Clock {
            slot: 1,
            epoch_start_timestamp: 2,
            epoch: 3,
            leader_schedule_epoch: 4,
            unix_timestamp: 5,
        };
        let clock_account = create_account_shared_data_for_test(&clock);
        mock_bank
            .account_shared_data
            .write()
            .unwrap()
            .insert(sysvar::clock::id(), clock_account);

        let epoch_schedule = EpochSchedule::custom(64, 2, true);
        let epoch_schedule_account = create_account_shared_data_for_test(&epoch_schedule);
        mock_bank
            .account_shared_data
            .write()
            .unwrap()
            .insert(sysvar::epoch_schedule::id(), epoch_schedule_account);

        let fees = sysvar::fees::Fees {
            fee_calculator: FeeCalculator {
                lamports_per_signature: 123,
            },
        };
        let fees_account = create_account_shared_data_for_test(&fees);
        mock_bank
            .account_shared_data
            .write()
            .unwrap()
            .insert(sysvar::fees::id(), fees_account);

        let rent = Rent::with_slots_per_epoch(2048);
        let rent_account = create_account_shared_data_for_test(&rent);
        mock_bank
            .account_shared_data
            .write()
            .unwrap()
            .insert(sysvar::rent::id(), rent_account);

        let transaction_processor = TransactionBatchProcessor::<TestForkGraph>::default();
        transaction_processor.fill_missing_sysvar_cache_entries(&mock_bank);

        let sysvar_cache = transaction_processor.sysvar_cache.read().unwrap();
        let cached_clock = sysvar_cache.get_clock();
        let cached_epoch_schedule = sysvar_cache.get_epoch_schedule();
        let cached_fees = sysvar_cache.get_fees();
        let cached_rent = sysvar_cache.get_rent();

        assert_eq!(
            cached_clock.expect("clock sysvar missing in cache"),
            clock.into()
        );
        assert_eq!(
            cached_epoch_schedule.expect("epoch_schedule sysvar missing in cache"),
            epoch_schedule.into()
        );
        assert_eq!(
            cached_fees.expect("fees sysvar missing in cache"),
            fees.into()
        );
        assert_eq!(
            cached_rent.expect("rent sysvar missing in cache"),
            rent.into()
        );
        assert!(sysvar_cache.get_slot_hashes().is_err());
        assert!(sysvar_cache.get_epoch_rewards().is_err());
    }

    #[test]
    #[allow(deprecated)]
    fn test_reset_and_fill_sysvar_cache() {
        let mock_bank = MockBankCallback::default();

        let clock = sysvar::clock::Clock {
            slot: 1,
            epoch_start_timestamp: 2,
            epoch: 3,
            leader_schedule_epoch: 4,
            unix_timestamp: 5,
        };
        let clock_account = create_account_shared_data_for_test(&clock);
        mock_bank
            .account_shared_data
            .write()
            .unwrap()
            .insert(sysvar::clock::id(), clock_account);

        let epoch_schedule = EpochSchedule::custom(64, 2, true);
        let epoch_schedule_account = create_account_shared_data_for_test(&epoch_schedule);
        mock_bank
            .account_shared_data
            .write()
            .unwrap()
            .insert(sysvar::epoch_schedule::id(), epoch_schedule_account);

        let fees = sysvar::fees::Fees {
            fee_calculator: FeeCalculator {
                lamports_per_signature: 123,
            },
        };
        let fees_account = create_account_shared_data_for_test(&fees);
        mock_bank
            .account_shared_data
            .write()
            .unwrap()
            .insert(sysvar::fees::id(), fees_account);

        let rent = Rent::with_slots_per_epoch(2048);
        let rent_account = create_account_shared_data_for_test(&rent);
        mock_bank
            .account_shared_data
            .write()
            .unwrap()
            .insert(sysvar::rent::id(), rent_account);

        let transaction_processor = TransactionBatchProcessor::<TestForkGraph>::default();
        // Fill the sysvar cache
        transaction_processor.fill_missing_sysvar_cache_entries(&mock_bank);
        // Reset the sysvar cache
        transaction_processor.reset_sysvar_cache();

        {
            let sysvar_cache = transaction_processor.sysvar_cache.read().unwrap();
            // Test that sysvar cache is empty and none of the values are found
            assert!(sysvar_cache.get_clock().is_err());
            assert!(sysvar_cache.get_epoch_schedule().is_err());
            assert!(sysvar_cache.get_fees().is_err());
            assert!(sysvar_cache.get_epoch_rewards().is_err());
            assert!(sysvar_cache.get_rent().is_err());
            assert!(sysvar_cache.get_epoch_rewards().is_err());
        }

        // Refill the cache and test the values are available.
        transaction_processor.fill_missing_sysvar_cache_entries(&mock_bank);

        let sysvar_cache = transaction_processor.sysvar_cache.read().unwrap();
        let cached_clock = sysvar_cache.get_clock();
        let cached_epoch_schedule = sysvar_cache.get_epoch_schedule();
        let cached_fees = sysvar_cache.get_fees();
        let cached_rent = sysvar_cache.get_rent();

        assert_eq!(
            cached_clock.expect("clock sysvar missing in cache"),
            clock.into()
        );
        assert_eq!(
            cached_epoch_schedule.expect("epoch_schedule sysvar missing in cache"),
            epoch_schedule.into()
        );
        assert_eq!(
            cached_fees.expect("fees sysvar missing in cache"),
            fees.into()
        );
        assert_eq!(
            cached_rent.expect("rent sysvar missing in cache"),
            rent.into()
        );
        assert!(sysvar_cache.get_slot_hashes().is_err());
        assert!(sysvar_cache.get_epoch_rewards().is_err());
    }

    #[test]
    fn test_add_builtin() {
        let mock_bank = MockBankCallback::default();
        let batch_processor = TransactionBatchProcessor::<TestForkGraph>::default();
        batch_processor.program_cache.write().unwrap().fork_graph =
            Some(Arc::new(RwLock::new(TestForkGraph {})));

        let key = Pubkey::new_unique();
        let name = "a_builtin_name";
        let program = ProgramCacheEntry::new_builtin(
            0,
            name.len(),
            |_invoke_context, _param0, _param1, _param2, _param3, _param4| {},
        );

        batch_processor.add_builtin(&mock_bank, key, name, program);

        assert_eq!(
            mock_bank.account_shared_data.read().unwrap()[&key].data(),
            name.as_bytes()
        );

        let mut loaded_programs_for_tx_batch = ProgramCacheForTxBatch::new_from_cache(
            0,
            0,
            &batch_processor.program_cache.read().unwrap(),
        );
        batch_processor.program_cache.write().unwrap().extract(
            &mut vec![(key, (ProgramCacheMatchCriteria::NoCriteria, 1))],
            &mut loaded_programs_for_tx_batch,
            true,
        );
        let entry = loaded_programs_for_tx_batch.find(&key).unwrap();

        // Repeating code because ProgramCacheEntry does not implement clone.
        let program = ProgramCacheEntry::new_builtin(
            0,
            name.len(),
            |_invoke_context, _param0, _param1, _param2, _param3, _param4| {},
        );
        assert_eq!(entry, Arc::new(program));
    }

    #[test]
    fn fast_concur_test() {
        let mut mock_bank = MockBankCallback::default();
        let batch_processor = TransactionBatchProcessor::<TestForkGraph>::new(5, 5, HashSet::new());
        batch_processor.program_cache.write().unwrap().fork_graph =
            Some(Arc::new(RwLock::new(TestForkGraph {})));

        let programs = vec![
            deploy_program("hello-solana".to_string(), &mut mock_bank),
            deploy_program("simple-transfer".to_string(), &mut mock_bank),
            deploy_program("clock-sysvar".to_string(), &mut mock_bank),
        ];

        let account_maps: HashMap<Pubkey, u64> = programs
            .iter()
            .enumerate()
            .map(|(idx, key)| (*key, idx as u64))
            .collect();

        for _ in 0..10 {
            let ths: Vec<_> = (0..4)
                .map(|_| {
                    let local_bank = mock_bank.clone();
                    let processor = TransactionBatchProcessor::new_from(
                        &batch_processor,
                        batch_processor.slot,
                        batch_processor.epoch,
                    );
                    let maps = account_maps.clone();
                    let programs = programs.clone();
                    thread::spawn(move || {
                        let result =
                            processor.replenish_program_cache(&local_bank, &maps, false, true);
                        for key in &programs {
                            let cache_entry = result.find(key);
                            assert!(matches!(
                                cache_entry.unwrap().program,
                                ProgramCacheEntryType::Loaded(_)
                            ));
                        }
                    })
                })
                .collect();

            for th in ths {
                th.join().unwrap();
            }
        }
    }

    fn deploy_program(name: String, mock_bank: &mut MockBankCallback) -> Pubkey {
        let program_account = Pubkey::new_unique();
        let program_data_account = Pubkey::new_unique();
        let state = UpgradeableLoaderState::Program {
            programdata_address: program_data_account,
        };

        // The program account must have funds and hold the executable binary
        let mut account_data = AccountSharedData::default();
        account_data.set_data(bincode::serialize(&state).unwrap());
        account_data.set_lamports(25);
        account_data.set_owner(bpf_loader_upgradeable::id());
        mock_bank
            .account_shared_data
            .write()
            .unwrap()
            .insert(program_account, account_data);

        let mut account_data = AccountSharedData::default();
        let state = UpgradeableLoaderState::ProgramData {
            slot: 0,
            upgrade_authority_address: None,
        };
        let mut header = bincode::serialize(&state).unwrap();
        let mut complement = vec![
            0;
            std::cmp::max(
                0,
                UpgradeableLoaderState::size_of_programdata_metadata().saturating_sub(header.len())
            )
        ];

        let mut dir = env::current_dir().unwrap();
        dir.push("tests");
        dir.push("example-programs");
        dir.push(name.as_str());
        let name = name.replace('-', "_");
        dir.push(name + "_program.so");
        let mut file = File::open(dir.clone()).expect("file not found");
        let metadata = fs::metadata(dir).expect("Unable to read metadata");
        let mut buffer = vec![0; metadata.len() as usize];
        file.read_exact(&mut buffer).expect("Buffer overflow");

        header.append(&mut complement);
        header.append(&mut buffer);
        account_data.set_data(header);
        mock_bank
            .account_shared_data
            .write()
            .unwrap()
            .insert(program_data_account, account_data);

        program_account
    }

    #[test]
    fn test_validate_transaction_fee_payer_exact_balance() {
        let lamports_per_signature = 5000;
        let message = new_unchecked_sanitized_message(Message::new_with_blockhash(
            &[
                ComputeBudgetInstruction::set_compute_unit_limit(2000u32),
                ComputeBudgetInstruction::set_compute_unit_price(1_000_000_000),
            ],
            Some(&Pubkey::new_unique()),
            &Hash::new_unique(),
        ));
        let compute_budget_limits =
            process_compute_budget_instructions(message.program_instructions_iter()).unwrap();
        let fee_payer_address = message.fee_payer();
        let current_epoch = 42;
        let rent_collector = RentCollector {
            epoch: current_epoch,
            ..RentCollector::default()
        };
        let min_balance = rent_collector.rent.minimum_balance(nonce::State::size());
        let transaction_fee = lamports_per_signature;
        let priority_fee = 2_000_000u64;
        let starting_balance = transaction_fee + priority_fee;
        assert!(
            starting_balance > min_balance,
            "we're testing that a rent exempt fee payer can be fully drained, \
        so ensure that the starting balance is more than the min balance"
        );

        let fee_payer_rent_epoch = current_epoch;
        let fee_payer_rent_debit = 0;
        let fee_payer_account = AccountSharedData::new_rent_epoch(
            starting_balance,
            0,
            &Pubkey::default(),
            fee_payer_rent_epoch,
        );
        let mut mock_accounts = HashMap::new();
        mock_accounts.insert(*fee_payer_address, fee_payer_account.clone());
        let mock_bank = MockBankCallback {
            account_shared_data: Arc::new(RwLock::new(mock_accounts)),
        };

        let mut error_counters = TransactionErrorMetrics::default();
        let batch_processor = TransactionBatchProcessor::<TestForkGraph>::default();
        let result = batch_processor.validate_transaction_fee_payer(
            &mock_bank,
            &message,
            CheckedTransactionDetails {
                nonce: None,
                lamports_per_signature,
            },
            &FeatureSet::default(),
            &FeeStructure::default(),
            &rent_collector,
            &mut error_counters,
        );

        let post_validation_fee_payer_account = {
            let mut account = fee_payer_account.clone();
            account.set_rent_epoch(RENT_EXEMPT_RENT_EPOCH);
            account.set_lamports(0);
            account
        };

        assert_eq!(
            result,
            Ok(ValidatedTransactionDetails {
                rollback_accounts: RollbackAccounts::new(
                    None, // nonce
                    *fee_payer_address,
                    post_validation_fee_payer_account.clone(),
                    fee_payer_rent_debit,
                    fee_payer_rent_epoch
                ),
                compute_budget_limits,
                fee_details: FeeDetails::new_for_tests(transaction_fee, priority_fee, false),
                fee_payer_rent_debit,
                fee_payer_account: post_validation_fee_payer_account,
            })
        );
    }

    #[test]
    fn test_validate_transaction_fee_payer_rent_paying() {
        let lamports_per_signature = 5000;
        let message = new_unchecked_sanitized_message(Message::new_with_blockhash(
            &[],
            Some(&Pubkey::new_unique()),
            &Hash::new_unique(),
        ));
        let compute_budget_limits =
            process_compute_budget_instructions(message.program_instructions_iter()).unwrap();
        let fee_payer_address = message.fee_payer();
        let mut rent_collector = RentCollector::default();
        rent_collector.rent.lamports_per_byte_year = 1_000_000;
        let min_balance = rent_collector.rent.minimum_balance(0);
        let transaction_fee = lamports_per_signature;
        let starting_balance = min_balance - 1;
        let fee_payer_account = AccountSharedData::new(starting_balance, 0, &Pubkey::default());
        let fee_payer_rent_debit = rent_collector
            .get_rent_due(
                fee_payer_account.lamports(),
                fee_payer_account.data().len(),
                fee_payer_account.rent_epoch(),
            )
            .lamports();
        assert!(fee_payer_rent_debit > 0);

        let mut mock_accounts = HashMap::new();
        mock_accounts.insert(*fee_payer_address, fee_payer_account.clone());
        let mock_bank = MockBankCallback {
            account_shared_data: Arc::new(RwLock::new(mock_accounts)),
        };

        let mut error_counters = TransactionErrorMetrics::default();
        let batch_processor = TransactionBatchProcessor::<TestForkGraph>::default();
        let result = batch_processor.validate_transaction_fee_payer(
            &mock_bank,
            &message,
            CheckedTransactionDetails {
                nonce: None,
                lamports_per_signature,
            },
            &FeatureSet::default(),
            &FeeStructure::default(),
            &rent_collector,
            &mut error_counters,
        );

        let post_validation_fee_payer_account = {
            let mut account = fee_payer_account.clone();
            account.set_rent_epoch(1);
            account.set_lamports(starting_balance - transaction_fee - fee_payer_rent_debit);
            account
        };

        assert_eq!(
            result,
            Ok(ValidatedTransactionDetails {
                rollback_accounts: RollbackAccounts::new(
                    None, // nonce
                    *fee_payer_address,
                    post_validation_fee_payer_account.clone(),
                    fee_payer_rent_debit,
                    0, // rent epoch
                ),
                compute_budget_limits,
                fee_details: FeeDetails::new_for_tests(transaction_fee, 0, false),
                fee_payer_rent_debit,
                fee_payer_account: post_validation_fee_payer_account,
            })
        );
    }

    #[test]
    fn test_validate_transaction_fee_payer_not_found() {
        let lamports_per_signature = 5000;
        let message =
            new_unchecked_sanitized_message(Message::new(&[], Some(&Pubkey::new_unique())));

        let mock_bank = MockBankCallback::default();
        let mut error_counters = TransactionErrorMetrics::default();
        let batch_processor = TransactionBatchProcessor::<TestForkGraph>::default();
        let result = batch_processor.validate_transaction_fee_payer(
            &mock_bank,
            &message,
            CheckedTransactionDetails {
                nonce: None,
                lamports_per_signature,
            },
            &FeatureSet::default(),
            &FeeStructure::default(),
            &RentCollector::default(),
            &mut error_counters,
        );

        assert_eq!(error_counters.account_not_found, 1);
        assert_eq!(result, Err(TransactionError::AccountNotFound));
    }

    #[test]
    fn test_validate_transaction_fee_payer_insufficient_funds() {
        let lamports_per_signature = 5000;
        let message =
            new_unchecked_sanitized_message(Message::new(&[], Some(&Pubkey::new_unique())));
        let fee_payer_address = message.fee_payer();
        let fee_payer_account = AccountSharedData::new(1, 0, &Pubkey::default());
        let mut mock_accounts = HashMap::new();
        mock_accounts.insert(*fee_payer_address, fee_payer_account.clone());
        let mock_bank = MockBankCallback {
            account_shared_data: Arc::new(RwLock::new(mock_accounts)),
        };

        let mut error_counters = TransactionErrorMetrics::default();
        let batch_processor = TransactionBatchProcessor::<TestForkGraph>::default();
        let result = batch_processor.validate_transaction_fee_payer(
            &mock_bank,
            &message,
            CheckedTransactionDetails {
                nonce: None,
                lamports_per_signature,
            },
            &FeatureSet::default(),
            &FeeStructure::default(),
            &RentCollector::default(),
            &mut error_counters,
        );

        assert_eq!(error_counters.insufficient_funds, 1);
        assert_eq!(result, Err(TransactionError::InsufficientFundsForFee));
    }

    #[test]
    fn test_validate_transaction_fee_payer_insufficient_rent() {
        let lamports_per_signature = 5000;
        let message =
            new_unchecked_sanitized_message(Message::new(&[], Some(&Pubkey::new_unique())));
        let fee_payer_address = message.fee_payer();
        let transaction_fee = lamports_per_signature;
        let rent_collector = RentCollector::default();
        let min_balance = rent_collector.rent.minimum_balance(0);
        let starting_balance = min_balance + transaction_fee - 1;
        let fee_payer_account = AccountSharedData::new(starting_balance, 0, &Pubkey::default());
        let mut mock_accounts = HashMap::new();
        mock_accounts.insert(*fee_payer_address, fee_payer_account.clone());
        let mock_bank = MockBankCallback {
            account_shared_data: Arc::new(RwLock::new(mock_accounts)),
        };

        let mut error_counters = TransactionErrorMetrics::default();
        let batch_processor = TransactionBatchProcessor::<TestForkGraph>::default();
        let result = batch_processor.validate_transaction_fee_payer(
            &mock_bank,
            &message,
            CheckedTransactionDetails {
                nonce: None,
                lamports_per_signature,
            },
            &FeatureSet::default(),
            &FeeStructure::default(),
            &rent_collector,
            &mut error_counters,
        );

        assert_eq!(
            result,
            Err(TransactionError::InsufficientFundsForRent { account_index: 0 })
        );
    }

    #[test]
    fn test_validate_transaction_fee_payer_invalid() {
        let lamports_per_signature = 5000;
        let message =
            new_unchecked_sanitized_message(Message::new(&[], Some(&Pubkey::new_unique())));
        let fee_payer_address = message.fee_payer();
        let fee_payer_account = AccountSharedData::new(1_000_000, 0, &Pubkey::new_unique());
        let mut mock_accounts = HashMap::new();
        mock_accounts.insert(*fee_payer_address, fee_payer_account.clone());
        let mock_bank = MockBankCallback {
            account_shared_data: Arc::new(RwLock::new(mock_accounts)),
        };

        let mut error_counters = TransactionErrorMetrics::default();
        let batch_processor = TransactionBatchProcessor::<TestForkGraph>::default();
        let result = batch_processor.validate_transaction_fee_payer(
            &mock_bank,
            &message,
            CheckedTransactionDetails {
                nonce: None,
                lamports_per_signature,
            },
            &FeatureSet::default(),
            &FeeStructure::default(),
            &RentCollector::default(),
            &mut error_counters,
        );

        assert_eq!(error_counters.invalid_account_for_fee, 1);
        assert_eq!(result, Err(TransactionError::InvalidAccountForFee));
    }

    #[test]
    fn test_validate_transaction_fee_payer_invalid_compute_budget() {
        let lamports_per_signature = 5000;
        let message = new_unchecked_sanitized_message(Message::new(
            &[
                ComputeBudgetInstruction::set_compute_unit_limit(2000u32),
                ComputeBudgetInstruction::set_compute_unit_limit(42u32),
            ],
            Some(&Pubkey::new_unique()),
        ));

        let mock_bank = MockBankCallback::default();
        let mut error_counters = TransactionErrorMetrics::default();
        let batch_processor = TransactionBatchProcessor::<TestForkGraph>::default();
        let result = batch_processor.validate_transaction_fee_payer(
            &mock_bank,
            &message,
            CheckedTransactionDetails {
                nonce: None,
                lamports_per_signature,
            },
            &FeatureSet::default(),
            &FeeStructure::default(),
            &RentCollector::default(),
            &mut error_counters,
        );

        assert_eq!(error_counters.invalid_compute_budget, 1);
        assert_eq!(result, Err(TransactionError::DuplicateInstruction(1u8)));
    }

    #[test]
    fn test_validate_transaction_fee_payer_is_nonce() {
        let feature_set = FeatureSet::default();
        let lamports_per_signature = 5000;
        let rent_collector = RentCollector::default();
        let compute_unit_limit = 2 * solana_compute_budget_program::DEFAULT_COMPUTE_UNITS;
        let message = new_unchecked_sanitized_message(Message::new_with_blockhash(
            &[
                ComputeBudgetInstruction::set_compute_unit_limit(compute_unit_limit as u32),
                ComputeBudgetInstruction::set_compute_unit_price(1_000_000),
            ],
            Some(&Pubkey::new_unique()),
            &Hash::new_unique(),
        ));
        let compute_budget_limits =
            process_compute_budget_instructions(message.program_instructions_iter()).unwrap();
        let fee_payer_address = message.fee_payer();
        let min_balance = Rent::default().minimum_balance(nonce::State::size());
        let transaction_fee = lamports_per_signature;
        let priority_fee = compute_unit_limit;

        // Sufficient Fees
        {
            let fee_payer_account = AccountSharedData::new_data(
                min_balance + transaction_fee + priority_fee,
                &nonce::state::Versions::new(nonce::State::Initialized(
                    nonce::state::Data::default(),
                )),
                &system_program::id(),
            )
            .unwrap();

            let mut mock_accounts = HashMap::new();
            mock_accounts.insert(*fee_payer_address, fee_payer_account.clone());
            let mock_bank = MockBankCallback {
                account_shared_data: Arc::new(RwLock::new(mock_accounts)),
            };

            let mut error_counters = TransactionErrorMetrics::default();
            let batch_processor = TransactionBatchProcessor::<TestForkGraph>::default();
            let nonce = Some(NoncePartial::new(
                *fee_payer_address,
                fee_payer_account.clone(),
            ));
            let result = batch_processor.validate_transaction_fee_payer(
                &mock_bank,
                &message,
                CheckedTransactionDetails {
                    nonce: nonce.clone(),
                    lamports_per_signature,
                },
                &feature_set,
                &FeeStructure::default(),
                &rent_collector,
                &mut error_counters,
            );

            let post_validation_fee_payer_account = {
                let mut account = fee_payer_account.clone();
                account.set_rent_epoch(RENT_EXEMPT_RENT_EPOCH);
                account.set_lamports(min_balance);
                account
            };

            assert_eq!(
                result,
                Ok(ValidatedTransactionDetails {
                    rollback_accounts: RollbackAccounts::new(
                        nonce,
                        *fee_payer_address,
                        post_validation_fee_payer_account.clone(),
                        0, // fee_payer_rent_debit
                        0, // fee_payer_rent_epoch
                    ),
                    compute_budget_limits,
                    fee_details: FeeDetails::new_for_tests(transaction_fee, priority_fee, false),
                    fee_payer_rent_debit: 0, // rent due
                    fee_payer_account: post_validation_fee_payer_account,
                })
            );
        }

        // Insufficient Fees
        {
            let fee_payer_account = AccountSharedData::new_data(
                transaction_fee + priority_fee, // no min_balance this time
                &nonce::state::Versions::new(nonce::State::Initialized(
                    nonce::state::Data::default(),
                )),
                &system_program::id(),
            )
            .unwrap();

            let mut mock_accounts = HashMap::new();
            mock_accounts.insert(*fee_payer_address, fee_payer_account.clone());
            let mock_bank = MockBankCallback {
                account_shared_data: Arc::new(RwLock::new(mock_accounts)),
            };

            let mut error_counters = TransactionErrorMetrics::default();
            let batch_processor = TransactionBatchProcessor::<TestForkGraph>::default();
            let result = batch_processor.validate_transaction_fee_payer(
                &mock_bank,
                &message,
                CheckedTransactionDetails {
                    nonce: None,
                    lamports_per_signature,
                },
                &feature_set,
                &FeeStructure::default(),
                &rent_collector,
                &mut error_counters,
            );

            assert_eq!(error_counters.insufficient_funds, 1);
            assert_eq!(result, Err(TransactionError::InsufficientFundsForFee));
        }
    }
}

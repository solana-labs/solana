use {
    crate::{
        account_loader::{
            load_accounts, LoadedTransaction, TransactionCheckResult, TransactionLoadResult,
        },
        account_overrides::AccountOverrides,
        message_processor::MessageProcessor,
        runtime_config::RuntimeConfig,
        transaction_account_state_info::TransactionAccountStateInfo,
        transaction_error_metrics::TransactionErrorMetrics,
        transaction_results::{
            DurableNonceFee, TransactionExecutionDetails, TransactionExecutionResult,
        },
    },
    log::debug,
    percentage::Percentage,
    solana_measure::measure::Measure,
    solana_program_runtime::{
        compute_budget::ComputeBudget,
        invoke_context::InvokeContext,
        loaded_programs::{
            ForkGraph, LoadProgramMetrics, ProgramCache, ProgramCacheEntry, ProgramCacheEntryOwner,
            ProgramCacheEntryType, ProgramCacheForTxBatch, ProgramCacheMatchCriteria,
            ProgramRuntimeEnvironment, DELAY_VISIBILITY_SLOT_OFFSET,
        },
        log_collector::LogCollector,
        sysvar_cache::SysvarCache,
        timings::{ExecuteDetailsTimings, ExecuteTimingType, ExecuteTimings},
    },
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount, PROGRAM_OWNERS},
        account_utils::StateMut,
        bpf_loader, bpf_loader_deprecated,
        bpf_loader_upgradeable::{self, UpgradeableLoaderState},
        clock::{Epoch, Slot},
        epoch_schedule::EpochSchedule,
        feature_set::FeatureSet,
        fee::FeeStructure,
        hash::Hash,
        inner_instruction::{InnerInstruction, InnerInstructionsList},
        instruction::{CompiledInstruction, InstructionError, TRANSACTION_LEVEL_STACK_HEIGHT},
        loader_v4::{self, LoaderV4State, LoaderV4Status},
        message::SanitizedMessage,
        pubkey::Pubkey,
        rent_collector::RentCollector,
        saturating_add_assign,
        transaction::{self, SanitizedTransaction, TransactionError},
        transaction_context::{ExecutionRecord, TransactionContext},
    },
    std::{
        cell::RefCell,
        collections::{hash_map::Entry, HashMap, HashSet},
        fmt::{Debug, Formatter},
        rc::Rc,
        sync::{atomic::Ordering, Arc, RwLock},
    },
};

/// A list of log messages emitted during a transaction
pub type TransactionLogMessages = Vec<String>;

pub struct LoadAndExecuteSanitizedTransactionsOutput {
    pub loaded_transactions: Vec<TransactionLoadResult>,
    // Vector of results indicating whether a transaction was executed or could not
    // be executed. Note executed transactions can still have failed!
    pub execution_results: Vec<TransactionExecutionResult>,
}

/// Configuration of the recording capabilities for transaction execution
#[derive(Copy, Clone)]
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

pub trait TransactionProcessingCallback {
    fn account_matches_owners(&self, account: &Pubkey, owners: &[Pubkey]) -> Option<usize>;

    fn get_account_shared_data(&self, pubkey: &Pubkey) -> Option<AccountSharedData>;

    fn get_last_blockhash_and_lamports_per_signature(&self) -> (Hash, u64);

    fn get_rent_collector(&self) -> &RentCollector;

    fn get_feature_set(&self) -> Arc<FeatureSet>;

    fn check_account_access(
        &self,
        _message: &SanitizedMessage,
        _account_index: usize,
        _account: &AccountSharedData,
        _error_counters: &mut TransactionErrorMetrics,
    ) -> transaction::Result<()> {
        Ok(())
    }

    fn get_program_match_criteria(&self, _program: &Pubkey) -> ProgramCacheMatchCriteria {
        ProgramCacheMatchCriteria::NoCriteria
    }

    fn add_builtin_account(&self, _name: &str, _program_id: &Pubkey) {}
}

#[derive(Debug)]
enum ProgramAccountLoadResult {
    InvalidAccountData(ProgramCacheEntryOwner),
    ProgramOfLoaderV1(AccountSharedData),
    ProgramOfLoaderV2(AccountSharedData),
    ProgramOfLoaderV3(AccountSharedData, AccountSharedData, Slot),
    ProgramOfLoaderV4(AccountSharedData, Slot),
}

#[derive(AbiExample)]
pub struct TransactionBatchProcessor<FG: ForkGraph> {
    /// Bank slot (i.e. block)
    slot: Slot,

    /// Bank epoch
    epoch: Epoch,

    /// initialized from genesis
    epoch_schedule: EpochSchedule,

    /// Transaction fee structure
    pub fee_structure: FeeStructure,

    /// Optional config parameters that can override runtime behavior
    pub runtime_config: Arc<RuntimeConfig>,

    /// SysvarCache is a collection of system variables that are
    /// accessible from on chain programs. It is passed to SVM from
    /// client code (e.g. Bank) and forwarded to the MessageProcessor.
    pub sysvar_cache: RwLock<SysvarCache>,

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
            .field("epoch_schedule", &self.epoch_schedule)
            .field("fee_structure", &self.fee_structure)
            .field("runtime_config", &self.runtime_config)
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
            epoch_schedule: EpochSchedule::default(),
            fee_structure: FeeStructure::default(),
            runtime_config: Arc::<RuntimeConfig>::default(),
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
    pub fn new(
        slot: Slot,
        epoch: Epoch,
        epoch_schedule: EpochSchedule,
        runtime_config: Arc<RuntimeConfig>,
        program_cache: Arc<RwLock<ProgramCache<FG>>>,
        builtin_program_ids: HashSet<Pubkey>,
    ) -> Self {
        Self {
            slot,
            epoch,
            epoch_schedule,
            fee_structure: FeeStructure::default(),
            runtime_config,
            sysvar_cache: RwLock::<SysvarCache>::default(),
            program_cache,
            builtin_program_ids: RwLock::new(builtin_program_ids),
        }
    }

    pub fn new_from(&self, slot: Slot, epoch: Epoch) -> Self {
        Self {
            slot,
            epoch,
            epoch_schedule: self.epoch_schedule.clone(),
            fee_structure: self.fee_structure.clone(),
            runtime_config: self.runtime_config.clone(),
            sysvar_cache: RwLock::<SysvarCache>::default(),
            program_cache: self.program_cache.clone(),
            builtin_program_ids: RwLock::new(self.builtin_program_ids.read().unwrap().clone()),
        }
    }

    /// Main entrypoint to the SVM.
    #[allow(clippy::too_many_arguments)]
    pub fn load_and_execute_sanitized_transactions<CB: TransactionProcessingCallback>(
        &self,
        callbacks: &CB,
        sanitized_txs: &[SanitizedTransaction],
        check_results: &mut [TransactionCheckResult],
        error_counters: &mut TransactionErrorMetrics,
        recording_config: ExecutionRecordingConfig,
        timings: &mut ExecuteTimings,
        account_overrides: Option<&AccountOverrides>,
        log_messages_bytes_limit: Option<usize>,
        limit_to_load_programs: bool,
    ) -> LoadAndExecuteSanitizedTransactionsOutput {
        let mut program_cache_time = Measure::start("program_cache");
        let mut program_accounts_map = Self::filter_executable_program_accounts(
            callbacks,
            sanitized_txs,
            check_results,
            PROGRAM_OWNERS,
        );
        for builtin_program in self.builtin_program_ids.read().unwrap().iter() {
            program_accounts_map.insert(*builtin_program, 0);
        }

        let programs_loaded_for_tx_batch = Rc::new(RefCell::new(self.replenish_program_cache(
            callbacks,
            &program_accounts_map,
            limit_to_load_programs,
        )));

        if programs_loaded_for_tx_batch.borrow().hit_max_limit {
            const ERROR: TransactionError = TransactionError::ProgramCacheHitMaxLimit;
            let loaded_transactions = vec![(Err(ERROR), None); sanitized_txs.len()];
            let execution_results =
                vec![TransactionExecutionResult::NotExecuted(ERROR); sanitized_txs.len()];
            return LoadAndExecuteSanitizedTransactionsOutput {
                loaded_transactions,
                execution_results,
            };
        }
        program_cache_time.stop();

        let mut load_time = Measure::start("accounts_load");
        let mut loaded_transactions = load_accounts(
            callbacks,
            sanitized_txs,
            check_results,
            error_counters,
            &self.fee_structure,
            account_overrides,
            &programs_loaded_for_tx_batch.borrow(),
        );
        load_time.stop();

        let mut execution_time = Measure::start("execution_time");

        let execution_results: Vec<TransactionExecutionResult> = loaded_transactions
            .iter_mut()
            .zip(sanitized_txs.iter())
            .map(|(accs, tx)| match accs {
                (Err(e), _nonce) => TransactionExecutionResult::NotExecuted(e.clone()),
                (Ok(loaded_transaction), nonce) => {
                    let compute_budget =
                        if let Some(compute_budget) = self.runtime_config.compute_budget {
                            compute_budget
                        } else {
                            let mut compute_budget_process_transaction_time =
                                Measure::start("compute_budget_process_transaction_time");
                            let maybe_compute_budget = ComputeBudget::try_from_instructions(
                                tx.message().program_instructions_iter(),
                            );
                            compute_budget_process_transaction_time.stop();
                            saturating_add_assign!(
                                timings
                                    .execute_accessories
                                    .compute_budget_process_transaction_us,
                                compute_budget_process_transaction_time.as_us()
                            );
                            if let Err(err) = maybe_compute_budget {
                                return TransactionExecutionResult::NotExecuted(err);
                            }
                            maybe_compute_budget.unwrap()
                        };

                    let result = self.execute_loaded_transaction(
                        callbacks,
                        tx,
                        loaded_transaction,
                        compute_budget,
                        nonce.as_ref().map(DurableNonceFee::from),
                        recording_config,
                        timings,
                        error_counters,
                        log_messages_bytes_limit,
                        &programs_loaded_for_tx_batch.borrow(),
                    );

                    if let TransactionExecutionResult::Executed {
                        details,
                        programs_modified_by_tx,
                    } = &result
                    {
                        // Update batch specific cache of the loaded programs with the modifications
                        // made by the transaction, if it executed successfully.
                        if details.status.is_ok() {
                            programs_loaded_for_tx_batch
                                .borrow_mut()
                                .merge(programs_modified_by_tx);
                        }
                    }

                    result
                }
            })
            .collect();

        execution_time.stop();

        const SHRINK_LOADED_PROGRAMS_TO_PERCENTAGE: u8 = 90;
        self.program_cache
            .write()
            .unwrap()
            .evict_using_2s_random_selection(
                Percentage::from(SHRINK_LOADED_PROGRAMS_TO_PERCENTAGE),
                self.slot,
            );

        debug!(
            "load: {}us execute: {}us txs_len={}",
            load_time.as_us(),
            execution_time.as_us(),
            sanitized_txs.len(),
        );

        timings.saturating_add_in_place(
            ExecuteTimingType::ProgramCacheUs,
            program_cache_time.as_us(),
        );
        timings.saturating_add_in_place(ExecuteTimingType::LoadUs, load_time.as_us());
        timings.saturating_add_in_place(ExecuteTimingType::ExecuteUs, execution_time.as_us());

        LoadAndExecuteSanitizedTransactionsOutput {
            loaded_transactions,
            execution_results,
        }
    }

    /// Returns a map from executable program accounts (all accounts owned by any loader)
    /// to their usage counters, for the transactions with a valid blockhash or nonce.
    fn filter_executable_program_accounts<CB: TransactionProcessingCallback>(
        callbacks: &CB,
        txs: &[SanitizedTransaction],
        check_results: &mut [TransactionCheckResult],
        program_owners: &[Pubkey],
    ) -> HashMap<Pubkey, u64> {
        let mut result: HashMap<Pubkey, u64> = HashMap::new();
        check_results.iter_mut().zip(txs).for_each(|etx| {
            if let ((Ok(()), _nonce, lamports_per_signature), tx) = etx {
                if lamports_per_signature.is_some() {
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
                } else {
                    // If the transaction's nonce account was not valid, and blockhash is not found,
                    // the transaction will fail to process. Let's not load any programs from the
                    // transaction, and update the status of the transaction.
                    *etx.0 = (Err(TransactionError::BlockhashNotFound), None, None);
                }
            }
        });
        result
    }

    /// Loads the program with the given pubkey.
    ///
    /// If the account doesn't exist it returns `None`. If the account does exist, it must be a program
    /// account (belong to one of the program loaders). Returns `Some(InvalidAccountData)` if the program
    /// account is `Closed`, contains invalid data or any of the programdata accounts are invalid.
    pub fn load_program_with_pubkey<CB: TransactionProcessingCallback>(
        &self,
        callbacks: &CB,
        pubkey: &Pubkey,
        reload: bool,
        effective_epoch: Epoch,
    ) -> Option<Arc<ProgramCacheEntry>> {
        let program_cache = self.program_cache.read().unwrap();
        let environments = program_cache.get_environments_for_epoch(effective_epoch);
        let mut load_program_metrics = LoadProgramMetrics {
            program_id: pubkey.to_string(),
            ..LoadProgramMetrics::default()
        };

        let mut loaded_program = match self.load_program_accounts(callbacks, pubkey)? {
            ProgramAccountLoadResult::InvalidAccountData(owner) => Ok(
                ProgramCacheEntry::new_tombstone(self.slot, owner, ProgramCacheEntryType::Closed),
            ),

            ProgramAccountLoadResult::ProgramOfLoaderV1(program_account) => {
                Self::load_program_from_bytes(
                    &mut load_program_metrics,
                    program_account.data(),
                    program_account.owner(),
                    program_account.data().len(),
                    0,
                    environments.program_runtime_v1.clone(),
                    reload,
                )
                .map_err(|_| (0, ProgramCacheEntryOwner::LoaderV1))
            }

            ProgramAccountLoadResult::ProgramOfLoaderV2(program_account) => {
                Self::load_program_from_bytes(
                    &mut load_program_metrics,
                    program_account.data(),
                    program_account.owner(),
                    program_account.data().len(),
                    0,
                    environments.program_runtime_v1.clone(),
                    reload,
                )
                .map_err(|_| (0, ProgramCacheEntryOwner::LoaderV2))
            }

            ProgramAccountLoadResult::ProgramOfLoaderV3(
                program_account,
                programdata_account,
                slot,
            ) => programdata_account
                .data()
                .get(UpgradeableLoaderState::size_of_programdata_metadata()..)
                .ok_or(Box::new(InstructionError::InvalidAccountData).into())
                .and_then(|programdata| {
                    Self::load_program_from_bytes(
                        &mut load_program_metrics,
                        programdata,
                        program_account.owner(),
                        program_account
                            .data()
                            .len()
                            .saturating_add(programdata_account.data().len()),
                        slot,
                        environments.program_runtime_v1.clone(),
                        reload,
                    )
                })
                .map_err(|_| (slot, ProgramCacheEntryOwner::LoaderV3)),

            ProgramAccountLoadResult::ProgramOfLoaderV4(program_account, slot) => program_account
                .data()
                .get(LoaderV4State::program_data_offset()..)
                .ok_or(Box::new(InstructionError::InvalidAccountData).into())
                .and_then(|elf_bytes| {
                    Self::load_program_from_bytes(
                        &mut load_program_metrics,
                        elf_bytes,
                        &loader_v4::id(),
                        program_account.data().len(),
                        slot,
                        environments.program_runtime_v2.clone(),
                        reload,
                    )
                })
                .map_err(|_| (slot, ProgramCacheEntryOwner::LoaderV4)),
        }
        .unwrap_or_else(|(slot, owner)| {
            let env = if let ProgramCacheEntryOwner::LoaderV4 = &owner {
                environments.program_runtime_v2.clone()
            } else {
                environments.program_runtime_v1.clone()
            };
            ProgramCacheEntry::new_tombstone(
                slot,
                owner,
                ProgramCacheEntryType::FailedVerification(env),
            )
        });

        let mut timings = ExecuteDetailsTimings::default();
        load_program_metrics.submit_datapoint(&mut timings);
        if !Arc::ptr_eq(
            &environments.program_runtime_v1,
            &program_cache.environments.program_runtime_v1,
        ) || !Arc::ptr_eq(
            &environments.program_runtime_v2,
            &program_cache.environments.program_runtime_v2,
        ) {
            // There can be two entries per program when the environment changes.
            // One for the old environment before the epoch boundary and one for the new environment after the epoch boundary.
            // These two entries have the same deployment slot, so they must differ in their effective slot instead.
            // This is done by setting the effective slot of the entry for the new environment to the epoch boundary.
            loaded_program.effective_slot = loaded_program
                .effective_slot
                .max(self.epoch_schedule.get_first_slot_in_epoch(effective_epoch));
        }
        loaded_program.update_access_slot(self.slot);
        Some(Arc::new(loaded_program))
    }

    fn replenish_program_cache<CB: TransactionProcessingCallback>(
        &self,
        callback: &CB,
        program_accounts_map: &HashMap<Pubkey, u64>,
        limit_to_load_programs: bool,
    ) -> ProgramCacheForTxBatch {
        let mut missing_programs: Vec<(Pubkey, (ProgramCacheMatchCriteria, u64))> =
            program_accounts_map
                .iter()
                .map(|(pubkey, count)| {
                    (
                        *pubkey,
                        (callback.get_program_match_criteria(pubkey), *count),
                    )
                })
                .collect();

        let mut loaded_programs_for_txs = None;
        let mut program_to_store = None;
        loop {
            let (program_to_load, task_cookie, task_waiter) = {
                // Lock the global cache.
                let mut program_cache = self.program_cache.write().unwrap();
                // Initialize our local cache.
                let is_first_round = loaded_programs_for_txs.is_none();
                if is_first_round {
                    loaded_programs_for_txs = Some(ProgramCacheForTxBatch::new_from_cache(
                        self.slot,
                        self.epoch,
                        &program_cache,
                    ));
                }
                // Submit our last completed loading task.
                if let Some((key, program)) = program_to_store.take() {
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
                }
                // Figure out which program needs to be loaded next.
                let program_to_load = program_cache.extract(
                    &mut missing_programs,
                    loaded_programs_for_txs.as_mut().unwrap(),
                    is_first_round,
                );
                let task_waiter = Arc::clone(&program_cache.loading_task_waiter);
                (program_to_load, task_waiter.cookie(), task_waiter)
                // Unlock the global cache again.
            };

            if let Some((key, count)) = program_to_load {
                // Load, verify and compile one program.
                let program = self
                    .load_program_with_pubkey(callback, &key, false, self.epoch)
                    .expect("called load_program_with_pubkey() with nonexistent account");
                program.tx_usage_counter.store(count, Ordering::Relaxed);
                program_to_store = Some((key, program));
            } else if missing_programs.is_empty() {
                break;
            } else {
                // Sleep until the next finish_cooperative_loading_task() call.
                // Once a task completes we'll wake up and try to load the
                // missing programs inside the tx batch again.
                let _new_cookie = task_waiter.wait(task_cookie);

                // This branch is not tested in the SVM because it requires concurrent threads.
                // In addition, one of them must be holding the mutex while the other must be
                // trying to lock it.
            }
        }

        loaded_programs_for_txs.unwrap()
    }

    /// Execute a transaction using the provided loaded accounts and update
    /// the executors cache if the transaction was successful.
    #[allow(clippy::too_many_arguments)]
    fn execute_loaded_transaction<CB: TransactionProcessingCallback>(
        &self,
        callback: &CB,
        tx: &SanitizedTransaction,
        loaded_transaction: &mut LoadedTransaction,
        compute_budget: ComputeBudget,
        durable_nonce_fee: Option<DurableNonceFee>,
        recording_config: ExecutionRecordingConfig,
        timings: &mut ExecuteTimings,
        error_counters: &mut TransactionErrorMetrics,
        log_messages_bytes_limit: Option<usize>,
        programs_loaded_for_tx_batch: &ProgramCacheForTxBatch,
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

        let lamports_before_tx =
            transaction_accounts_lamports_sum(&transaction_accounts, tx.message()).unwrap_or(0);

        let mut transaction_context = TransactionContext::new(
            transaction_accounts,
            callback.get_rent_collector().rent.clone(),
            compute_budget.max_invoke_stack_height,
            compute_budget.max_instruction_trace_length,
        );
        #[cfg(debug_assertions)]
        transaction_context.set_signature(tx.signature());

        let pre_account_state_info = TransactionAccountStateInfo::new(
            &callback.get_rent_collector().rent,
            &transaction_context,
            tx.message(),
        );

        let log_collector = if recording_config.enable_log_recording {
            match log_messages_bytes_limit {
                None => Some(LogCollector::new_ref()),
                Some(log_messages_bytes_limit) => Some(LogCollector::new_ref_with_limit(Some(
                    log_messages_bytes_limit,
                ))),
            }
        } else {
            None
        };

        let (blockhash, lamports_per_signature) =
            callback.get_last_blockhash_and_lamports_per_signature();

        let mut executed_units = 0u64;
        let mut programs_modified_by_tx = ProgramCacheForTxBatch::new(
            self.slot,
            programs_loaded_for_tx_batch.environments.clone(),
            programs_loaded_for_tx_batch.upcoming_environments.clone(),
            programs_loaded_for_tx_batch.latest_root_epoch,
        );
        let sysvar_cache = &self.sysvar_cache.read().unwrap();

        let mut invoke_context = InvokeContext::new(
            &mut transaction_context,
            sysvar_cache,
            log_collector.clone(),
            compute_budget,
            programs_loaded_for_tx_batch,
            &mut programs_modified_by_tx,
            callback.get_feature_set(),
            blockhash,
            lamports_per_signature,
        );

        let mut process_message_time = Measure::start("process_message_time");
        let process_result = MessageProcessor::process_message(
            tx.message(),
            &loaded_transaction.program_indices,
            &mut invoke_context,
            timings,
            &mut executed_units,
        );
        process_message_time.stop();

        drop(invoke_context);

        saturating_add_assign!(
            timings.execute_accessories.process_message_us,
            process_message_time.as_us()
        );

        let mut status = process_result
            .and_then(|info| {
                let post_account_state_info = TransactionAccountStateInfo::new(
                    &callback.get_rent_collector().rent,
                    &transaction_context,
                    tx.message(),
                );
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
                        error_counters.invalid_rent_paying_account += 1;
                    }
                    TransactionError::InvalidAccountIndex => {
                        error_counters.invalid_account_index += 1;
                    }
                    _ => {
                        error_counters.instruction_error += 1;
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

        let inner_instructions = if recording_config.enable_cpi_recording {
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
            timings.details.total_account_count,
            loaded_transaction.accounts.len() as u64
        );
        saturating_add_assign!(timings.details.changed_account_count, touched_account_count);

        let return_data =
            if recording_config.enable_return_data_recording && !return_data.data.is_empty() {
                Some(return_data)
            } else {
                None
            };

        TransactionExecutionResult::Executed {
            details: TransactionExecutionDetails {
                status,
                log_messages,
                inner_instructions,
                durable_nonce_fee,
                return_data,
                executed_units,
                accounts_data_len_delta,
            },
            programs_modified_by_tx: Box::new(programs_modified_by_tx),
        }
    }

    /// Find the slot in which the program was most recently modified.
    /// Returns slot 0 for programs deployed with v1/v2 loaders, since programs deployed
    /// with those loaders do not retain deployment slot information.
    /// Returns an error if the program's account state can not be found or parsed.
    pub fn program_modification_slot<CB: TransactionProcessingCallback>(
        &self,
        callbacks: &CB,
        pubkey: &Pubkey,
    ) -> transaction::Result<Slot> {
        let program = callbacks
            .get_account_shared_data(pubkey)
            .ok_or(TransactionError::ProgramAccountNotFound)?;
        if bpf_loader_upgradeable::check_id(program.owner()) {
            if let Ok(UpgradeableLoaderState::Program {
                programdata_address,
            }) = program.state()
            {
                let programdata = callbacks
                    .get_account_shared_data(&programdata_address)
                    .ok_or(TransactionError::ProgramAccountNotFound)?;
                if let Ok(UpgradeableLoaderState::ProgramData {
                    slot,
                    upgrade_authority_address: _,
                }) = programdata.state()
                {
                    return Ok(slot);
                }
            }
            Err(TransactionError::ProgramAccountNotFound)
        } else if loader_v4::check_id(program.owner()) {
            let state = solana_loader_v4_program::get_state(program.data())
                .map_err(|_| TransactionError::ProgramAccountNotFound)?;
            Ok(state.slot)
        } else {
            Ok(0)
        }
    }

    fn load_program_from_bytes(
        load_program_metrics: &mut LoadProgramMetrics,
        programdata: &[u8],
        loader_key: &Pubkey,
        account_size: usize,
        deployment_slot: Slot,
        program_runtime_environment: ProgramRuntimeEnvironment,
        reloading: bool,
    ) -> std::result::Result<ProgramCacheEntry, Box<dyn std::error::Error>> {
        if reloading {
            // Safety: this is safe because the program is being reloaded in the cache.
            unsafe {
                ProgramCacheEntry::reload(
                    loader_key,
                    program_runtime_environment.clone(),
                    deployment_slot,
                    deployment_slot.saturating_add(DELAY_VISIBILITY_SLOT_OFFSET),
                    programdata,
                    account_size,
                    load_program_metrics,
                )
            }
        } else {
            ProgramCacheEntry::new(
                loader_key,
                program_runtime_environment.clone(),
                deployment_slot,
                deployment_slot.saturating_add(DELAY_VISIBILITY_SLOT_OFFSET),
                programdata,
                account_size,
                load_program_metrics,
            )
        }
    }

    fn load_program_accounts<CB: TransactionProcessingCallback>(
        &self,
        callbacks: &CB,
        pubkey: &Pubkey,
    ) -> Option<ProgramAccountLoadResult> {
        let program_account = callbacks.get_account_shared_data(pubkey)?;

        if loader_v4::check_id(program_account.owner()) {
            return Some(
                solana_loader_v4_program::get_state(program_account.data())
                    .ok()
                    .and_then(|state| {
                        (!matches!(state.status, LoaderV4Status::Retracted)).then_some(state.slot)
                    })
                    .map(|slot| ProgramAccountLoadResult::ProgramOfLoaderV4(program_account, slot))
                    .unwrap_or(ProgramAccountLoadResult::InvalidAccountData(
                        ProgramCacheEntryOwner::LoaderV4,
                    )),
            );
        }

        if bpf_loader_deprecated::check_id(program_account.owner()) {
            return Some(ProgramAccountLoadResult::ProgramOfLoaderV1(program_account));
        }

        if bpf_loader::check_id(program_account.owner()) {
            return Some(ProgramAccountLoadResult::ProgramOfLoaderV2(program_account));
        }

        if let Ok(UpgradeableLoaderState::Program {
            programdata_address,
        }) = program_account.state()
        {
            if let Some(programdata_account) =
                callbacks.get_account_shared_data(&programdata_address)
            {
                if let Ok(UpgradeableLoaderState::ProgramData {
                    slot,
                    upgrade_authority_address: _,
                }) = programdata_account.state()
                {
                    return Some(ProgramAccountLoadResult::ProgramOfLoaderV3(
                        program_account,
                        programdata_account,
                        slot,
                    ));
                }
            }
        }
        Some(ProgramAccountLoadResult::InvalidAccountData(
            ProgramCacheEntryOwner::LoaderV3,
        ))
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
        solana_program_runtime::{
            loaded_programs::{BlockRelation, ProgramRuntimeEnvironments},
            solana_rbpf::program::BuiltinProgram,
        },
        solana_sdk::{
            account::{create_account_shared_data_for_test, WritableAccount},
            bpf_loader,
            fee_calculator::FeeCalculator,
            message::{LegacyMessage, Message, MessageHeader},
            rent_debits::RentDebits,
            reserved_account_keys::ReservedAccountKeys,
            signature::{Keypair, Signature},
            sysvar::{self, rent::Rent},
            transaction::{SanitizedTransaction, Transaction, TransactionError},
            transaction_context::TransactionContext,
        },
        std::{
            env,
            fs::{self, File},
            io::Read,
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
        rent_collector: RentCollector,
        feature_set: Arc<FeatureSet>,
        pub account_shared_data: RefCell<HashMap<Pubkey, AccountSharedData>>,
    }

    impl TransactionProcessingCallback for MockBankCallback {
        fn account_matches_owners(&self, account: &Pubkey, owners: &[Pubkey]) -> Option<usize> {
            if let Some(data) = self.account_shared_data.borrow().get(account) {
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
            self.account_shared_data.borrow().get(pubkey).cloned()
        }

        fn get_last_blockhash_and_lamports_per_signature(&self) -> (Hash, u64) {
            (Hash::new_unique(), 2)
        }

        fn get_rent_collector(&self) -> &RentCollector {
            &self.rent_collector
        }

        fn get_feature_set(&self) -> Arc<FeatureSet> {
            self.feature_set.clone()
        }

        fn add_builtin_account(&self, name: &str, program_id: &Pubkey) {
            let mut account_data = AccountSharedData::default();
            account_data.set_data(name.as_bytes().to_vec());
            self.account_shared_data
                .borrow_mut()
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
    fn test_load_program_accounts_account_not_found() {
        let mock_bank = MockBankCallback::default();
        let key = Pubkey::new_unique();
        let batch_processor = TransactionBatchProcessor::<TestForkGraph>::default();

        let result = batch_processor.load_program_accounts(&mock_bank, &key);
        assert!(result.is_none());

        let mut account_data = AccountSharedData::default();
        account_data.set_owner(bpf_loader_upgradeable::id());
        let state = UpgradeableLoaderState::Program {
            programdata_address: Pubkey::new_unique(),
        };
        account_data.set_data(bincode::serialize(&state).unwrap());
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key, account_data.clone());

        let result = batch_processor.load_program_accounts(&mock_bank, &key);
        assert!(matches!(
            result,
            Some(ProgramAccountLoadResult::InvalidAccountData(_))
        ));

        account_data.set_data(Vec::new());
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key, account_data);

        let result = batch_processor.load_program_accounts(&mock_bank, &key);

        assert!(matches!(
            result,
            Some(ProgramAccountLoadResult::InvalidAccountData(_))
        ));
    }

    #[test]
    fn test_load_program_accounts_loader_v4() {
        let key = Pubkey::new_unique();
        let mock_bank = MockBankCallback::default();
        let mut account_data = AccountSharedData::default();
        account_data.set_owner(loader_v4::id());
        let batch_processor = TransactionBatchProcessor::<TestForkGraph>::default();
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key, account_data.clone());

        let result = batch_processor.load_program_accounts(&mock_bank, &key);
        assert!(matches!(
            result,
            Some(ProgramAccountLoadResult::InvalidAccountData(_))
        ));

        account_data.set_data(vec![0; 64]);
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key, account_data.clone());
        let result = batch_processor.load_program_accounts(&mock_bank, &key);
        assert!(matches!(
            result,
            Some(ProgramAccountLoadResult::InvalidAccountData(_))
        ));

        let loader_data = LoaderV4State {
            slot: 25,
            authority_address: Pubkey::new_unique(),
            status: LoaderV4Status::Deployed,
        };
        let encoded = unsafe {
            std::mem::transmute::<&LoaderV4State, &[u8; LoaderV4State::program_data_offset()]>(
                &loader_data,
            )
        };
        account_data.set_data(encoded.to_vec());
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key, account_data.clone());

        let result = batch_processor.load_program_accounts(&mock_bank, &key);

        match result {
            Some(ProgramAccountLoadResult::ProgramOfLoaderV4(data, slot)) => {
                assert_eq!(data, account_data);
                assert_eq!(slot, 25);
            }

            _ => panic!("Invalid result"),
        }
    }

    #[test]
    fn test_load_program_accounts_loader_v1_or_v2() {
        let key = Pubkey::new_unique();
        let mock_bank = MockBankCallback::default();
        let mut account_data = AccountSharedData::default();
        account_data.set_owner(bpf_loader::id());
        let batch_processor = TransactionBatchProcessor::<TestForkGraph>::default();
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key, account_data.clone());

        let result = batch_processor.load_program_accounts(&mock_bank, &key);
        match result {
            Some(ProgramAccountLoadResult::ProgramOfLoaderV1(data))
            | Some(ProgramAccountLoadResult::ProgramOfLoaderV2(data)) => {
                assert_eq!(data, account_data);
            }
            _ => panic!("Invalid result"),
        }
    }

    #[test]
    fn test_load_program_accounts_success() {
        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        let mock_bank = MockBankCallback::default();
        let batch_processor = TransactionBatchProcessor::<TestForkGraph>::default();

        let mut account_data = AccountSharedData::default();
        account_data.set_owner(bpf_loader_upgradeable::id());

        let state = UpgradeableLoaderState::Program {
            programdata_address: key2,
        };
        account_data.set_data(bincode::serialize(&state).unwrap());
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key1, account_data.clone());

        let state = UpgradeableLoaderState::ProgramData {
            slot: 25,
            upgrade_authority_address: None,
        };
        let mut account_data2 = AccountSharedData::default();
        account_data2.set_data(bincode::serialize(&state).unwrap());
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key2, account_data2.clone());

        let result = batch_processor.load_program_accounts(&mock_bank, &key1);

        match result {
            Some(ProgramAccountLoadResult::ProgramOfLoaderV3(data1, data2, slot)) => {
                assert_eq!(data1, account_data);
                assert_eq!(data2, account_data2);
                assert_eq!(slot, 25);
            }

            _ => panic!("Invalid result"),
        }
    }

    fn load_test_program() -> Vec<u8> {
        let mut dir = env::current_dir().unwrap();
        dir.push("tests");
        dir.push("example-programs");
        dir.push("hello-solana");
        dir.push("hello_solana_program.so");
        let mut file = File::open(dir.clone()).expect("file not found");
        let metadata = fs::metadata(dir).expect("Unable to read metadata");
        let mut buffer = vec![0; metadata.len() as usize];
        file.read_exact(&mut buffer).expect("Buffer overflow");
        buffer
    }

    #[test]
    fn test_load_program_from_bytes() {
        let buffer = load_test_program();

        let mut metrics = LoadProgramMetrics::default();
        let loader = bpf_loader_upgradeable::id();
        let size = buffer.len();
        let slot = 2;
        let environment = ProgramRuntimeEnvironment::new(BuiltinProgram::new_mock());

        let result = TransactionBatchProcessor::<TestForkGraph>::load_program_from_bytes(
            &mut metrics,
            &buffer,
            &loader,
            size,
            slot,
            environment.clone(),
            false,
        );

        assert!(result.is_ok());

        let result = TransactionBatchProcessor::<TestForkGraph>::load_program_from_bytes(
            &mut metrics,
            &buffer,
            &loader,
            size,
            slot,
            environment,
            true,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn test_load_program_not_found() {
        let mock_bank = MockBankCallback::default();
        let key = Pubkey::new_unique();
        let batch_processor = TransactionBatchProcessor::<TestForkGraph>::default();

        let result = batch_processor.load_program_with_pubkey(&mock_bank, &key, false, 50);
        assert!(result.is_none());
    }

    #[test]
    fn test_load_program_invalid_account_data() {
        let key = Pubkey::new_unique();
        let mock_bank = MockBankCallback::default();
        let mut account_data = AccountSharedData::default();
        account_data.set_owner(loader_v4::id());
        let batch_processor = TransactionBatchProcessor::<TestForkGraph>::default();
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key, account_data.clone());

        let result = batch_processor.load_program_with_pubkey(&mock_bank, &key, false, 20);

        let loaded_program = ProgramCacheEntry::new_tombstone(
            0,
            ProgramCacheEntryOwner::LoaderV4,
            ProgramCacheEntryType::FailedVerification(
                batch_processor
                    .program_cache
                    .read()
                    .unwrap()
                    .get_environments_for_epoch(20)
                    .clone()
                    .program_runtime_v1,
            ),
        );
        assert_eq!(result.unwrap(), Arc::new(loaded_program));
    }

    #[test]
    fn test_load_program_program_loader_v1_or_v2() {
        let key = Pubkey::new_unique();
        let mock_bank = MockBankCallback::default();
        let mut account_data = AccountSharedData::default();
        account_data.set_owner(bpf_loader::id());
        let batch_processor = TransactionBatchProcessor::<TestForkGraph>::default();
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key, account_data.clone());

        // This should return an error
        let result = batch_processor.load_program_with_pubkey(&mock_bank, &key, false, 20);
        let loaded_program = ProgramCacheEntry::new_tombstone(
            0,
            ProgramCacheEntryOwner::LoaderV2,
            ProgramCacheEntryType::FailedVerification(
                batch_processor
                    .program_cache
                    .read()
                    .unwrap()
                    .get_environments_for_epoch(20)
                    .clone()
                    .program_runtime_v1,
            ),
        );
        assert_eq!(result.unwrap(), Arc::new(loaded_program));

        let buffer = load_test_program();
        account_data.set_data(buffer);

        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key, account_data.clone());

        let result = batch_processor.load_program_with_pubkey(&mock_bank, &key, false, 20);

        let environments = ProgramRuntimeEnvironments::default();
        let expected = TransactionBatchProcessor::<TestForkGraph>::load_program_from_bytes(
            &mut LoadProgramMetrics::default(),
            account_data.data(),
            account_data.owner(),
            account_data.data().len(),
            0,
            environments.program_runtime_v1.clone(),
            false,
        );

        assert_eq!(result.unwrap(), Arc::new(expected.unwrap()));
    }

    #[test]
    fn test_load_program_program_loader_v3() {
        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        let mock_bank = MockBankCallback::default();
        let batch_processor = TransactionBatchProcessor::<TestForkGraph>::default();

        let mut account_data = AccountSharedData::default();
        account_data.set_owner(bpf_loader_upgradeable::id());

        let state = UpgradeableLoaderState::Program {
            programdata_address: key2,
        };
        account_data.set_data(bincode::serialize(&state).unwrap());
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key1, account_data.clone());

        let state = UpgradeableLoaderState::ProgramData {
            slot: 0,
            upgrade_authority_address: None,
        };
        let mut account_data2 = AccountSharedData::default();
        account_data2.set_data(bincode::serialize(&state).unwrap());
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key2, account_data2.clone());

        // This should return an error
        let result = batch_processor.load_program_with_pubkey(&mock_bank, &key1, false, 0);
        let loaded_program = ProgramCacheEntry::new_tombstone(
            0,
            ProgramCacheEntryOwner::LoaderV3,
            ProgramCacheEntryType::FailedVerification(
                batch_processor
                    .program_cache
                    .read()
                    .unwrap()
                    .get_environments_for_epoch(0)
                    .clone()
                    .program_runtime_v1,
            ),
        );
        assert_eq!(result.unwrap(), Arc::new(loaded_program));

        let mut buffer = load_test_program();
        let mut header = bincode::serialize(&state).unwrap();
        let mut complement = vec![
            0;
            std::cmp::max(
                0,
                UpgradeableLoaderState::size_of_programdata_metadata() - header.len()
            )
        ];
        header.append(&mut complement);
        header.append(&mut buffer);
        account_data.set_data(header);

        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key2, account_data.clone());

        let result = batch_processor.load_program_with_pubkey(&mock_bank, &key1, false, 20);

        let data = account_data.data();
        account_data
            .set_data(data[UpgradeableLoaderState::size_of_programdata_metadata()..].to_vec());

        let environments = ProgramRuntimeEnvironments::default();
        let expected = TransactionBatchProcessor::<TestForkGraph>::load_program_from_bytes(
            &mut LoadProgramMetrics::default(),
            account_data.data(),
            account_data.owner(),
            account_data.data().len(),
            0,
            environments.program_runtime_v1.clone(),
            false,
        );
        assert_eq!(result.unwrap(), Arc::new(expected.unwrap()));
    }

    #[test]
    fn test_load_program_of_loader_v4() {
        let key = Pubkey::new_unique();
        let mock_bank = MockBankCallback::default();
        let mut account_data = AccountSharedData::default();
        account_data.set_owner(loader_v4::id());
        let batch_processor = TransactionBatchProcessor::<TestForkGraph>::default();

        let loader_data = LoaderV4State {
            slot: 0,
            authority_address: Pubkey::new_unique(),
            status: LoaderV4Status::Deployed,
        };
        let encoded = unsafe {
            std::mem::transmute::<&LoaderV4State, &[u8; LoaderV4State::program_data_offset()]>(
                &loader_data,
            )
        };
        account_data.set_data(encoded.to_vec());
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key, account_data.clone());

        let result = batch_processor.load_program_with_pubkey(&mock_bank, &key, false, 0);
        let loaded_program = ProgramCacheEntry::new_tombstone(
            0,
            ProgramCacheEntryOwner::LoaderV4,
            ProgramCacheEntryType::FailedVerification(
                batch_processor
                    .program_cache
                    .read()
                    .unwrap()
                    .get_environments_for_epoch(0)
                    .clone()
                    .program_runtime_v1,
            ),
        );
        assert_eq!(result.unwrap(), Arc::new(loaded_program));

        let mut header = account_data.data().to_vec();
        let mut complement =
            vec![0; std::cmp::max(0, LoaderV4State::program_data_offset() - header.len())];
        header.append(&mut complement);

        let mut buffer = load_test_program();
        header.append(&mut buffer);

        account_data.set_data(header);
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key, account_data.clone());

        let result = batch_processor.load_program_with_pubkey(&mock_bank, &key, false, 20);

        let data = account_data.data()[LoaderV4State::program_data_offset()..].to_vec();
        account_data.set_data(data);
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key, account_data.clone());

        let environments = ProgramRuntimeEnvironments::default();
        let expected = TransactionBatchProcessor::<TestForkGraph>::load_program_from_bytes(
            &mut LoadProgramMetrics::default(),
            account_data.data(),
            account_data.owner(),
            account_data.data().len(),
            0,
            environments.program_runtime_v1.clone(),
            false,
        );
        assert_eq!(result.unwrap(), Arc::new(expected.unwrap()));
    }

    #[test]
    fn test_load_program_effective_slot() {
        let key = Pubkey::new_unique();
        let mock_bank = MockBankCallback::default();
        let mut account_data = AccountSharedData::default();
        account_data.set_owner(loader_v4::id());
        let batch_processor = TransactionBatchProcessor::<TestForkGraph>::default();

        batch_processor
            .program_cache
            .write()
            .unwrap()
            .upcoming_environments = Some(ProgramRuntimeEnvironments::default());
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key, account_data.clone());

        let result = batch_processor.load_program_with_pubkey(&mock_bank, &key, false, 20);

        let slot = batch_processor.epoch_schedule.get_first_slot_in_epoch(20);
        assert_eq!(result.unwrap().effective_slot, slot);
    }

    #[test]
    fn test_program_modification_slot_account_not_found() {
        let batch_processor = TransactionBatchProcessor::<TestForkGraph>::default();
        let mock_bank = MockBankCallback::default();
        let key = Pubkey::new_unique();

        let result = batch_processor.program_modification_slot(&mock_bank, &key);
        assert_eq!(result.err(), Some(TransactionError::ProgramAccountNotFound));

        let mut account_data = AccountSharedData::default();
        account_data.set_owner(bpf_loader_upgradeable::id());
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key, account_data.clone());

        let result = batch_processor.program_modification_slot(&mock_bank, &key);
        assert_eq!(result.err(), Some(TransactionError::ProgramAccountNotFound));

        let state = UpgradeableLoaderState::Program {
            programdata_address: Pubkey::new_unique(),
        };
        account_data.set_data(bincode::serialize(&state).unwrap());
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key, account_data.clone());

        let result = batch_processor.program_modification_slot(&mock_bank, &key);
        assert_eq!(result.err(), Some(TransactionError::ProgramAccountNotFound));

        account_data.set_owner(loader_v4::id());
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key, account_data.clone());

        let result = batch_processor.program_modification_slot(&mock_bank, &key);
        assert_eq!(result.err(), Some(TransactionError::ProgramAccountNotFound));
    }

    #[test]
    fn test_program_modification_slot_success() {
        let batch_processor = TransactionBatchProcessor::<TestForkGraph>::default();
        let mock_bank = MockBankCallback::default();
        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        let mut account_data = AccountSharedData::default();
        account_data.set_owner(bpf_loader_upgradeable::id());

        let state = UpgradeableLoaderState::Program {
            programdata_address: key2,
        };
        account_data.set_data(bincode::serialize(&state).unwrap());
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key1, account_data);

        let state = UpgradeableLoaderState::ProgramData {
            slot: 77,
            upgrade_authority_address: None,
        };
        let mut account_data = AccountSharedData::default();
        account_data.set_data(bincode::serialize(&state).unwrap());
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key2, account_data);

        let result = batch_processor.program_modification_slot(&mock_bank, &key1);
        assert_eq!(result.unwrap(), 77);

        let mut account_data = AccountSharedData::default();
        account_data.set_owner(loader_v4::id());
        let state = LoaderV4State {
            slot: 58,
            authority_address: Pubkey::new_unique(),
            status: LoaderV4Status::Deployed,
        };

        let encoded = unsafe {
            std::mem::transmute::<&LoaderV4State, &[u8; LoaderV4State::program_data_offset()]>(
                &state,
            )
        };
        account_data.set_data(encoded.to_vec());
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key1, account_data.clone());

        let result = batch_processor.program_modification_slot(&mock_bank, &key1);
        assert_eq!(result.unwrap(), 58);

        account_data.set_owner(Pubkey::new_unique());
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(key2, account_data);

        let result = batch_processor.program_modification_slot(&mock_bank, &key2);
        assert_eq!(result.unwrap(), 0);
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
        let loaded_programs = ProgramCacheForTxBatch::default();
        let mock_bank = MockBankCallback::default();
        let batch_processor = TransactionBatchProcessor::<TestForkGraph>::default();

        let sanitized_transaction = SanitizedTransaction::new_for_tests(
            sanitized_message,
            vec![Signature::new_unique()],
            false,
        );

        let mut loaded_transaction = LoadedTransaction {
            accounts: vec![(Pubkey::new_unique(), AccountSharedData::default())],
            program_indices: vec![vec![0]],
            rent: 0,
            rent_debits: RentDebits::default(),
        };

        let mut record_config = ExecutionRecordingConfig {
            enable_cpi_recording: false,
            enable_log_recording: true,
            enable_return_data_recording: false,
        };

        let result = batch_processor.execute_loaded_transaction(
            &mock_bank,
            &sanitized_transaction,
            &mut loaded_transaction,
            ComputeBudget::default(),
            None,
            record_config,
            &mut ExecuteTimings::default(),
            &mut TransactionErrorMetrics::default(),
            None,
            &loaded_programs,
        );

        let TransactionExecutionResult::Executed {
            details: TransactionExecutionDetails { log_messages, .. },
            ..
        } = result
        else {
            panic!("Unexpected result")
        };
        assert!(log_messages.is_some());

        let result = batch_processor.execute_loaded_transaction(
            &mock_bank,
            &sanitized_transaction,
            &mut loaded_transaction,
            ComputeBudget::default(),
            None,
            record_config,
            &mut ExecuteTimings::default(),
            &mut TransactionErrorMetrics::default(),
            Some(2),
            &loaded_programs,
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

        record_config.enable_log_recording = false;
        record_config.enable_cpi_recording = true;

        let result = batch_processor.execute_loaded_transaction(
            &mock_bank,
            &sanitized_transaction,
            &mut loaded_transaction,
            ComputeBudget::default(),
            None,
            record_config,
            &mut ExecuteTimings::default(),
            &mut TransactionErrorMetrics::default(),
            None,
            &loaded_programs,
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
        let loaded_programs = ProgramCacheForTxBatch::default();
        let mock_bank = MockBankCallback::default();
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
            rent: 0,
            rent_debits: RentDebits::default(),
        };

        let record_config = ExecutionRecordingConfig::new_single_setting(false);
        let mut error_metrics = TransactionErrorMetrics::new();

        let _ = batch_processor.execute_loaded_transaction(
            &mock_bank,
            &sanitized_transaction,
            &mut loaded_transaction,
            ComputeBudget::default(),
            None,
            record_config,
            &mut ExecuteTimings::default(),
            &mut error_metrics,
            None,
            &loaded_programs,
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

        batch_processor.replenish_program_cache(&mock_bank, &account_maps, true);
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
            .borrow_mut()
            .insert(key, account_data);

        let mut account_maps: HashMap<Pubkey, u64> = HashMap::new();
        account_maps.insert(key, 4);

        for limit_to_load_programs in [false, true] {
            let result = batch_processor.replenish_program_cache(
                &mock_bank,
                &account_maps,
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
            .borrow_mut()
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
            .borrow_mut()
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
            sanitized_transaction_2,
            sanitized_transaction_1,
        ];
        let mut lock_results = vec![
            (Ok(()), None, Some(25)),
            (Ok(()), None, Some(25)),
            (Ok(()), None, None),
            (Err(TransactionError::ProgramAccountNotFound), None, None),
        ];
        let owners = vec![owner1, owner2];

        let result = TransactionBatchProcessor::<TestForkGraph>::filter_executable_program_accounts(
            &mock_bank,
            &transactions,
            lock_results.as_mut_slice(),
            &owners,
        );

        assert_eq!(
            lock_results[2],
            (Err(TransactionError::BlockhashNotFound), None, None)
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
        bank.account_shared_data.borrow_mut().insert(
            non_program_pubkey1,
            AccountSharedData::new(1, 10, &account5_pubkey),
        );
        bank.account_shared_data.borrow_mut().insert(
            non_program_pubkey2,
            AccountSharedData::new(1, 10, &account5_pubkey),
        );
        bank.account_shared_data.borrow_mut().insert(
            program1_pubkey,
            AccountSharedData::new(40, 1, &account5_pubkey),
        );
        bank.account_shared_data.borrow_mut().insert(
            program2_pubkey,
            AccountSharedData::new(40, 1, &account5_pubkey),
        );
        bank.account_shared_data.borrow_mut().insert(
            account1_pubkey,
            AccountSharedData::new(1, 10, &non_program_pubkey1),
        );
        bank.account_shared_data.borrow_mut().insert(
            account2_pubkey,
            AccountSharedData::new(1, 10, &non_program_pubkey2),
        );
        bank.account_shared_data.borrow_mut().insert(
            account3_pubkey,
            AccountSharedData::new(40, 1, &program1_pubkey),
        );
        bank.account_shared_data.borrow_mut().insert(
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
                &mut [(Ok(()), None, Some(0)), (Ok(()), None, Some(0))],
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
        bank.account_shared_data.borrow_mut().insert(
            non_program_pubkey1,
            AccountSharedData::new(1, 10, &account5_pubkey),
        );
        bank.account_shared_data.borrow_mut().insert(
            non_program_pubkey2,
            AccountSharedData::new(1, 10, &account5_pubkey),
        );
        bank.account_shared_data.borrow_mut().insert(
            program1_pubkey,
            AccountSharedData::new(40, 1, &account5_pubkey),
        );
        bank.account_shared_data.borrow_mut().insert(
            program2_pubkey,
            AccountSharedData::new(40, 1, &account5_pubkey),
        );
        bank.account_shared_data.borrow_mut().insert(
            account1_pubkey,
            AccountSharedData::new(1, 10, &non_program_pubkey1),
        );
        bank.account_shared_data.borrow_mut().insert(
            account2_pubkey,
            AccountSharedData::new(1, 10, &non_program_pubkey2),
        );
        bank.account_shared_data.borrow_mut().insert(
            account3_pubkey,
            AccountSharedData::new(40, 1, &program1_pubkey),
        );
        bank.account_shared_data.borrow_mut().insert(
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
        let mut lock_results = vec![(Ok(()), None, Some(0)), (Ok(()), None, None)];
        let programs =
            TransactionBatchProcessor::<TestForkGraph>::filter_executable_program_accounts(
                &bank,
                &[sanitized_tx1, sanitized_tx2],
                &mut lock_results,
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
        assert_eq!(lock_results[1].0, Err(TransactionError::BlockhashNotFound));
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
            .borrow_mut()
            .insert(sysvar::clock::id(), clock_account);

        let epoch_schedule = EpochSchedule::custom(64, 2, true);
        let epoch_schedule_account = create_account_shared_data_for_test(&epoch_schedule);
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(sysvar::epoch_schedule::id(), epoch_schedule_account);

        let fees = sysvar::fees::Fees {
            fee_calculator: FeeCalculator {
                lamports_per_signature: 123,
            },
        };
        let fees_account = create_account_shared_data_for_test(&fees);
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(sysvar::fees::id(), fees_account);

        let rent = Rent::with_slots_per_epoch(2048);
        let rent_account = create_account_shared_data_for_test(&rent);
        mock_bank
            .account_shared_data
            .borrow_mut()
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
            .borrow_mut()
            .insert(sysvar::clock::id(), clock_account);

        let epoch_schedule = EpochSchedule::custom(64, 2, true);
        let epoch_schedule_account = create_account_shared_data_for_test(&epoch_schedule);
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(sysvar::epoch_schedule::id(), epoch_schedule_account);

        let fees = sysvar::fees::Fees {
            fee_calculator: FeeCalculator {
                lamports_per_signature: 123,
            },
        };
        let fees_account = create_account_shared_data_for_test(&fees);
        mock_bank
            .account_shared_data
            .borrow_mut()
            .insert(sysvar::fees::id(), fees_account);

        let rent = Rent::with_slots_per_epoch(2048);
        let rent_account = create_account_shared_data_for_test(&rent);
        mock_bank
            .account_shared_data
            .borrow_mut()
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
            mock_bank.account_shared_data.borrow()[&key].data(),
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
}

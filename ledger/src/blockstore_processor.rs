use {
    crate::{
        block_error::BlockError,
        blockstore::Blockstore,
        blockstore_db::BlockstoreError,
        blockstore_meta::SlotMeta,
        leader_schedule_cache::LeaderScheduleCache,
        replayer::{ProcessCallback, ReplayRequest, ReplayResponse, Replayer, ReplayerHandle},
        scheduler::build_dependency_graphs,
    },
    chrono_humanize::{Accuracy, HumanTime, Tense},
    crossbeam_channel::{unbounded, Sender},
    itertools::Itertools,
    log::*,
    rayon::ThreadPool,
    solana_entry::entry::{
        self, create_ticks, Entry, EntrySlice, EntryType, EntryVerificationStatus, VerifyRecyclers,
    },
    solana_measure::measure::Measure,
    solana_program_runtime::timings::ExecuteTimings,
    solana_rayon_threadlimit::get_thread_count,
    solana_runtime::{
        accounts_background_service::DroppedSlotsReceiver,
        accounts_db::{AccountShrinkThreshold, AccountsDbConfig},
        accounts_index::AccountSecondaryIndexes,
        accounts_update_notifier_interface::AccountsUpdateNotifier,
        bank::{
            Bank, RentDebits, TransactionBalancesSet, TransactionExecutionDetails,
            TransactionExecutionResult,
        },
        bank_forks::BankForks,
        block_cost_limits::*,
        commitment::VOTE_THRESHOLD_SIZE,
        snapshot_config::SnapshotConfig,
        snapshot_package::{AccountsPackageSender, SnapshotType},
        snapshot_utils,
        transaction_cost_metrics_sender::TransactionCostMetricsSender,
        vote_account::VoteAccount,
        vote_sender_types::ReplayVoteSender,
    },
    solana_sdk::{
        clock::Slot,
        genesis_config::GenesisConfig,
        hash::Hash,
        pubkey::Pubkey,
        signature::Keypair,
        timing,
        transaction::{
            Result, SanitizedTransaction, TransactionError, TransactionVerificationMode,
            VersionedTransaction,
        },
    },
    solana_transaction_status::token_balances::TransactionTokenBalancesSet,
    std::{
        cell::RefCell,
        collections::{HashMap, HashSet},
        path::PathBuf,
        result,
        sync::{Arc, RwLock},
        time::{Duration, Instant},
    },
    thiserror::Error,
};

// it tracks the block cost available capacity - number of compute-units allowed
// by max block cost limit
#[derive(Debug)]
pub struct BlockCostCapacityMeter {
    pub capacity: u64,
    pub accumulated_cost: u64,
}

impl Default for BlockCostCapacityMeter {
    fn default() -> Self {
        BlockCostCapacityMeter::new(MAX_BLOCK_UNITS)
    }
}

impl BlockCostCapacityMeter {
    pub fn new(capacity_limit: u64) -> Self {
        Self {
            capacity: capacity_limit,
            accumulated_cost: 0_u64,
        }
    }

    // return the remaining capacity
    pub fn accumulate(&mut self, cost: u64) -> u64 {
        self.accumulated_cost += cost;
        self.capacity.saturating_sub(self.accumulated_cost)
    }
}

thread_local!(static PAR_THREAD_POOL: RefCell<ThreadPool> = RefCell::new(rayon::ThreadPoolBuilder::new()
                    .num_threads(get_thread_count())
                    .thread_name(|ix| format!("blockstore_processor_{}", ix))
                    .build()
                    .unwrap())
);

fn first_err(results: &[Result<()>]) -> Result<()> {
    for r in results {
        if r.is_err() {
            return r.clone();
        }
    }
    Ok(())
}
//
// // Includes transaction signature for unit-testing
// fn get_first_error(
//     batch: &TransactionBatch,
//     fee_collection_results: Vec<Result<()>>,
// ) -> Option<(Result<()>, Signature)> {
//     let mut first_err = None;
//     for (result, transaction) in fee_collection_results
//         .iter()
//         .zip(batch.sanitized_transactions())
//     {
//         if let Err(ref err) = result {
//             if first_err.is_none() {
//                 first_err = Some((result.clone(), *transaction.signature()));
//             }
//             warn!(
//                 "Unexpected validator error: {:?}, transaction: {:?}",
//                 err, transaction
//             );
//             datapoint_error!(
//                 "validator_process_entry_error",
//                 (
//                     "error",
//                     format!("error: {:?}, transaction: {:?}", err, transaction),
//                     String
//                 )
//             );
//         }
//     }
//     first_err
// }

fn execute_batches(
    bank: &Arc<Bank>,
    transactions: &[SanitizedTransaction],
    entry_callback: Option<&ProcessCallback>,
    transaction_status_sender: Option<&TransactionStatusSender>,
    replay_vote_sender: Option<&ReplayVoteSender>,
    timings: &mut ExecuteTimings,
    cost_capacity_meter: Arc<RwLock<BlockCostCapacityMeter>>,
    replayer_handle: &ReplayerHandle,
) -> Result<()> {
    let tx_account_locks_results: Vec<Result<_>> = transactions
        .iter()
        .map(|tx| tx.get_account_locks(&bank.feature_set))
        .collect();

    let now = Instant::now();
    let dependency_graph = build_dependency_graphs(&tx_account_locks_results)?;
    let dependency_graph_elapsed = now.elapsed();
    debug!(
        "slot: {:?} txs: {:?} dependency_graph_elapsed: {:?}",
        bank.slot(),
        transactions.len(),
        dependency_graph_elapsed,
    );
    timings.planning_elapsed += now.elapsed().as_micros() as u64;

    let mut num_left_to_process = transactions.len();
    let mut is_processed: Vec<bool> = vec![false; transactions.len()];
    let mut is_processing: Vec<bool> = vec![false; transactions.len()];

    // first batch that gets executed has no dependencies
    let mut indices_need_scheduling: Vec<usize> = dependency_graph
        .iter()
        .enumerate()
        .filter_map(|(idx, indices)| indices.is_empty().then(|| idx))
        .collect();

    while num_left_to_process > 0 {
        // send them to get executed
        debug!("sending {} to get scheduled", indices_need_scheduling.len());

        for idx in &indices_need_scheduling {
            replayer_handle
                .send(ReplayRequest {
                    bank: bank.clone(),
                    tx: transactions[*idx].clone(),
                    transaction_status_sender: transaction_status_sender.cloned(),
                    replay_vote_sender: replay_vote_sender.cloned(),
                    cost_capacity_meter: cost_capacity_meter.clone(),
                    entry_callback: entry_callback.cloned(),
                    idx: Some(*idx),
                })
                .unwrap();
            is_processing[*idx] = true;
        }

        while num_left_to_process > 0 {
            debug!(
                "waiting for results num_processed: {:?} num_processing: {:?} num_left_to_process: {:?}",
                is_processed.iter().map(|p| if *p { 1 } else { 0 }).sum::<usize>(),
                is_processing.iter().map(|p| if *p { 1 } else { 0 }).sum::<usize>(),
                num_left_to_process
            );
            let results = replayer_handle.recv_and_drain().unwrap();
            debug!("got {} results", results.len());

            for ReplayResponse {
                result,
                timing,
                idx,
            } in results
            {
                timings.accumulate(&timing);
                let idx = idx.unwrap();
                is_processed[idx] = true;
                is_processing[idx] = false;
                num_left_to_process -= 1;

                result?;
            }

            indices_need_scheduling = dependency_graph
                .iter()
                .enumerate()
                .filter_map(|(i, dependencies)| {
                    if !is_processed[i]
                        && !is_processing[i]
                        && dependencies
                            .iter()
                            .all(|dependency_idx| is_processed[*dependency_idx])
                    {
                        Some(i)
                    } else {
                        None
                    }
                })
                .collect();
            if !indices_need_scheduling.is_empty() {
                debug!(
                    "more ready to be scheduled: {:?}",
                    indices_need_scheduling.len()
                );
                break;
            }
        }
    }

    info!("replay time: {:?}", now.elapsed());

    Ok(())
}

/// Process an ordered list of entries in parallel
/// 1. In order lock accounts for each entry while the lock succeeds, up to a Tick entry
/// 2. Process the locked group in parallel
/// 3. Register the `Tick` if it's available
/// 4. Update the leader scheduler, goto 1
pub fn process_entries_for_tests(
    bank: &Arc<Bank>,
    entries: Vec<Entry>,
    randomize: bool,
    transaction_status_sender: Option<&TransactionStatusSender>,
    replay_vote_sender: Option<&ReplayVoteSender>,
    replayer_handle: &ReplayerHandle,
) -> Result<()> {
    let verify_transaction = {
        let bank = bank.clone();
        move |versioned_tx: VersionedTransaction| -> Result<SanitizedTransaction> {
            bank.verify_transaction(versioned_tx, TransactionVerificationMode::FullVerification)
        }
    };

    let mut timings = ExecuteTimings::default();
    let mut entries = entry::verify_transactions(entries, Arc::new(verify_transaction))?;
    let result = process_entries_with_callback(
        bank,
        &mut entries,
        randomize,
        None,
        transaction_status_sender,
        replay_vote_sender,
        None,
        &mut timings,
        Arc::new(RwLock::new(BlockCostCapacityMeter::default())),
        replayer_handle,
    );

    debug!("process_entries: {:?}", timings);
    result
}

// Note: If randomize is true this will shuffle entries' transactions in-place.
fn process_entries_with_callback(
    bank: &Arc<Bank>,
    entries: &[EntryType],
    _randomize: bool,
    entry_callback: Option<&ProcessCallback>,
    transaction_status_sender: Option<&TransactionStatusSender>,
    replay_vote_sender: Option<&ReplayVoteSender>,
    transaction_cost_metrics_sender: Option<&TransactionCostMetricsSender>,
    timings: &mut ExecuteTimings,
    cost_capacity_meter: Arc<RwLock<BlockCostCapacityMeter>>,
    replayer_handle: &ReplayerHandle,
) -> Result<()> {
    let mut executable_txs = vec![];
    let mut tick_hashes = vec![];
    // let mut rng = thread_rng();

    for entry in entries {
        match entry {
            EntryType::Tick(hash) => {
                // If it's a tick, save it for later
                tick_hashes.push(hash);
                if bank.is_block_boundary(bank.tick_height() + tick_hashes.len() as u64) {
                    // If it's a tick that will cause a new blockhash to be created,
                    // execute the group and register the tick
                    execute_batches(
                        bank,
                        &executable_txs,
                        entry_callback,
                        transaction_status_sender,
                        replay_vote_sender,
                        timings,
                        cost_capacity_meter.clone(),
                        replayer_handle,
                    )?;
                    executable_txs.clear();
                    for hash in &tick_hashes {
                        bank.register_tick(hash);
                    }
                    tick_hashes.clear();
                }
            }
            EntryType::Transactions(transactions) => {
                if let Some(transaction_cost_metrics_sender) = transaction_cost_metrics_sender {
                    transaction_cost_metrics_sender
                        .send_cost_details(bank.clone(), transactions.iter());
                }

                // TODO (LB): add randomization back in
                // TODO (LB): add back in lock check here.
                // TODO (LB): avoid clone
                executable_txs.extend(transactions.clone());
            }
        }
    }
    execute_batches(
        bank,
        &executable_txs,
        entry_callback,
        transaction_status_sender,
        replay_vote_sender,
        timings,
        cost_capacity_meter,
        replayer_handle,
    )?;
    for hash in tick_hashes {
        bank.register_tick(hash);
    }
    Ok(())
}

#[derive(Error, Debug)]
pub enum BlockstoreProcessorError {
    #[error("failed to load entries, error: {0}")]
    FailedToLoadEntries(#[from] BlockstoreError),

    #[error("failed to load meta")]
    FailedToLoadMeta,

    #[error("invalid block error: {0}")]
    InvalidBlock(#[from] BlockError),

    #[error("invalid transaction error: {0}")]
    InvalidTransaction(#[from] TransactionError),

    #[error("no valid forks found")]
    NoValidForksFound,

    #[error("invalid hard fork slot {0}")]
    InvalidHardFork(Slot),

    #[error("root bank with mismatched capitalization at {0}")]
    RootBankWithMismatchedCapitalization(Slot),
}

#[derive(Default, Clone)]
pub struct ProcessOptions {
    pub bpf_jit: bool,
    pub poh_verify: bool,
    pub full_leader_cache: bool,
    pub dev_halt_at_slot: Option<Slot>,
    pub entry_callback: Option<ProcessCallback>,
    pub override_num_threads: Option<usize>,
    pub new_hard_forks: Option<Vec<Slot>>,
    pub debug_keys: Option<Arc<HashSet<Pubkey>>>,
    pub account_indexes: AccountSecondaryIndexes,
    pub accounts_db_caching_enabled: bool,
    pub limit_load_slot_count_from_snapshot: Option<usize>,
    pub allow_dead_slots: bool,
    pub accounts_db_test_hash_calculation: bool,
    pub accounts_db_skip_shrink: bool,
    pub accounts_db_config: Option<AccountsDbConfig>,
    pub verify_index: bool,
    pub shrink_ratio: AccountShrinkThreshold,
}

pub fn test_process_blockstore(
    genesis_config: &GenesisConfig,
    blockstore: &Blockstore,
    opts: ProcessOptions,
) -> (BankForks, LeaderScheduleCache) {
    let (mut bank_forks, leader_schedule_cache, .., pruned_banks_receiver) =
        crate::bank_forks_utils::load_bank_forks(
            genesis_config,
            blockstore,
            Vec::new(),
            None,
            None,
            &opts,
            None,
            None,
        );
    let (accounts_package_sender, _) = unbounded();
    process_blockstore_from_root(
        blockstore,
        &mut bank_forks,
        &leader_schedule_cache,
        &opts,
        None,
        None,
        None,
        accounts_package_sender,
        pruned_banks_receiver,
    )
    .unwrap();
    (bank_forks, leader_schedule_cache)
}

pub(crate) fn process_blockstore_for_bank_0(
    genesis_config: &GenesisConfig,
    blockstore: &Blockstore,
    account_paths: Vec<PathBuf>,
    opts: &ProcessOptions,
    cache_block_meta_sender: Option<&CacheBlockMetaSender>,
    accounts_update_notifier: Option<AccountsUpdateNotifier>,
    replayer_handle: &ReplayerHandle,
) -> BankForks {
    // Setup bank for slot 0
    let bank0 = Bank::new_with_paths(
        genesis_config,
        account_paths,
        opts.debug_keys.clone(),
        Some(&crate::builtins::get(opts.bpf_jit)),
        opts.account_indexes.clone(),
        opts.accounts_db_caching_enabled,
        opts.shrink_ratio,
        false,
        opts.accounts_db_config.clone(),
        accounts_update_notifier,
    );
    let bank_forks = BankForks::new(bank0);

    info!("processing ledger for slot 0...");
    process_bank_0(
        &bank_forks.root_bank(),
        blockstore,
        opts,
        &VerifyRecyclers::default(),
        cache_block_meta_sender,
        replayer_handle,
    );
    bank_forks
}

/// Process blockstore from a known root bank
#[allow(clippy::too_many_arguments)]
pub fn process_blockstore_from_root(
    blockstore: &Blockstore,
    bank_forks: &mut BankForks,
    leader_schedule_cache: &LeaderScheduleCache,
    opts: &ProcessOptions,
    transaction_status_sender: Option<&TransactionStatusSender>,
    cache_block_meta_sender: Option<&CacheBlockMetaSender>,
    snapshot_config: Option<&SnapshotConfig>,
    accounts_package_sender: AccountsPackageSender,
    pruned_banks_receiver: DroppedSlotsReceiver,
) -> result::Result<Option<Slot>, BlockstoreProcessorError> {
    if let Some(num_threads) = opts.override_num_threads {
        PAR_THREAD_POOL.with(|pool| {
            *pool.borrow_mut() = rayon::ThreadPoolBuilder::new()
                .num_threads(num_threads)
                .build()
                .unwrap()
        });
    }

    // Starting slot must be a root, and thus has no parents
    assert_eq!(bank_forks.banks().len(), 1);
    let bank = bank_forks.root_bank();
    assert!(bank.parent().is_none());

    let start_slot = bank.slot();
    info!("processing ledger from slot {}...", start_slot);
    let now = Instant::now();

    // ensure start_slot is rooted for correct replay
    if blockstore.is_primary_access() {
        blockstore
            .set_roots(std::iter::once(&start_slot))
            .expect("Couldn't set root slot on startup");
    } else {
        assert!(
            blockstore.is_root(start_slot),
            "starting slot isn't root and can't update due to being secondary blockstore access: {}", start_slot
        );
    }

    if let Ok(metas) = blockstore.slot_meta_iterator(start_slot) {
        if let Some((slot, _meta)) = metas.last() {
            info!("ledger holds data through slot {}", slot);
        }
    }

    let mut timing = ExecuteTimings::default();
    // Iterate and replay slots from blockstore starting from `start_slot`

    let mut last_full_snapshot_slot = None;

    if let Some(start_slot_meta) = blockstore
        .meta(start_slot)
        .unwrap_or_else(|_| panic!("Failed to get meta for slot {}", start_slot))
    {
        load_frozen_forks(
            bank_forks,
            start_slot,
            &start_slot_meta,
            blockstore,
            leader_schedule_cache,
            opts,
            transaction_status_sender,
            cache_block_meta_sender,
            snapshot_config,
            accounts_package_sender,
            &mut timing,
            &mut last_full_snapshot_slot,
            pruned_banks_receiver,
        )?;
    } else {
        // If there's no meta for the input `start_slot`, then we started from a snapshot
        // and there's no point in processing the rest of blockstore and implies blockstore
        // should be empty past this point.
    };

    let processing_time = now.elapsed();

    let debug_verify = opts.accounts_db_test_hash_calculation;
    let mut time_cap = Measure::start("capitalization");
    // We might be promptly restarted after bad capitalization was detected while creating newer snapshot.
    // In that case, we're most likely restored from the last good snapshot and replayed up to this root.
    // So again check here for the bad capitalization to avoid to continue until the next snapshot creation.
    if !bank_forks
        .root_bank()
        .calculate_and_verify_capitalization(debug_verify)
    {
        return Err(
            BlockstoreProcessorError::RootBankWithMismatchedCapitalization(bank_forks.root()),
        );
    }
    time_cap.stop();

    datapoint_info!(
        "process_blockstore_from_root",
        ("total_time_us", processing_time.as_micros(), i64),
        ("frozen_banks", bank_forks.frozen_banks().len(), i64),
        ("slot", bank_forks.root(), i64),
        ("forks", bank_forks.banks().len(), i64),
        ("calculate_capitalization_us", time_cap.as_us(), i64),
    );

    info!("ledger processing timing: {:?}", timing);
    let mut bank_slots = bank_forks.banks().keys().copied().collect::<Vec<_>>();
    bank_slots.sort_unstable();

    info!(
        "ledger processed in {}. root slot is {}, {} bank{}: {}",
        HumanTime::from(chrono::Duration::from_std(processing_time).unwrap())
            .to_text_en(Accuracy::Precise, Tense::Present),
        bank_forks.root(),
        bank_slots.len(),
        if bank_slots.len() > 1 { "s" } else { "" },
        bank_slots.iter().map(|slot| slot.to_string()).join(", "),
    );
    assert!(bank_forks.active_banks().is_empty());

    Ok(last_full_snapshot_slot)
}

/// Verify that a segment of entries has the correct number of ticks and hashes
fn verify_ticks(
    bank: &Bank,
    entries: &[Entry],
    slot_full: bool,
    tick_hash_count: &mut u64,
) -> std::result::Result<(), BlockError> {
    let next_bank_tick_height = bank.tick_height() + entries.tick_count();
    let max_bank_tick_height = bank.max_tick_height();

    if next_bank_tick_height > max_bank_tick_height {
        warn!("Too many entry ticks found in slot: {}", bank.slot());
        return Err(BlockError::TooManyTicks);
    }

    if next_bank_tick_height < max_bank_tick_height && slot_full {
        info!("Too few entry ticks found in slot: {}", bank.slot());
        return Err(BlockError::TooFewTicks);
    }

    if next_bank_tick_height == max_bank_tick_height {
        let has_trailing_entry = entries.last().map(|e| !e.is_tick()).unwrap_or_default();
        if has_trailing_entry {
            warn!("Slot: {} did not end with a tick entry", bank.slot());
            return Err(BlockError::TrailingEntry);
        }

        if !slot_full {
            warn!("Slot: {} was not marked full", bank.slot());
            return Err(BlockError::InvalidLastTick);
        }
    }

    let hashes_per_tick = bank.hashes_per_tick().unwrap_or(0);
    if !entries.verify_tick_hash_count(tick_hash_count, hashes_per_tick) {
        warn!(
            "Tick with invalid number of hashes found in slot: {}",
            bank.slot()
        );
        return Err(BlockError::InvalidTickHashCount);
    }

    Ok(())
}

fn confirm_full_slot(
    blockstore: &Blockstore,
    bank: &Arc<Bank>,
    opts: &ProcessOptions,
    recyclers: &VerifyRecyclers,
    progress: &mut ConfirmationProgress,
    transaction_status_sender: Option<&TransactionStatusSender>,
    replay_vote_sender: Option<&ReplayVoteSender>,
    timing: &mut ExecuteTimings,
    replayer_handle: &ReplayerHandle,
) -> result::Result<(), BlockstoreProcessorError> {
    let mut confirmation_timing = ConfirmationTiming::default();
    let skip_verification = !opts.poh_verify;
    confirm_slot(
        blockstore,
        bank,
        &mut confirmation_timing,
        progress,
        skip_verification,
        transaction_status_sender,
        replay_vote_sender,
        None,
        opts.entry_callback.as_ref(),
        recyclers,
        opts.allow_dead_slots,
        replayer_handle,
    )?;

    timing.accumulate(&confirmation_timing.execute_timings);

    if !bank.is_complete() {
        Err(BlockstoreProcessorError::InvalidBlock(
            BlockError::Incomplete,
        ))
    } else {
        Ok(())
    }
}

pub struct ConfirmationTiming {
    pub started: Instant,
    pub replay_elapsed: u64,
    pub poh_verify_elapsed: u64,
    pub transaction_verify_elapsed: u64,
    pub fetch_elapsed: u64,
    pub fetch_fail_elapsed: u64,
    pub execute_timings: ExecuteTimings,
    pub planning_elapsed: u64,
}

impl Default for ConfirmationTiming {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            replay_elapsed: 0,
            poh_verify_elapsed: 0,
            transaction_verify_elapsed: 0,
            fetch_elapsed: 0,
            fetch_fail_elapsed: 0,
            execute_timings: ExecuteTimings::default(),
            planning_elapsed: 0,
        }
    }
}

#[derive(Default)]
pub struct ConfirmationProgress {
    pub last_entry: Hash,
    pub tick_hash_count: u64,
    pub num_shreds: u64,
    pub num_entries: usize,
    pub num_txs: usize,
}

impl ConfirmationProgress {
    pub fn new(last_entry: Hash) -> Self {
        Self {
            last_entry,
            ..Self::default()
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn confirm_slot(
    blockstore: &Blockstore,
    bank: &Arc<Bank>,
    timing: &mut ConfirmationTiming,
    progress: &mut ConfirmationProgress,
    skip_verification: bool,
    transaction_status_sender: Option<&TransactionStatusSender>,
    replay_vote_sender: Option<&ReplayVoteSender>,
    transaction_cost_metrics_sender: Option<&TransactionCostMetricsSender>,
    entry_callback: Option<&ProcessCallback>,
    recyclers: &VerifyRecyclers,
    allow_dead_slots: bool,
    replayer_handle: &ReplayerHandle,
) -> result::Result<(), BlockstoreProcessorError> {
    let slot = bank.slot();

    let (entries, num_shreds, slot_full) = {
        let mut load_elapsed = Measure::start("load_elapsed");
        let load_result = blockstore
            .get_slot_entries_with_shred_info(slot, progress.num_shreds, allow_dead_slots)
            .map_err(BlockstoreProcessorError::FailedToLoadEntries);
        load_elapsed.stop();
        if load_result.is_err() {
            timing.fetch_fail_elapsed += load_elapsed.as_us();
        } else {
            timing.fetch_elapsed += load_elapsed.as_us();
        }
        load_result
    }?;

    let num_entries = entries.len();
    let num_txs = entries.iter().map(|e| e.transactions.len()).sum::<usize>();
    trace!(
        "Fetched entries for slot {}, num_entries: {}, num_shreds: {}, num_txs: {}, slot_full: {}",
        slot,
        num_entries,
        num_shreds,
        num_txs,
        slot_full,
    );

    if !skip_verification {
        let tick_hash_count = &mut progress.tick_hash_count;
        verify_ticks(bank, &entries, slot_full, tick_hash_count).map_err(|err| {
            warn!(
                "{:#?}, slot: {}, entry len: {}, tick_height: {}, last entry: {}, last_blockhash: {}, shred_index: {}, slot_full: {}",
                err,
                slot,
                num_entries,
                bank.tick_height(),
                progress.last_entry,
                bank.last_blockhash(),
                num_shreds,
                slot_full,
            );
            err
        })?;
    }

    let last_entry_hash = entries.last().map(|e| e.hash);
    let verifier = if !skip_verification {
        datapoint_debug!("verify-batch-size", ("size", num_entries as i64, i64));
        let entry_state = entries.start_verify(&progress.last_entry, recyclers.clone());
        if entry_state.status() == EntryVerificationStatus::Failure {
            warn!("Ledger proof of history failed at slot: {}", slot);
            return Err(BlockError::InvalidEntryHash.into());
        }
        Some(entry_state)
    } else {
        None
    };

    let verify_transaction = {
        let bank = bank.clone();
        move |versioned_tx: VersionedTransaction,
              verification_mode: TransactionVerificationMode|
              -> Result<SanitizedTransaction> {
            bank.verify_transaction(versioned_tx, verification_mode)
        }
    };

    let check_start = Instant::now();
    let check_result = entry::start_verify_transactions(
        entries,
        skip_verification,
        recyclers.clone(),
        Arc::new(verify_transaction),
    );
    let transaction_cpu_duration_us = timing::duration_as_us(&check_start.elapsed());

    match check_result {
        Ok(mut check_result) => {
            let entries = check_result.entries();
            assert!(entries.is_some());

            let mut replay_elapsed = Measure::start("replay_elapsed");
            let mut execute_timings = ExecuteTimings::default();
            let cost_capacity_meter = Arc::new(RwLock::new(BlockCostCapacityMeter::default()));
            // Note: This will shuffle entries' transactions in-place.
            let process_result = process_entries_with_callback(
                bank,
                &entries.unwrap(),
                true, // shuffle transactions.
                entry_callback,
                transaction_status_sender,
                replay_vote_sender,
                transaction_cost_metrics_sender,
                &mut execute_timings,
                cost_capacity_meter,
                replayer_handle,
            )
            .map_err(BlockstoreProcessorError::from);
            replay_elapsed.stop();
            timing.replay_elapsed += replay_elapsed.as_us();
            timing.planning_elapsed += execute_timings.planning_elapsed;

            timing.execute_timings.accumulate(&execute_timings);

            // If running signature verification on the GPU, wait for that
            // computation to finish, and get the result of it. If we did the
            // signature verification on the CPU, this just returns the
            // already-computed result produced in start_verify_transactions.
            // Either way, check the result of the signature verification.
            if !check_result.finish_verify() {
                warn!("Ledger proof of history failed at slot: {}", bank.slot());
                return Err(TransactionError::SignatureFailure.into());
            }

            if let Some(mut verifier) = verifier {
                let verified = verifier.finish_verify();
                timing.poh_verify_elapsed += verifier.poh_duration_us();
                // The GPU Entry verification (if any) is kicked off right when the CPU-side
                // Entry verification finishes, so these times should be disjoint
                timing.transaction_verify_elapsed +=
                    transaction_cpu_duration_us + check_result.gpu_verify_duration();
                if !verified {
                    warn!("Ledger proof of history failed at slot: {}", bank.slot());
                    return Err(BlockError::InvalidEntryHash.into());
                }
            }

            process_result?;

            progress.num_shreds += num_shreds;
            progress.num_entries += num_entries;
            progress.num_txs += num_txs;
            if let Some(last_entry_hash) = last_entry_hash {
                progress.last_entry = last_entry_hash;
            }

            Ok(())
        }
        Err(err) => {
            warn!("Ledger proof of history failed at slot: {}", bank.slot());
            Err(err.into())
        }
    }
}

// Special handling required for processing the entries in slot 0
fn process_bank_0(
    bank0: &Arc<Bank>,
    blockstore: &Blockstore,
    opts: &ProcessOptions,
    recyclers: &VerifyRecyclers,
    cache_block_meta_sender: Option<&CacheBlockMetaSender>,
    replayer_handle: &ReplayerHandle,
) {
    assert_eq!(bank0.slot(), 0);
    let mut progress = ConfirmationProgress::new(bank0.last_blockhash());
    confirm_full_slot(
        blockstore,
        bank0,
        opts,
        recyclers,
        &mut progress,
        None,
        None,
        &mut ExecuteTimings::default(),
        replayer_handle,
    )
    .expect("processing for bank 0 must succeed");
    bank0.freeze();
    blockstore.insert_bank_hash(bank0.slot(), bank0.hash(), false);
    cache_block_meta(bank0, cache_block_meta_sender);
}

// Given a bank, add its children to the pending slots queue if those children slots are
// complete
fn process_next_slots(
    bank: &Arc<Bank>,
    meta: &SlotMeta,
    blockstore: &Blockstore,
    leader_schedule_cache: &LeaderScheduleCache,
    pending_slots: &mut Vec<(SlotMeta, Bank, Hash)>,
) -> result::Result<(), BlockstoreProcessorError> {
    if meta.next_slots.is_empty() {
        return Ok(());
    }

    // This is a fork point if there are multiple children, create a new child bank for each fork
    for next_slot in &meta.next_slots {
        let next_meta = blockstore
            .meta(*next_slot)
            .map_err(|err| {
                warn!("Failed to load meta for slot {}: {:?}", next_slot, err);
                BlockstoreProcessorError::FailedToLoadMeta
            })?
            .unwrap();

        // Only process full slots in blockstore_processor, replay_stage
        // handles any partials
        if next_meta.is_full() {
            let next_bank = Bank::new_from_parent(
                bank,
                &leader_schedule_cache
                    .slot_leader_at(*next_slot, Some(bank))
                    .unwrap(),
                *next_slot,
            );
            trace!(
                "New bank for slot {}, parent slot is {}",
                next_slot,
                bank.slot(),
            );
            pending_slots.push((next_meta, next_bank, bank.last_blockhash()));
        }
    }

    // Reverse sort by slot, so the next slot to be processed can be popped
    pending_slots.sort_by(|a, b| b.1.slot().cmp(&a.1.slot()));
    Ok(())
}

// Iterate through blockstore processing slots starting from the root slot pointed to by the
// given `meta` and return a vector of frozen bank forks
#[allow(clippy::too_many_arguments)]
fn load_frozen_forks(
    bank_forks: &mut BankForks,
    start_slot: Slot,
    start_slot_meta: &SlotMeta,
    blockstore: &Blockstore,
    leader_schedule_cache: &LeaderScheduleCache,
    opts: &ProcessOptions,
    transaction_status_sender: Option<&TransactionStatusSender>,
    cache_block_meta_sender: Option<&CacheBlockMetaSender>,
    snapshot_config: Option<&SnapshotConfig>,
    accounts_package_sender: AccountsPackageSender,
    timing: &mut ExecuteTimings,
    last_full_snapshot_slot: &mut Option<Slot>,
    pruned_banks_receiver: DroppedSlotsReceiver,
) -> result::Result<(), BlockstoreProcessorError> {
    let recyclers = VerifyRecyclers::default();
    let mut all_banks = HashMap::new();
    let mut last_status_report = Instant::now();
    let mut last_free = Instant::now();
    let mut pending_slots = vec![];
    let mut slots_elapsed = 0;
    let mut txs = 0;
    let blockstore_max_root = blockstore.max_root();
    let mut root = bank_forks.root();
    let max_root = std::cmp::max(root, blockstore_max_root);

    let (_replayer, replayer_handle) = Replayer::new(get_thread_count());

    info!(
        "load_frozen_forks() latest root from blockstore: {}, max_root: {}",
        blockstore_max_root, max_root,
    );

    process_next_slots(
        &bank_forks.get(start_slot).unwrap(),
        start_slot_meta,
        blockstore,
        leader_schedule_cache,
        &mut pending_slots,
    )?;

    info!("processing next slots..");

    let dev_halt_at_slot = opts.dev_halt_at_slot.unwrap_or(std::u64::MAX);
    if bank_forks.root() != dev_halt_at_slot {
        while !pending_slots.is_empty() {
            timing.details.per_program_timings.clear();
            let (meta, bank, last_entry_hash) = pending_slots.pop().unwrap();
            let slot = bank.slot();
            if last_status_report.elapsed() > Duration::from_secs(2) {
                let secs = last_status_report.elapsed().as_secs() as f32;
                last_status_report = Instant::now();
                info!(
                    "processing ledger: slot={}, last root slot={} slots={} slots/s={:?} txs/s={}",
                    slot,
                    root,
                    slots_elapsed,
                    slots_elapsed as f32 / secs,
                    txs as f32 / secs,
                );
                slots_elapsed = 0;
                txs = 0;
            }

            let mut progress = ConfirmationProgress::new(last_entry_hash);

            let bank = bank_forks.insert(bank);
            info!("processing single slot..");
            if process_single_slot(
                blockstore,
                &bank,
                opts,
                &recyclers,
                &mut progress,
                transaction_status_sender,
                cache_block_meta_sender,
                None,
                timing,
                &replayer_handle,
            )
            .is_err()
            {
                assert!(bank_forks.remove(bank.slot()).is_some());
                continue;
            }
            txs += progress.num_txs;

            // Block must be frozen by this point, otherwise `process_single_slot` would
            // have errored above
            assert!(bank.is_frozen());
            all_banks.insert(bank.slot(), bank.clone());

            // If we've reached the last known root in blockstore, start looking
            // for newer cluster confirmed roots
            let new_root_bank = {
                if bank_forks.root() >= max_root {
                    supermajority_root_from_vote_accounts(
                        bank.slot(),
                        bank.total_epoch_stake(),
                        &bank.vote_accounts(),
                    ).and_then(|supermajority_root| {
                        if supermajority_root > root {
                            // If there's a cluster confirmed root greater than our last
                            // replayed root, then because the cluster confirmed root should
                            // be descended from our last root, it must exist in `all_banks`
                            let cluster_root_bank = all_banks.get(&supermajority_root).unwrap();

                            // cluster root must be a descendant of our root, otherwise something
                            // is drastically wrong
                            assert!(cluster_root_bank.ancestors.contains_key(&root));
                            info!(
                                "blockstore processor found new cluster confirmed root: {}, observed in bank: {}",
                                cluster_root_bank.slot(), bank.slot()
                            );

                            // Ensure cluster-confirmed root and parents are set as root in blockstore
                            let mut rooted_slots = vec![];
                            let mut new_root_bank = cluster_root_bank.clone();
                            loop {
                                if new_root_bank.slot() == root { break; } // Found the last root in the chain, yay!
                                assert!(new_root_bank.slot() > root);

                                rooted_slots.push((new_root_bank.slot(), new_root_bank.hash()));
                                // As noted, the cluster confirmed root should be descended from
                                // our last root; therefore parent should be set
                                new_root_bank = new_root_bank.parent().unwrap();
                            }
                            inc_new_counter_info!("load_frozen_forks-cluster-confirmed-root", rooted_slots.len());
                            blockstore.set_roots(rooted_slots.iter().map(|(slot, _hash)| slot)).expect("Blockstore::set_roots should succeed");
                            blockstore.set_duplicate_confirmed_slots_and_hashes(rooted_slots.into_iter()).expect("Blockstore::set_duplicate_confirmed should succeed");
                            Some(cluster_root_bank)
                        } else {
                            None
                        }
                    })
                } else if blockstore.is_root(slot) {
                    Some(&bank)
                } else {
                    None
                }
            };

            if let Some(new_root_bank) = new_root_bank {
                root = new_root_bank.slot();

                leader_schedule_cache.set_root(new_root_bank);
                let _ = bank_forks.set_root(
                    root,
                    &solana_runtime::accounts_background_service::AbsRequestSender::default(),
                    None,
                );

                if let Some(snapshot_config) = snapshot_config {
                    let block_height = new_root_bank.block_height();
                    if snapshot_utils::should_take_full_snapshot(
                        block_height,
                        snapshot_config.full_snapshot_archive_interval_slots,
                    ) {
                        info!("Taking snapshot of new root bank that has crossed the full snapshot interval! slot: {}", root);
                        *last_full_snapshot_slot = Some(root);
                        new_root_bank.exhaustively_free_unused_resource(*last_full_snapshot_slot);
                        last_free = Instant::now();
                        new_root_bank.update_accounts_hash_with_index_option(
                            snapshot_config.accounts_hash_use_index,
                            snapshot_config.accounts_hash_debug_verify,
                            false,
                        );
                        snapshot_utils::snapshot_bank(
                            new_root_bank,
                            new_root_bank.src.slot_deltas(&new_root_bank.src.roots()),
                            &accounts_package_sender,
                            &snapshot_config.bank_snapshots_dir,
                            &snapshot_config.snapshot_archives_dir,
                            snapshot_config.snapshot_version,
                            snapshot_config.archive_format,
                            None,
                            Some(SnapshotType::FullSnapshot),
                        )
                        .expect("Failed to snapshot bank while loading frozen banks");
                        trace!(
                            "took bank snapshot for new root bank, block height: {}, slot: {}",
                            block_height,
                            root
                        );
                    }
                }

                if last_free.elapsed() > Duration::from_secs(10) {
                    // Purge account state for all dropped banks
                    for (pruned_slot, pruned_bank_id) in pruned_banks_receiver.try_iter() {
                        // Simulate this purge being from the AccountsBackgroundService
                        let is_from_abs = true;
                        new_root_bank.rc.accounts.purge_slot(
                            pruned_slot,
                            pruned_bank_id,
                            is_from_abs,
                        );
                    }

                    // Must be called after `squash()`, so that AccountsDb knows what
                    // the roots are for the cache flushing in exhaustively_free_unused_resource().
                    // This could take few secs; so update last_free later
                    new_root_bank.exhaustively_free_unused_resource(*last_full_snapshot_slot);
                    last_free = Instant::now();
                }

                // Filter out all non descendants of the new root
                pending_slots
                    .retain(|(_, pending_bank, _)| pending_bank.ancestors.contains_key(&root));
                all_banks.retain(|_, bank| bank.ancestors.contains_key(&root));
            }

            slots_elapsed += 1;

            trace!(
                "Bank for {}slot {} is complete",
                if root == slot { "root " } else { "" },
                slot,
            );

            process_next_slots(
                &bank,
                &meta,
                blockstore,
                leader_schedule_cache,
                &mut pending_slots,
            )?;

            if slot >= dev_halt_at_slot {
                bank.force_flush_accounts_cache();
                let _ = bank.verify_bank_hash(false, true);
                break;
            }
        }
    }

    Ok(())
}

// `roots` is sorted largest to smallest by root slot
fn supermajority_root(roots: &[(Slot, u64)], total_epoch_stake: u64) -> Option<Slot> {
    if roots.is_empty() {
        return None;
    }

    // Find latest root
    let mut total = 0;
    let mut prev_root = roots[0].0;
    for (root, stake) in roots.iter() {
        assert!(*root <= prev_root);
        total += stake;
        if total as f64 / total_epoch_stake as f64 > VOTE_THRESHOLD_SIZE {
            return Some(*root);
        }
        prev_root = *root;
    }

    None
}

fn supermajority_root_from_vote_accounts(
    bank_slot: Slot,
    total_epoch_stake: u64,
    vote_accounts: &HashMap<Pubkey, (/*stake:*/ u64, VoteAccount)>,
) -> Option<Slot> {
    let mut roots_stakes: Vec<(Slot, u64)> = vote_accounts
        .iter()
        .filter_map(|(key, (stake, account))| {
            if *stake == 0 {
                return None;
            }

            match account.vote_state().as_ref() {
                Err(_) => {
                    warn!(
                        "Unable to get vote_state from account {} in bank: {}",
                        key, bank_slot
                    );
                    None
                }
                Ok(vote_state) => Some((vote_state.root_slot?, *stake)),
            }
        })
        .collect();

    // Sort from greatest to smallest slot
    roots_stakes.sort_unstable_by(|a, b| a.0.cmp(&b.0).reverse());

    // Find latest root
    supermajority_root(&roots_stakes, total_epoch_stake)
}

// Processes and replays the contents of a single slot, returns Error
// if failed to play the slot
fn process_single_slot(
    blockstore: &Blockstore,
    bank: &Arc<Bank>,
    opts: &ProcessOptions,
    recyclers: &VerifyRecyclers,
    progress: &mut ConfirmationProgress,
    transaction_status_sender: Option<&TransactionStatusSender>,
    cache_block_meta_sender: Option<&CacheBlockMetaSender>,
    replay_vote_sender: Option<&ReplayVoteSender>,
    timing: &mut ExecuteTimings,
    replayer_handle: &ReplayerHandle,
) -> result::Result<(), BlockstoreProcessorError> {
    // Mark corrupt slots as dead so validators don't replay this slot and
    // see AlreadyProcessed errors later in ReplayStage
    confirm_full_slot(blockstore, bank, opts, recyclers, progress, transaction_status_sender, replay_vote_sender, timing, replayer_handle).map_err(|err| {
        let slot = bank.slot();
        warn!("slot {} failed to verify: {}", slot, err);
        if blockstore.is_primary_access() {
            blockstore
                .set_dead_slot(slot)
                .expect("Failed to mark slot as dead in blockstore");
        } else {
            assert!(blockstore.is_dead(slot), "Failed slot isn't dead and can't update due to being secondary blockstore access: {}", slot);
        }
        err
    })?;

    bank.freeze(); // all banks handled by this routine are created from complete slots
    blockstore.insert_bank_hash(bank.slot(), bank.hash(), false);
    cache_block_meta(bank, cache_block_meta_sender);

    Ok(())
}

#[allow(clippy::large_enum_variant)]
pub enum TransactionStatusMessage {
    Batch(TransactionStatusBatch),
    Freeze(Slot),
}

pub struct TransactionStatusBatch {
    pub bank: Arc<Bank>,
    pub transactions: Vec<SanitizedTransaction>,
    pub execution_results: Vec<Option<TransactionExecutionDetails>>,
    pub balances: TransactionBalancesSet,
    pub token_balances: TransactionTokenBalancesSet,
    pub rent_debits: Vec<RentDebits>,
}

#[derive(Clone)]
pub struct TransactionStatusSender {
    pub sender: Sender<TransactionStatusMessage>,
}

impl TransactionStatusSender {
    pub fn send_transaction_status_batch(
        &self,
        bank: Arc<Bank>,
        transactions: Vec<SanitizedTransaction>,
        execution_results: Vec<TransactionExecutionResult>,
        balances: TransactionBalancesSet,
        token_balances: TransactionTokenBalancesSet,
        rent_debits: Vec<RentDebits>,
    ) {
        let slot = bank.slot();

        if let Err(e) = self
            .sender
            .send(TransactionStatusMessage::Batch(TransactionStatusBatch {
                bank,
                transactions,
                execution_results: execution_results
                    .into_iter()
                    .map(|result| match result {
                        TransactionExecutionResult::Executed { details, .. } => Some(details),
                        TransactionExecutionResult::NotExecuted(_) => None,
                    })
                    .collect(),
                balances,
                token_balances,
                rent_debits,
            }))
        {
            trace!(
                "Slot {} transaction_status send batch failed: {:?}",
                slot,
                e
            );
        }
    }

    pub fn send_transaction_status_freeze_message(&self, bank: &Arc<Bank>) {
        let slot = bank.slot();
        if let Err(e) = self.sender.send(TransactionStatusMessage::Freeze(slot)) {
            trace!(
                "Slot {} transaction_status send freeze message failed: {:?}",
                slot,
                e
            );
        }
    }
}

pub type CacheBlockMetaSender = Sender<Arc<Bank>>;

pub fn cache_block_meta(bank: &Arc<Bank>, cache_block_meta_sender: Option<&CacheBlockMetaSender>) {
    if let Some(cache_block_meta_sender) = cache_block_meta_sender {
        cache_block_meta_sender
            .send(bank.clone())
            .unwrap_or_else(|err| warn!("cache_block_meta_sender failed: {:?}", err));
    }
}

// used for tests only
pub fn fill_blockstore_slot_with_ticks(
    blockstore: &Blockstore,
    ticks_per_slot: u64,
    slot: u64,
    parent_slot: u64,
    last_entry_hash: Hash,
) -> Hash {
    // Only slot 0 can be equal to the parent_slot
    assert!(slot.saturating_sub(1) >= parent_slot);
    let num_slots = (slot - parent_slot).max(1);
    let entries = create_ticks(num_slots * ticks_per_slot, 0, last_entry_hash);
    let last_entry_hash = entries.last().unwrap().hash;

    blockstore
        .write_entries(
            slot,
            0,
            0,
            ticks_per_slot,
            Some(parent_slot),
            true,
            &Arc::new(Keypair::new()),
            entries,
            0,
        )
        .unwrap();

    last_entry_hash
}

#[cfg(test)]
pub mod tests {
    use {
        super::*,
        crate::genesis_utils::{
            create_genesis_config, create_genesis_config_with_leader, GenesisConfigInfo,
        },
        matches::assert_matches,
        rand::{thread_rng, Rng},
        solana_entry::entry::{create_ticks, next_entry, next_entry_mut},
        solana_program_runtime::{
            accounts_data_meter::MAX_ACCOUNTS_DATA_LEN, invoke_context::InvokeContext,
        },
        solana_runtime::{
            genesis_utils::{
                self, create_genesis_config_with_vote_accounts, ValidatorVoteKeypairs,
            },
            vote_account::VoteAccount,
        },
        solana_sdk::{
            account::{AccountSharedData, WritableAccount},
            epoch_schedule::EpochSchedule,
            hash::Hash,
            instruction::InstructionError,
            native_token::LAMPORTS_PER_SOL,
            pubkey::Pubkey,
            signature::{Keypair, Signer},
            system_instruction::{SystemError, MAX_PERMITTED_DATA_LENGTH},
            system_transaction,
            transaction::{Transaction, TransactionError},
        },
        solana_vote_program::{
            self,
            vote_state::{VoteState, VoteStateVersions, MAX_LOCKOUT_HISTORY},
            vote_transaction,
        },
        std::{collections::BTreeSet, sync::RwLock},
        tempfile::TempDir,
        trees::tr,
    };

    #[test]
    fn test_process_blockstore_with_missing_hashes() {
        solana_logger::setup();

        let hashes_per_tick = 2;
        let GenesisConfigInfo {
            mut genesis_config, ..
        } = create_genesis_config(10_000);
        genesis_config.poh_config.hashes_per_tick = Some(hashes_per_tick);
        let ticks_per_slot = genesis_config.ticks_per_slot;

        let (ledger_path, blockhash) = create_new_tmp_ledger_auto_delete!(&genesis_config);
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();

        let parent_slot = 0;
        let slot = 1;
        let entries = create_ticks(ticks_per_slot, hashes_per_tick - 1, blockhash);
        assert_matches!(
            blockstore.write_entries(
                slot,
                0,
                0,
                ticks_per_slot,
                Some(parent_slot),
                true,
                &Arc::new(Keypair::new()),
                entries,
                0,
            ),
            Ok(_)
        );

        let (bank_forks, ..) = test_process_blockstore(
            &genesis_config,
            &blockstore,
            ProcessOptions {
                poh_verify: true,
                ..ProcessOptions::default()
            },
        );
        assert_eq!(frozen_bank_slots(&bank_forks), vec![0]);
    }

    #[test]
    fn test_process_blockstore_with_invalid_slot_tick_count() {
        solana_logger::setup();

        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let ticks_per_slot = genesis_config.ticks_per_slot;

        // Create a new ledger with slot 0 full of ticks
        let (ledger_path, blockhash) = create_new_tmp_ledger_auto_delete!(&genesis_config);
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();

        // Write slot 1 with one tick missing
        let parent_slot = 0;
        let slot = 1;
        let entries = create_ticks(ticks_per_slot - 1, 0, blockhash);
        assert_matches!(
            blockstore.write_entries(
                slot,
                0,
                0,
                ticks_per_slot,
                Some(parent_slot),
                true,
                &Arc::new(Keypair::new()),
                entries,
                0,
            ),
            Ok(_)
        );

        // Should return slot 0, the last slot on the fork that is valid
        let (bank_forks, ..) = test_process_blockstore(
            &genesis_config,
            &blockstore,
            ProcessOptions {
                poh_verify: true,
                ..ProcessOptions::default()
            },
        );
        assert_eq!(frozen_bank_slots(&bank_forks), vec![0]);

        // Write slot 2 fully
        let _last_slot2_entry_hash =
            fill_blockstore_slot_with_ticks(&blockstore, ticks_per_slot, 2, 0, blockhash);

        let (bank_forks, ..) = test_process_blockstore(
            &genesis_config,
            &blockstore,
            ProcessOptions {
                poh_verify: true,
                ..ProcessOptions::default()
            },
        );

        // One valid fork, one bad fork.  process_blockstore() should only return the valid fork
        assert_eq!(frozen_bank_slots(&bank_forks), vec![0, 2]);
        assert_eq!(bank_forks.working_bank().slot(), 2);
        assert_eq!(bank_forks.root(), 0);
    }

    #[test]
    fn test_process_blockstore_with_slot_with_trailing_entry() {
        solana_logger::setup();

        let GenesisConfigInfo {
            mint_keypair,
            genesis_config,
            ..
        } = create_genesis_config(10_000);
        let ticks_per_slot = genesis_config.ticks_per_slot;

        let (ledger_path, blockhash) = create_new_tmp_ledger_auto_delete!(&genesis_config);
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();

        let mut entries = create_ticks(ticks_per_slot, 0, blockhash);
        let trailing_entry = {
            let keypair = Keypair::new();
            let tx = system_transaction::transfer(&mint_keypair, &keypair.pubkey(), 1, blockhash);
            next_entry(&blockhash, 1, vec![tx])
        };
        entries.push(trailing_entry);

        // Tricks blockstore into writing the trailing entry by lying that there is one more tick
        // per slot.
        let parent_slot = 0;
        let slot = 1;
        assert_matches!(
            blockstore.write_entries(
                slot,
                0,
                0,
                ticks_per_slot + 1,
                Some(parent_slot),
                true,
                &Arc::new(Keypair::new()),
                entries,
                0,
            ),
            Ok(_)
        );

        let opts = ProcessOptions {
            poh_verify: true,
            accounts_db_test_hash_calculation: true,
            ..ProcessOptions::default()
        };
        let (bank_forks, ..) = test_process_blockstore(&genesis_config, &blockstore, opts);
        assert_eq!(frozen_bank_slots(&bank_forks), vec![0]);
    }

    #[test]
    fn test_process_blockstore_with_incomplete_slot() {
        solana_logger::setup();

        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let ticks_per_slot = genesis_config.ticks_per_slot;

        /*
          Build a blockstore in the ledger with the following fork structure:

               slot 0 (all ticks)
                 |
               slot 1 (all ticks but one)
                 |
               slot 2 (all ticks)

           where slot 1 is incomplete (missing 1 tick at the end)
        */

        // Create a new ledger with slot 0 full of ticks
        let (ledger_path, mut blockhash) = create_new_tmp_ledger_auto_delete!(&genesis_config);
        debug!("ledger_path: {:?}", ledger_path);

        let blockstore = Blockstore::open(ledger_path.path()).unwrap();

        // Write slot 1
        // slot 1, points at slot 0.  Missing one tick
        {
            let parent_slot = 0;
            let slot = 1;
            let mut entries = create_ticks(ticks_per_slot, 0, blockhash);
            blockhash = entries.last().unwrap().hash;

            // throw away last one
            entries.pop();

            assert_matches!(
                blockstore.write_entries(
                    slot,
                    0,
                    0,
                    ticks_per_slot,
                    Some(parent_slot),
                    false,
                    &Arc::new(Keypair::new()),
                    entries,
                    0,
                ),
                Ok(_)
            );
        }

        // slot 2, points at slot 1
        fill_blockstore_slot_with_ticks(&blockstore, ticks_per_slot, 2, 1, blockhash);

        let opts = ProcessOptions {
            poh_verify: true,
            accounts_db_test_hash_calculation: true,
            ..ProcessOptions::default()
        };
        let (bank_forks, ..) = test_process_blockstore(&genesis_config, &blockstore, opts);

        assert_eq!(frozen_bank_slots(&bank_forks), vec![0]); // slot 1 isn't "full", we stop at slot zero

        /* Add a complete slot such that the store looks like:

                                 slot 0 (all ticks)
                               /                  \
               slot 1 (all ticks but one)        slot 3 (all ticks)
                      |
               slot 2 (all ticks)
        */
        let opts = ProcessOptions {
            poh_verify: true,
            accounts_db_test_hash_calculation: true,
            ..ProcessOptions::default()
        };
        fill_blockstore_slot_with_ticks(&blockstore, ticks_per_slot, 3, 0, blockhash);
        // Slot 0 should not show up in the ending bank_forks_info
        let (bank_forks, ..) = test_process_blockstore(&genesis_config, &blockstore, opts);

        // slot 1 isn't "full", we stop at slot zero
        assert_eq!(frozen_bank_slots(&bank_forks), vec![0, 3]);
    }

    #[test]
    fn test_process_blockstore_with_two_forks_and_squash() {
        solana_logger::setup();

        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let ticks_per_slot = genesis_config.ticks_per_slot;

        // Create a new ledger with slot 0 full of ticks
        let (ledger_path, blockhash) = create_new_tmp_ledger_auto_delete!(&genesis_config);
        debug!("ledger_path: {:?}", ledger_path);
        let mut last_entry_hash = blockhash;

        /*
            Build a blockstore in the ledger with the following fork structure:

                 slot 0
                   |
                 slot 1
                 /   \
            slot 2   |
               /     |
            slot 3   |
                     |
                   slot 4 <-- set_root(true)

        */
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();

        // Fork 1, ending at slot 3
        let last_slot1_entry_hash =
            fill_blockstore_slot_with_ticks(&blockstore, ticks_per_slot, 1, 0, last_entry_hash);
        last_entry_hash = fill_blockstore_slot_with_ticks(
            &blockstore,
            ticks_per_slot,
            2,
            1,
            last_slot1_entry_hash,
        );
        let last_fork1_entry_hash =
            fill_blockstore_slot_with_ticks(&blockstore, ticks_per_slot, 3, 2, last_entry_hash);

        // Fork 2, ending at slot 4
        let last_fork2_entry_hash = fill_blockstore_slot_with_ticks(
            &blockstore,
            ticks_per_slot,
            4,
            1,
            last_slot1_entry_hash,
        );

        info!("last_fork1_entry.hash: {:?}", last_fork1_entry_hash);
        info!("last_fork2_entry.hash: {:?}", last_fork2_entry_hash);

        blockstore.set_roots(vec![0, 1, 4].iter()).unwrap();

        let opts = ProcessOptions {
            poh_verify: true,
            accounts_db_test_hash_calculation: true,
            ..ProcessOptions::default()
        };
        let (bank_forks, ..) = test_process_blockstore(&genesis_config, &blockstore, opts);

        // One fork, other one is ignored b/c not a descendant of the root
        assert_eq!(frozen_bank_slots(&bank_forks), vec![4]);

        assert!(&bank_forks[4]
            .parents()
            .iter()
            .map(|bank| bank.slot())
            .next()
            .is_none());

        // Ensure bank_forks holds the right banks
        verify_fork_infos(&bank_forks);

        assert_eq!(bank_forks.root(), 4);
    }

    #[test]
    fn test_process_blockstore_with_two_forks() {
        solana_logger::setup();

        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let ticks_per_slot = genesis_config.ticks_per_slot;

        // Create a new ledger with slot 0 full of ticks
        let (ledger_path, blockhash) = create_new_tmp_ledger_auto_delete!(&genesis_config);
        debug!("ledger_path: {:?}", ledger_path);
        let mut last_entry_hash = blockhash;

        /*
            Build a blockstore in the ledger with the following fork structure:

                 slot 0
                   |
                 slot 1  <-- set_root(true)
                 /   \
            slot 2   |
               /     |
            slot 3   |
                     |
                   slot 4

        */
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();

        // Fork 1, ending at slot 3
        let last_slot1_entry_hash =
            fill_blockstore_slot_with_ticks(&blockstore, ticks_per_slot, 1, 0, last_entry_hash);
        last_entry_hash = fill_blockstore_slot_with_ticks(
            &blockstore,
            ticks_per_slot,
            2,
            1,
            last_slot1_entry_hash,
        );
        let last_fork1_entry_hash =
            fill_blockstore_slot_with_ticks(&blockstore, ticks_per_slot, 3, 2, last_entry_hash);

        // Fork 2, ending at slot 4
        let last_fork2_entry_hash = fill_blockstore_slot_with_ticks(
            &blockstore,
            ticks_per_slot,
            4,
            1,
            last_slot1_entry_hash,
        );

        info!("last_fork1_entry.hash: {:?}", last_fork1_entry_hash);
        info!("last_fork2_entry.hash: {:?}", last_fork2_entry_hash);

        blockstore.set_roots(vec![0, 1].iter()).unwrap();

        let opts = ProcessOptions {
            poh_verify: true,
            accounts_db_test_hash_calculation: true,
            ..ProcessOptions::default()
        };
        let (bank_forks, ..) = test_process_blockstore(&genesis_config, &blockstore, opts);

        assert_eq!(frozen_bank_slots(&bank_forks), vec![1, 2, 3, 4]);
        assert_eq!(bank_forks.working_bank().slot(), 4);
        assert_eq!(bank_forks.root(), 1);

        assert_eq!(
            &bank_forks[3]
                .parents()
                .iter()
                .map(|bank| bank.slot())
                .collect::<Vec<_>>(),
            &[2, 1]
        );
        assert_eq!(
            &bank_forks[4]
                .parents()
                .iter()
                .map(|bank| bank.slot())
                .collect::<Vec<_>>(),
            &[1]
        );

        assert_eq!(bank_forks.root(), 1);

        // Ensure bank_forks holds the right banks
        verify_fork_infos(&bank_forks);
    }

    #[test]
    fn test_process_blockstore_with_dead_slot() {
        solana_logger::setup();

        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let ticks_per_slot = genesis_config.ticks_per_slot;
        let (ledger_path, blockhash) = create_new_tmp_ledger_auto_delete!(&genesis_config);
        debug!("ledger_path: {:?}", ledger_path);

        /*
                   slot 0
                     |
                   slot 1
                  /     \
                 /       \
           slot 2 (dead)  \
                           \
                        slot 3
        */
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();
        let slot1_blockhash =
            fill_blockstore_slot_with_ticks(&blockstore, ticks_per_slot, 1, 0, blockhash);
        fill_blockstore_slot_with_ticks(&blockstore, ticks_per_slot, 2, 1, slot1_blockhash);
        blockstore.set_dead_slot(2).unwrap();
        fill_blockstore_slot_with_ticks(&blockstore, ticks_per_slot, 3, 1, slot1_blockhash);

        let (bank_forks, ..) =
            test_process_blockstore(&genesis_config, &blockstore, ProcessOptions::default());

        assert_eq!(frozen_bank_slots(&bank_forks), vec![0, 1, 3]);
        assert_eq!(bank_forks.working_bank().slot(), 3);
        assert_eq!(
            &bank_forks[3]
                .parents()
                .iter()
                .map(|bank| bank.slot())
                .collect::<Vec<_>>(),
            &[1, 0]
        );
        verify_fork_infos(&bank_forks);
    }

    #[test]
    fn test_process_blockstore_with_dead_child() {
        solana_logger::setup();

        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let ticks_per_slot = genesis_config.ticks_per_slot;
        let (ledger_path, blockhash) = create_new_tmp_ledger_auto_delete!(&genesis_config);
        debug!("ledger_path: {:?}", ledger_path);

        /*
                   slot 0
                     |
                   slot 1
                  /     \
                 /       \
              slot 2      \
               /           \
           slot 4 (dead)   slot 3
        */
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();
        let slot1_blockhash =
            fill_blockstore_slot_with_ticks(&blockstore, ticks_per_slot, 1, 0, blockhash);
        let slot2_blockhash =
            fill_blockstore_slot_with_ticks(&blockstore, ticks_per_slot, 2, 1, slot1_blockhash);
        fill_blockstore_slot_with_ticks(&blockstore, ticks_per_slot, 4, 2, slot2_blockhash);
        blockstore.set_dead_slot(4).unwrap();
        fill_blockstore_slot_with_ticks(&blockstore, ticks_per_slot, 3, 1, slot1_blockhash);

        let (bank_forks, ..) =
            test_process_blockstore(&genesis_config, &blockstore, ProcessOptions::default());

        // Should see the parent of the dead child
        assert_eq!(frozen_bank_slots(&bank_forks), vec![0, 1, 2, 3]);
        assert_eq!(bank_forks.working_bank().slot(), 3);

        assert_eq!(
            &bank_forks[3]
                .parents()
                .iter()
                .map(|bank| bank.slot())
                .collect::<Vec<_>>(),
            &[1, 0]
        );
        assert_eq!(
            &bank_forks[2]
                .parents()
                .iter()
                .map(|bank| bank.slot())
                .collect::<Vec<_>>(),
            &[1, 0]
        );
        assert_eq!(bank_forks.working_bank().slot(), 3);
        verify_fork_infos(&bank_forks);
    }

    #[test]
    fn test_root_with_all_dead_children() {
        solana_logger::setup();

        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let ticks_per_slot = genesis_config.ticks_per_slot;
        let (ledger_path, blockhash) = create_new_tmp_ledger_auto_delete!(&genesis_config);
        debug!("ledger_path: {:?}", ledger_path);

        /*
                   slot 0
                 /        \
                /          \
           slot 1 (dead)  slot 2 (dead)
        */
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();
        fill_blockstore_slot_with_ticks(&blockstore, ticks_per_slot, 1, 0, blockhash);
        fill_blockstore_slot_with_ticks(&blockstore, ticks_per_slot, 2, 0, blockhash);
        blockstore.set_dead_slot(1).unwrap();
        blockstore.set_dead_slot(2).unwrap();
        let (bank_forks, ..) =
            test_process_blockstore(&genesis_config, &blockstore, ProcessOptions::default());

        // Should see only the parent of the dead children
        assert_eq!(frozen_bank_slots(&bank_forks), vec![0]);
        verify_fork_infos(&bank_forks);
    }

    #[test]
    fn test_process_blockstore_epoch_boundary_root() {
        solana_logger::setup();

        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let ticks_per_slot = genesis_config.ticks_per_slot;

        // Create a new ledger with slot 0 full of ticks
        let (ledger_path, blockhash) = create_new_tmp_ledger_auto_delete!(&genesis_config);
        let mut last_entry_hash = blockhash;

        let blockstore = Blockstore::open(ledger_path.path()).unwrap();

        // Let `last_slot` be the number of slots in the first two epochs
        let epoch_schedule = get_epoch_schedule(&genesis_config, Vec::new());
        let last_slot = epoch_schedule.get_last_slot_in_epoch(1);

        // Create a single chain of slots with all indexes in the range [0, v + 1]
        for i in 1..=last_slot + 1 {
            last_entry_hash = fill_blockstore_slot_with_ticks(
                &blockstore,
                ticks_per_slot,
                i,
                i - 1,
                last_entry_hash,
            );
        }

        // Set a root on the last slot of the last confirmed epoch
        let rooted_slots: Vec<Slot> = (0..=last_slot).collect();
        blockstore.set_roots(rooted_slots.iter()).unwrap();

        // Set a root on the next slot of the confirmed epoch
        blockstore
            .set_roots(std::iter::once(&(last_slot + 1)))
            .unwrap();

        // Check that we can properly restart the ledger / leader scheduler doesn't fail
        let opts = ProcessOptions {
            poh_verify: true,
            accounts_db_test_hash_calculation: true,
            ..ProcessOptions::default()
        };
        let (bank_forks, ..) = test_process_blockstore(&genesis_config, &blockstore, opts);

        // There is one fork, head is last_slot + 1
        assert_eq!(frozen_bank_slots(&bank_forks), vec![last_slot + 1]);

        // The latest root should have purged all its parents
        assert!(&bank_forks[last_slot + 1]
            .parents()
            .iter()
            .map(|bank| bank.slot())
            .next()
            .is_none());
    }

    #[test]
    fn test_first_err() {
        assert_eq!(first_err(&[Ok(())]), Ok(()));
        assert_eq!(
            first_err(&[Ok(()), Err(TransactionError::AlreadyProcessed)]),
            Err(TransactionError::AlreadyProcessed)
        );
        assert_eq!(
            first_err(&[
                Ok(()),
                Err(TransactionError::AlreadyProcessed),
                Err(TransactionError::AccountInUse)
            ]),
            Err(TransactionError::AlreadyProcessed)
        );
        assert_eq!(
            first_err(&[
                Ok(()),
                Err(TransactionError::AccountInUse),
                Err(TransactionError::AlreadyProcessed)
            ]),
            Err(TransactionError::AccountInUse)
        );
        assert_eq!(
            first_err(&[
                Err(TransactionError::AccountInUse),
                Ok(()),
                Err(TransactionError::AlreadyProcessed)
            ]),
            Err(TransactionError::AccountInUse)
        );
    }

    #[test]
    fn test_process_empty_entry_is_registered() {
        solana_logger::setup();

        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(2);
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));
        let keypair = Keypair::new();
        let slot_entries = create_ticks(genesis_config.ticks_per_slot, 1, genesis_config.hash());
        let tx = system_transaction::transfer(
            &mint_keypair,
            &keypair.pubkey(),
            1,
            slot_entries.last().unwrap().hash,
        );

        // First, ensure the TX is rejected because of the unregistered last ID
        assert_eq!(
            bank.process_transaction(&tx),
            Err(TransactionError::BlockhashNotFound)
        );

        // Now ensure the TX is accepted despite pointing to the ID of an empty entry.
        process_entries_for_tests(&bank, slot_entries, true, None, None).unwrap();
        assert_eq!(bank.process_transaction(&tx), Ok(()));
    }

    #[test]
    fn test_process_ledger_simple() {
        solana_logger::setup();
        let leader_pubkey = solana_sdk::pubkey::new_rand();
        let mint = 100;
        let hashes_per_tick = 10;
        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config_with_leader(mint, &leader_pubkey, 50);
        genesis_config.poh_config.hashes_per_tick = Some(hashes_per_tick);
        let (ledger_path, mut last_entry_hash) =
            create_new_tmp_ledger_auto_delete!(&genesis_config);
        debug!("ledger_path: {:?}", ledger_path);

        let deducted_from_mint = 3;
        let mut entries = vec![];
        let blockhash = genesis_config.hash();
        for _ in 0..deducted_from_mint {
            // Transfer one token from the mint to a random account
            let keypair = Keypair::new();
            let tx = system_transaction::transfer(&mint_keypair, &keypair.pubkey(), 1, blockhash);
            let entry = next_entry_mut(&mut last_entry_hash, 1, vec![tx]);
            entries.push(entry);

            // Add a second Transaction that will produce a
            // InstructionError<0, ResultWithNegativeLamports> error when processed
            let keypair2 = Keypair::new();
            let tx =
                system_transaction::transfer(&mint_keypair, &keypair2.pubkey(), 101, blockhash);
            let entry = next_entry_mut(&mut last_entry_hash, 1, vec![tx]);
            entries.push(entry);
        }

        let remaining_hashes = hashes_per_tick - entries.len() as u64;
        let tick_entry = next_entry_mut(&mut last_entry_hash, remaining_hashes, vec![]);
        entries.push(tick_entry);

        // Fill up the rest of slot 1 with ticks
        entries.extend(create_ticks(
            genesis_config.ticks_per_slot - 1,
            genesis_config.poh_config.hashes_per_tick.unwrap(),
            last_entry_hash,
        ));
        let last_blockhash = entries.last().unwrap().hash;

        let blockstore = Blockstore::open(ledger_path.path()).unwrap();
        blockstore
            .write_entries(
                1,
                0,
                0,
                genesis_config.ticks_per_slot,
                None,
                true,
                &Arc::new(Keypair::new()),
                entries,
                0,
            )
            .unwrap();
        let opts = ProcessOptions {
            poh_verify: true,
            accounts_db_test_hash_calculation: true,
            ..ProcessOptions::default()
        };
        let (bank_forks, ..) = test_process_blockstore(&genesis_config, &blockstore, opts);

        assert_eq!(frozen_bank_slots(&bank_forks), vec![0, 1]);
        assert_eq!(bank_forks.root(), 0);
        assert_eq!(bank_forks.working_bank().slot(), 1);

        let bank = bank_forks[1].clone();
        assert_eq!(
            bank.get_balance(&mint_keypair.pubkey()),
            mint - deducted_from_mint
        );
        assert_eq!(bank.tick_height(), 2 * genesis_config.ticks_per_slot);
        assert_eq!(bank.last_blockhash(), last_blockhash);
    }

    #[test]
    fn test_process_ledger_with_one_tick_per_slot() {
        let GenesisConfigInfo {
            mut genesis_config, ..
        } = create_genesis_config(123);
        genesis_config.ticks_per_slot = 1;
        let (ledger_path, _blockhash) = create_new_tmp_ledger_auto_delete!(&genesis_config);

        let blockstore = Blockstore::open(ledger_path.path()).unwrap();
        let opts = ProcessOptions {
            poh_verify: true,
            accounts_db_test_hash_calculation: true,
            ..ProcessOptions::default()
        };
        let (bank_forks, ..) = test_process_blockstore(&genesis_config, &blockstore, opts);

        assert_eq!(frozen_bank_slots(&bank_forks), vec![0]);
        let bank = bank_forks[0].clone();
        assert_eq!(bank.tick_height(), 1);
    }

    #[test]
    fn test_process_ledger_options_override_threads() {
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(123);
        let (ledger_path, _blockhash) = create_new_tmp_ledger_auto_delete!(&genesis_config);

        let blockstore = Blockstore::open(ledger_path.path()).unwrap();
        let opts = ProcessOptions {
            override_num_threads: Some(1),
            accounts_db_test_hash_calculation: true,
            ..ProcessOptions::default()
        };
        test_process_blockstore(&genesis_config, &blockstore, opts);
        PAR_THREAD_POOL.with(|pool| {
            assert_eq!(pool.borrow().current_num_threads(), 1);
        });
    }

    #[test]
    fn test_process_ledger_options_full_leader_cache() {
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(123);
        let (ledger_path, _blockhash) = create_new_tmp_ledger_auto_delete!(&genesis_config);

        let blockstore = Blockstore::open(ledger_path.path()).unwrap();
        let opts = ProcessOptions {
            full_leader_cache: true,
            accounts_db_test_hash_calculation: true,
            ..ProcessOptions::default()
        };
        let (_bank_forks, leader_schedule) =
            test_process_blockstore(&genesis_config, &blockstore, opts);
        assert_eq!(leader_schedule.max_schedules(), std::usize::MAX);
    }

    #[test]
    fn test_process_ledger_options_entry_callback() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(100);
        let (ledger_path, last_entry_hash) = create_new_tmp_ledger_auto_delete!(&genesis_config);
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();
        let blockhash = genesis_config.hash();
        let keypairs = [Keypair::new(), Keypair::new(), Keypair::new()];

        let tx = system_transaction::transfer(&mint_keypair, &keypairs[0].pubkey(), 1, blockhash);
        let entry_1 = next_entry(&last_entry_hash, 1, vec![tx]);

        let tx = system_transaction::transfer(&mint_keypair, &keypairs[1].pubkey(), 1, blockhash);
        let entry_2 = next_entry(&entry_1.hash, 1, vec![tx]);

        let mut entries = vec![entry_1, entry_2];
        entries.extend(create_ticks(
            genesis_config.ticks_per_slot,
            0,
            last_entry_hash,
        ));
        blockstore
            .write_entries(
                1,
                0,
                0,
                genesis_config.ticks_per_slot,
                None,
                true,
                &Arc::new(Keypair::new()),
                entries,
                0,
            )
            .unwrap();

        let callback_counter: Arc<RwLock<usize>> = Arc::default();
        let entry_callback = {
            let counter = callback_counter.clone();
            let pubkeys: Vec<Pubkey> = keypairs.iter().map(|k| k.pubkey()).collect();
            Arc::new(move |bank: &Bank| {
                let mut counter = counter.write().unwrap();
                assert_eq!(bank.get_balance(&pubkeys[*counter]), 1);
                assert_eq!(bank.get_balance(&pubkeys[*counter + 1]), 0);
                *counter += 1;
            })
        };

        let opts = ProcessOptions {
            override_num_threads: Some(1),
            entry_callback: Some(entry_callback),
            accounts_db_test_hash_calculation: true,
            ..ProcessOptions::default()
        };
        test_process_blockstore(&genesis_config, &blockstore, opts);
        assert_eq!(*callback_counter.write().unwrap(), 2);
    }

    #[test]
    fn test_process_entries_tick() {
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(1000);
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));

        // ensure bank can process a tick
        assert_eq!(bank.tick_height(), 0);
        let tick = next_entry(&genesis_config.hash(), 1, vec![]);
        assert_eq!(
            process_entries_for_tests(&bank, vec![tick], true, None, None),
            Ok(())
        );
        assert_eq!(bank.tick_height(), 1);
    }

    #[test]
    fn test_process_entries_2_entries_collision() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(1000);
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();

        let blockhash = bank.last_blockhash();

        // ensure bank can process 2 entries that have a common account and no tick is registered
        let tx = system_transaction::transfer(
            &mint_keypair,
            &keypair1.pubkey(),
            2,
            bank.last_blockhash(),
        );
        let entry_1 = next_entry(&blockhash, 1, vec![tx]);
        let tx = system_transaction::transfer(
            &mint_keypair,
            &keypair2.pubkey(),
            2,
            bank.last_blockhash(),
        );
        let entry_2 = next_entry(&entry_1.hash, 1, vec![tx]);
        assert_eq!(
            process_entries_for_tests(&bank, vec![entry_1, entry_2], true, None, None),
            Ok(())
        );
        assert_eq!(bank.get_balance(&keypair1.pubkey()), 2);
        assert_eq!(bank.get_balance(&keypair2.pubkey()), 2);
        assert_eq!(bank.last_blockhash(), blockhash);
    }

    #[test]
    fn test_process_entries_2_txes_collision() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(1000);
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();
        let keypair3 = Keypair::new();

        // fund: put 4 in each of 1 and 2
        assert_matches!(bank.transfer(4, &mint_keypair, &keypair1.pubkey()), Ok(_));
        assert_matches!(bank.transfer(4, &mint_keypair, &keypair2.pubkey()), Ok(_));

        // construct an Entry whose 2nd transaction would cause a lock conflict with previous entry
        let entry_1_to_mint = next_entry(
            &bank.last_blockhash(),
            1,
            vec![system_transaction::transfer(
                &keypair1,
                &mint_keypair.pubkey(),
                1,
                bank.last_blockhash(),
            )],
        );

        let entry_2_to_3_mint_to_1 = next_entry(
            &entry_1_to_mint.hash,
            1,
            vec![
                system_transaction::transfer(
                    &keypair2,
                    &keypair3.pubkey(),
                    2,
                    bank.last_blockhash(),
                ), // should be fine
                system_transaction::transfer(
                    &keypair1,
                    &mint_keypair.pubkey(),
                    2,
                    bank.last_blockhash(),
                ), // will collide
            ],
        );

        assert_eq!(
            process_entries_for_tests(
                &bank,
                vec![entry_1_to_mint, entry_2_to_3_mint_to_1],
                false,
                None,
                None,
            ),
            Ok(())
        );

        assert_eq!(bank.get_balance(&keypair1.pubkey()), 1);
        assert_eq!(bank.get_balance(&keypair2.pubkey()), 2);
        assert_eq!(bank.get_balance(&keypair3.pubkey()), 2);
    }

    #[test]
    fn test_process_entries_2_txes_collision_and_error() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(1000);
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();
        let keypair3 = Keypair::new();
        let keypair4 = Keypair::new();

        // fund: put 4 in each of 1 and 2
        assert_matches!(bank.transfer(4, &mint_keypair, &keypair1.pubkey()), Ok(_));
        assert_matches!(bank.transfer(4, &mint_keypair, &keypair2.pubkey()), Ok(_));
        assert_matches!(bank.transfer(4, &mint_keypair, &keypair4.pubkey()), Ok(_));

        // construct an Entry whose 2nd transaction would cause a lock conflict with previous entry
        let entry_1_to_mint = next_entry(
            &bank.last_blockhash(),
            1,
            vec![
                system_transaction::transfer(
                    &keypair1,
                    &mint_keypair.pubkey(),
                    1,
                    bank.last_blockhash(),
                ),
                system_transaction::transfer(
                    &keypair4,
                    &keypair4.pubkey(),
                    1,
                    Hash::default(), // Should cause a transaction failure with BlockhashNotFound
                ),
            ],
        );

        let entry_2_to_3_mint_to_1 = next_entry(
            &entry_1_to_mint.hash,
            1,
            vec![
                system_transaction::transfer(
                    &keypair2,
                    &keypair3.pubkey(),
                    2,
                    bank.last_blockhash(),
                ), // should be fine
                system_transaction::transfer(
                    &keypair1,
                    &mint_keypair.pubkey(),
                    2,
                    bank.last_blockhash(),
                ), // will collide
            ],
        );

        assert!(process_entries_for_tests(
            &bank,
            vec![entry_1_to_mint.clone(), entry_2_to_3_mint_to_1.clone()],
            false,
            None,
            None,
        )
        .is_err());

        // First transaction in first entry succeeded, so keypair1 lost 1 lamport
        assert_eq!(bank.get_balance(&keypair1.pubkey()), 3);
        assert_eq!(bank.get_balance(&keypair2.pubkey()), 4);

        // Check all accounts are unlocked
        let txs1 = entry_1_to_mint.transactions;
        let txs2 = entry_2_to_3_mint_to_1.transactions;
        let batch1 = bank.prepare_entry_batch(txs1).unwrap();
        for result in batch1.lock_results() {
            assert!(result.is_ok());
        }
        // txs1 and txs2 have accounts that conflict, so we must drop txs1 first
        drop(batch1);
        let batch2 = bank.prepare_entry_batch(txs2).unwrap();
        for result in batch2.lock_results() {
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_process_entries_2nd_entry_collision_with_self_and_error() {
        solana_logger::setup();

        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(1000);
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();
        let keypair3 = Keypair::new();

        // fund: put some money in each of 1 and 2
        assert_matches!(bank.transfer(5, &mint_keypair, &keypair1.pubkey()), Ok(_));
        assert_matches!(bank.transfer(4, &mint_keypair, &keypair2.pubkey()), Ok(_));

        // 3 entries: first has a transfer, 2nd has a conflict with 1st, 3rd has a conflict with itself
        let entry_1_to_mint = next_entry(
            &bank.last_blockhash(),
            1,
            vec![system_transaction::transfer(
                &keypair1,
                &mint_keypair.pubkey(),
                1,
                bank.last_blockhash(),
            )],
        );
        // should now be:
        // keypair1=4
        // keypair2=4
        // keypair3=0

        let entry_2_to_3_and_1_to_mint = next_entry(
            &entry_1_to_mint.hash,
            1,
            vec![
                system_transaction::transfer(
                    &keypair2,
                    &keypair3.pubkey(),
                    2,
                    bank.last_blockhash(),
                ), // should be fine
                system_transaction::transfer(
                    &keypair1,
                    &mint_keypair.pubkey(),
                    2,
                    bank.last_blockhash(),
                ), // will collide with predecessor
            ],
        );
        // should now be:
        // keypair1=2
        // keypair2=2
        // keypair3=2

        let entry_conflict_itself = next_entry(
            &entry_2_to_3_and_1_to_mint.hash,
            1,
            vec![
                system_transaction::transfer(
                    &keypair1,
                    &keypair3.pubkey(),
                    1,
                    bank.last_blockhash(),
                ),
                system_transaction::transfer(
                    &keypair1,
                    &keypair2.pubkey(),
                    1,
                    bank.last_blockhash(),
                ), // should be fine
            ],
        );
        // would now be:
        // keypair1=0
        // keypair2=3
        // keypair3=3

        assert!(process_entries_for_tests(
            &bank,
            vec![
                entry_1_to_mint,
                entry_2_to_3_and_1_to_mint,
                entry_conflict_itself,
            ],
            false,
            None,
            None,
        )
        .is_err());

        // last entry should have been aborted before par_execute_entries
        assert_eq!(bank.get_balance(&keypair1.pubkey()), 2);
        assert_eq!(bank.get_balance(&keypair2.pubkey()), 2);
        assert_eq!(bank.get_balance(&keypair3.pubkey()), 2);
    }

    #[test]
    fn test_process_entries_2_entries_par() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(1000);
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();
        let keypair3 = Keypair::new();
        let keypair4 = Keypair::new();

        //load accounts
        let tx = system_transaction::transfer(
            &mint_keypair,
            &keypair1.pubkey(),
            1,
            bank.last_blockhash(),
        );
        assert_eq!(bank.process_transaction(&tx), Ok(()));
        let tx = system_transaction::transfer(
            &mint_keypair,
            &keypair2.pubkey(),
            1,
            bank.last_blockhash(),
        );
        assert_eq!(bank.process_transaction(&tx), Ok(()));

        // ensure bank can process 2 entries that do not have a common account and no tick is registered
        let blockhash = bank.last_blockhash();
        let tx =
            system_transaction::transfer(&keypair1, &keypair3.pubkey(), 1, bank.last_blockhash());
        let entry_1 = next_entry(&blockhash, 1, vec![tx]);
        let tx =
            system_transaction::transfer(&keypair2, &keypair4.pubkey(), 1, bank.last_blockhash());
        let entry_2 = next_entry(&entry_1.hash, 1, vec![tx]);
        assert_eq!(
            process_entries_for_tests(&bank, vec![entry_1, entry_2], true, None, None),
            Ok(())
        );
        assert_eq!(bank.get_balance(&keypair3.pubkey()), 1);
        assert_eq!(bank.get_balance(&keypair4.pubkey()), 1);
        assert_eq!(bank.last_blockhash(), blockhash);
    }

    #[test]
    fn test_process_entry_tx_random_execution_with_error() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(1_000_000_000);
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));

        const NUM_TRANSFERS_PER_ENTRY: usize = 8;
        const NUM_TRANSFERS: usize = NUM_TRANSFERS_PER_ENTRY * 32;
        // large enough to scramble locks and results

        let keypairs: Vec<_> = (0..NUM_TRANSFERS * 2).map(|_| Keypair::new()).collect();

        // give everybody one lamport
        for keypair in &keypairs {
            bank.transfer(1, &mint_keypair, &keypair.pubkey())
                .expect("funding failed");
        }
        let mut hash = bank.last_blockhash();

        let present_account_key = Keypair::new();
        let present_account = AccountSharedData::new(1, 10, &Pubkey::default());
        bank.store_account(&present_account_key.pubkey(), &present_account);

        let entries: Vec<_> = (0..NUM_TRANSFERS)
            .step_by(NUM_TRANSFERS_PER_ENTRY)
            .map(|i| {
                let mut transactions = (0..NUM_TRANSFERS_PER_ENTRY)
                    .map(|j| {
                        system_transaction::transfer(
                            &keypairs[i + j],
                            &keypairs[i + j + NUM_TRANSFERS].pubkey(),
                            1,
                            bank.last_blockhash(),
                        )
                    })
                    .collect::<Vec<_>>();

                transactions.push(system_transaction::create_account(
                    &mint_keypair,
                    &present_account_key, // puts a TX error in results
                    bank.last_blockhash(),
                    1,
                    0,
                    &solana_sdk::pubkey::new_rand(),
                ));

                next_entry_mut(&mut hash, 0, transactions)
            })
            .collect();
        assert_eq!(
            process_entries_for_tests(&bank, entries, true, None, None),
            Ok(())
        );
    }

    #[test]
    fn test_process_entry_tx_random_execution_no_error() {
        // entropy multiplier should be big enough to provide sufficient entropy
        // but small enough to not take too much time while executing the test.
        let entropy_multiplier: usize = 25;
        let initial_lamports = 100;

        // number of accounts need to be in multiple of 4 for correct
        // execution of the test.
        let num_accounts = entropy_multiplier * 4;
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config((num_accounts + 1) as u64 * initial_lamports);

        let bank = Arc::new(Bank::new_for_tests(&genesis_config));

        let mut keypairs: Vec<Keypair> = vec![];

        for _ in 0..num_accounts {
            let keypair = Keypair::new();
            let create_account_tx = system_transaction::transfer(
                &mint_keypair,
                &keypair.pubkey(),
                0,
                bank.last_blockhash(),
            );
            assert_eq!(bank.process_transaction(&create_account_tx), Ok(()));
            assert_matches!(
                bank.transfer(initial_lamports, &mint_keypair, &keypair.pubkey()),
                Ok(_)
            );
            keypairs.push(keypair);
        }

        let mut tx_vector: Vec<Transaction> = vec![];

        for i in (0..num_accounts).step_by(4) {
            tx_vector.append(&mut vec![
                system_transaction::transfer(
                    &keypairs[i + 1],
                    &keypairs[i].pubkey(),
                    initial_lamports,
                    bank.last_blockhash(),
                ),
                system_transaction::transfer(
                    &keypairs[i + 3],
                    &keypairs[i + 2].pubkey(),
                    initial_lamports,
                    bank.last_blockhash(),
                ),
            ]);
        }

        // Transfer lamports to each other
        let entry = next_entry(&bank.last_blockhash(), 1, tx_vector);
        assert_eq!(
            process_entries_for_tests(&bank, vec![entry], true, None, None),
            Ok(())
        );
        bank.squash();

        // Even number keypair should have balance of 2 * initial_lamports and
        // odd number keypair should have balance of 0, which proves
        // that even in case of random order of execution, overall state remains
        // consistent.
        for (i, keypair) in keypairs.iter().enumerate() {
            if i % 2 == 0 {
                assert_eq!(bank.get_balance(&keypair.pubkey()), 2 * initial_lamports);
            } else {
                assert_eq!(bank.get_balance(&keypair.pubkey()), 0);
            }
        }
    }

    #[test]
    fn test_process_entries_2_entries_tick() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(1000);
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();
        let keypair3 = Keypair::new();
        let keypair4 = Keypair::new();

        //load accounts
        let tx = system_transaction::transfer(
            &mint_keypair,
            &keypair1.pubkey(),
            1,
            bank.last_blockhash(),
        );
        assert_eq!(bank.process_transaction(&tx), Ok(()));
        let tx = system_transaction::transfer(
            &mint_keypair,
            &keypair2.pubkey(),
            1,
            bank.last_blockhash(),
        );
        assert_eq!(bank.process_transaction(&tx), Ok(()));

        let blockhash = bank.last_blockhash();
        while blockhash == bank.last_blockhash() {
            bank.register_tick(&Hash::default());
        }

        // ensure bank can process 2 entries that do not have a common account and tick is registered
        let tx = system_transaction::transfer(&keypair2, &keypair3.pubkey(), 1, blockhash);
        let entry_1 = next_entry(&blockhash, 1, vec![tx]);
        let tick = next_entry(&entry_1.hash, 1, vec![]);
        let tx =
            system_transaction::transfer(&keypair1, &keypair4.pubkey(), 1, bank.last_blockhash());
        let entry_2 = next_entry(&tick.hash, 1, vec![tx]);
        assert_eq!(
            process_entries_for_tests(
                &bank,
                vec![entry_1, tick, entry_2.clone()],
                true,
                None,
                None
            ),
            Ok(())
        );
        assert_eq!(bank.get_balance(&keypair3.pubkey()), 1);
        assert_eq!(bank.get_balance(&keypair4.pubkey()), 1);

        // ensure that an error is returned for an empty account (keypair2)
        let tx =
            system_transaction::transfer(&keypair2, &keypair3.pubkey(), 1, bank.last_blockhash());
        let entry_3 = next_entry(&entry_2.hash, 1, vec![tx]);
        assert_eq!(
            process_entries_for_tests(&bank, vec![entry_3], true, None, None),
            Err(TransactionError::AccountNotFound)
        );
    }

    #[test]
    fn test_update_transaction_statuses() {
        // Make sure instruction errors still update the signature cache
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(11_000);
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));
        let pubkey = solana_sdk::pubkey::new_rand();
        bank.transfer(1_000, &mint_keypair, &pubkey).unwrap();
        assert_eq!(bank.transaction_count(), 1);
        assert_eq!(bank.get_balance(&pubkey), 1_000);
        assert_eq!(
            bank.transfer(10_001, &mint_keypair, &pubkey),
            Err(TransactionError::InstructionError(
                0,
                SystemError::ResultWithNegativeLamports.into(),
            ))
        );
        assert_eq!(
            bank.transfer(10_001, &mint_keypair, &pubkey),
            Err(TransactionError::AlreadyProcessed)
        );

        // Make sure other errors don't update the signature cache
        let tx = system_transaction::transfer(&mint_keypair, &pubkey, 1000, Hash::default());
        let signature = tx.signatures[0];

        // Should fail with blockhash not found
        assert_eq!(
            bank.process_transaction(&tx).map(|_| signature),
            Err(TransactionError::BlockhashNotFound)
        );

        // Should fail again with blockhash not found
        assert_eq!(
            bank.process_transaction(&tx).map(|_| signature),
            Err(TransactionError::BlockhashNotFound)
        );
    }

    #[test]
    fn test_update_transaction_statuses_fail() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(11_000);
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();
        let success_tx = system_transaction::transfer(
            &mint_keypair,
            &keypair1.pubkey(),
            1,
            bank.last_blockhash(),
        );
        let fail_tx = system_transaction::transfer(
            &mint_keypair,
            &keypair2.pubkey(),
            2,
            bank.last_blockhash(),
        );

        let entry_1_to_mint = next_entry(
            &bank.last_blockhash(),
            1,
            vec![
                success_tx,
                fail_tx.clone(), // will collide
            ],
        );

        assert_eq!(
            process_entries_for_tests(&bank, vec![entry_1_to_mint], false, None, None),
            Err(TransactionError::AccountInUse)
        );

        // Should not see duplicate signature error
        assert_eq!(bank.process_transaction(&fail_tx), Ok(()));
    }

    #[test]
    fn test_halt_at_slot_starting_snapshot_root() {
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(123);

        // Create roots at slots 0, 1
        let forks = tr(0) / tr(1);
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();
        blockstore.add_tree(
            forks,
            false,
            true,
            genesis_config.ticks_per_slot,
            genesis_config.hash(),
        );
        blockstore.set_roots(vec![0, 1].iter()).unwrap();

        // Specify halting at slot 0
        let opts = ProcessOptions {
            poh_verify: true,
            dev_halt_at_slot: Some(0),
            accounts_db_test_hash_calculation: true,
            ..ProcessOptions::default()
        };
        let (bank_forks, ..) = test_process_blockstore(&genesis_config, &blockstore, opts);

        // Should be able to fetch slot 0 because we specified halting at slot 0, even
        // if there is a greater root at slot 1.
        assert!(bank_forks.get(0).is_some());
    }

    #[test]
    fn test_process_blockstore_from_root() {
        let GenesisConfigInfo {
            mut genesis_config, ..
        } = create_genesis_config(123);

        let ticks_per_slot = 1;
        genesis_config.ticks_per_slot = ticks_per_slot;
        let (ledger_path, blockhash) = create_new_tmp_ledger_auto_delete!(&genesis_config);
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();

        /*
          Build a blockstore in the ledger with the following fork structure:

               slot 0 (all ticks)
                 |
               slot 1 (all ticks)
                 |
               slot 2 (all ticks)
                 |
               slot 3 (all ticks) -> root
                 |
               slot 4 (all ticks)
                 |
               slot 5 (all ticks) -> root
                 |
               slot 6 (all ticks)
        */

        let mut last_hash = blockhash;
        for i in 0..6 {
            last_hash =
                fill_blockstore_slot_with_ticks(&blockstore, ticks_per_slot, i + 1, i, last_hash);
        }
        blockstore.set_roots(vec![3, 5].iter()).unwrap();

        // Set up bank1
        let mut bank_forks = BankForks::new(Bank::new_for_tests(&genesis_config));
        let bank0 = bank_forks.get(0).unwrap();
        let opts = ProcessOptions {
            poh_verify: true,
            accounts_db_test_hash_calculation: true,
            ..ProcessOptions::default()
        };
        let recyclers = VerifyRecyclers::default();
        process_bank_0(&bank0, &blockstore, &opts, &recyclers, None);
        let bank1 = bank_forks.insert(Bank::new_from_parent(&bank0, &Pubkey::default(), 1));
        confirm_full_slot(
            &blockstore,
            &bank1,
            &opts,
            &recyclers,
            &mut ConfirmationProgress::new(bank0.last_blockhash()),
            None,
            None,
            &mut ExecuteTimings::default(),
        )
        .unwrap();
        bank_forks.set_root(
            1,
            &solana_runtime::accounts_background_service::AbsRequestSender::default(),
            None,
        );

        let leader_schedule_cache = LeaderScheduleCache::new_from_bank(&bank1);

        // Test process_blockstore_from_root() from slot 1 onwards
        let (accounts_package_sender, _) = unbounded();
        let (_pruned_banks_sender, pruned_banks_receiver) = unbounded();
        process_blockstore_from_root(
            &blockstore,
            &mut bank_forks,
            &leader_schedule_cache,
            &opts,
            None,
            None,
            None,
            accounts_package_sender,
            pruned_banks_receiver,
        )
        .unwrap();

        assert_eq!(frozen_bank_slots(&bank_forks), vec![5, 6]);
        assert_eq!(bank_forks.working_bank().slot(), 6);
        assert_eq!(bank_forks.root(), 5);

        // Verify the parents of the head of the fork
        assert_eq!(
            &bank_forks[6]
                .parents()
                .iter()
                .map(|bank| bank.slot())
                .collect::<Vec<_>>(),
            &[5]
        );

        // Check that bank forks has the correct banks
        verify_fork_infos(&bank_forks);
    }

    /// Test that processing the blockstore is aware of incremental snapshots.  When processing the
    /// blockstore from a root, like what happens when loading from a snapshot, there may be new
    /// roots that cross a full snapshot interval.  In these cases, a bank snapshot must be taken,
    /// so that a full snapshot archive is created and available by the time the background
    /// services spin up.
    ///
    /// For this test, process enough roots to cross the full snapshot interval multiple times.
    /// Ensure afterwards that the snapshots were created.
    #[test]
    fn test_process_blockstore_from_root_with_snapshots() {
        solana_logger::setup();
        let GenesisConfigInfo {
            mut genesis_config, ..
        } = create_genesis_config(123);

        let ticks_per_slot = 1;
        genesis_config.ticks_per_slot = ticks_per_slot;
        let (ledger_path, blockhash) = create_new_tmp_ledger_auto_delete!(&genesis_config);
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();

        const ROOT_INTERVAL_SLOTS: Slot = 2;
        const FULL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS: Slot = ROOT_INTERVAL_SLOTS * 5;
        const LAST_SLOT: Slot = FULL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS * 4;

        let mut last_hash = blockhash;
        for i in 1..=LAST_SLOT {
            last_hash =
                fill_blockstore_slot_with_ticks(&blockstore, ticks_per_slot, i, i - 1, last_hash);
        }

        let roots_to_set = (0..=LAST_SLOT)
            .step_by(ROOT_INTERVAL_SLOTS as usize)
            .collect_vec();
        blockstore.set_roots(roots_to_set.iter()).unwrap();

        // Set up bank1
        let mut bank_forks = BankForks::new(Bank::new_for_tests(&genesis_config));
        let bank0 = bank_forks.get(0).unwrap();
        let opts = ProcessOptions {
            poh_verify: true,
            accounts_db_test_hash_calculation: true,
            ..ProcessOptions::default()
        };
        let recyclers = VerifyRecyclers::default();
        process_bank_0(&bank0, &blockstore, &opts, &recyclers, None);

        let slot_start_processing = 1;
        let bank = bank_forks.insert(Bank::new_from_parent(
            &bank0,
            &Pubkey::default(),
            slot_start_processing,
        ));
        confirm_full_slot(
            &blockstore,
            &bank,
            &opts,
            &recyclers,
            &mut ConfirmationProgress::new(bank0.last_blockhash()),
            None,
            None,
            &mut ExecuteTimings::default(),
        )
        .unwrap();
        bank_forks.set_root(
            1,
            &solana_runtime::accounts_background_service::AbsRequestSender::default(),
            None,
        );

        let bank_snapshots_tempdir = TempDir::new().unwrap();
        let snapshot_config = SnapshotConfig {
            full_snapshot_archive_interval_slots: FULL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS,
            bank_snapshots_dir: bank_snapshots_tempdir.path().to_path_buf(),
            ..SnapshotConfig::default()
        };

        let (accounts_package_sender, accounts_package_receiver) = unbounded();
        let leader_schedule_cache = LeaderScheduleCache::new_from_bank(&bank);

        let (_pruned_banks_sender, pruned_banks_receiver) = unbounded();
        process_blockstore_from_root(
            &blockstore,
            &mut bank_forks,
            &leader_schedule_cache,
            &opts,
            None,
            None,
            Some(&snapshot_config),
            accounts_package_sender.clone(),
            pruned_banks_receiver,
        )
        .unwrap();

        // The `drop()` is necessary here in order to call `.iter()` on the channel below
        drop(accounts_package_sender);

        // Ensure all the AccountsPackages were created and sent to the AccountsPackageReceiver
        let received_accounts_package_slots = accounts_package_receiver
            .iter()
            .map(|accounts_package| accounts_package.slot)
            .collect::<Vec<_>>();
        let expected_slots = (slot_start_processing..=LAST_SLOT)
            .filter(|slot| slot % FULL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS == 0)
            .collect::<Vec<_>>();
        assert_eq!(received_accounts_package_slots, expected_slots);

        // Ensure all the bank snapshots were created
        let bank_snapshots = snapshot_utils::get_bank_snapshots(&bank_snapshots_tempdir);
        let mut bank_snapshot_slots = bank_snapshots
            .into_iter()
            .map(|bank_snapshot| bank_snapshot.slot)
            .collect::<Vec<_>>();
        bank_snapshot_slots.sort_unstable();
        assert_eq!(bank_snapshot_slots, expected_slots);
    }

    #[test]
    #[ignore]
    fn test_process_entries_stress() {
        // this test throws lots of rayon threads at process_entries()
        //  finds bugs in very low-layer stuff
        solana_logger::setup();
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(1_000_000_000);
        let mut bank = Arc::new(Bank::new_for_tests(&genesis_config));

        const NUM_TRANSFERS_PER_ENTRY: usize = 8;
        const NUM_TRANSFERS: usize = NUM_TRANSFERS_PER_ENTRY * 32;

        let keypairs: Vec<_> = (0..NUM_TRANSFERS * 2).map(|_| Keypair::new()).collect();

        // give everybody one lamport
        for keypair in &keypairs {
            bank.transfer(1, &mint_keypair, &keypair.pubkey())
                .expect("funding failed");
        }

        let present_account_key = Keypair::new();
        let present_account = AccountSharedData::new(1, 10, &Pubkey::default());
        bank.store_account(&present_account_key.pubkey(), &present_account);

        let mut i = 0;
        let mut hash = bank.last_blockhash();
        let mut root: Option<Arc<Bank>> = None;
        loop {
            let entries: Vec<_> = (0..NUM_TRANSFERS)
                .step_by(NUM_TRANSFERS_PER_ENTRY)
                .map(|i| {
                    next_entry_mut(&mut hash, 0, {
                        let mut transactions = (i..i + NUM_TRANSFERS_PER_ENTRY)
                            .map(|i| {
                                system_transaction::transfer(
                                    &keypairs[i],
                                    &keypairs[i + NUM_TRANSFERS].pubkey(),
                                    1,
                                    bank.last_blockhash(),
                                )
                            })
                            .collect::<Vec<_>>();

                        transactions.push(system_transaction::create_account(
                            &mint_keypair,
                            &present_account_key, // puts a TX error in results
                            bank.last_blockhash(),
                            100,
                            100,
                            &solana_sdk::pubkey::new_rand(),
                        ));
                        transactions
                    })
                })
                .collect();
            info!("paying iteration {}", i);
            process_entries_for_tests(&bank, entries, true, None, None).expect("paying failed");

            let entries: Vec<_> = (0..NUM_TRANSFERS)
                .step_by(NUM_TRANSFERS_PER_ENTRY)
                .map(|i| {
                    next_entry_mut(
                        &mut hash,
                        0,
                        (i..i + NUM_TRANSFERS_PER_ENTRY)
                            .map(|i| {
                                system_transaction::transfer(
                                    &keypairs[i + NUM_TRANSFERS],
                                    &keypairs[i].pubkey(),
                                    1,
                                    bank.last_blockhash(),
                                )
                            })
                            .collect::<Vec<_>>(),
                    )
                })
                .collect();

            info!("refunding iteration {}", i);
            process_entries_for_tests(&bank, entries, true, None, None).expect("refunding failed");

            // advance to next block
            process_entries_for_tests(
                &bank,
                (0..bank.ticks_per_slot())
                    .map(|_| next_entry_mut(&mut hash, 1, vec![]))
                    .collect::<Vec<_>>(),
                true,
                None,
                None,
            )
            .expect("process ticks failed");

            if i % 16 == 0 {
                if let Some(old_root) = root {
                    old_root.squash();
                }
                root = Some(bank.clone());
            }
            i += 1;

            bank = Arc::new(Bank::new_from_parent(
                &bank,
                &Pubkey::default(),
                bank.slot() + thread_rng().gen_range(1, 3),
            ));
        }
    }

    #[test]
    fn test_process_ledger_ticks_ordering() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(100);
        let bank0 = Arc::new(Bank::new_for_tests(&genesis_config));
        let genesis_hash = genesis_config.hash();
        let keypair = Keypair::new();

        // Simulate a slot of virtual ticks, creates a new blockhash
        let mut entries = create_ticks(genesis_config.ticks_per_slot, 1, genesis_hash);

        // The new blockhash is going to be the hash of the last tick in the block
        let new_blockhash = entries.last().unwrap().hash;
        // Create an transaction that references the new blockhash, should still
        // be able to find the blockhash if we process transactions all in the same
        // batch
        let tx = system_transaction::transfer(&mint_keypair, &keypair.pubkey(), 1, new_blockhash);
        let entry = next_entry(&new_blockhash, 1, vec![tx]);
        entries.push(entry);

        process_entries_for_tests(&bank0, entries, true, None, None).unwrap();
        assert_eq!(bank0.get_balance(&keypair.pubkey()), 1)
    }

    fn get_epoch_schedule(
        genesis_config: &GenesisConfig,
        account_paths: Vec<PathBuf>,
    ) -> EpochSchedule {
        let bank = Bank::new_with_paths_for_tests(
            genesis_config,
            account_paths,
            None,
            None,
            AccountSecondaryIndexes::default(),
            false,
            AccountShrinkThreshold::default(),
            false,
        );
        *bank.epoch_schedule()
    }

    fn frozen_bank_slots(bank_forks: &BankForks) -> Vec<Slot> {
        let mut slots: Vec<_> = bank_forks.frozen_banks().keys().cloned().collect();
        slots.sort_unstable();
        slots
    }

    // Check that `bank_forks` contains all the ancestors and banks for each fork identified in
    // `bank_forks_info`
    fn verify_fork_infos(bank_forks: &BankForks) {
        for slot in frozen_bank_slots(bank_forks) {
            let head_bank = &bank_forks[slot];
            let mut parents = head_bank.parents();
            parents.push(head_bank.clone());

            // Ensure the tip of each fork and all its parents are in the given bank_forks
            for parent in parents {
                let parent_bank = &bank_forks[parent.slot()];
                assert_eq!(parent_bank.slot(), parent.slot());
                assert!(parent_bank.is_frozen());
            }
        }
    }

    #[test]
    fn test_get_first_error() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(1_000_000_000);
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));

        let present_account_key = Keypair::new();
        let present_account = AccountSharedData::new(1, 10, &Pubkey::default());
        bank.store_account(&present_account_key.pubkey(), &present_account);

        let keypair = Keypair::new();

        // Create array of two transactions which throw different errors
        let account_not_found_tx = system_transaction::transfer(
            &keypair,
            &solana_sdk::pubkey::new_rand(),
            42,
            bank.last_blockhash(),
        );
        let account_not_found_sig = account_not_found_tx.signatures[0];
        let invalid_blockhash_tx = system_transaction::transfer(
            &mint_keypair,
            &solana_sdk::pubkey::new_rand(),
            42,
            Hash::default(),
        );
        let txs = vec![account_not_found_tx, invalid_blockhash_tx];
        let batch = bank.prepare_batch_for_tests(txs);
        let (
            TransactionResults {
                fee_collection_results,
                ..
            },
            _balances,
        ) = batch.bank().load_execute_and_commit_transactions(
            &batch,
            MAX_PROCESSING_AGE,
            false,
            false,
            false,
            &mut ExecuteTimings::default(),
        );
        let (err, signature) = get_first_error(&batch, fee_collection_results).unwrap();
        assert_eq!(err.unwrap_err(), TransactionError::AccountNotFound);
        assert_eq!(signature, account_not_found_sig);
    }

    #[test]
    fn test_replay_vote_sender() {
        let validator_keypairs: Vec<_> =
            (0..10).map(|_| ValidatorVoteKeypairs::new_rand()).collect();
        let GenesisConfigInfo {
            genesis_config,
            voting_keypair: _,
            ..
        } = create_genesis_config_with_vote_accounts(
            1_000_000_000,
            &validator_keypairs,
            vec![100; validator_keypairs.len()],
        );
        let bank0 = Arc::new(Bank::new_for_tests(&genesis_config));
        bank0.freeze();

        let bank1 = Arc::new(Bank::new_from_parent(
            &bank0,
            &solana_sdk::pubkey::new_rand(),
            1,
        ));

        // The new blockhash is going to be the hash of the last tick in the block
        let bank_1_blockhash = bank1.last_blockhash();

        // Create an transaction that references the new blockhash, should still
        // be able to find the blockhash if we process transactions all in the same
        // batch
        let mut expected_successful_voter_pubkeys = BTreeSet::new();
        let vote_txs: Vec<_> = validator_keypairs
            .iter()
            .enumerate()
            .map(|(i, validator_keypairs)| {
                if i % 3 == 0 {
                    // These votes are correct
                    expected_successful_voter_pubkeys
                        .insert(validator_keypairs.vote_keypair.pubkey());
                    vote_transaction::new_vote_transaction(
                        vec![0],
                        bank0.hash(),
                        bank_1_blockhash,
                        &validator_keypairs.node_keypair,
                        &validator_keypairs.vote_keypair,
                        &validator_keypairs.vote_keypair,
                        None,
                    )
                } else if i % 3 == 1 {
                    // These have the wrong authorized voter
                    vote_transaction::new_vote_transaction(
                        vec![0],
                        bank0.hash(),
                        bank_1_blockhash,
                        &validator_keypairs.node_keypair,
                        &validator_keypairs.vote_keypair,
                        &Keypair::new(),
                        None,
                    )
                } else {
                    // These have an invalid vote for non-existent bank 2
                    vote_transaction::new_vote_transaction(
                        vec![bank1.slot() + 1],
                        bank0.hash(),
                        bank_1_blockhash,
                        &validator_keypairs.node_keypair,
                        &validator_keypairs.vote_keypair,
                        &validator_keypairs.vote_keypair,
                        None,
                    )
                }
            })
            .collect();
        let entry = next_entry(&bank_1_blockhash, 1, vote_txs);
        let (replay_vote_sender, replay_vote_receiver) = unbounded();
        let _ =
            process_entries_for_tests(&bank1, vec![entry], true, None, Some(&replay_vote_sender));
        let successes: BTreeSet<Pubkey> = replay_vote_receiver
            .try_iter()
            .map(|(vote_pubkey, ..)| vote_pubkey)
            .collect();
        assert_eq!(successes, expected_successful_voter_pubkeys);
    }

    fn make_slot_with_vote_tx(
        blockstore: &Blockstore,
        ticks_per_slot: u64,
        tx_landed_slot: Slot,
        parent_slot: Slot,
        parent_blockhash: &Hash,
        vote_tx: Transaction,
        slot_leader_keypair: &Arc<Keypair>,
    ) {
        // Add votes to `last_slot` so that `root` will be confirmed
        let vote_entry = next_entry(parent_blockhash, 1, vec![vote_tx]);
        let mut entries = create_ticks(ticks_per_slot, 0, vote_entry.hash);
        entries.insert(0, vote_entry);
        blockstore
            .write_entries(
                tx_landed_slot,
                0,
                0,
                ticks_per_slot,
                Some(parent_slot),
                true,
                slot_leader_keypair,
                entries,
                0,
            )
            .unwrap();
    }

    fn run_test_process_blockstore_with_supermajority_root(blockstore_root: Option<Slot>) {
        solana_logger::setup();
        /*
            Build fork structure:
                 slot 0
                   |
                 slot 1 <- (blockstore root)
                 /    \
            slot 2    |
               |      |
            slot 4    |
                    slot 5
                      |
                `expected_root_slot`
                     /    \
                  ...    minor fork
                  /
            `last_slot`
                 |
            `really_last_slot`
        */
        let starting_fork_slot = 5;
        let mut main_fork = tr(starting_fork_slot);
        let mut main_fork_ref = main_fork.root_mut().get_mut();

        // Make enough slots to make a root slot > blockstore_root
        let expected_root_slot = starting_fork_slot + blockstore_root.unwrap_or(0);
        let really_expected_root_slot = expected_root_slot + 1;
        let last_main_fork_slot = expected_root_slot + MAX_LOCKOUT_HISTORY as u64 + 1;
        let really_last_main_fork_slot = last_main_fork_slot + 1;

        // Make `minor_fork`
        let last_minor_fork_slot = really_last_main_fork_slot + 1;
        let minor_fork = tr(last_minor_fork_slot);

        // Make 'main_fork`
        for slot in starting_fork_slot + 1..last_main_fork_slot {
            if slot - 1 == expected_root_slot {
                main_fork_ref.push_front(minor_fork.clone());
            }
            main_fork_ref.push_front(tr(slot));
            main_fork_ref = main_fork_ref.front_mut().unwrap().get_mut();
        }
        let forks = tr(0) / (tr(1) / (tr(2) / (tr(4))) / main_fork);
        let validator_keypairs = ValidatorVoteKeypairs::new_rand();
        let GenesisConfigInfo { genesis_config, .. } =
            genesis_utils::create_genesis_config_with_vote_accounts(
                10_000,
                &[&validator_keypairs],
                vec![100],
            );
        let ticks_per_slot = genesis_config.ticks_per_slot();
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();
        blockstore.add_tree(forks, false, true, ticks_per_slot, genesis_config.hash());

        if let Some(blockstore_root) = blockstore_root {
            blockstore
                .set_roots(std::iter::once(&blockstore_root))
                .unwrap();
        }

        let opts = ProcessOptions {
            poh_verify: true,
            accounts_db_test_hash_calculation: true,
            ..ProcessOptions::default()
        };
        let (bank_forks, ..) = test_process_blockstore(&genesis_config, &blockstore, opts.clone());

        // prepare to add votes
        let last_vote_bank_hash = bank_forks.get(last_main_fork_slot - 1).unwrap().hash();
        let last_vote_blockhash = bank_forks
            .get(last_main_fork_slot - 1)
            .unwrap()
            .last_blockhash();
        let slots: Vec<_> = (expected_root_slot..last_main_fork_slot).collect();
        let vote_tx = vote_transaction::new_vote_transaction(
            slots,
            last_vote_bank_hash,
            last_vote_blockhash,
            &validator_keypairs.node_keypair,
            &validator_keypairs.vote_keypair,
            &validator_keypairs.vote_keypair,
            None,
        );

        // Add votes to `last_slot` so that `root` will be confirmed
        let leader_keypair = Arc::new(validator_keypairs.node_keypair);
        make_slot_with_vote_tx(
            &blockstore,
            ticks_per_slot,
            last_main_fork_slot,
            last_main_fork_slot - 1,
            &last_vote_blockhash,
            vote_tx,
            &leader_keypair,
        );

        let (bank_forks, ..) = test_process_blockstore(&genesis_config, &blockstore, opts.clone());

        assert_eq!(bank_forks.root(), expected_root_slot);
        assert_eq!(
            bank_forks.frozen_banks().len() as u64,
            last_minor_fork_slot - really_expected_root_slot + 1
        );

        // Minor fork at `last_main_fork_slot + 1` was above the `expected_root_slot`
        // so should not have been purged
        //
        // Fork at slot 2 was purged because it was below the `expected_root_slot`
        for slot in 0..=last_minor_fork_slot {
            // this slot will be created below
            if slot == really_last_main_fork_slot {
                continue;
            }
            if slot >= expected_root_slot {
                let bank = bank_forks.get(slot).unwrap();
                assert_eq!(bank.slot(), slot);
                assert!(bank.is_frozen());
            } else {
                assert!(bank_forks.get(slot).is_none());
            }
        }

        // really prepare to add votes
        let last_vote_bank_hash = bank_forks.get(last_main_fork_slot).unwrap().hash();
        let last_vote_blockhash = bank_forks
            .get(last_main_fork_slot)
            .unwrap()
            .last_blockhash();
        let slots: Vec<_> = vec![last_main_fork_slot];
        let vote_tx = vote_transaction::new_vote_transaction(
            slots,
            last_vote_bank_hash,
            last_vote_blockhash,
            &leader_keypair,
            &validator_keypairs.vote_keypair,
            &validator_keypairs.vote_keypair,
            None,
        );

        // Add votes to `really_last_slot` so that `root` will be confirmed again
        make_slot_with_vote_tx(
            &blockstore,
            ticks_per_slot,
            really_last_main_fork_slot,
            last_main_fork_slot,
            &last_vote_blockhash,
            vote_tx,
            &leader_keypair,
        );

        let (bank_forks, ..) = test_process_blockstore(&genesis_config, &blockstore, opts);

        assert_eq!(bank_forks.root(), really_expected_root_slot);
    }

    #[test]
    fn test_process_blockstore_with_supermajority_root_without_blockstore_root() {
        run_test_process_blockstore_with_supermajority_root(None);
    }

    #[test]
    fn test_process_blockstore_with_supermajority_root_with_blockstore_root() {
        run_test_process_blockstore_with_supermajority_root(Some(1))
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn test_supermajority_root_from_vote_accounts() {
        let convert_to_vote_accounts =
            |roots_stakes: Vec<(Slot, u64)>| -> HashMap<Pubkey, (u64, VoteAccount)> {
                roots_stakes
                    .into_iter()
                    .map(|(root, stake)| {
                        let mut vote_state = VoteState::default();
                        vote_state.root_slot = Some(root);
                        let mut vote_account = AccountSharedData::new(
                            1,
                            VoteState::size_of(),
                            &solana_vote_program::id(),
                        );
                        let versioned = VoteStateVersions::new_current(vote_state);
                        VoteState::serialize(&versioned, vote_account.data_as_mut_slice()).unwrap();
                        (
                            solana_sdk::pubkey::new_rand(),
                            (stake, VoteAccount::from(vote_account)),
                        )
                    })
                    .collect()
            };

        let total_stake = 10;
        let slot = 100;

        // Supermajority root should be None
        assert!(
            supermajority_root_from_vote_accounts(slot, total_stake, &HashMap::default()).is_none()
        );

        // Supermajority root should be None
        let roots_stakes = vec![(8, 1), (3, 1), (4, 1), (8, 1)];
        let accounts = convert_to_vote_accounts(roots_stakes);
        assert!(supermajority_root_from_vote_accounts(slot, total_stake, &accounts).is_none());

        // Supermajority root should be 4, has 7/10 of the stake
        let roots_stakes = vec![(8, 1), (3, 1), (4, 1), (8, 5)];
        let accounts = convert_to_vote_accounts(roots_stakes);
        assert_eq!(
            supermajority_root_from_vote_accounts(slot, total_stake, &accounts).unwrap(),
            4
        );

        // Supermajority root should be 8, it has 7/10 of the stake
        let roots_stakes = vec![(8, 1), (3, 1), (4, 1), (8, 6)];
        let accounts = convert_to_vote_accounts(roots_stakes);
        assert_eq!(
            supermajority_root_from_vote_accounts(slot, total_stake, &accounts).unwrap(),
            8
        );
    }

    #[test]
    fn test_rebatch_transactions() {
        let dummy_leader_pubkey = solana_sdk::pubkey::new_rand();
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config_with_leader(500, &dummy_leader_pubkey, 100);
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));

        let pubkey = solana_sdk::pubkey::new_rand();
        let keypair2 = Keypair::new();
        let pubkey2 = solana_sdk::pubkey::new_rand();

        let txs = vec![
            SanitizedTransaction::from_transaction_for_tests(system_transaction::transfer(
                &mint_keypair,
                &pubkey,
                1,
                genesis_config.hash(),
            )),
            SanitizedTransaction::from_transaction_for_tests(system_transaction::transfer(
                &keypair2,
                &pubkey2,
                1,
                genesis_config.hash(),
            )),
        ];

        let batch = bank.prepare_sanitized_batch(&txs);
        assert!(batch.needs_unlock());

        let batch2 = rebatch_transactions(
            batch.lock_results(),
            &bank,
            batch.sanitized_transactions(),
            0,
            1,
        );
        assert!(batch.needs_unlock());
        assert!(!batch2.needs_unlock());
    }

    #[test]
    fn test_check_accounts_data_block_size() {
        const ACCOUNT_SIZE: u64 = MAX_PERMITTED_DATA_LENGTH;
        const NUM_ACCOUNTS: u64 = MAX_ACCOUNT_DATA_BLOCK_LEN / ACCOUNT_SIZE;

        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config((1_000_000 + NUM_ACCOUNTS + 1) * LAMPORTS_PER_SOL);
        let bank = Bank::new_for_tests(&genesis_config);
        let bank = Arc::new(bank);
        assert!(bank
            .feature_set
            .is_active(&feature_set::cap_accounts_data_size_per_block::id()));

        for _ in 0..NUM_ACCOUNTS {
            let transaction = system_transaction::create_account(
                &mint_keypair,
                &Keypair::new(),
                bank.last_blockhash(),
                LAMPORTS_PER_SOL,
                ACCOUNT_SIZE,
                &solana_sdk::system_program::id(),
            );
            let entry = next_entry(&bank.last_blockhash(), 1, vec![transaction]);
            assert_eq!(
                process_entries_for_tests(&bank, vec![entry], true, None, None),
                Ok(()),
            );
        }

        let transaction = system_transaction::create_account(
            &mint_keypair,
            &Keypair::new(),
            bank.last_blockhash(),
            LAMPORTS_PER_SOL,
            ACCOUNT_SIZE,
            &solana_sdk::system_program::id(),
        );
        let entry = next_entry(&bank.last_blockhash(), 1, vec![transaction]);
        assert_eq!(
            process_entries_for_tests(&bank, vec![entry], true, None, None),
            Err(TransactionError::WouldExceedAccountDataBlockLimit)
        );
    }

    #[test]
    fn test_check_accounts_data_total_size() {
        const REMAINING_ACCOUNTS_DATA_SIZE: u64 =
            MAX_ACCOUNT_DATA_BLOCK_LEN - MAX_PERMITTED_DATA_LENGTH;
        const INITIAL_ACCOUNTS_DATA_SIZE: u64 =
            MAX_ACCOUNTS_DATA_LEN - REMAINING_ACCOUNTS_DATA_SIZE;
        const ACCOUNT_SIZE: u64 = MAX_PERMITTED_DATA_LENGTH;
        const SHRINK_SIZE: u64 = 5678;
        const ACCOUNTS_DATA_SIZE_DELTA_PER_ITERATION: u64 = ACCOUNT_SIZE - SHRINK_SIZE;
        const NUM_ITERATIONS: u64 =
            REMAINING_ACCOUNTS_DATA_SIZE / ACCOUNTS_DATA_SIZE_DELTA_PER_ITERATION;
        const ACCOUNT_BALANCE: u64 = 70 * LAMPORTS_PER_SOL; // rent exempt amount for a 10MB account is a little less than 70 SOL

        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config((1_000_000 + NUM_ITERATIONS + 1) * ACCOUNT_BALANCE);
        let mut bank = Bank::new_for_tests(&genesis_config);
        let mock_realloc_program_id = Pubkey::new_unique();
        bank.add_builtin(
            "mock_realloc_program",
            &mock_realloc_program_id,
            mock_realloc::process_instruction,
        );
        bank.set_accounts_data_size_initial_for_tests(INITIAL_ACCOUNTS_DATA_SIZE);
        let bank = Arc::new(bank);
        let bank = Arc::new(Bank::new_from_parent(&bank, &Pubkey::new_unique(), 1));
        assert!(bank
            .feature_set
            .is_active(&feature_set::cap_accounts_data_len::id()));

        for _ in 0..NUM_ITERATIONS {
            let accounts_data_size_before = bank.load_accounts_data_size();

            // Create a new account, with the full size
            let new_account = Keypair::new();
            let transaction = system_transaction::create_account(
                &mint_keypair,
                &new_account,
                bank.last_blockhash(),
                ACCOUNT_BALANCE,
                ACCOUNT_SIZE,
                &mock_realloc_program_id,
            );

            let entry = next_entry(&bank.last_blockhash(), 1, vec![transaction]);
            assert_eq!(
                process_entries_for_tests(&bank, vec![entry], true, None, None),
                Ok(()),
            );
            let accounts_data_size_after = bank.load_accounts_data_size();
            assert_eq!(
                accounts_data_size_after - accounts_data_size_before,
                ACCOUNT_SIZE,
            );

            // Resize the account to be smaller
            let new_size = ACCOUNT_SIZE - SHRINK_SIZE;
            let transaction = mock_realloc::create_transaction(
                &mint_keypair,
                &new_account.pubkey(),
                new_size as usize,
                mock_realloc_program_id,
                bank.last_blockhash(),
            );

            let entry = next_entry(&bank.last_blockhash(), 1, vec![transaction]);
            assert_eq!(
                process_entries_for_tests(&bank, vec![entry], true, None, None),
                Ok(()),
            );
            let accounts_data_size_after = bank.load_accounts_data_size();
            assert_eq!(
                accounts_data_size_after - accounts_data_size_before,
                new_size,
            );
        }

        let transaction = system_transaction::create_account(
            &mint_keypair,
            &Keypair::new(),
            bank.last_blockhash(),
            ACCOUNT_BALANCE,
            ACCOUNT_SIZE,
            &solana_sdk::system_program::id(),
        );
        let entry = next_entry(&bank.last_blockhash(), 1, vec![transaction]);
        assert!(matches!(
            process_entries_for_tests(&bank, vec![entry], true, None, None),
            Err(TransactionError::InstructionError(
                _,
                InstructionError::MaxAccountsDataSizeExceeded,
            ))
        ));
    }

    mod mock_realloc {
        use {
            super::*,
            serde::{Deserialize, Serialize},
        };

        #[derive(Debug, Serialize, Deserialize)]
        enum Instruction {
            Realloc { new_size: usize },
        }

        pub fn process_instruction(
            _first_instruction_account: usize,
            data: &[u8],
            invoke_context: &mut InvokeContext,
        ) -> result::Result<(), InstructionError> {
            let transaction_context = &invoke_context.transaction_context;
            let instruction_context = transaction_context.get_current_instruction_context()?;
            if let Ok(instruction) = bincode::deserialize(data) {
                match instruction {
                    Instruction::Realloc { new_size } => instruction_context
                        .try_borrow_instruction_account(transaction_context, 0)?
                        .set_data_length(new_size),
                }
                Ok(())
            } else {
                Err(InstructionError::InvalidInstructionData)
            }
        }

        pub fn create_transaction(
            payer: &Keypair,
            reallocd: &Pubkey,
            new_size: usize,
            mock_realloc_program_id: Pubkey,
            recent_blockhash: Hash,
        ) -> Transaction {
            let account_metas = vec![solana_sdk::instruction::AccountMeta::new(*reallocd, false)];
            let instruction = solana_sdk::instruction::Instruction::new_with_bincode(
                mock_realloc_program_id,
                &Instruction::Realloc { new_size },
                account_metas,
            );
            Transaction::new_signed_with_payer(
                &[instruction],
                Some(&payer.pubkey()),
                &[payer],
                recent_blockhash,
            )
        }
    }
}
